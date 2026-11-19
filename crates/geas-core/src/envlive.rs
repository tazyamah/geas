// SPDX-License-Identifier: GPL-3.0-or-later
//! 動いている環境を外から確かめる（serverspec 的な検査）。
//!
//! 仕様書の「ルート」の表から、読むだけの問い合わせ（GET）を作り、動いているサーバーに送って、
//! 答えの HTTP の状態が仕様どおりかを見る。判定は env-verify と同じ 4 つ（合格・不合格・確かめられなかった・人の保証）。
//! 届かない・読めないものは、合格にしない（確かめられなかった）。書き込みは一切送らない。
use crate::ast::Program;
use crate::envcheck::{Res, FAIL, PASS, UNKNOWN};
use crate::rest;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// 1 つの問い合わせと、仕様が許す答えの状態
#[derive(Debug, Clone)]
pub struct Probe {
    pub id: String,
    pub text: String,
    pub path: String,
    pub expect: Vec<u16>,
}

/// 実在しないはずの ID（show の「無い」を確かめる）
pub const MISSING_ID: &str = "geas-probe-none";
pub const UNDECLARED: &str = "/__geas_undeclared";

fn fill(path: &str) -> String {
    path.split('/').map(|s| if s.starts_with(':') { MISSING_ID } else { s }).collect::<Vec<_>>().join("/")
}

/// 仕様から、読むだけの問い合わせの一覧を作る
pub fn probes(p: &Program) -> Vec<Probe> {
    let inner = rest::internal_targets(p);
    // 役割を宣言したアプリは、ヘッダが無い呼び出しを 401（役割に合わなければ 403）で断る
    let guarded: &[u16] = if p.actors.is_empty() { &[] } else { &[401, 403] };
    let mut out: Vec<Probe> = vec![];
    for r in rest::routes(p) {
        if r.method != "GET" && !inner.contains(&r.target) {
            continue; // 外から呼べる書き込みは、送らない
        }
        let path = fill(&r.path);
        if inner.contains(&r.target) {
            out.push(Probe {
                id: format!("url.GET {}", r.path),
                text: format!("内部の口 {}（{}）は、外に出ていない", r.path, r.target),
                path,
                expect: vec![404, 405],
            });
        } else if r.path.contains(':') {
            let mut e = vec![404, 400, 422];
            // 入れ子の一覧は、親の ID が無くても「空の一覧」を 200 で返す（親は絞り込みの条件）
            let text = if r.kind == "index" {
                e.push(200);
                format!("{}（{}）が、無い ID でも壊れずに答える", r.path, r.target)
            } else {
                format!("{}（{}）は、無い ID に「無い」と答える", r.path, r.target)
            };
            e.extend_from_slice(guarded);
            out.push(Probe { id: format!("url.GET {}", r.path), text, path, expect: e });
        } else {
            let mut e = vec![200];
            e.extend_from_slice(guarded);
            out.push(Probe { id: format!("url.GET {}", r.path), text: format!("{}（{}）が答える", r.path, r.target), path, expect: e });
        }
    }
    out.push(Probe { id: "url.undeclared".into(), text: "宣言していない URL は、404 を返す".into(), path: UNDECLARED.into(), expect: vec![404] });
    out
}

/// 送る側。base（http://ホスト:口 など）に path を足して GET し、HTTP の状態を返す
pub type Fetch<'a> = &'a dyn Fn(&str, &str) -> Result<u16, String>;

pub fn run(probes: &[Probe], base: &str, fetch: Fetch) -> Vec<Res> {
    let base = base.trim_end_matches('/');
    probes
        .iter()
        .map(|pb| {
            let mk = |status: &'static str, detail: String| Res { id: pb.id.clone(), aspect: "url".into(), text: pb.text.clone(), status, detail };
            let want = pb.expect.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("・");
            match fetch(base, &pb.path) {
                Err(e) => mk(UNKNOWN, format!("届かなかった：{}", e)),
                Ok(s) if pb.expect.contains(&s) => mk(PASS, format!("GET {} → {}（仕様が許す状態：{}）", pb.path, s, want)),
                Ok(s) => mk(FAIL, format!("GET {} → {}（仕様が許す状態：{}）", pb.path, s, want)),
            }
        })
        .collect()
}

