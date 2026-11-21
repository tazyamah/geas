// SPDX-License-Identifier: GPL-3.0-or-later
//! container・infra vm_docker の宣言：検査の通り・断り、生成するファイルの中身
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_geas");

fn base() -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("tests").join("fixtures").join("runtime_demo").join("runtime_demo.geas");
    std::fs::read_to_string(p).unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("geas-container-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(args: &[&str]) -> (i32, String) {
    let r = Command::new(BIN).args(args).output().unwrap();
    (r.status.code().unwrap_or(-1), format!("{}{}", String::from_utf8_lossy(&r.stdout), String::from_utf8_lossy(&r.stderr)))
}

const OK: &str = "\ncontainer {\n  port 8080\n  data \"/var/lib/app\"\n}\ninfra vm_docker \"小包\" {\n  host_port 443 via proxy\n  volume host \"/srv/parcel/data\"\n  host_name \"parcel.example.com\"\n}\n";

fn check(name: &str, tail: &str) -> (i32, String) {
    let d = tmp(name);
    let f = d.join("a.geas");
    std::fs::write(&f, base() + tail).unwrap();
    let r = run(&["check", f.to_str().unwrap()]);
    let _ = std::fs::remove_dir_all(&d);
    r
}

#[test]
fn 正しい宣言は検査を通る() {
    let (c, out) = check("ok", OK);
    assert_eq!(c, 0, "{}", out);
}

#[test]
fn infraだけでcontainerが無いと断る() {
    let (c, out) = check("noc", "\ninfra vm_docker \"x\" {\n  host_port 443 via proxy\n  volume host \"/srv/x\"\n}\n");
    assert_eq!(c, 1, "{}", out);
    assert!(out.contains("infra には container"), "{}", out);
}

#[test]
fn ホスト側のボリュームが無いと断る() {
    let (c, out) = check("novol", &OK.replace("  volume host \"/srv/parcel/data\"\n", ""));
    assert_eq!(c, 1, "{}", out);
    assert!(out.contains("volume host"), "{}", out);
}

#[test]
fn 相対パスとドットドットは断る() {
    let (c, out) = check("rel", &OK.replace("/var/lib/app", "var/lib/app"));
    assert_eq!(c, 1, "{}", out);
    let (c, out) = check("dd", &OK.replace("/srv/parcel/data", "/srv/../etc"));
    assert_eq!(c, 1, "{}", out);
}

#[test]
fn 宣言していないシークレットをcontainerが受けると断る() {
    let (c, out) = check("sec", &OK.replace("port 8080\n", "port 8080\n  secret Payment.api_key\n"));
    assert_eq!(c, 1, "{}", out);
    assert!(out.contains("needs secret"), "{}", out);
}

