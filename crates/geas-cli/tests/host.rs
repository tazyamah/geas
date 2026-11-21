// SPDX-License-Identifier: GPL-3.0-or-later
//! 言語機能ごとの動作テスト（その 2）：境界（host::call）・時刻・slot・合成型・ライブラリ・convert・割り算・タイムゾーン・モジュール・標準モジュール Gtin。
//! 小さな .geas から生成した Rust を実際に組んで動かす。どの機能にどのテストがあるかは docs/language-test-map.md。
//! （Python 版の tests/test_geas.py の、生成物を動かす試験の移し。静的な検査の試験は、golden.rs の出力の一致が担う）
mod common;
use common::*;

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

const OVERFLOW: &str = r####"ltype Count = int 0..

entity Tray {
  tray_id: id unique
  n: Count required
}
result PutResult {
  Done(tray_id: id)
}
library triple(x: Count) -> Count from rust "std" "" call "x * 3" guarantee result >= x trust check "3 倍するだけ"
example triple(1) == 3
deed put_tray {
  input n: Count required
  writes Tray
  result PutResult
  effects none

  let t = create Tray { n: triple(n) }
  return Done(t.tray_id)
}
nexus Filling {
  put_tray: Done → reply
}
"####;

const COMP: &str = r####"ltype 横 = int 0..100
ltype 点 { x: 横, y: 横 }
ltype 線 {
  始点: 点
  終点: 点
}

entity 地図 {
  地図ID: id unique
  位置: 点 required
  道: 線 required
}
result 置いた結果 {
  置いた(地図ID: id)
}
result 読んだ結果 {
  読んだ(地図ID: id)
  なし
  右寄り
}
deed 置く {
  input 位置: 点 required
  writes 地図
  result 置いた結果
  effects none

  let 地図 = create 地図 { 位置: 位置, 道: 線 { 始点: 位置, 終点: 点 { x: 位置.x, y: 5 } } }
  return 置いた(地図.地図ID)
}
deed 横を見る {
  input 地図ID: id required
  reads 地図.道
  result 読んだ結果
  effects none

  let 地図 = 地図[地図ID] else { return なし }
  if 地図.道.終点.x > 50 { return 右寄り }
  return 読んだ(地図ID)
}
nexus 流れ {
  置く: 置いた → reply
  横を見る: 読んだ → reply, なし → reply, 右寄り → reply
}
"####;

const DIVIDE: &str = r####"ltype 日数 = int 0..
ltype 週数 = int 0..
ltype 余り日 = int 0..6

convert 週(d: 日数) -> 週数 = d / 7
convert 端(d: 日数) -> 余り日 = d % 7
"####;

const UTC: &str = r####"convert 実在日時(v: vtime fill { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 }) -> time = time(
  (v.day + (153 * (v.month + 12 * ((14 - v.month) / 12) - 3) + 2) / 5 + 365 * (v.year + 4800 - (14 - v.month) / 12)
   + (v.year + 4800 - (14 - v.month) / 12) / 4 - (v.year + 4800 - (14 - v.month) / 12) / 100
   + (v.year + 4800 - (14 - v.month) / 12) / 400 - 32045 - 2440588) * 86400 + v.hour * 3600 + v.minute * 60 + v.second)

entity 予定 {
  予定ID: id unique
  時刻: time required
}
result 予約結果 {
  予約済(予定ID: id)
}
deed 予約 {
  input 日時: vtime required
  writes 予定
  result 予約結果
  effects none

  let 予定 = create 予定 { 時刻: 実在日時(日時) }
  return 予約済(予定.予定ID)
}
nexus 流れ {
  予約: 予約済 → reply
}
"####;

const IDMAP: &str = r####"ltype Dots = int 0..100
ltype Pos { x: Dots, y: Dots optional }
ltype Table = id → Pos
result Shown {
  Big
  Small
}
deed show {
  input table: Table required
  result Shown
  effects none

  let n = sum { v.x | (k, v) in table }
  if n > 6 { return Big }
  return Small
}
nexus Flow {
  show: Big → reply, Small → reply
}
"####;

const GEO: &str = r####"module Geo

/// 位置の数
ltype Dots = int 0..100
/// 位置
ltype Pos { x: Dots, y: Dots optional }