/// 標準の送り方。http:// は自前、https:// は curl（TLS の部品を持ち込まないため）
pub fn fetch_default(base: &str, path: &str) -> Result<u16, String> {
    if let Some(rest) = base.strip_prefix("http://") {
        return http_get(rest, path);
    }
    if base.starts_with("https://") {
        let out = std::process::Command::new("curl")
            .args(["-sS", "--max-time", "10", "-o", "/dev/null", "-w", "%{http_code}"])
            .arg(format!("{}{}", base, path))
            .output()
            .map_err(|e| format!("https には curl が要る（{}）", e))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        return String::from_utf8_lossy(&out.stdout).trim().parse::<u16>().map_err(|e| e.to_string());
    }
    Err("http:// か https:// で始まる URL を渡してください".into())
}

fn http_get(hostport: &str, path: &str) -> Result<u16, String> {
    let hostport = hostport.trim_end_matches('/');
    let addr = if hostport.contains(':') { hostport.to_string() } else { format!("{}:80", hostport) };
    let sa = addr.to_socket_addrs().map_err(|e| format!("{}（{}）", e, addr))?.next().ok_or_else(|| format!("名前を引けない：{}", addr))?;
    let mut s = TcpStream::connect_timeout(&sa, Duration::from_secs(5)).map_err(|e| format!("{}（{}）", e, addr))?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    write!(s, "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: geas-env-live\r\n\r\n", path, hostport).map_err(|e| e.to_string())?;
    let mut buf = vec![];
    let mut chunk = [0u8; 1024];
    // 状態行だけあればよい。ヘッダの終わりまで読んで打ち切る
    loop {
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 65536 {
                    break;
                }
            }
            Err(e) => {
                if buf.is_empty() {
                    return Err(e.to_string());
                }
                break;
            }
        }
    }
    let head = String::from_utf8_lossy(&buf);
    let line = head.lines().next().unwrap_or("");
    let mut it = line.split_whitespace();
    match (it.next(), it.next().and_then(|c| c.parse::<u16>().ok())) {
        (Some(v), Some(code)) if v.starts_with("HTTP/") => Ok(code),
        _ => Err(format!("HTTP の答えではない：{:?}", line)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pb(expect: &[u16]) -> Probe {
        Probe { id: "url.GET /x".into(), text: "t".into(), path: "/x".into(), expect: expect.to_vec() }
    }

    #[test]
    fn 仕様が許す状態なら合格_外れたら不合格() {
        let ok = run(&[pb(&[200])], "http://h", &|_, _| Ok(200));
        assert_eq!(ok[0].status, PASS);
        let ng = run(&[pb(&[200])], "http://h", &|_, _| Ok(500));
        assert_eq!(ng[0].status, FAIL);
    }

    #[test]
    fn 届かないものは合格にしない() {
        let r = run(&[pb(&[200])], "http://h", &|_, _| Err("refused".into()));
        assert_eq!(r[0].status, UNKNOWN);
    }

    #[test]
    fn 末尾のスラッシュは除いて送る() {
        let seen = std::cell::RefCell::new(String::new());
        run(&[pb(&[200])], "http://h/", &|b, p| {
            *seen.borrow_mut() = format!("{}{}", b, p);
            Ok(200)
        });
        assert_eq!(*seen.borrow(), "http://h/x");
    }

    #[test]
    fn 自前の送り方が状態行を読める() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for st in l.incoming().take(1) {
                let mut st = st.unwrap();
                let mut b = [0u8; 512];
                let _ = st.read(&mut b);
                let _ = st.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
            }
        });
        assert_eq!(fetch_default(&format!("http://127.0.0.1:{}", port), "/a"), Ok(404));
    }
}
