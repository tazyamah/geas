// SPDX-License-Identifier: GPL-3.0-or-later
//! 「無いかもしれない」項目・入力、文字列の長さと文字種、時刻、slot、library の戻り値の保証の試験（検査と、生成したホストの境界の動き）。
//! 元は Python 版の tests/test_geas.py の中ほど（タグ python-final）。
mod common;
use common::*;
use geas_core::check::ERROR;
use geas_core::spec::build;
use serde_json::json;

fn code() -> String {
    std::fs::read_to_string(root().join("tests/fixtures/shop/shop.geas")).unwrap()
}

fn errs(src: &str) -> Vec<String> {
    with_check(src, false, |ck| ck.diags.iter().filter(|d| d.level == ERROR).map(|d| d.msg.clone()).collect())
}

fn any(v: &[String], needle: &str) -> bool {
    v.iter().any(|m| m.contains(needle))
}

fn ok(src: &str) -> bool {
    with_check(src, false, |ck| {
        assert!(ck.ok(), "{:?}", ck.diags.iter().map(|d| d.msg.clone()).collect::<Vec<_>>());
        true
    })
}

fn is_ready(src: &str) {
    with_check(src, false, |ck| assert!(ck.ready(), "{:?}", ck.diags.iter().map(|d| d.msg.clone()).collect::<Vec<_>>()));
}

/// Debug 表示された文字列の中の "result":"値"
fn result_is(line: &str, v: &str) -> bool {
    line.contains(&format!("\\\"result\\\":\\\"{}\\\"", v))
}

fn call_req(name: &str, ctx: &str, input: &str, store: &str) -> String {
    format!("{{\"name\":\"{}\",\"ctx\":{},\"input\":{},\"store\":{}}}", name, ctx, input, store)
}

const NOTE: &str = r####"ltype 重さ値 = int 0..

entity メモ {
  メモID: id unique
  本文: text required
  題: text
  重さ: 重さ値
}
result 書いた結果 {
  書いた(メモID: id)
}
result 題の結果 {
  題あり(題: text)
  無し
  題なし
}
deed 書く {
  input 本文: text required
  writes メモ
  result 書いた結果
  effects none

  let メモ = create メモ { 本文: 本文 }
  return 書いた(メモ.メモID)
}
deed 題を見る {
  input メモID: id required
  reads メモ.題
  result 題の結果
  effects none

  let メモ = メモ[メモID] else { return 無し }
  let 題 = メモ.題 else { return 題なし }
  return 題あり(題)
}
nexus 流れ {
  書く: 書いた → 題を見る
  題を見る: 題あり → reply, 無し → reply, 題なし → reply
}
"####;

#[test]
fn nullを書いた後は確かめ直す() {
    let src = NOTE.replace("  reads メモ.題\n", "  reads メモ.題\n  writes メモ.題\n").replace("  let 題 = メモ.題 else { return 題なし }\n  return 題あり(題)", "  let 題 = メモ.題 else { return 題なし }\n  メモ.題 = null\n  return 題あり(メモ.題)");
    assert!(any(&errs(&src), "null の"));
}

#[test]
fn nullは下限の不変条件を満たさない() {
    assert!(any(&errs(&format!("{}\ninvariant メモ: 重さ >= 0\n", NOTE)), "null は満たしません"));
}

#[test]
fn 仕様書の必須は_requiredとキーと状態() {
    with_check(NOTE, false, |ck| {
        let spec = build(ck);
        let req = |d: &str, f: &str| spec["データ"].as_array().unwrap().iter().find(|r| r[0] == d && r[1] == f).unwrap()[3].clone();
        assert!(req("メモ", "本文") == json!("○") && req("メモ", "メモID") == json!("○") && req("メモ", "題") == json!(""));
    });
}