/// そのまま
convert same(d: Dots) -> Dots = d
"####;

const GEO_APP: &str = r####"use "geo.geas"

ltype Table = id → Geo.Pos
result Shown {
  Big
  Small
}
deed show {
  input table: Table required, limit: Geo.Dots required
  result Shown
  effects none

  let n = sum { v.x | (k, v) in table }
  if n > Geo.same(limit) { return Big }
  return Small
}
nexus Flow {
  show: Big → reply, Small → reply
}
"####;

const GTIN: &str = r####"use "gtin.geas"
ltype Jan = Gtin.Gtin13
ltype Plain = text 13..13 of "0-9"
entity Item {
  item_id: id unique
  jan: Jan required
}
deed show_item {
  input item_id: id required, jan: Jan required, raw: Plain required
  reads Item
  writes Item.jan
  result R
  effects none

  let item = Item[item_id] else { return Missing }
  item.jan = jan
  return Done
}
result R {
  Done
  Missing
}
"####;

const RAW_SRC: &str = r####"ltype PartNo = text 8..8 of "0-9" read part_no_reader
ltype Plain = text 8..8 of "0-9"
entity Item {
  item_id: id unique
  part_no: PartNo required
}
result PutResult {
  Put(item_id: id)
}
convert part_no_reader(r: raw) -> PartNo {
  require len(r) == 8 "8桁にしてください"
  require only(r, "0-9") "数字だけにしてください"
  return text(r)
}
deed put_item {
  input part_no: PartNo required
  writes Item
  result PutResult
  effects none

  let item = create Item { part_no: part_no }
  return Put(item.item_id)
}
nexus Flow {
  put_item: Put → reply
}
"####;

fn req(name: &str, ctx: &str, input: &str, store: &str) -> String {
    format!("{{\"name\":\"{}\",\"ctx\":{},\"input\":{},\"store\":{}}}", name, ctx, input, store)
}

#[test]
fn ホストはnullとキーが無いことを区別する() {
    let note = fixture_note();
    let call = |s: &str| req("題を見る", "{}", "{\"メモID\":\"M1\"}", &format!("{{\"メモ\":{{\"M1\":{}}}}}", s));
    let got = host_calls(&note, &[call("{\"メモID\":\"M1\",\"本文\":\"a\",\"題\":\"t\",\"重さ\":null}"), call("{\"メモID\":\"M1\",\"本文\":\"a\",\"題\":null,\"重さ\":null}"), call("{\"メモID\":\"M1\",\"本文\":\"a\",\"重さ\":null}")]);
    assert!(got[0].contains("\\\"result\\\":\\\"題あり\\\"") && got[0].contains("\\\"body\\\":{\\\"題\\\":\\\"t\\\"}"), "{}", got[0]);
    assert!(got[1].contains("\\\"result\\\":\\\"題なし\\\""), "{}", got[1]);
    assert!(got[2].starts_with("Err(") && got[2].contains('題'), "{}", got[2]);
}

fn fixture_note() -> String {
    NOTE.to_string()
}

#[test]
fn ホストは無いかもしれない入力のキー無しとnullを同じに扱う() {
    let calls = [("書く", "{\"本文\":\"a\",\"題\":\"t\",\"重さ\":3}"), ("書く", "{\"本文\":\"a\",\"重さ\":3}"), ("書く", "{\"本文\":\"a\",\"題\":null,\"重さ\":3}"), ("書く", "{\"本文\":\"a\",\"題\":\"\",\"重さ\":3}"), ("書く", "{\"題\":\"t\",\"重さ\":3}"), ("書く", "{\"本文\":null,\"題\":\"t\",\"重さ\":3}"), ("書く", "{\"本文\":\"a\",\"題\":\"t\",\"重さ\":10}"), ("下書き", "{\"本文\":\"a\"}")];
    let rs: Vec<String> = calls.iter().map(|(n, i)| req(n, "{\"new_メモ_id\":\"M1\"}", i, "{}")).collect();
    let got = host_calls(NULLIN, &rs);
    let ok = |x: &str, v: &str| x.contains(&format!("\\\"result\\\":\\\"{}\\\"", v));
    assert!(ok(&got[0], "書いた") && ok(&got[1], "無題") && ok(&got[2], "無題"), "{:?}", &got[..3]);
    assert!(ok(&got[3], "書いた"), "{}", got[3]); // 空文字は「ある」
    assert!(got[4..7].iter().all(|g| g.starts_with("Err(")), "{:?}", &got[4..7]); // 必須の入力が無い・null、範囲の外は入力不正
    assert!(ok(&got[7], "書いた") && got[7].contains("\\\"題\\\":null"), "{}", got[7]); // 無い入力は、そのまま null の項目へ
}

