// SPDX-License-Identifier: GPL-3.0-or-later
//! 外部作用（over file・over http）、常駐（resident）、データ用フォルダ（path・data_folder）の検査と、仕様書・生成物の定義への出方。
//! 元は Python 版の test_file_check.py・test_http_check.py・test_resident_check.py・test_data_folder_check.py（タグ python-final）。
mod common;
use common::*;
use geas_core::hostgen::definitions;
use geas_core::spec::build;
use serde_json::json;

fn mutate_in(src: &str, old: &str, new: &str) -> String {
    assert!(src.contains(old), "{}", old);
    src.replacen(old, new, 1)
}

fn any(v: &[String], needle: &str) -> bool {
    v.iter().any(|m| m.contains(needle))
}

// ---- over file ----
const BLOCK: &str = "  write \"{dir}/note.txt\" from content\n  ok → Done take size from bytes\n  exists → Refused\n  not_found → Refused\n  denied → Refused\n  failed → Refused\n";

fn file_src() -> String {
    fixture("file_demo/file_demo.geas")
}

fn ferrors(old: &str, new: &str) -> Vec<String> {
    errors_loose(&mutate_in(&file_src(), old, new))
}

fn with_block(block: &str) -> String {
    mutate_in(&file_src(), BLOCK, block)
}

#[test]
fn ファイル操作の見本は通る() {
    assert!(ferrors(BLOCK, BLOCK).is_empty());
}

#[test]
fn 操作はwriteとoverwriteとreadとdeleteの4つで1つだけ() {
    assert!(ferrors(BLOCK, &BLOCK.replace("write ", "overwrite ").replace("  exists → Refused\n", "")).is_empty());
    assert!(ferrors(BLOCK, "  delete \"{dir}/note.txt\"\n  ok → Refused\n  not_found → Refused\n  failed → Refused\n").is_empty());
    assert!(ferrors(BLOCK, "  read \"{dir}/note.txt\"\n  ok → Refused\n  failed → Refused\n").is_empty());
    let e = parse_err(&with_block(&(BLOCK.to_string() + "  read \"{dir}/note.txt\"\n"))).expect("構文の誤りのはず");
    assert!(e.contains("1 つだけ"), "{}", e);
    assert!(parse_err(&with_block(&BLOCK.replace("write ", "append "))).is_some());
}

#[test]
fn ファイル操作の場所はpath型の引数と相対パスだけ() {
    assert!(any(&ferrors(BLOCK, &BLOCK.replace("{dir}", "{note_id}")), "実行時に入力されるパスの型"));
    assert!(any(&ferrors(BLOCK, &BLOCK.replace("{dir}", "{nothing}")), "引数にありません"));
    let e = parse_err(&with_block(&BLOCK.replace("\"{dir}/note.txt\"", "\"/etc/passwd\""))).expect("構文の誤りのはず");
    assert!(e.contains("{引数}/ファイル名"), "{}", e);
    for bad in ["../x.txt", "a//b.txt", "a/./b", "a\\\\b", "x:y", "a?b"] {
        assert!(any(&ferrors(BLOCK, &BLOCK.replace("note.txt", bad)), "相対パス"), "{}", bad);
    }
}

#[test]
fn ファイル操作の結果は操作ごとに書けるものが決まりokとfailedは必須() {
    assert!(any(&ferrors(BLOCK, &BLOCK.replace("  failed → Refused\n", "")), "failed → 種類 が要ります"));
    assert!(any(&ferrors(BLOCK, &BLOCK.replace("  ok → Done take size from bytes\n", "")), "ok → 種類 が要ります"));
    assert!(ferrors(BLOCK, &BLOCK.replace("write ", "overwrite ")).iter().any(|m| m.contains("exists") && m.contains("write のときだけ")));
    assert!(ferrors(BLOCK, &BLOCK.replace("→ Done", "→ Nope")).iter().any(|m| m.contains("結果の種類") && m.contains("ありません")));
}