#[test]
fn ホストはnullとキーが無いことを区別する() {
    let rq = |s: &str| call_req("題を見る", "{}", "{\"メモID\":\"M1\"}", &format!("{{\"メモ\":{{\"M1\":{}}}}}", s));
    let got = host_calls(NOTE, &[rq("{\"メモID\":\"M1\",\"本文\":\"a\",\"題\":\"t\",\"重さ\":null}"), rq("{\"メモID\":\"M1\",\"本文\":\"a\",\"題\":null,\"重さ\":null}"), rq("{\"メモID\":\"M1\",\"本文\":\"a\",\"重さ\":null}")]);
    assert!(result_is(&got[0], "題あり") && got[0].contains("\\\"body\\\":{\\\"題\\\":\\\"t\\\"}"), "{}", got[0]);
    assert!(result_is(&got[1], "題なし"), "{}", got[1]);
    assert!(got[2].starts_with("Err(") && got[2].contains("題"), "{}", got[2]);
}

const NULLIN: &str = r####"ltype 重さ値 = int 0..
ltype 軽い重さ = int 0..9

entity メモ {
  メモID: id unique
  本文: text required
  題: text
  重さ: 重さ値
}
result 書いた結果 {
  書いた(メモID: id)
  無題
}
deed 書く {
  input 本文: text required, 題: text, 重さ: 軽い重さ
  writes メモ
  result 書いた結果
  effects none

  let t = 題 else { return 無題 }
  if 重さ is null { return 無題 }
  if 重さ > 5 { return 無題 }
  let メモ = create メモ { 本文: 本文, 題: t, 重さ: 重さ }
  return 書いた(メモ.メモID)
}
deed 下書き {
  input 本文: text required, 題: text
  writes メモ
  result 書いた結果
  effects none

  let メモ = create メモ { 本文: 本文, 題: 題 }
  return 書いた(メモ.メモID)
}
nexus 流れ {
  書く: 書いた → reply, 無題 → reply
  下書き: 書いた → reply, 無題 → reply
}
"####;

#[test]
fn 入力は既定で無いかもしれず_確かめてから使う() {
    assert!(ok(NULLIN));
    assert!(any(&errs(&NULLIN.replace("  if 重さ is null { return 無題 }\n", "")), "入力「重さ」は無いことがあります"));
    let e = parse_err(&NULLIN.replace("題: text, 重さ", "題: text optional, 重さ")).unwrap();
    assert!(e.contains("required"), "{}", e);
}

#[test]
fn トリガの引数には必ず要る入力を渡す() {
    assert!(any(&errs(&mutate(&code(), "deed request_payment {\n  input order_id: id required\n", "deed request_payment {\n  input order_id: id\n")), "required の入力を渡してください"));
}

#[test]
fn ホストは無いかもしれない入力のキー無しと_nullを同じに扱う() {
    let calls = [("書く", "{\"本文\":\"a\",\"題\":\"t\",\"重さ\":3}"), ("書く", "{\"本文\":\"a\",\"重さ\":3}"), ("書く", "{\"本文\":\"a\",\"題\":null,\"重さ\":3}"),
        ("書く", "{\"本文\":\"a\",\"題\":\"\",\"重さ\":3}"), ("書く", "{\"題\":\"t\",\"重さ\":3}"), ("書く", "{\"本文\":null,\"題\":\"t\",\"重さ\":3}"),
        ("書く", "{\"本文\":\"a\",\"題\":\"t\",\"重さ\":10}"), ("下書き", "{\"本文\":\"a\"}")];
    let reqs: Vec<String> = calls.iter().map(|(n, i)| call_req(n, "{\"new_メモ_id\":\"M1\"}", i, "{}")).collect();
    let got = host_calls(NULLIN, &reqs);
    assert!(result_is(&got[0], "書いた") && result_is(&got[1], "無題") && result_is(&got[2], "無題"), "{:?}", &got[..3]);
    assert!(result_is(&got[3], "書いた"), "{}", got[3]); // 空文字は「ある」
    assert!(got[4..7].iter().all(|g| g.starts_with("Err(")), "{:?}", &got[4..7]); // 必須の入力が無い・null、範囲の外は入力不正
    assert!(result_is(&got[7], "書いた") && got[7].contains("\\\"題\\\":null"), "{}", got[7]); // 無い入力は、そのまま null の項目へ
}

