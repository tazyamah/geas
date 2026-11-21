// SPDX-License-Identifier: GPL-3.0-or-later
//! 生成した Rust を実際に組んで動かす通しの確認。cargo（と、サーバーの確認では依存の crate を取れる通信）が要る。
//!
//! - 計画書の具体例を、生成したコードで動かして、計画の期待と合うこと（geas compare）。依存の無い小さな crate なので、そのまま走る。
//! - 生成したサーバー（--host --runtime）を組んで起動し、口が答えること。組むのに時間がかかるので、`cargo test -- --ignored` で走らせる。
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_geas");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn fixture(dir: &str, name: &str) -> (PathBuf, PathBuf) {
    let d = root().join("tests").join("fixtures").join(dir);
    (d.join(format!("{}.geas", name)), d.join(format!("{}_plan.md", name)))
}

/// 具体例を動かした結果が、計画の期待と食い違わないこと（計画とコードの静的な差は、このテストの対象ではない）
fn compare(dir: &str, name: &str) {
    let (code, plan) = fixture(dir, name);
    let r = Command::new(BIN).env("GEAS_LANG", "ja").arg("compare").arg(&code).arg(&plan).output().unwrap();
    let out = String::from_utf8_lossy(&r.stdout);
    assert!(!out.contains("具体例の不一致"), "{} の具体例が計画と合っていません:\n{}\n{}", name, out, String::from_utf8_lossy(&r.stderr));
    assert!(out.contains("突合の結果"), "{}\n{}", out, String::from_utf8_lossy(&r.stderr));
}

#[test]
fn お店の具体例が生成したコードで計画どおりに動く() {
    compare("overseas_shop", "overseas_shop");
}

#[test]
fn 小さなお店の具体例が生成したコードで計画どおりに動く() {
    compare("shop", "shop");
}

#[test]
fn 常駐のデモの具体例が生成したコードで計画どおりに動く() {
    compare("runtime_demo", "runtime_demo");
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn get(port: u16, path: &str) -> Option<String> {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(2))).ok()?;
    write!(s, "GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n", path).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    Some(out)
}

#[test]
#[ignore = "サーバーを組むので遅い（cargo test -- --ignored）"]
fn 生成したサーバーが起動して口が答える() {
    let (code, plan) = fixture("runtime_demo", "runtime_demo");
    let tmp = std::env::temp_dir().join(format!("geas-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let r = Command::new(BIN).env("GEAS_LANG", "ja").arg("rust").arg(&code).arg("-o").arg(&tmp).args(["--host", "--runtime", "--plan"]).arg(&plan).output().unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let b = Command::new("cargo").args(["build", "--release", "--quiet"]).current_dir(tmp.join("server")).output().unwrap();
    assert!(b.status.success(), "{}", String::from_utf8_lossy(&b.stderr));
    let port = free_port();
    let mut srv = Command::new(tmp.join("server").join("target").join("release").join("geas_server"))
        .args(["--port", &port.to_string(), "--db"])
        .arg(tmp.join("e2e.sqlite"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut ok = false;
    for _ in 0..50 {
        if get(port, "/api/health").map_or(false, |r| r.starts_with("HTTP/1.1 200")) {
            ok = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let _ = srv.kill();
    let _ = srv.wait();
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(ok, "生成したサーバーが /api/health に答えません");
}