#[test]
fn takeはokだけでcontentはreadだけ中身が要る結果は取り出す() {
    assert!(any(&ferrors(BLOCK, &BLOCK.replace("take size from bytes", "take size from content")), "取り出せるのは"));
    assert!(any(&ferrors(BLOCK, &BLOCK.replace("not_found → Refused", "not_found → Done take size from bytes")), "take は、ok だけ"));
    let e = ferrors(BLOCK, &BLOCK.replace(" take size from bytes", ""));
    assert!(any(&e, "take size") || any(&e, "取り出してください"), "{:?}", e);
    assert!(any(&ferrors(BLOCK, "  delete \"{dir}/note.txt\"\n  ok → Done take size from bytes\n  failed → Refused\n"), "取り出せるのは"));
    let e = parse_err(&mutate_in(&file_src(), "take size from bytes", "take size")).expect("構文の誤りのはず");
    assert!(e.contains("from"), "{}", e);
}

#[test]
fn 書く内容は文字列の引数でrepliesが要る() {
    assert!(any(&ferrors(BLOCK, &BLOCK.replace("from content", "from dir")), "別にしてください"));
    assert!(any(&ferrors(BLOCK, &BLOCK.replace("from content", "from nothing")), "引数にありません"));
    assert!(any(&ferrors("replies ExportSettled over file", "over file"), "replies"));
}

#[test]
fn ファイル操作は生成物の定義に操作と場所と結果が出る() {
    let blocks = [
        ("write", BLOCK.to_string()),
        ("overwrite", BLOCK.replace("write ", "overwrite ").replace("  exists → Refused\n", "")),
        ("delete", "  delete \"{dir}/note.txt\"\n  ok → Refused\n  failed → Refused\n".to_string()),
        ("read", "  read \"{dir}/note.txt\"\n  ok → Refused\n  failed → Refused\n".to_string()),
    ];
    for (op, block) in blocks {
        with_check(&with_block(&block), false, |ck| {
            let d = definitions(ck).unwrap()["effects"]["Files.export"]["file"].clone();
            assert!(d["op"] == op && d["dir"] == "dir" && d["rest"] == "note.txt", "{}", d);
            assert_eq!(d["rows"]["failed"], json!({"outcome": "Refused", "take": {}}));
        });
    }
    with_check(&file_src(), false, |ck| {
        assert_eq!(definitions(ck).unwrap()["effects"]["Files.export"]["file"]["rows"]["ok"], json!({"outcome": "Done", "take": {"size": "bytes"}}));
    });
}

// ---- over http ----
fn tweet() -> String {
    fixture("solo_tweet/solo_tweet.geas")
}

fn herrors(old: &str, new: &str) -> Vec<String> {
    errors_loose(&mutate_in(&tweet(), old, new))
}

#[test]
fn httpの見本は通る() {
    assert!(errors_loose(&tweet()).is_empty());
}

#[test]
fn 通信先はhttpsのホスト名だけ() {
    for url in ["http://api.x.com/2/tweets", "https://127.0.0.1/x", "https://user@api.x.com/x", "https://api.x.com/x?a=1", "ftp://api.x.com/x"] {
        assert!(any(&herrors("https://api.x.com/2/tweets", url), "通信先"), "{}", url);
    }
}

#[test]
fn 認証はneeds_secretで宣言したものだけ同じ外部のものだけ() {
    assert!(any(&herrors("bearer Twitter.token", "bearer Twitter.other"), "宣言していません"));
    assert!(any(&herrors("bearer Twitter.token", "bearer Other.token"), "同じ外部"));
}

#[test]
fn 返事の状態は漏れなく結ぶ() {
    assert!(any(&herrors("  status other → Rejected\n", ""), "status other")); // 書いていない状態の行き先が無い
    assert!(any(&herrors("status 400..499 → Rejected", "status 200..499 → Rejected"), "重なって"));
    assert!(any(&herrors("status 400..499 → Rejected", "status 400..499 → Nope"), "結果の種類「Nope」"));
    assert!(herrors(" take post_id from \"data.id\"", "").iter().any(|m| m.contains("take") && m.contains("post_id"))); // gives の中身を取り出さない
    assert!(any(&herrors("\"data.id\"", "\"data..id\""), "道すじ"));
    assert!(any(&herrors("body \"text\" from content", "body \"text\" from nothing"), "引数にありません"));
    assert!(any(&herrors("body \"text\" from content", "body \"text\" from content, \"text\" from tweet_id"), "重なって"));
    assert!(any(&herrors(" replies PostSettled over http", " over http"), "replies"));
}

