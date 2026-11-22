// SPDX-License-Identifier: GPL-3.0-or-later
//! 実行時にユーザーが入力するパス（ltype 名前 = path）：実行中の OS の書式だけを受け、展開は OS の「場所の別名」
//! （mac・Linux は先頭の ~、Windows は %名前%）だけ。$名前 は展開しない。絶対パスで、.. を含まないこと。失敗は入力不正。
//! 元は Python 版の tests/test_user_path.py（タグ python-final）。
mod common;
use common::*;
use geas_core::check::ERROR;
use geas_core::rustgen::{generate, OS_PATH_RS};
use std::process::Command;

const SRC: &str = "ltype ExportDir = path
entity Export {
  export_id: id unique
  dir: ExportDir required
}
result PutResult {
  Put(export_id: id)
}
deed put_export {
  input dir: ExportDir required
  writes Export
  result PutResult
  effects none

  let e = create Export { dir: dir }
  return Put(e.export_id)
}
nexus Flow {
  put_export: Put → reply
}
";

fn chk<R>(src: &str, f: impl FnOnce(&geas_core::check::Checker) -> R) -> R {
    with_check(src, false, f)
}

#[test]
fn path型は入力と項目に使え変換は言語が足す() {
    chk(SRC, |ck| {
        assert!(ck.ready() && !ck.diags.iter().any(|d| d.level == ERROR), "{:?}", ck.diags);
        let out = generate(ck);
        assert!(out.contains("pub fn export_dir_os_path(r: &str) -> Result<String, String>") && out.contains("fn os_path_expand("));
        assert!(out.contains("export_dir_os_path(&self.dir)")); // 入力の取り込みで、型の変換が働く
    });
}

#[test]
fn os_pathはrawから変換するconvertの中でだけ使える() {
    let bad = SRC.replace("let e = create Export { dir: dir }", "let e = create Export { dir: os_path(dir) }");
    chk(&bad, |ck| assert!(ck.diags.iter().any(|d| d.level == ERROR && (d.msg.contains("convert") || d.msg.contains("os_path")))));
}

/// HOME を差し替えても cargo が動くよう、rustup と cargo の置き場は元の HOME から固定する
fn cargo_cmd(dir: &std::path::Path) -> Command {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    let mut c = Command::new("cargo");
    c.args(["run", "--quiet", "--offline"]).current_dir(dir);
    if std::env::var_os("RUSTUP_HOME").is_none() {
        c.env("RUSTUP_HOME", format!("{}/.rustup", home));
    }
    if std::env::var_os("CARGO_HOME").is_none() {
        c.env("CARGO_HOME", format!("{}/.cargo", home));
    }
    c
}

#[test]
fn パスは実行中のosの書式で展開され不正なら入力不正() {
    let t = Tmp::new("userpath");
    let main = r##"use geas_app::host::call;
fn main() {
    for v in ["/srv/out", "~/out", "~", "$HOME/out", "out/dir", "/a/../b", "~bob/x", "  ", "/tmp/x\ny"] {
        let esc = v.replace('"', "\\\"").replace('\n', "\\n");
        let req = format!(r#"{{"name":"put_export","ctx":{{"new_Export_id":"A"}},"input":{{"dir":"{}"}},"store":{{}}}}"#, esc);
        println!("{:?}", call(&req));
    }
}
"##;
    build_crate(SRC, main, &t.0, true);
    let r = cargo_cmd(&t.0).env("HOME", "/home/tester").output().unwrap();
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 2500));
    let out = String::from_utf8_lossy(&r.stdout).to_string();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 9, "{}", out);
    let (ok, home_slash, home, dollar, rel, dotdot, user, blank, newline) = (lines[0], lines[1], lines[2], lines[3], lines[4], lines[5], lines[6], lines[7], lines[8]);
    assert!(ok.starts_with("Ok(") && ok.contains("/srv/out"), "{}", ok);
    // 先頭の ~ は展開できると確かめる。値は入力のまま（展開は、パスを使う口の仕事）
    assert!(home_slash.starts_with("Ok(") && home_slash.contains("\\\"dir\\\":\\\"~/out\\\""), "{}", home_slash);
    assert!(home.starts_with("Ok("), "{}", home);
    assert!(dollar.starts_with("Err(") && dollar.contains("絶対パス"), "{}", dollar); // $名前 は展開しない（文字どおり。絶対パスでもない）
    assert!(rel.starts_with("Err(") && rel.contains("絶対パス"), "{}", rel);
    assert!(dotdot.starts_with("Err(") && dotdot.contains(".."), "{}", dotdot);
    assert!(user.starts_with("Err(") && user.contains("~ユーザー名"), "{}", user);
    assert!(blank.starts_with("Err("), "{}", blank);
    assert!(newline.starts_with("Err(") && newline.contains("改行"), "{}", newline);
    // HOME が無ければ、~ は展開できないので、入力不正（黙って別の場所にしない）
    let r = cargo_cmd(&t.0).env_remove("HOME").output().unwrap();
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 2500));
    let out = String::from_utf8_lossy(&r.stdout).to_string();
    let no_home: Vec<&str> = out.lines().collect();
    assert!(no_home[0].starts_with("Ok(") && no_home[1].starts_with("Err(") && no_home[1].contains("HOME") && no_home[2].starts_with("Err("), "{:?}", no_home);
}

#[test]
fn windowsの書式の分岐もlinuxで動かして確かめる() {
    // cfg!(windows) を true に置き換えた同じ関数を、Linux で動かす（Windows 実機が無くても、展開と書式の判定を確かめる）
    let t = Tmp::new("winpath");
    let main = OS_PATH_RS.replace("cfg!(windows)", "true")
        + r####"
fn main() {
    for v in [r"C:\Users\x", "C:/Users/x", r"%APPDATA%\out", r"\\srv\share\x", r"%NOPE%\x", r"%APPDATA\x", "relative", r"D:\a\..\b", r"C:\a?b", "$HOME/x", "~/x", r"C:\a:b", "/usr/x"] {
        println!("{:?}", os_path_expand(v));
    }
}
"####;
    std::fs::write(t.0.join("main.rs"), main).unwrap();
    let r = Command::new("rustc").args(["-O", "--edition", "2021"]).arg(t.0.join("main.rs")).arg("-o").arg(t.0.join("t")).output().unwrap();
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 2000));
    let r = Command::new(t.0.join("t")).env("APPDATA", r"C:\Users\me\AppData\Roaming").output().unwrap();
    let out = String::from_utf8_lossy(&r.stdout).to_string();
    let got: Vec<&str> = out.lines().collect();
    assert!(got[0].starts_with("Ok(") && got[1].starts_with("Ok(") && got[3].starts_with("Ok("), "{:?}", got);
    assert_eq!(got[2], r#"Ok("C:\\Users\\me\\AppData\\Roaming\\out")"#); // %名前% は展開する
    assert!(got[4].contains("NOPE が設定されていません") && got[5].contains("閉じていません")); // 未定義・閉じていない % は失敗
    for i in [6, 9, 10, 12] {
        assert!(got[i].starts_with("Err(") && got[i].contains("絶対パス"), "{}", got[i]); // 相対・$名前・~ は、Windows では展開せず、絶対パスでもない
    }
    assert!(got[7].contains(".. は使えません") && got[8].contains("使えない文字") && got[11].contains("使えない文字"), "{:?}", got);
}