#[test]
fn ホストは入力の長さと文字を境界で確かめる() {
    let a21 = format!("{{\"名\":\"{}\"}}", "a".repeat(21));
    let a20 = format!("{{\"名\":\"{}\"}}", "a".repeat(20));
    let inputs = ["{\"名\":\"abc_09\"}".to_string(), "{\"名\":\"\"}".to_string(), a21, "{\"名\":\"ABC\"}".to_string(), "{\"名\":\"あ\"}".to_string(), a20];
    let rs: Vec<String> = inputs.iter().map(|i| req("登録する", "{\"new_利用者_id\":\"U1\"}", i, "{}")).collect();
    let got = host_calls(TEXT, &rs);
    assert!(got[0].contains("登録した") && got[5].contains("登録した"), "{} {}", got[0], got[5]);
    assert!(got[1..5].iter().all(|g| g.starts_with("Err(")), "{:?}", &got[1..5]); // 空・長すぎ・使えない文字は入力不正
}

#[test]
fn ホストは時刻の計算をして範囲外の時刻は境界で止める() {
    let calls = [(1000i64, 400i64), (1000, 10i64.pow(16)), (10i64.pow(16), 400)];
    let rs: Vec<String> = calls.iter().map(|(n, b)| req("約束する", &format!("{{\"new_約束_id\":\"K1\",\"現在\":{}}}", n), &format!("{{\"開始\":{}}}", b), "{}")).collect();
    let got = host_calls(TIMEC, &rs);
    assert!(got[0].contains("87400") && got[0].contains("600"), "{}", got[0]); // 期限 = 1000 + 86400、待ち = 1000 - 400
    assert!(got[1].starts_with("Err(") && got[2].starts_with("Err("), "{:?}", &got[1..]);
}

#[test]
fn slotも境界で宣言の範囲を確かめる() {
    let rs: Vec<String> = [100, 101, 0].iter().map(|k| req("置く", &format!("{{\"倍率\":{},\"new_品_id\":\"A\"}}", k), "{\"数\":10}", "{}")).collect();
    let got = host_calls(SLOT, &rs);
    assert!(got[0].starts_with("Ok(") && got[0].contains("\\\"数\\\":1000"), "{}", got[0]); // 範囲の端は通る
    assert!(got[1].starts_with("Err(") && got[1].contains("倍率"), "{}", got[1]); // 宣言の外は処理を走らせない
    assert!(got[2].starts_with("Err(") && got[2].contains("倍率"), "{}", got[2]);
}

#[test]
fn ライブラリの戻り値は呼ぶたびに確かめ違反とpanicは保存を変えずに止まる() {
    let r = req("置く", "{\"new_品_id\":\"A\"}", "{\"税抜\":1000}", "{}");
    let mut outs = std::collections::HashMap::new();
    for (label, expr, level) in [("良い", "x + x / 10", "check"), ("悪い", "x - 1", "check"), ("panic", "{ let v: Vec<i64> = Vec::new(); v[0] }", "check"), ("確かめない", "x - 1", "assume")] {
        let src = PART.replace("\"x + x / 10\"", &format!("\"{}\"", expr.replace('"', "\\\""))).replace("trust check", &format!("trust {}", level));
        outs.insert(label, host_calls(&src, &[r.clone()]).remove(0));
    }
    assert!(outs["良い"].starts_with("Ok(") && outs["良い"].contains("\\\"価格\\\":1100"), "{}", outs["良い"]);
    assert!(outs["悪い"].starts_with("Err(") && outs["悪い"].contains("税込") && outs["悪い"].contains("result >= x"), "{}", outs["悪い"]);
    assert!(outs["panic"].starts_with("Err(") && outs["panic"].contains("税込") && outs["panic"].contains("panic"), "{}", outs["panic"]);
    assert!(outs["確かめない"].starts_with("Ok(") && outs["確かめない"].contains("\\\"価格\\\":999"), "{}", outs["確かめない"]); // assume は人が引き受けた。確かめない
}

