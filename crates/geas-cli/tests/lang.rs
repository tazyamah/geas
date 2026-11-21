// SPDX-License-Identifier: GPL-3.0-or-later
//! 日本語・英語モード：既定は英語、`--lang`・`GEAS_LANG` で日本語にできる。表示名は `@en` を使う。
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_geas");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn run(args: &[&str], env: Option<&str>) -> (String, String, i32) {
    let mut c = Command::new(BIN);
    c.args(args).env_remove("GEAS_LANG");
    if let Some(v) = env {
        c.env("GEAS_LANG", v);
    }
    let r = c.output().unwrap();
    (String::from_utf8_lossy(&r.stdout).into_owned(), String::from_utf8_lossy(&r.stderr).into_owned(), r.status.code().unwrap_or(-1))
}

fn shop() -> String {
    root().join("tests").join("fixtures").join("shop").join("shop.geas").to_string_lossy().into_owned()
}

fn has_ja(s: &str) -> bool {
    s.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c) || ('\u{4e00}'..='\u{9fff}').contains(&c))
}

#[test]
fn 既定は英語() {
    let (o, _, c) = run(&["check", &shop()], None);
    assert_eq!(c, 0);
    assert!(o.starts_with("Check: passed"), "{}", o);
}

#[test]
fn lang_と環境変数で日本語にできる() {
    let (o, _, _) = run(&["check", &shop(), "--lang", "ja"], None);
    assert!(o.starts_with("検査: 通りました"), "{}", o);
    let (o, _, _) = run(&["check", &shop()], Some("ja"));
    assert!(o.starts_with("検査: 通りました"), "{}", o);
    // --lang が環境変数より優先
    let (o, _, _) = run(&["--lang=en", "check", &shop()], Some("ja"));
    assert!(o.starts_with("Check: passed"), "{}", o);
}

#[test]
fn 知らない言語は断る() {
    let (_, e, c) = run(&["check", &shop(), "--lang", "fr"], None);
    assert_eq!(c, 2);
    assert!(e.contains("invalid choice"), "{}", e);
    let (_, e, c) = run(&["check", &shop()], Some("fr"));
    assert_eq!(c, 2);
    assert!(e.contains("GEAS_LANG"), "{}", e);
}

#[test]
fn 検査の指摘と構文エラーも英語() {
    let dir = std::env::temp_dir().join(format!("geas-lang-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("bad.geas");
    std::fs::write(&f, "app \"x\"\nentity A {\n  id: id unique\n  n: int 1..99\n}\n").unwrap();
    let (o, e, c) = run(&["check", &f.to_string_lossy()], None);
    let o = format!("{}{}", o, e);
    assert_eq!(c, 2);
    assert!(o.contains("Syntax error") && o.contains("Give ranges and combinations a name"), "{}", o);
    assert!(!has_ja(&o.replace("n: int 1..99", "")), "{}", o);
    let (j, _, _) = run(&["check", &f.to_string_lossy(), "--json"], None);
    assert!(j.contains("\"code\": \"syntax\""), "{}", j);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 仕様書は英語で出て_en_の表示名を使う() {
    let dir = std::env::temp_dir().join(format!("geas-lang-spec-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = std::fs::read_to_string(shop()).unwrap();
    let f = dir.join("shop.geas");
    std::fs::write(&f, src.replacen("/// 在庫数\n", "/// 在庫数\n/// @en Stock count\n", 1)).unwrap();
    let out = dir.join("spec.md");
    let (_, _, c) = run(&["spec", &f.to_string_lossy(), "-o", &out.to_string_lossy()], None);
    assert_eq!(c, 0);
    let en = std::fs::read_to_string(&out).unwrap();
    assert!(en.contains("## Data") && en.contains("## Deeds"), "{}", &en[..400.min(en.len())]);
    assert!(!en.contains("## データ"));
    assert!(en.contains("Stock count"));
    let (_, _, c) = run(&["spec", &f.to_string_lossy(), "-o", &out.to_string_lossy()], Some("ja"));
    assert_eq!(c, 0);
    let ja = std::fs::read_to_string(&out).unwrap();
    assert!(ja.contains("## データ") && ja.contains("在庫数"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn レビュー画面は_lang_属性を言語に合わせる() {
    let dir = std::env::temp_dir().join(format!("geas-lang-rev-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("r.html");
    run(&["review", &shop(), "-o", &out.to_string_lossy()], None);
    assert!(std::fs::read_to_string(&out).unwrap().contains("<html lang=\"en\">"));
    run(&["review", &shop(), "-o", &out.to_string_lossy()], Some("ja"));
    assert!(std::fs::read_to_string(&out).unwrap().contains("<html lang=\"ja\">"));
    let _ = std::fs::remove_dir_all(&dir);
}
