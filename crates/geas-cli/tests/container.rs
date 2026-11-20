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
    let _ = std::fs::remove_dir_all(&d);
}