#[test]
fn サーバーとして組んでもライブラリの計算の溢れはpanicとして止まる() {
    // サーバーは、生成したクレートを依存にして組む（リリースビルド）。Cargo は組み立ての起点の [profile] しか使わないので、
    // 溢れの確認がサーバーの Cargo.toml に無いと、10^19 × 3 は u64 を溢れて回り込み、保証（result >= x）もすり抜けて書かれる
    let t = Tmp::new("overflow");
    build_crate(OVERFLOW, "fn main() {}\n", &t.0, true);
    geas_core::crategen::write_server(&t.0, None, false).unwrap();
    let server = t.0.join("server");
    let cargo = std::fs::read_to_string(server.join("Cargo.toml")).unwrap();
    assert!(cargo.contains("geas_app = { path = \"..\" }"));
    // 組み方（Cargo.toml の [profile]）はサーバーのままにし、実行部の代わりに処理を呼ぶだけの main を置く（重い依存は外す）
    let slim: String = cargo.split_inclusive('\n').filter(|x| !(x.starts_with("rusqlite") || x.starts_with("serde_json") || x.starts_with("tiny_http"))).collect();
    std::fs::write(server.join("Cargo.toml"), slim).unwrap();
    for e in std::fs::read_dir(server.join("src")).unwrap() {
        let p = e.unwrap().path();
        if p.file_name().unwrap() != "main.rs" {
            std::fs::remove_file(p).unwrap();
        }
    }
    std::fs::write(
        server.join("src").join("main.rs"),
        "use geas_app::host::call;\nfn main() {\n    for n in [\"1000\", \"10000000000000000000\"] {\n        println!(\"{:?}\", call(&format!(r#\"{{\"name\":\"put_tray\",\"ctx\":{{\"new_Tray_id\":\"A\"}},\"input\":{{\"n\":{}}},\"store\":{{}}}}\"#, n)));\n    }\n}\n",
    )
    .unwrap();
    let _ = std::fs::remove_file(server.join("Cargo.lock"));
    let got = cargo_run(&server, true);
    assert!(got[0].starts_with("Ok(") && got[0].contains("\\\"n\\\":3000"), "{}", got[0]);
    assert!(got[1].starts_with("Err(") && got[1].contains("panic"), "{}", got[1]); // 回り込んだ値を書かずに止まる
}

#[test]
fn ライブラリは版を固定した依存として書き出され外のクレートも呼べる() {
    let src = PART.replace("from rust \"std\" \"\"", "from rust \"serde_json\" \"1.0.154\"").replace("\"x + x / 10\"", "\"serde_json::from_str::<i64>(&x.to_string()).unwrap_or(0) + x / 10\"");
    let t = Tmp::new("dep");
    build_crate(&src, "use geas_app::host::call;\nfn main() { println!(\"{:?}\", call(r#\"{\"name\":\"置く\",\"ctx\":{\"new_品_id\":\"A\"},\"input\":{\"税抜\":1000},\"store\":{}}\"#)); }\n", &t.0, true);
    assert!(std::fs::read_to_string(t.0.join("Cargo.toml")).unwrap().contains("[dependencies]\nserde_json = \"=1.0.154\""));
    let got = cargo_run(&t.0, false);
    assert!(got[0].starts_with("Ok(") && got[0].contains("\\\"価格\\\":1100"), "{}", got[0]);
}

#[test]
fn 組み込みの丸めは単位より近い値を返す() {
    // 組み込みの部品の保証は、宣言せず、ここで確かめる（切り捨て・切り上げは差が 1 未満、四捨五入は 1/2 以下）
    let main = "use geas_app::*;\nfn main() {\n    let mut bad = 0;\n    for a in -50i128..50 {\n        for b in 1i128..13 {\n            let x = Ratio { num: a, den: b };\n            let (lo, up, rn) = (floor(x).unwrap(), ceil(x).unwrap(), half_up(x).unwrap());\n            if !(lo * b <= a && a < (lo + 1) * b) { bad += 1; }\n            if !((up - 1) * b < a && a <= up * b) { bad += 1; }\n            if !(2 * (rn * b - a).abs() <= b) { bad += 1; }\n        }\n    }\n    println!(\"{}\", bad);\n}\n";
    assert_eq!(run_app(PART, main), strs(&["0"]));
}