// ---- resident ----
#[test]
fn 常駐の見本は通り仕様書に常駐が出る() {
    with_check(&tweet(), false, |ck| {
        assert!(ck.diags.iter().all(|d| d.level != "エラー"));
        let spec = build(ck);
        let rows: std::collections::HashMap<String, String> = spec["常駐"].as_array().unwrap().iter().map(|r| (r[0].as_str().unwrap().to_string(), r[1].as_str().unwrap().to_string())).collect();
        assert_eq!(rows["右クリックのメニュー「つぶやきを書く」"], "画面 TweetList を開く");
        assert!(rows.keys().any(|k| k.contains("終了")));
        assert_eq!(rows["右クリックのメニュー「未投稿をまとめて X へ」"], "出来事 未投稿をまとめて X へ を起こす");
        assert_eq!(rows["左クリック"], "窓を開閉する");
    });
}

#[test]
fn 常駐を書かなければ仕様書に常駐は出ない() {
    let s = tweet();
    let src = format!("{}{}", &s[..s.find("// 常駐").unwrap()], &s[s.find("needs secret").unwrap()..]);
    assert!(!src.contains("resident"));
    with_check(&src, false, |ck| assert!(build(ck)["常駐"].as_array().unwrap().is_empty()));
}

#[test]
fn メニューが開けるのは入口の画面で引数を持たないものだけ() {
    let m = "menu \"つぶやきを書く\" → TweetList";
    assert!(any(&errors_loose(&mutate_in(&tweet(), m, "menu \"つぶやきを書く\" → TweetDetail")), "開けません"));
    assert!(any(&errors_loose(&mutate_in(&tweet(), m, "menu \"つぶやきを書く\" → Nothing")), "がありません"));
}

#[test]
fn 終了の項目は言語が足すラベルは重ねない() {
    let m = "menu \"つぶやきを書く\" → TweetList";
    assert!(any(&errors_loose(&mutate_in(&tweet(), m, "menu \"終了\" → TweetList")), "終了"));
    assert!(any(&errors_loose(&mutate_in(&tweet(), m, "menu \"a\" → TweetList\n  menu \"a\" → TweetList")), "重なって"));
}

#[test]
fn startはshownかhiddenだけresidentは1つだけ() {
    for bad in ["start now", "start"] {
        assert!(parse_err(&mutate_in(&tweet(), "tooltip \"ひとりつぶやき\"", bad)).is_some(), "{}", bad);
    }
    let e = parse_err(&(tweet() + "\nresident {\n}\n")).expect("構文の誤りのはず");
    assert!(e.contains("1 つだけ"), "{}", e);
}

#[test]
fn メニューから起こせる出来事は外から入力を受けないものだけ() {
    assert!(any(&errors_loose(&mutate_in(&tweet(), "fire PostSaved", "fire Nothing")), "がありません"));
    assert!(any(&errors_loose(&mutate_in(&tweet(), "each saved_tweets → post_to_x", "external → write_tweet")), "external"));
}

#[test]
fn 区切り線は先頭や末尾や連続に置けずclickはtoggleかmenuだけ() {
    assert!(any(&errors_loose(&mutate_in(&tweet(), "  menu \"つぶやきを書く\" → TweetList\n", "  separator\n  menu \"つぶやきを書く\" → TweetList\n")), "先頭や末尾"));
    assert!(any(&errors_loose(&mutate_in(&tweet(), "  separator\n", "  separator\n  separator\n")), "続いて"));
    assert!(parse_err(&mutate_in(&tweet(), "click toggle", "click right")).is_some());
}

// ---- データ用フォルダ ----
fn folder_full() -> String {
    tweet()
}

/// 見本の宣言（path ～ data_folder）は外して、試験ごとに足す
fn folder_base() -> String {
    let f = folder_full();
    format!("{}{}", &f[..f.find("// データ用フォルダ").unwrap()], &f[f.find("needs secret").unwrap()..])
}

fn with_lines(lines: &str) -> String {
    folder_base().replacen("needs secret", &format!("{}\nneeds secret", lines), 1)
}

fn one(os_name: &str, text: &str, relative: bool) -> String {
    if relative {
        format!("path P = relative \"{}\"\ndata_folder P", text)
    } else {
        format!("path P = absolute {{\n  {} \"{}\"\n}}\ndata_folder P", os_name, text)
    }
}

fn derrors(lines: &str) -> Vec<String> {
    errors_loose(&with_lines(lines))
}

const B: &str = "\\\\"; // geas の文字列の中の \ は \\ と書く