#[test]
fn ホストは宣言にない入力の項目を無いものとして扱う() {
    let reqs: Vec<String> = ["{\"本文\":\"a\",\"重さ\":3}", "{\"本文\":\"a\",\"重さ\":3,\"余分\":\"x\",\"題名\":\"typo\"}"].iter().map(|i| call_req("書く", "{\"new_メモ_id\":\"M1\"}", i, "{}")).collect();
    let got = host_calls(NULLIN, &reqs);
    assert!(got[0] == got[1] && result_is(&got[0], "無題"), "{:?}", got); // 宣言にない項目は、来ていないのと同じ
}

const TEXT: &str = r####"ltype 利用者名 = text 1..20 of "a-z0-9_"

entity 利用者 {
  利用者ID: id unique
  名: 利用者名 required
}
result 登録結果 {
  登録した(利用者ID: id)
}
deed 登録する {
  input 名: 利用者名 required
  writes 利用者
  result 登録結果
  effects none

  let 利用者 = create 利用者 { 名: 名 }
  return 登録した(利用者.利用者ID)
}
nexus 流れ {
  登録する: 登録した → reply
}
"####;

#[test]
fn 文字列の長さと使える文字は仕様書の型に出る() {
    with_check(TEXT, false, |ck| {
        assert!(ck.ok(), "{:?}", ck.diags);
        let t = &ck.data["利用者"].fields[1].r#type;
        assert_eq!(geas_core::ast::show_type(t).unwrap(), "文字列 1..20 文字種 [0-9_a-z]");
    });
}

/// 項目の型は利用者名のまま、入力の名前の型だけを、新しい論理型（decl）に変える
fn input_only(decl: &str) -> String {
    let src = mutate(TEXT, "ltype 利用者名 = text 1..20 of \"a-z0-9_\"\n", &format!("ltype 利用者名 = text 1..20 of \"a-z0-9_\"\n{}\n", decl));
    mutate(&src, "input 名: 利用者名", "input 名: 入力名")
}

#[test]
fn 長さや文字が広い入力を項目へ入れると止まる() {
    assert!(errs(&input_only("ltype 入力名 = text 1..10 of \"a-z0-9_\"")).is_empty()); // 狭いのはよい
    let m = errs(&input_only("ltype 入力名 = text 1..30 of \"a-z0-9_\""));
    assert!(m.iter().any(|x| x.contains("長さ") && x.contains("収まる")), "{:?}", m);
    assert!(any(&errs(&mutate(TEXT, "input 名: 利用者名", "input 名: text")), "長さ"));
    let m = errs(&input_only("ltype 入力名 = text 1..20 of \"a-z0-9_A-Z\""));
    assert!(m.iter().any(|x| x.contains("文字") && x.contains("収まる")), "{:?}", m);
    assert!(any(&errs(&input_only("ltype 入力名 = text 1..20")), "文字"));
}

#[test]
fn 文字種の書き方の誤りは止まる() {
    for bad in ["of \"z-a\"", "of \"\"", "of \"a\\\\\""] {
        // 最後は、文字種の値が a\（\ のあとに文字が無い）
        assert!(parse_err(&mutate(TEXT, "of \"a-z0-9_\"\n", &format!("{}\n", bad))).is_some(), "{}", bad);
    }
}

#[test]
fn ホストは入力の長さと文字を境界で確かめる() {
    let long = format!("{{\"名\":\"{}\"}}", "a".repeat(21));
    let max = format!("{{\"名\":\"{}\"}}", "a".repeat(20));
    let inputs = ["{\"名\":\"abc_09\"}".to_string(), "{\"名\":\"\"}".to_string(), long, "{\"名\":\"ABC\"}".to_string(), "{\"名\":\"あ\"}".to_string(), max];
    let reqs: Vec<String> = inputs.iter().map(|i| call_req("登録する", "{\"new_利用者_id\":\"U1\"}", i, "{}")).collect();
    let got = host_calls(TEXT, &reqs);
    assert!(got[0].contains("登録した") && got[5].contains("登録した"), "{} {}", got[0], got[5]);
    assert!(got[1..5].iter().all(|g| g.starts_with("Err(")), "{:?}", &got[1..5]); // 空・長すぎ・使えない文字は入力不正
}