#[test]
fn 生成するファイルは宣言どおり() {
    let d = tmp("gen");
    let f = d.join("a.geas");
    std::fs::write(&f, base() + OK).unwrap();
    let out = d.join("out");
    let (c, msg) = run(&["container", f.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(c, 0, "{}", msg);
    let docker = std::fs::read_to_string(out.join("Dockerfile")).unwrap();
    assert!(docker.contains("\"--port\", \"8080\"") && docker.contains("\"--db\", \"/var/lib/app/app.sqlite\"") && docker.contains("\"--allow-host\", \"parcel.example.com\""), "{}", docker);
    let compose = std::fs::read_to_string(out.join("compose.yaml")).unwrap();
    assert!(compose.contains("\"127.0.0.1:8080:8080\"") && compose.contains("\"/srv/parcel/data:/var/lib/app\"") && compose.contains("read_only: true"), "{}", compose);
    assert!(!out.join("secrets.env.example").exists());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn 仕様書にコンテナと基盤の表が出る() {
    let d = tmp("spec");
    let f = d.join("a.geas");
    std::fs::write(&f, base() + OK).unwrap();
    let (c, out) = run(&["spec", f.to_str().unwrap()]);
    assert_eq!(c, 0, "{}", out);
    assert!(out.contains("## コンテナと基盤") && out.contains("parcel.example.com"), "{}", out);
    assert!(out.contains("```mermaid\nflowchart LR") && out.contains("subgraph host") && out.contains("proxy -->|\"8080\"| app"), "{}", out);
    // レビュー画面には SVG の構成図が出る
    let html = d.join("r.html");
    let (c, msg) = run(&["review", f.to_str().unwrap(), "-o", html.to_str().unwrap()]);
    assert_eq!(c, 0, "{}", msg);
    let h = std::fs::read_to_string(&html).unwrap();
    assert!(h.contains("aria-label=\"構成図\"") && h.contains("/srv/parcel/data"), "レビュー画面に構成図が出ていません");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn doctorは宣言した口とボリュームと環境ファイルと応答を確かめる() {
    use std::io::{Read, Write};
    use std::os::unix::fs::PermissionsExt;
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for st in l.incoming() {
            let Ok(mut st) = st else { break };
            let mut b = [0u8; 512];
            let _ = st.read(&mut b);
            let _ = st.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    });
    let d = tmp("doctor");
    let vol = d.join("data");
    std::fs::create_dir_all(&vol).unwrap();
    std::fs::set_permissions(&vol, std::fs::Permissions::from_mode(0o777)).unwrap();
    let envf = d.join("secrets.env");
    std::fs::write(&envf, "GEAS_SECRET_PAY_API_KEY=abc\n").unwrap();
    std::fs::set_permissions(&envf, std::fs::Permissions::from_mode(0o600)).unwrap();
    let src = base()
        + "\nneeds secret Pay.api_key \"決済の鍵\"\n"
        + &format!(
            "container {{\n  port {}\n  data \"/var/lib/app\"\n  secret Pay.api_key\n}}\ninfra vm_docker \"小包\" {{\n  host_port 443 via proxy\n  volume host \"{}\"\n  secrets_from env_file \"{}\"\n}}\n",
            port,
            vol.display(),
            envf.display()
        );
    let f = d.join("a.geas");
    std::fs::write(&f, src).unwrap();
    let (_, out) = run(&["doctor", f.to_str().unwrap()]);
    assert!(out.contains("[合格] declared.port"), "{}", out);
    assert!(out.contains("[合格] declared.health"), "{}", out);
    if out.contains("ホスト（コンテナの外）") {
        assert!(out.contains("[合格] declared.volume"), "{}", out);
        assert!(out.contains("[合格] declared.env_file"), "{}", out);
        assert!(out.contains("[合格] secret.Pay.api_key"), "{}", out);
        // 環境ファイルが誰にでも読めると不合格
        std::fs::set_permissions(&envf, std::fs::Permissions::from_mode(0o644)).unwrap();
        let (_, out2) = run(&["doctor", f.to_str().unwrap()]);
        assert!(out2.contains("[不合格] declared.env_file"), "{}", out2);
        // 値のないシークレットは不合格（値は出力に出ない）
        std::fs::write(&envf, "GEAS_SECRET_PAY_API_KEY=\n").unwrap();
        let (_, out3) = run(&["doctor", f.to_str().unwrap()]);
        assert!(out3.contains("[不合格] secret.Pay.api_key"), "{}", out3);
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn doctorは待ち受けていない口を不合格にする() {
    let d = tmp("doctor-down");
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    let f = d.join("a.geas");
    std::fs::write(&f, base() + &format!("\ncontainer {{\n  port {}\n  data \"/var/lib/app\"\n}}\ninfra vm_docker \"小包\" {{\n  host_port 443 via proxy\n  volume host \"{}\"\n}}\n", port, d.display())).unwrap();
    let (code, out) = run(&["doctor", f.to_str().unwrap()]);
    assert_eq!(code, 1, "{}", out);
    assert!(out.contains("[不合格] declared.port"), "{}", out);
    let _ = std::fs::remove_dir_all(&d);
}
