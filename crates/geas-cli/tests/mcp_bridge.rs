// SPDX-License-Identifier: GPL-3.0-or-later
//! MCP の橋渡し（geas mcp-bridge）：標準入出力の JSON-RPC を、動いているサーバーの REST に中継する。
//! 中継先は、この試験の中で立てる模擬のサーバー（受けた要求をそのまま返す）。
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_geas");

/// 受けた要求の先頭行・If-Match・本文を、JSON で返すだけのサーバー（ETag: "v1" も付ける）
fn mock() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let mut s = s.unwrap();
            let mut buf = vec![];
            let mut chunk = [0u8; 4096];
            loop {
                let n = s.read(&mut chunk).unwrap();
                buf.extend_from_slice(&chunk[..n]);
                if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..p]).into_owned();
                    let cl: usize = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
                    if buf.len() >= p + 4 + cl {
                        let first = head.lines().next().unwrap().to_string();
                        let im = head.lines().find_map(|l| l.strip_prefix("If-Match: ")).unwrap_or("").to_string();
                        let body = String::from_utf8_lossy(&buf[p + 4..p + 4 + cl]).into_owned();
                        let code = if first.starts_with("PUT") { "412 Precondition Failed" } else { "200 OK" };
                        let out = json!({"line": first, "if_match": im, "body": body}).to_string();
                        let _ = s.write_all(format!("HTTP/1.1 {}\r\nETag: \"v1\"\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", code, out.len(), out).as_bytes());
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
        }
    });
    port
}

fn bridge(port: u16, lines: &[Value], raw: &[&str]) -> Vec<Value> {
    let dir = std::env::temp_dir().join(format!("geas-bridge-{}-{}", std::process::id(), port));
    std::fs::create_dir_all(&dir).unwrap();
    let tools = json!([
        {"name": "get", "description": "d", "inputSchema": {}, "geas": {"method": "GET", "path": "/api/items/:id", "q": ["name", "n"], "body": [], "etag": true}},
        {"name": "put", "inputSchema": {}, "geas": {"method": "PUT", "path": "/api/items/:id", "body": ["x"], "if_match": true}}
    ]);
    std::fs::write(dir.join("tools.json"), tools.to_string()).unwrap();
    let mut c = Command::new(BIN).arg("mcp-bridge").arg(dir.join("tools.json")).arg(format!("http://127.0.0.1:{}", port)).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let mut input: String = lines.iter().map(|l| l.to_string() + "\n").collect();
    for r in raw {
        input.push_str(r);
        input.push('\n');
    }
    c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let mut out = String::new();
    c.stdout.take().unwrap().read_to_string(&mut out).unwrap();
    c.wait().unwrap();
    let _ = std::fs::remove_dir_all(dir);
    out.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

#[test]
fn ツール定義を返し呼び出しをrestに中継し読めない行でも止まらない() {
    let port = mock();
    let got = bridge(
        port,
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"}),
            json!({"jsonrpc": "2.0", "method": "notifications/x"}), // 通知には返事しない
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "get", "arguments": {"id": "a b", "name": "日本 語", "n": 3}}}),
            json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "put", "arguments": {"id": "1", "x": 5, "if_match": "v1"}}}),
            json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "nope"}}),
            json!({"jsonrpc": "2.0", "id": 6, "method": "zzz"}),
        ],
        &["not json", "[1]"],
    );
    assert_eq!(got.len(), 8); // 通知の 1 件を除く
    assert_eq!(got[0]["result"]["protocolVersion"], "2025-06-18");
    // ツール定義には、geas の中継用の情報を出さない
    assert!(got[1]["result"]["tools"].as_array().unwrap().iter().all(|t| t.get("geas").is_none()));
    // 経路の :id は % で符号化され、絞り込みは q[名前]=値 になる。1 件の読み出しの ETag は本文の前に付く
    let t = got[2]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(t.starts_with("HTTP 200\nETag: \"v1\"\n"), "{}", t);
    assert!(t.contains("/api/items/a%20b?q%5Bname%5D=%E6%97%A5%E6%9C%AC+%E8%AA%9E&q%5Bn%5D=3"), "{}", t);
    // 書き込みの版は If-Match に引用符つきで載り、400 以上は isError
    assert_eq!(got[3]["result"]["isError"], true);
    let t = got[3]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(t.starts_with("HTTP 412") && t.contains("\\\"v1\\\"") && t.contains("{\\\"x\\\":5}"), "{}", t);
    assert_eq!(got[4]["error"]["code"], -32602);
    assert_eq!(got[5]["error"]["code"], -32601);
    assert_eq!(got[6]["error"]["code"], -32700); // JSON でない行
    assert_eq!(got[7]["error"]["code"], -32600); // オブジェクトでない行
}