const TIMEC: &str = r####"entity 約束 {
  約束ID: id unique
  期限: time
  待ち秒: int
}
result 約束結果 {
  約束した(約束ID: id)
}
deed 約束する {
  input 開始: time required
  slot 現在: time
  writes 約束
  result 約束結果
  effects none

  let 約束 = create 約束 { 期限: 現在 + 24 hour, 待ち秒: 現在 - 開始 }
  return 約束した(約束.約束ID)
}
nexus 流れ {
  約束する: 約束した → reply
}
"####;

#[test]
fn 時刻に期間を足し引きでき_時刻の差は秒数になる() {
    assert!(ok(TIMEC));
    assert!(ok(&mutate(TIMEC, "現在 + 24 hour", "現在 - 90 min")));
    assert!(ok(&mutate(TIMEC, "現在 + 24 hour", "24 hour + 現在")));
    for bad in ["現在 + 開始", "現在 + 5", "24 hour", "現在 * 2"] {
        assert!(!errs(&mutate(TIMEC, "期限: 現在 + 24 hour", &format!("期限: {}", bad))).is_empty(), "{}", bad);
    }
    assert!(!errs(&mutate(TIMEC, "待ち秒: 現在 - 開始", "待ち秒: 24 hour - 現在")).is_empty());
    for unit in ["day", "week", "month", "year"] {
        // 暦で数え方が変わる単位は、まだ書けない
        let e = parse_err(&mutate(TIMEC, "24 hour", &format!("1 {}", unit))).unwrap();
        assert!(e.contains("暦で数え方が変わる"), "{}", e);
    }
    // 絞り込み・期限の単位も同じ
    assert!(parse_err(&mutate(&code(), "deadline now + 30 min", "deadline now + 1 day")).unwrap().contains("暦で数え方が変わる"));
    assert!(parse_err(&mutate(TIMEC, "24 hour", "99999999999 sec")).unwrap().contains("長すぎます"));
}

#[test]
fn ホストは時刻の計算をして_範囲外の時刻は境界で止める() {
    let big = 10_i64.pow(16);
    let reqs: Vec<String> = [(1000, 400), (1000, big), (big, 400)].iter().map(|(n, b)| call_req("約束する", &format!("{{\"new_約束_id\":\"K1\",\"現在\":{}}}", n), &format!("{{\"開始\":{}}}", b), "{}")).collect();
    let got = host_calls(TIMEC, &reqs);
    assert!(got[0].contains("87400") && got[0].contains("600"), "{}", got[0]); // 期限 = 1000 + 86400、待ち = 1000 - 400
    assert!(got[1].starts_with("Err(") && got[2].starts_with("Err("), "{:?}", &got[1..]);
}

const SLOT: &str = r####"ltype 千以下の数 = int 0..1000
ltype 十以下の数 = int 0..10
ltype 倍率値 = int 1..100

entity 品 {
  品ID: id unique
  数: 千以下の数 required
}
result 置いた結果 {
  置いた(品ID: id)
}
deed 置く {
  input 数: 十以下の数 required
  slot 倍率: 倍率値
  writes 品
  result 置いた結果
  effects none

  let 品 = create 品 { 数: 数 * 倍率 }
  return 置いた(品.品ID)
}
nexus 流れ {
  置く: 置いた → reply
}
"####;

#[test]
fn slotも境界で宣言の範囲を確かめる() {
    is_ready(SLOT);
    let reqs: Vec<String> = [100, 101, 0].iter().map(|k| call_req("置く", &format!("{{\"倍率\":{},\"new_品_id\":\"A\"}}", k), "{\"数\":10}", "{}")).collect();
    let got = host_calls(SLOT, &reqs);
    assert!(got[0].starts_with("Ok(") && got[0].contains("\\\"数\\\":1000"), "{}", got[0]); // 範囲の端は通る
    assert!(got[1].starts_with("Err(") && got[1].contains("倍率"), "{}", got[1]); // 宣言の外は処理を走らせない
    assert!(got[2].starts_with("Err(") && got[2].contains("倍率"), "{}", got[2]);
}

const PART: &str = r####"ltype 非負金額 = money(JPY) 0..
ltype 税抜の限り = money(JPY) 0..1000000