#[test]
fn 合成型はホストの境界で成分ごとに確かめる() {
    let rec = |x: i32| format!("{{\"地図\":{{\"G1\":{{\"地図ID\":\"G1\",\"位置\":{{\"x\":1,\"y\":2}},\"道\":{{\"始点\":{{\"x\":1,\"y\":2}},\"終点\":{{\"x\":{},\"y\":5}}}}}}}}}}", x);
    let rs = vec![
        req("置く", "{\"new_地図_id\":\"G1\"}", "{\"位置\":{\"x\":10,\"y\":20}}", "{}"),
        req("置く", "{\"new_地図_id\":\"G1\"}", "{\"位置\":{\"x\":10,\"y\":200}}", "{}"),
        req("置く", "{\"new_地図_id\":\"G1\"}", "{\"位置\":{\"x\":10}}", "{}"),
        req("置く", "{\"new_地図_id\":\"G1\"}", "{\"位置\":{\"x\":10,\"y\":20,\"z\":1}}", "{}"),
        req("横を見る", "{}", "{\"地図ID\":\"G1\"}", &rec(5)),
        req("横を見る", "{}", "{\"地図ID\":\"G1\"}", &rec(70)),
    ];
    let got = host_calls(COMP, &rs);
    assert!(got[0].contains("\\\"位置\\\":{\\\"x\\\":10,\\\"y\\\":20}") && got[0].contains("\\\"終点\\\":{\\\"x\\\":10,\\\"y\\\":5}"), "{}", got[0]);
    assert!(got[1].contains("範囲外") && got[1].contains("位置.y"), "{}", got[1]);
    assert!(got[2].starts_with("Err(") && got[2].contains('y'), "{}", got[2]);
    assert!(got[3].starts_with("Ok(") && got[3].contains("置いた"), "{}", got[3]); // 宣言にない成分は来ていないものとして扱う
    assert!(got[4].contains("読んだ") && got[5].contains("右寄り"), "{:?}", &got[4..6]);
}

fn convert_src() -> String {
    PART.replace("library 税込(x: 非負金額) -> 非負金額 from rust \"std\" \"\" call \"x + x / 10\" guarantee result >= x trust check \"整数の計算だけで、panic も繰り返しもしない\"\nexample 税込(100) == 110", "convert 税込(x: 非負金額) -> 非負金額 = x + x")
}

#[test]
fn convertは生成コードの中で計算され実行時の範囲検査は出さない() {
    build_only(&convert_src(), "fn main() {}\n");
}

#[test]
fn 割り算と余りは生成コードで動く() {
    build_only(DIVIDE, "fn main() {}\n");
}

fn tz_src() -> String {
    format!(
        "timezone +09:00\nltype Offset = int -50400..50400\n{}",
        UTC.replace("fill { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 })", "fill { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 }, offset: Offset = tz_offset)").replace("+ v.second)", "+ v.second - offset)")
    )
}

#[test]
fn 標準のタイムゾーンで暦の秒がずれ引数で上書きできる() {
    let main = "use geas_app::*;\nfn t(日時: vtime) -> i64 {\n    let ctx = 予約Ctx { new_予定_id: \"e1\".to_string() };\n    let o = 予約(&ctx, &Store::default(), &予約Input { 日時 }).unwrap();\n    match &o.Write[0] { Write::予定_New(r) => r.時刻 }\n}\nfn main() {\n    println!(\"{}\", t(vtime { year: Some(1970), month: Some(1), day: Some(2), hour: None, minute: None, second: None }));\n}\n";
    // 日本時間の1970-01-02 0:00 は、UTC の前日 15:00
    assert_eq!(run_app(&tz_src(), main), vec![(86400 - 9 * 3600).to_string()]);
}

#[test]
fn idの写像の値は合成型でもホストから受け取って足せる() {
    let r = req("show", "{}", "{\"table\":{\"a\":{\"x\":3,\"y\":null},\"b\":{\"x\":4,\"y\":5}}}", "{}");
    assert!(host_calls(IDMAP, &[r]).remove(0).contains("Big")); // 3 + 4 = 7 > 6
}