#[test]
fn データ用フォルダの見本の宣言は通り仕様書に出る() {
    with_check(&folder_full(), false, |ck| {
        let spec = build(ck);
        let rows: std::collections::HashMap<String, String> = spec["データ用フォルダ"].as_array().unwrap().iter().map(|r| (r[0].as_str().unwrap().to_string(), r[1].as_str().unwrap().to_string())).collect();
        assert_eq!(rows["データ用フォルダ"], "パス DataRoot");
        assert_eq!(rows["パス DataRoot（windows）"], "%APPDATA%\\ひとりつぶやき");
        assert!(rows.contains_key("パス DataRoot（linux）") && rows.contains_key("パス DataRoot（macos）"));
    });
}

#[test]
fn 各osの書き方と相対パスは通る() {
    assert!(derrors(&one("windows", &format!("%APPDATA%{}ひとり", B), false)).is_empty());
    assert!(derrors(&one("windows", &format!("D:{}data", B), false)).is_empty());
    assert!(derrors(&one("macos", "~/Library/Application Support/tweet", false)).is_empty());
    assert!(derrors(&one("linux", "${HOME}/.local/share/tweet", false)).is_empty());
    assert!(derrors(&one("", "data/x", true)).is_empty());
}

#[test]
fn absoluteの中はそのosの書き方の絶対パス() {
    assert!(any(&derrors(&one("windows", "$HOME/x", false)), "Windows のパスの書き方"));
    assert!(any(&derrors(&one("windows", "~/x", false)), "Windows のパスの書き方"));
    assert!(any(&derrors(&one("windows", "data", false)), "絶対パス"));
    assert!(any(&derrors(&one("linux", "%APPDATA%/x", false)), "mac・Linux のパスの書き方"));
    assert!(any(&derrors(&one("macos", &format!("C:{}x", B), false)), "mac・Linux のパスの書き方"));
    assert!(any(&derrors(&one("linux", "data/x", false)), "絶対パス"));
}

#[test]
fn relativeは相対パスだけ外へ出られない() {
    assert!(any(&derrors(&one("", "/var/x", true)), "相対パス"));
    assert!(any(&derrors(&one("", "%APPDATA%/x", true)), "相対パス"));
    assert!(any(&derrors(&one("", "../x", true)), "exe のあるフォルダの外"));
}

#[test]
fn 環境変数は必ずある名前かneeds_envで宣言する() {
    assert!(any(&derrors(&one("windows", &format!("%FOO%{}x", B), false)), "needs env FOO"));
    assert!(any(&derrors(&one("linux", "$XDG_DATA_HOME/x", false)), "needs env XDG_DATA_HOME"));
    assert!(derrors(&format!("needs env FOO \"置き場の指定\"\n{}", one("windows", &format!("%FOO%{}x", B), false))).is_empty());
    assert!(any(&derrors("needs env FOO \"a\"\nneeds env foo \"b\""), "2つ以上"));
}

#[test]
fn データ用フォルダの書き方の誤り() {
    assert!(any(&derrors("path P = absolute {\n  linux \"$HOME/a\"\n  linux \"$HOME/b\"\n}\ndata_folder P"), "2つ以上"));
    assert!(any(&derrors("path P = absolute {\n}\ndata_folder P"), "OS の行がありません"));
    for bad in ["path P = absolute {\n  beos \"x\"\n}", "path P = something \"x\"", "path P = relative", "path P = relative \"a\"\npath P = relative \"b\"", "path P = relative \"a\"\ndata_folder Q", "path P = relative \"a\"\ndata_folder P\ndata_folder P"] {
        assert!(parse_err(&with_lines(bad)).is_some(), "{}", bad);
    }
}

#[test]
fn 組むosの行が無ければ生成を止める() {
    let t = Tmp::new("target");
    let src = t.0.join("a.geas");
    std::fs::write(&src, with_lines(&one("linux", "$HOME/.x", false))).unwrap();
    let r = std::process::Command::new(env!("CARGO_BIN_EXE_geas"))
        .env("GEAS_LANG", "ja")
        .args(["rust"])
        .arg(&src)
        .arg("-o")
        .arg(t.0.join("o"))
        .args(["--host", "--runtime", "--standalone", "--target", "windows"])
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&r.stderr);
    assert!(r.status.code() == Some(1) && err.contains("windows") && err.contains("パス P"), "{}", err);
}
