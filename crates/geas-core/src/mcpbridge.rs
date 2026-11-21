// SPDX-License-Identifier: GPL-3.0-or-later
//! MCP サーバー（標準入出力・JSON-RPC）の橋渡し。ツール定義（tools.json、geas が生成）を読み、ツールの呼び出しを、動いているサーバーの REST に中継するだけの汎用のもの。
//! アプリを知らない。使い方：`geas mcp-bridge tools.json http://127.0.0.1:8080`
use serde_json::{json, Map, Value};
use std::io::{BufRead, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

const PROTOCOL: &str = "2025-06-18";
/// サーバーの返事を読む上限（バイト）。サーバーの本文の上限と同じ
const MAX_REPLY: usize = 16 * 1024 * 1024;

/// URL の部品の % エンコード（英数字と `_.-~` 以外を符号化する。plus=true なら空白は +）
fn quote(s: &str, plus: bool) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => o.push(b as char),
            b' ' if plus => o.push('+'),
            _ => o.push_str(&format!("%{:02X}", b)),
        }
    }
    o
}

/// 引数の値を、文字列にする（文字列はそのまま、真偽は True／False、数はそのまま）
fn as_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        other => other.to_string(),
    }
}

struct Reply {
    status: u16,
    etag: Option<String>,
    body: String,
}

fn http(base: &str, method: &str, path: &str, headers: &[(&str, String)], body: Option<Vec<u8>>) -> Result<Reply, String> {
    let rest = base.trim_end_matches('/').strip_prefix("http://").ok_or_else(|| format!("http:// で始まるサーバーの URL だけに対応しています: {}", base))?;
    let (host, prefix) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let addr = if host.contains(':') { host.to_string() } else { format!("{}:80", host) };
    let sa = addr.to_socket_addrs().map_err(|e| e.to_string())?.next().ok_or("アドレスを引けません")?;
    let mut s = TcpStream::connect_timeout(&sa, Duration::from_secs(30)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(30))).ok();
    let mut req = format!("{} {}{} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n", method, prefix, path, host);
    for (k, v) in headers {
        req.push_str(&format!("{}: {}\r\n", k, v));
    }
    let blen = body.as_ref().map(|b| b.len());
    if method != "GET" || blen.is_some() {
        req.push_str(&format!("Content-Length: {}\r\n", blen.unwrap_or(0)));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    if let Some(b) = body {
        s.write_all(&b).map_err(|e| e.to_string())?;
    }
    let mut raw = vec![];
    s.take(MAX_REPLY as u64 + 65536).read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or("HTTP の返事を読めません")?;
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.lines();
    let status: u16 = lines.next().and_then(|l| l.split_whitespace().nth(1)).and_then(|c| c.parse().ok()).ok_or("HTTP の状態行を読めません")?;
    let mut etag = None;
    let mut chunked = false;
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "etag" => etag = Some(v.trim().to_string()),
                "transfer-encoding" if v.to_ascii_lowercase().contains("chunked") => chunked = true,
                _ => {}
            }
        }
    }
    let mut data = raw[split + 4..].to_vec();
    if chunked {
        let mut out = vec![];
        let mut i = 0;
        loop {
            let Some(e) = data[i..].windows(2).position(|w| w == b"\r\n") else { break };
            let n = usize::from_str_radix(String::from_utf8_lossy(&data[i..i + e]).split(';').next().unwrap_or("").trim(), 16).unwrap_or(0);
            i += e + 2;
            if n == 0 || i + n > data.len() {
                break;
            }
            out.extend_from_slice(&data[i..i + n]);
            i += n + 2;
        }
        data = out;
    }
    data.truncate(MAX_REPLY);
    Ok(Reply { status, etag, body: String::from_utf8_lossy(&data).into_owned() })
}