entity 品 {
  品ID: id unique
  価格: 非負金額 required
}
result 置いた結果 {
  置いた(品ID: id)
}
library 税込(x: 非負金額) -> 非負金額 from rust "std" "" call "x + x / 10" guarantee result >= x trust check "整数の計算だけで、panic も繰り返しもしない"
example 税込(100) == 110
deed 置く {
  input 税抜: 税抜の限り required
  writes 品
  result 置いた結果
  effects none

  let 品 = create 品 { 価格: 税込(税抜) }
  return 置いた(品.品ID)
}
nexus 流れ {
  置く: 置いた → reply
}
"####;

#[test]
fn 部品の保証は_geasの式で書き_引数とresultだけを使う() {
    with_check(PART, false, |ck| {
        assert!(ck.ready(), "{:?}", ck.diags);
        let spec = build(ck);
        assert_eq!(spec["部品"], json!([["税込", "x: 金額(円) 0..", "金額(円) 0..", "result >= x"]]));
        // 外のコードは人の保証に出る
        assert!(spec["人の保証"].as_array().unwrap().iter().any(|r| r[0] == "（ライブラリ 税込）" && r[1].as_str().unwrap().contains("std") && r[1].as_str().unwrap().contains("x + x / 10")));
    });
    let pe = |s: &str| parse_err(s).unwrap_or_default();
    assert!(pe(&PART.replace("library", "part")).contains("part は廃止"));
    assert!(pe(&PART.replace(" trust check \"整数の計算だけで、panic も繰り返しもしない\"", "")).contains("人の保証が要ります"));
    assert!(pe(&PART.replace("trust check \"整数の計算だけで、panic も繰り返しもしない\"", "trust check")).contains("理由"));
    assert!(pe(&PART.replace("trust check", "trust never")).contains("check（毎回"));
    assert!(any(&errs(&PART.replace("\"std\" \"\"", "\"sha2\" \"latest\"")), "版"));
    assert!(any(&errs(&PART.replace("\"x + x / 10\"", "\"unsafe { x }\"")), "unsafe"));
    assert!(any(&errs(&PART.replace("result >= x", "x >= 0")), "戻り値（result）を使っていません"));
    assert!(any(&errs(&PART.replace("result >= x", "result + x")), "真偽の式"));
    assert!(any(&errs(&PART.replace("result >= x", "result >= y")), "定義されていません"));
}

#[test]
fn 部品の呼び出しは宣言と型が合わないと止まる() {
    assert!(any(&errs(&PART.replace("税込(税抜)", "税込(税抜, 税抜)")), "引数は 1 個"));
    assert!(any(&errs(&PART.replace("税込(税抜)", "税込(int(税抜))")), "型が違います"));
    assert!(any(&errs(&PART.replace("税込", "round")), "組み込みの部品と同じ名前"));
}

#[test]
fn ライブラリの戻り値は呼ぶたびに確かめ_違反とpanicは保存を変えずに止まる() {
    let req = call_req("置く", "{\"new_品_id\":\"A\"}", "{\"税抜\":1000}", "{}");
    let mut outs = std::collections::HashMap::new();
    for (label, expr, level) in [("良い", "x + x / 10", "check"), ("悪い", "x - 1", "check"), ("panic", "{ let v: Vec<i64> = Vec::new(); v[0] }", "check"), ("確かめない", "x - 1", "assume")] {
        let src = PART.replace("\"x + x / 10\"", &format!("\"{}\"", expr)).replace("trust check", &format!("trust {}", level));
        is_ready(&src);
        outs.insert(label, host_calls(&src, &[req.clone()])[0].clone());
    }
    assert!(outs["良い"].starts_with("Ok(") && outs["良い"].contains("\\\"価格\\\":1100"), "{}", outs["良い"]);
    assert!(outs["悪い"].starts_with("Err(") && outs["悪い"].contains("税込") && outs["悪い"].contains("result >= x"), "{}", outs["悪い"]);
    assert!(outs["panic"].starts_with("Err(") && outs["panic"].contains("税込") && outs["panic"].contains("panic"), "{}", outs["panic"]);
    assert!(outs["確かめない"].starts_with("Ok(") && outs["確かめない"].contains("\\\"価格\\\":999"), "{}", outs["確かめない"]); // assume は人が引き受けた。確かめない
}