#[test]
fn モジュールを使ったコードがビルドでき動く() {
    let t = Tmp::new("module");
    std::fs::write(t.0.join("geo.geas"), GEO).unwrap();
    let app = t.0.join("app.geas");
    std::fs::write(&app, GEO_APP).unwrap();
    let out = t.0.join("crate");
    std::fs::create_dir_all(&out).unwrap();
    let r = "{\"name\":\"show\",\"ctx\":{},\"input\":{\"table\":{\"a\":{\"x\":3,\"y\":null},\"b\":{\"x\":4,\"y\":5}},\"limit\":6},\"store\":{}}";
    build_crate_at(GEO_APP, &app, &format!("fn main() {{\n    println!(\"{{}}\", geas_app::host::call(r#\"{}\"#).unwrap());\n}}\n", r), &out, true);
    let got = cargo_run(&out, false);
    assert!(got[0].contains("Big"), "{}", got[0]); // 3 + 4 = 7 > 6
}

#[test]
fn 標準モジュールgtinは正しい検査数字だけ通す() {
    // 標準モジュール Gtin（gtin.geas）の隣に置いたものとして読む
    let at = root().join("tests").join("fixtures").join("overseas_shop").join("gtin_demo.geas");
    let mut lines = vec![];
    for (n, good) in [(8, "49123456"), (12, "012345678905"), (13, "4901234567894"), (14, "14901234567891")] {
        let last = good.chars().last().unwrap().to_digit(10).unwrap();
        let bad = format!("{}{}", &good[..good.len() - 1], (last + 1) % 10);
        lines.push(format!("  assert!(Gtin_read{}(\"{}\").is_ok());", n, good));
        lines.push(format!("  assert!(Gtin_read{}(\"{}\").is_err());", n, bad));
        lines.push(format!("  assert!(Gtin_read{}(\"{}a\").is_err());", n, &good[..good.len() - 1]));
        lines.push(format!("  assert!(Gtin_read{}(\"{}0\").is_err());", n, good));
    }
    let t = Tmp::new("gtin");
    build_crate_at(GTIN, &at, &format!("use geas_app::*;\nfn main() {{\n{}\n}}\n", lines.join("\n")), &t.0, false);
    cargo_run(&t.0, false);
}

#[test]
fn 型に結んだ変換は拒否を入力不正にし通った値だけが処理に入る() {
    let main = "use geas_app::host::call;\nfn main() {\n    for v in [\"12345678\", \"1234567\", \"1234567a\"] {\n        let req = format!(r#\"{{\"name\":\"put_item\",\"ctx\":{{\"new_Item_id\":\"A\"}},\"input\":{{\"part_no\":\"{}\"}},\"store\":{{}}}}\"#, v);\n        println!(\"{:?}\", call(&req));\n    }\n}\n";
    let got = run_app_host(RAW_SRC, main);
    assert!(got[0].starts_with("Ok(") && got[0].contains("12345678"), "{}", got[0]);
    assert!(got[1].starts_with("Err(") && got[1].contains("8桁"), "{}", got[1]);
    assert!(got[2].starts_with("Err(") && got[2].contains("数字だけ"), "{}", got[2]);
}

#[test]
fn 拒否の理由に引用符と改行とバックスラッシュを書いてもそのまま返る() {
    let src = RAW_SRC.replace("\"8桁にしてください\"", r#""8桁に\n\"してください\"\\""#);
    let main = "use geas_app::host::call;\nfn main() {\n    let req = r#\"{\"name\":\"put_item\",\"ctx\":{\"new_Item_id\":\"A\"},\"input\":{\"part_no\":\"1234567\"},\"store\":{}}\"#;\n    match call(req) { Ok(s) => println!(\"OK {}\", s), Err(e) => println!(\"ERR {}\", e) }\n}\n";
    let t = Tmp::new("quote");
    build_crate(&src, main, &t.0, true);
    let r = std::process::Command::new("cargo").args(["run", "--quiet", "--offline"]).current_dir(&t.0).output().unwrap();
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 2500));
    assert_eq!(String::from_utf8_lossy(&r.stdout), "ERR 8桁に\n\"してください\"\\\n");
}