/// ツール 1 つの呼び出しを、REST にする。戻り値は (HTTP の状態, 本文)
fn call(tool: &Value, args: &Map<String, Value>, base: &str) -> Result<(u16, String), String> {
    let g = &tool["geas"];
    let mut segs = vec![];
    for s in g["path"].as_str().unwrap_or("").split('/') {
        if let Some(n) = s.strip_prefix(':') {
            let v = args.get(n).ok_or_else(|| format!("KeyError: '{}'", n))?;
            segs.push(quote(&as_text(v), false));
        } else {
            segs.push(s.to_string());
        }
    }
    let mut path = segs.join("/");
    let qnames: Vec<&str> = g["q"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
    // 絞り込みの条件（指定しないものは書かない）
    let given: Vec<&str> = qnames.iter().copied().filter(|n| args.get(*n).map_or(false, |v| !v.is_null())).collect();
    let json_hdr = ("Content-Type", "application/json".to_string());
    if g["post_q"].as_bool().unwrap_or(false) {
        // 事情があって POST で受ける問い合わせ：条件は、本文の {"q": {…}}
        let mut q = Map::new();
        for n in &given {
            q.insert(n.to_string(), args[*n].clone());
        }
        let data = serde_json::to_vec(&json!({ "q": q })).unwrap();
        let r = http(base, "POST", &path, &[json_hdr], Some(data))?;
        return Ok((r.status, r.body));
    }
    if !given.is_empty() {
        let q: Vec<String> = given.iter().map(|n| format!("{}={}", quote(&format!("q[{}]", n), true), quote(&as_text(&args[*n]), true))).collect();
        path.push('?');
        path.push_str(&q.join("&"));
    }
    let method = g["method"].as_str().unwrap_or("GET");
    let body = if method != "GET" {
        let mut b = Map::new();
        for k in g["body"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) {
            if let Some(k) = k.as_str() {
                if let Some(v) = args.get(k) {
                    b.insert(k.to_string(), v.clone());
                }
            }
        }
        Some(serde_json::to_vec(&Value::Object(b)).unwrap())
    } else {
        None
    };
    let mut headers = vec![json_hdr];
    if g["if_match"].as_bool().unwrap_or(false) {
        // 書き込みの版の確認：読んだときの行の版を、ヘッダ If-Match で送る
        if let Some(t) = args.get("if_match").filter(|v| !v.is_null() && as_text(v) != "") {
            let tag = as_text(t).trim().to_string();
            headers.push(("If-Match", if tag.starts_with('"') { tag } else { format!("\"{}\"", tag) }));
        }
    }
    let r = http(base, method, &path, &headers, body)?;
    // 1 件の読み出し：答えの版（ETag）を、本文の前に 1 行で付ける（書き込みのツールの if_match に、そのまま渡せる）
    if r.status < 400 && g["etag"].as_bool().unwrap_or(false) {
        if let Some(e) = &r.etag {
            return Ok((r.status, format!("ETag: {}\n{}", e, r.body)));
        }
    }
    Ok((r.status, r.body))
}

/// 要求 1 件への返事（通知には返事しない）
pub fn handle(msg: &Map<String, Value>, tools: &[Value], base: &str) -> Option<Value> {
    let id = msg.get("id").filter(|v| !v.is_null())?.clone();
    let m = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let res = match m {
        "initialize" => json!({"protocolVersion": PROTOCOL, "capabilities": {"tools": {}}, "serverInfo": {"name": "geas", "version": "0.1.0"}}),
        "tools/list" => {
            let ts: Vec<Value> = tools.iter().map(|t| Value::Object(t.as_object().unwrap().iter().filter(|(k, _)| *k != "geas").map(|(k, v)| (k.clone(), v.clone())).collect())).collect();
            json!({ "tools": ts })
        }
        "tools/call" => {
            let empty = Map::new();
            let prm = msg.get("params").and_then(|v| v.as_object()).unwrap_or(&empty);
            let name = prm.get("name").and_then(|v| v.as_str());
            let Some(tool) = tools.iter().find(|t| t["name"].as_str() == name && name.is_some()) else {
                return Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32602, "message": format!("ツール「{}」はありません", name.unwrap_or("None"))}}));
            };
            let args = prm.get("arguments").and_then(|v| v.as_object()).cloned().unwrap_or_default();
            match call(tool, &args, base) {
                Ok((code, text)) => json!({"isError": code >= 400, "content": [{"type": "text", "text": format!("HTTP {}\n{}", code, text)}]}),
                Err(e) => json!({"isError": true, "content": [{"type": "text", "text": format!("呼べませんでした: {}", e)}]}),
            }
        }
        "ping" => json!({}),
        _ => return Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("未対応: {}", m)}})),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": res}))
}

/// 標準入力から 1 行 1 要求で読み、標準出力へ返事を 1 行ずつ書く
pub fn serve(tools_path: &str, base: &str, input: &mut dyn BufRead, out: &mut dyn Write) -> Result<(), String> {
    let text = std::fs::read_to_string(tools_path).map_err(|e| format!("{}: {}", tools_path, e))?;
    let tools: Vec<Value> = serde_json::from_str(&text).map_err(|e| format!("{}: {}", tools_path, e))?;
    let mut buf = vec![];
    loop {
        buf.clear();
        if input.read_until(b'\n', &mut buf).map_err(|e| e.to_string())? == 0 {
            return Ok(());
        }
        let line = String::from_utf8_lossy(&buf);
        if line.trim().is_empty() {
            continue;
        }
        // 読めない 1 行で橋渡しを止めない（JSON でない・オブジェクトでない行は、JSON-RPC の誤りとして返す）
        let reply = match serde_json::from_str::<Value>(&line) {
            Err(_) => Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "JSON として読めません"}})),
            Ok(Value::Object(m)) => handle(&m, &tools, base),
            Ok(_) => Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": "要求は JSON のオブジェクトで送ります"}})),
        };
        if let Some(r) = reply {
            writeln!(out, "{}", r).map_err(|e| e.to_string())?;
            out.flush().map_err(|e| e.to_string())?;
        }
    }
}
