// SPDX-License-Identifier: GPL-3.0-or-later
//! 言語機能ごとの動作テストの共通部分：小さな .geas から crate を生成して実際に動かす。
//! 生成物は依存の無い crate なので、`cargo run --offline` で組める。
#![allow(dead_code)]
use geas_core::check::{defs::check, ERROR, QUESTION};
use geas_core::crategen::write_crate;
use geas_core::parser::parse;
use geas_core::rustgen::generate;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

/// テストごとの作業フォルダ（終わったら消す）
pub struct Tmp(pub PathBuf);

impl Tmp {
    pub fn new(tag: &str) -> Tmp {
        let d = std::env::temp_dir().join(format!("geas-lang-{}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst), tag));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Tmp(d)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

pub fn fixture(rel: &str) -> String {
    std::fs::read_to_string(root().join("tests").join("fixtures").join(rel)).unwrap()
}

/// 検査の診断を「レベル: 内容」の行にする
pub fn diags(src: &str) -> Vec<(String, String)> {
    let mut prog = parse(src, None).unwrap_or_else(|e| panic!("構文の誤り: {:?}", e));
    let ck = check(&mut prog, true);
    ck.diags.iter().map(|d| (d.level.clone(), d.msg.clone())).collect()
}

pub fn msgs(src: &str, level: &str) -> Vec<String> {
    diags(src).into_iter().filter(|(l, _)| l == level).map(|(_, m)| m).collect()
}

pub fn errors(src: &str) -> Vec<String> {
    msgs(src, ERROR)
}

pub fn questions(src: &str) -> Vec<String> {
    msgs(src, QUESTION)
}

/// 検査を通った .geas から crate を作る（main は src/bin/plan_examples.rs）
pub fn build_crate(src: &str, main: &str, dir: &Path, host: bool) {
    let mut prog = parse(src, None).unwrap_or_else(|e| panic!("構文の誤り: {:?}", e));
    let ck = check(&mut prog, false);
    assert!(ck.ready(), "検査が通りません: {:?}", ck.diags.iter().map(|d| format!("{}: {}", d.level, d.msg)).collect::<Vec<_>>());
    write_crate(&ck, &generate(&ck), main, dir, host).unwrap();
}

/// crate を作って main を動かし、標準出力の行を返す
pub fn run_app(src: &str, main: &str) -> Vec<String> {
    let t = Tmp::new("run");
    build_crate(src, main, &t.0, false);
    let r = Command::new("cargo").args(["run", "--quiet", "--offline"]).current_dir(&t.0).output().unwrap();
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 2500));
    String::from_utf8_lossy(&r.stdout).lines().map(|s| s.to_string()).collect()
}

/// crate を作って組むだけ（組めることの確認）
pub fn build_only(src: &str, main: &str) {
    let t = Tmp::new("build");
    build_crate(src, main, &t.0, false);
    let r = Command::new("cargo").args(["build", "--quiet", "--offline"]).current_dir(&t.0).output().unwrap();
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 2500));
}

pub fn tail(s: &str, n: usize) -> String {
    let cs: Vec<char> = s.chars().collect();
    cs[cs.len().saturating_sub(n)..].iter().collect()
}

pub fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// host（境界の口 `host::call`）つきで crate を作って動かす
pub fn run_app_host(src: &str, main: &str) -> Vec<String> {
    let t = Tmp::new("host");
    build_crate(src, main, &t.0, true);
    let r = Command::new("cargo").args(["run", "--quiet", "--offline"]).current_dir(&t.0).output().unwrap();
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 2500));
    String::from_utf8_lossy(&r.stdout).lines().map(|s| s.to_string()).collect()
}

/// host::call を要求ごとに 1 回呼び、戻り値の `{:?}`（Ok(…)／Err(…)）を 1 行ずつ返す
pub fn host_calls(src: &str, requests: &[String]) -> Vec<String> {
    let mut main = String::from("fn main() {\n");
    for r in requests {
        main.push_str(&format!("    println!(\"{{:?}}\", geas_app::host::call(r#\"{}\"#));\n", r));
    }
    main.push_str("}\n");
    run_app_host(src, &main)
}

/// 絶対のパスを付けて検査する（use の相対参照・標準モジュールの探索のため）
pub fn check_at(src: &str, path: &Path) -> Vec<(String, String)> {
    let mut prog = parse(src, Some(path)).unwrap_or_else(|e| panic!("構文の誤り: {:?}", e));
    let ck = check(&mut prog, true);
    ck.diags.iter().map(|d| (d.level.clone(), d.msg.clone())).collect()
}

/// モジュールを使うコードから crate を作る
pub fn build_crate_at(src: &str, path: &Path, main: &str, dir: &Path, host: bool) {
    let mut prog = parse(src, Some(path)).unwrap_or_else(|e| panic!("構文の誤り: {:?}", e));
    let ck = check(&mut prog, false);
    assert!(ck.ready(), "検査が通りません: {:?}", ck.diags.iter().map(|d| format!("{}: {}", d.level, d.msg)).collect::<Vec<_>>());
    write_crate(&ck, &generate(&ck), main, dir, host).unwrap();
}

pub fn cargo_run(dir: &Path, release: bool) -> Vec<String> {
    let mut args = vec!["run", "--quiet", "--offline"];
    if release {
        args.push("--release");
    }
    let r = Command::new("cargo").args(args).current_dir(dir).output().unwrap();
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 2500));
    String::from_utf8_lossy(&r.stdout).lines().map(|s| s.to_string()).collect()
}

/// 構文の誤りの文言（構文が通れば None）
pub fn parse_err(src: &str) -> Option<String> {
    parse(src, None).err().map(|e| format!("{}", e))
}

/// 検査して、結果を closure に渡す（Checker はプログラムを借りるので、この形にする。名前の厳密な検査は strict）
pub fn with_check<R>(src: &str, strict: bool, f: impl FnOnce(&geas_core::check::Checker) -> R) -> R {
    let mut prog = parse(src, None).unwrap_or_else(|e| panic!("構文の誤り: {}", e));
    let ck = check(&mut prog, strict);
    f(&ck)
}

/// 厳密でない検査のエラーの文言
pub fn errors_loose(src: &str) -> Vec<String> {
    with_check(src, false, |ck| ck.diags.iter().filter(|d| d.level == ERROR).map(|d| d.msg.clone()).collect())
}

pub mod serve;

/// 検査して closure に渡す。path があれば、use の相対参照・標準モジュールの探索はそのファイルの隣から行う
pub fn with_check_at<R>(src: &str, path: Option<&Path>, strict: bool, f: impl FnOnce(&geas_core::check::Checker) -> R) -> R {
    let mut prog = parse(src, path).unwrap_or_else(|e| panic!("構文の誤り: {}", e));
    let ck = check(&mut prog, strict);
    f(&ck)
}

/// old が 1 回だけ置き換わる（無ければ失敗）
pub fn mutate(src: &str, old: &str, new: &str) -> String {
    assert!(src.contains(old), "{}", old);
    src.replacen(old, new, 1)
}
