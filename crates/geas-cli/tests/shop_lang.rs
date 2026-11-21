// SPDX-License-Identifier: GPL-3.0-or-later
//! 言語の決まりの試験（その 2）：library の依存と溢れ、組み込みの丸め、問い合わせ（null・page・並び）、ltype・entity・store、合成型、vtime、
//! convert・割り算・UTC・timezone、待ち合わせ（all）、仕様書のフロー図と表示名、名前の書き方、id → 値の写像、モジュール。
//! 元は Python 版の tests/test_geas.py の後半（タグ python-final）。
mod common;
use common::*;
use geas_core::check::ERROR;
use geas_core::crategen::{write_crate, write_server};
use geas_core::rustgen::generate;
use geas_core::spec::{build, render};
use serde_json::json;
use std::path::Path;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_geas");

fn code() -> String {
    std::fs::read_to_string(root().join("tests/fixtures/shop/shop.geas")).unwrap()
}

fn errs(src: &str) -> Vec<String> {
    with_check(src, false, |ck| ck.diags.iter().filter(|d| d.level == ERROR).map(|d| d.msg.clone()).collect())
}

fn serrs(src: &str) -> Vec<String> {
    with_check(src, true, |ck| ck.diags.iter().filter(|d| d.level == ERROR).map(|d| d.msg.clone()).collect())
}

fn any(v: &[String], needle: &str) -> bool {
    v.iter().any(|m| m.contains(needle))
}

fn ready(src: &str) -> bool {
    with_check(src, false, |ck| {
        assert!(ck.ready(), "{:?}", ck.diags.iter().map(|d| d.msg.clone()).collect::<Vec<_>>());
        true
    })
}

fn okk(src: &str) -> bool {
    with_check(src, false, |ck| {
        assert!(ck.ok(), "{:?}", ck.diags.iter().map(|d| d.msg.clone()).collect::<Vec<_>>());
        true
    })
}

fn pe(src: &str) -> String {
    parse_err(src).unwrap_or_else(|| panic!("構文の誤りになるはず: {}", src))
}

fn call_req(name: &str, ctx: &str, input: &str, store: &str) -> String {
    format!("{{\"name\":\"{}\",\"ctx\":{},\"input\":{},\"store\":{}}}", name, ctx, input, store)
}

/// 生成した crate を作って main を動かし、標準出力の行を返す（host なし）
fn run_plain(src: &str, main: &str) -> Vec<String> {
    run_app(src, main)
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

#[test]
fn サーバーとして組んでも_ライブラリの計算の溢れは_panicとして止まる() {
    // サーバーは、生成したクレートを依存にして組む（リリースビルド）。Cargo は組み立ての起点の [profile] しか使わないので、
    // 溢れの確認がサーバーの Cargo.toml に無いと、10^19 × 3 は u64 を溢れて回り込み、保証（result >= x）もすり抜けて書かれる
    let t = Tmp::new("ovf");
    with_check(OVERFLOW, false, |ck| {
        assert!(ck.ready(), "{:?}", ck.diags);
        write_crate(ck, &generate(ck), "fn main() {}\n", &t.0, true).unwrap();
    });
    write_server(&t.0, None, false).unwrap();
    let server = t.0.join("server");
    // 組み方（Cargo.toml の [profile]）はサーバーのままにし、実行部の代わりに処理を呼ぶだけの main を置く（重い依存は外す）
    let cargo = std::fs::read_to_string(server.join("Cargo.toml")).unwrap();
    assert!(cargo.contains("geas_app = { path = \"..\" }"));
    let kept: String = cargo.split_inclusive('\n').filter(|l| !(l.starts_with("rusqlite") || l.starts_with("serde_json") || l.starts_with("tiny_http"))).collect();
    std::fs::write(server.join("Cargo.toml"), kept).unwrap();
    for e in std::fs::read_dir(server.join("src")).unwrap().flatten() {
        // 実行部の lib（経路の処理・保存）は外す。ここでは処理を呼ぶだけの main で足りる
        if e.file_name() != "main.rs" {
            std::fs::remove_file(e.path()).unwrap();
        }
    }
    std::fs::write(server.join("src/main.rs"), "use geas_app::host::call;\nfn main() {\n    for n in [\"1000\", \"10000000000000000000\"] {\n        println!(\"{:?}\", call(&format!(r#\"{{\"name\":\"put_tray\",\"ctx\":{{\"new_Tray_id\":\"A\"}},\"input\":{{\"n\":{}}},\"store\":{{}}}}\"#, n)));\n    }\n}\n").unwrap();
    let r = Command::new("cargo").args(["run", "--release", "--quiet", "--offline"]).env("CARGO_TARGET_DIR", common::serve::target_dir()).current_dir(&server).output().unwrap();
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 2500));
    let out = String::from_utf8_lossy(&r.stdout).to_string();
    let lines: Vec<&str> = out.trim().lines().collect();
    assert!(lines[0].starts_with("Ok(") && lines[0].contains("\\\"n\\\":3000"), "{}", lines[0]);
    assert!(lines[1].starts_with("Err(") && lines[1].contains("panic"), "{}", lines[1]); // 回り込んだ値を書かずに止まる
}

#[test]
fn ライブラリは版を固定した依存として書き出され_外のクレートも呼べる() {
    // 外のクレート（serde_json）を、版を固定して依存に書き出す。取得できない環境では、依存の書き出しだけ確かめる
    let src = PART.replace("from rust \"std\" \"\"", "from rust \"serde_json\" \"1.0.154\"").replace("\"x + x / 10\"", "\"serde_json::from_str::<i64>(&x.to_string()).unwrap_or(0) + x / 10\"");
    let t = Tmp::new("libdep");
    with_check(&src, false, |ck| {
        assert!(ck.ready(), "{:?}", ck.diags);
        write_crate(ck, &generate(ck), "fn main() {}\n", &t.0, true).unwrap();
    });
    assert!(std::fs::read_to_string(t.0.join("Cargo.toml")).unwrap().contains("[dependencies]\nserde_json = \"=1.0.154\""));
    let two = format!("{}library other(x: 非負金額) -> 非負金額 from rust \"serde_json\" \"1.0.0\" call \"x\" guarantee result >= x trust check \"そのまま返す\"\nexample other(100) == 100\n", src);
    with_check(&two, false, |ck| {
        let e = geas_core::rustgen::library_deps(ck).unwrap_err();
        assert!(e.contains("版が2つ"), "{}", e);
    });
}

#[test]
fn 組み込みの丸めは単位より近い値を返す() {
    // 組み込みの部品の保証は、宣言せず、ここで確かめる（切り捨て・切り上げは差が 1 未満、四捨五入は 1/2 以下）
    let main = "use geas_app::*;\nfn main() {\n    let mut bad = 0;\n    for a in -50i128..50 {\n        for b in 1i128..13 {\n            let x = Ratio { num: a, den: b };\n            let (lo, up, rn) = (floor(x).unwrap(), ceil(x).unwrap(), half_up(x).unwrap());\n            if !(lo * b <= a && a < (lo + 1) * b) { bad += 1; }\n            if !((up - 1) * b < a && a <= up * b) { bad += 1; }\n            if !(2 * (rn * b - a).abs() <= b) { bad += 1; }\n        }\n    }\n    println!(\"{}\", bad);\n}\n";
    assert_eq!(run_plain(PART, main), vec!["0".to_string()]);
}

const NULLQ: &str = r####"ltype 重さ値 = int 0..
ltype 札の数 = int 1..9
ltype 札束 = 人 → 札の数

entity 人 {
  人ID: id unique
  名前: text
}
entity メモ {
  メモID: id unique
  書いた人: ref 人
  重さ: 重さ値
  札: 札束
}
query 一覧 {
  input 最小: int
  from メモ
  filter 重さ at_least 最小
  order 重さ desc
  select メモID, 書いた人.名前 as 名前, sum(札) as 札数, count(札) as 札の種類
  reads メモ.メモID, メモ.書いた人, 人.名前, メモ.重さ, メモ.札
}
"####;

#[test]
fn nullになりうる項目の問い合わせも_rustがビルドできる() {
    assert!(okk(NULLQ));
    let main = "use geas_app::*;\nfn main() {\n    let mut s = Store::default();\n    s.人.insert(\"P\".into(), 人 { 人ID: \"P\".into(), 名前: None });\n    s.メモ.insert(\"A\".into(), メモ { メモID: \"A\".into(), 書いた人: Some(\"P\".into()), 重さ: None, 札: None });\n    s.メモ.insert(\"B\".into(), メモ { メモID: \"B\".into(), 書いた人: None, 重さ: Some(3), 札: Some([(\"P\".to_string(), 2u8)].into()) });\n    let all = 一覧(&一覧Ctx {}, &s, &一覧Input { 最小: None });\n    println!(\"{}\", all.iter().map(|x| x.show()).collect::<Vec<_>>().join(\";\"));\n    println!(\"{}\", 一覧(&一覧Ctx {}, &s, &一覧Input { 最小: Some(1) }).len());\n}\n";
    let out = run_plain(NULLQ, main);
    assert_eq!(out[0], "メモID=B/名前=null/札数=2/札の種類=1;メモID=A/名前=null/札数=0/札の種類=0"); // null は並びの最後
    assert_eq!(out[1], "1"); // 重さが null のものは絞り込みを満たさない
}

const PAGEQ: &str = r####"ltype 重さ値 = int 0..
ltype 件数値 = int 1..100

entity メモ {
  メモID: id unique
  重さ: 重さ値
}
query 一覧 {
  input 件数: 件数値
  from メモ
  order 重さ desc
  page 件数
  reads メモ.重さ
}
query 先頭 {
  from メモ
  order 重さ asc
  page 2
  reads メモ.重さ
}
"####;

fn query_row(src: &str, name: &str) -> serde_json::Value {
    with_check(src, false, |ck| build(ck)["問い合わせ"].as_array().unwrap().iter().find(|q| q["問い合わせ"] == name).unwrap().clone())
}

#[test]
fn pageはorderが無いとエラーで_limitは使えない() {
    assert!(okk(PAGEQ));
    assert!(any(&errs(&mutate(PAGEQ, "  order 重さ asc\n  page 2", "  page 2")), "order が要ります"));
    assert!(pe(&mutate(PAGEQ, "page 2", "limit 2")).contains("page に替わりました"));
    assert!(parse_err(&mutate(PAGEQ, "page 2", "page 0")).is_some());
}

#[test]
fn pageの件数は仕様書に出る() {
    assert_eq!(query_row(PAGEQ, "先頭")["ページ"], "2件");
    assert_eq!(query_row(PAGEQ, "一覧")["ページ"], "件数（件数が無ければ全件）");
}

#[test]
fn 並びの向きは_仕様書では日時なら新しい順_数なら大きい順と読む() {
    assert_eq!(query_row(PAGEQ, "一覧")["並び"], "重さ 大きい順");
    assert_eq!(query_row(PAGEQ, "先頭")["並び"], "重さ 小さい順");
    let timed = mutate(&mutate(&mutate(PAGEQ, "  重さ: 重さ値\n", "  重さ: 重さ値\n  書いた時刻: time\n"), "  order 重さ desc", "  order 書いた時刻 desc"), "  order 重さ asc", "  order 書いた時刻 asc").replace("reads メモ.重さ", "reads メモ.書いた時刻");
    assert!(okk(&timed));
    assert_eq!(query_row(&timed, "一覧")["並び"], "書いた時刻 新しい順");
    assert_eq!(query_row(&timed, "先頭")["並び"], "書いた時刻 古い順");
}

#[test]
fn pageは並びのうえで先頭の1ページを返す() {
    let main = "use geas_app::*;\nfn main() {\n    let mut s = Store::default();\n    for (id, w) in [(\"A\", 1i64), (\"B\", 5), (\"C\", 3)] { s.メモ.insert(id.into(), メモ { メモID: id.into(), 重さ: Some(w as _) }); }\n    println!(\"{:?}\", 先頭(&先頭Ctx {}, &s, &先頭Input { after: None }));\n    println!(\"{:?}\", 一覧(&一覧Ctx {}, &s, &一覧Input { 件数: Some(2), after: None }));\n}\n";
    let out = run_plain(PAGEQ, main);
    assert_eq!((out[0].as_str(), out[1].as_str()), ("[\"A\", \"C\"]", "[\"B\", \"C\"]")); // asc の先頭2件／desc の先頭2件
}

#[test]
fn 問いが残っているうちは生成しない() {
    // 問いが残る（エラーは無い）コードは、検査は通るが、生成には進まない（rust コマンドは止まる）
    let src = mutate(&code(), "let total = subtotal + round(subtotal * tax_rate, floor, JPY)", "let total = subtotal + subtotal * tax_rate");
    with_check(&src, false, |ck| assert!(ck.ok() && !ck.ready()));
    let t = Tmp::new("questions");
    let f = t.0.join("q.geas");
    std::fs::write(&f, &src).unwrap();
    let r = Command::new(BIN).env("GEAS_LANG", "ja").arg("rust").arg(&f).arg("-o").arg(t.0.join("out")).output().unwrap();
    let out = String::from_utf8_lossy(&r.stdout).to_string() + &String::from_utf8_lossy(&r.stderr);
    assert!(!r.status.success() && out.contains("問い") && !t.0.join("out").exists(), "{}", out);
}

// ---- 論理型（ltype）と entity ----
const LTYPE: &str = r####"ltype Quantity = int 1..99
ltype ProductQuantity = Product → Quantity

entity Product {
  product_id: id unique
  stock: Quantity required
}

entity Order {
  order_id: id unique
  items: ProductQuantity required
}
"####;

#[test]
fn ltypeは使った場所に定義が写される() {
    let p = geas_core::parser::parse(LTYPE, None).unwrap();
    let stock = &p.data.iter().find(|d| d.name == "Product").unwrap().fields.iter().find(|f| f.name == "stock").unwrap().r#type;
    let r = stock.range.as_ref().unwrap();
    assert!(stock.kind == "整数" && r.lo == Some(1) && r.hi == Some(99));
    let lines = &p.data.iter().find(|d| d.name == "Order").unwrap().fields.iter().find(|f| f.name == "items").unwrap().r#type;
    assert!(lines.kind == "写像" && lines.key.as_deref() == Some("Product") && lines.value.as_ref().unwrap().range.as_ref().unwrap().hi == Some(99));
}

#[test]
fn ltypeの名前の重複と組み込み名は止まる() {
    assert!(pe("ltype 数量 = int 1..99\nltype 数量 = int 1..9\n").contains("2つ以上"));
    assert!(pe("ltype int = int 1..99\n").contains("組み込み"));
}

#[test]
fn dataはentityに改名した() {
    assert!(pe("data 商品 {\n  商品ID: id unique\n}\n").contains("entity"));
}

#[test]
fn 範囲や組み合わせの直書きは止まり_名前を付けるよう案内する() {
    for ty in ["int 1..99", "text 1..20", "money(JPY) 0..", "set of id", "product → Quantity"] {
        assert!(pe(&format!("entity Product {{\n  product_id: id unique\n  項目: {}\n}}\n", ty)).contains("ltype"), "{}", ty);
    }
    assert!(parse_err("entity Product {\n  product_id: id unique\n  項目: int\n  名: text\n  金: money(JPY)\n}\n").is_none()); // 約束のない組み込みはそのまま書ける
}

// ---- store（保存先）は entity の外に書く ----
const STORE: &str = r####"entity メモ {
  メモID: id unique
  本文: text required
}
store メモ in Dropbox
"####;

#[test]
fn storeを書くと_未同期の状態が足される() {
    let p = geas_core::parser::parse(STORE, None).unwrap();
    assert_eq!(p.data[0].store.as_deref(), Some("Dropbox"));
    assert!(p.states.iter().any(|s| s.name == "記録状態"));
    assert!(p.data[0].fields.iter().any(|f| f.name == "記録"));
}

#[test]
fn storeが無ければ_保存の状態は足されない() {
    let p = geas_core::parser::parse(&STORE.replace("store メモ in Dropbox\n", ""), None).unwrap();
    assert!(p.data[0].store.is_none());
    assert!(!p.states.iter().any(|s| s.name == "記録状態"));
}

#[test]
fn storeの誤りは止まる() {
    assert!(pe(&STORE.replace("store メモ", "store 謎")).contains("entity がありません"));
    assert!(pe(&format!("{}store メモ in 端末\n", STORE)).contains("2つ以上"));
    assert!(pe(&STORE.replace("entity メモ {", "entity メモ store Dropbox {").replace("store メモ in Dropbox\n", "")).contains("別の行"));
}

// ---- 合成型 ----
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

#[test]
fn 合成型は項目と入力に使え_成分は平らに読める() {
    with_check(COMP, false, |ck| {
        assert!(ck.ok() && ck.ready(), "{:?}", ck.diags);
        let sp = build(ck);
        assert!(sp["データ"].as_array().unwrap().contains(&json!(["地図", "位置", "点（x: 整数 0..100、y: 整数 0..100）", "○", ""])));
    });
}

#[test]
fn 合成型の誤りは止まる() {
    // 成分の書き漏れ・余計な成分・型の違い
    assert!(any(&errs(&mutate(COMP, "点 { x: 位置.x, y: 5 }", "点 { x: 位置.x }")), "成分「y」がありません"));
    assert!(any(&errs(&mutate(COMP, "点 { x: 位置.x, y: 5 }", "点 { x: 位置.x, y: 5, z: 1 }")), "成分「z」がありません"));
    assert!(any(&errs(&mutate(COMP, "点 { x: 位置.x, y: 5 }", "点 { x: 位置.x, y: 位置 }")), "型の違う値"));
    // 範囲が収まると示せない（入力は 0..1000 で、成分は 0..100）
    let wide = mutate(COMP, "ltype 点 {", "ltype 広い横 = int 0..1000\nltype 点 {");
    let wide = mutate(&mutate(&wide, "deed 置く {\n  input 位置: 点 required", "deed 置く {\n  input 位置: 点 required, 幅: 広い横 required"), "点 { x: 位置.x, y: 5 }", "点 { x: 位置.x, y: 幅 }");
    assert!(any(&errs(&wide), "成分の範囲に収まると示せません"));
    // 存在しない成分を読む
    assert!(any(&errs(&mutate(COMP, "地図.道.終点.x > 50", "地図.道.終点.z > 50")), "成分「z」がありません"));
}

#[test]
fn 合成型は問い合わせの入力と部品にはまだ使えない() {
    let m = errs(&mutate(COMP, "読んだ(地図ID: id)", "読んだ(地図ID: id, 場所: 点)"));
    assert!(!m.iter().any(|x| x.contains("合成型") && x.contains("にだけ")), "{:?}", m); // 結果の中身には使える
    let m = errs(&format!("{}\nquery 点を探す {{\n  input 位置: 点\n  from 地図\n  reads 地図.地図ID\n}}\n", COMP));
    assert!(any(&m, "にだけ使えます"), "{:?}", m);
}

#[test]
fn 合成型の宣言の誤りは止まる() {
    for (src, msg) in [("ltype 点 {\n  x: int\n  x: int\n}\n", "2つ以上"), ("ltype 点 {\n}\n", "成分がありません"), ("ltype 横 = int 0..9\nltype 横束 = Product → 横\nltype 点 {\n  束: 横束\n}\n", "使えません")] {
        assert!(pe(src).contains(msg), "{}", src);
    }
}

#[test]
fn 合成型はホストの境界で成分ごとに確かめる() {
    let rec = |x: i64| format!("{{\"地図\":{{\"G1\":{{\"地図ID\":\"G1\",\"位置\":{{\"x\":1,\"y\":2}},\"道\":{{\"始点\":{{\"x\":1,\"y\":2}},\"終点\":{{\"x\":{},\"y\":5}}}}}}}}}}", x);
    let reqs = vec![
        call_req("置く", "{\"new_地図_id\":\"G1\"}", "{\"位置\":{\"x\":10,\"y\":20}}", "{}"),
        call_req("置く", "{\"new_地図_id\":\"G1\"}", "{\"位置\":{\"x\":10,\"y\":200}}", "{}"),
        call_req("置く", "{\"new_地図_id\":\"G1\"}", "{\"位置\":{\"x\":10}}", "{}"),
        call_req("置く", "{\"new_地図_id\":\"G1\"}", "{\"位置\":{\"x\":10,\"y\":20,\"z\":1}}", "{}"),
        call_req("横を見る", "{}", "{\"地図ID\":\"G1\"}", &rec(5)),
        call_req("横を見る", "{}", "{\"地図ID\":\"G1\"}", &rec(70)),
    ];
    let got = host_calls(COMP, &reqs);
    assert!(got[0].contains("\\\"位置\\\":{\\\"x\\\":10,\\\"y\\\":20}") && got[0].contains("\\\"終点\\\":{\\\"x\\\":10,\\\"y\\\":5}"), "{}", got[0]);
    assert!(got[1].contains("範囲外") && got[1].contains("位置.y"), "{}", got[1]);
    assert!(got[2].starts_with("Err(") && got[2].contains('y'), "{}", got[2]);
    assert!(got[3].starts_with("Ok(") && got[3].contains("置いた")); // 宣言にない成分は来ていないものとして扱う
    assert!(got[4].contains("読んだ") && got[5].contains("右寄り"), "{:?}", &got[4..6]);
}

// ---- vtime（成分は未指定にできる）----
const VDT: &str = r####"entity 約束 {
  約束ID: id unique
  期日: vtime required
}
result 決めた結果 {
  決めた(約束ID: id)
}
result 月結果 {
  未指定
  前半
  後半
  なし
}
deed 決める {
  input 期日: vtime required
  writes 約束
  result 決めた結果
  effects none

  let 約束 = create 約束 { 期日: 期日 }
  return 決めた(約束.約束ID)
}
deed 誕生日にする {
  writes 約束
  result 決めた結果
  effects none

  let 約束 = create 約束 { 期日: vtime { month: 10, day: 26 } }
  return 決めた(約束.約束ID)
}
deed 月を見る {
  input 約束ID: id required
  reads 約束.期日
  result 月結果
  effects none

  let 約束 = 約束[約束ID] else { return なし }
  if 約束.期日.month is null { return 未指定 }
  if 約束.期日.month > 6 { return 後半 }
  return 前半
}
nexus 流れ {
  決める: 決めた → reply
  誕生日にする: 決めた → reply
  月を見る: 前半 → reply, 後半 → reply, 未指定 → reply, なし → reply
}
"####;

#[test]
fn vtimeの成分は未指定にでき_確かめずに読むと止まる() {
    with_check(VDT, false, |ck| {
        assert!(ck.ok() && ck.ready(), "{:?}", ck.diags);
        let sp = build(ck);
        assert!(sp["データ"].as_array().unwrap().iter().any(|r| r[1] == "期日" && r[2].as_str().unwrap().starts_with("vtime（year: 整数 1..9999")), "{:?}", sp["データ"]);
    });
    assert!(any(&errs(&mutate(VDT, "  if 約束.期日.month is null { return 未指定 }\n", "")), "未指定かもしれない成分"));
    // 必須の成分を持つ合成型では、書き漏れがエラー
    let src = format!("ltype 点 {{\n  x: int\n  y: int optional\n}}\n{}", mutate(VDT, "vtime { month: 10, day: 26 }", "点 { y: 1 }").replace("期日: vtime required", "期日: 点 required").replace("input 期日: vtime required", "input 期日: 点 required").replace("約束.期日.month", "約束.期日.y"));
    let m = errs(&src);
    assert!(any(&m, "成分「x」がありません") || parse_err(&src).map_or(false, |e| e.contains("optional")), "{:?}", m);
}

#[test]
fn vtimeの成分の範囲は構造の上限だけ() {
    assert!(any(&errs(&mutate(VDT, "vtime { month: 10, day: 26 }", "vtime { month: 13, day: 26 }")), "成分の範囲に収まると示せません"));
}

#[test]
fn vtimeはホストの境界で成分ごとに確かめ_未指定はnull() {
    let rec = |body: &str| format!("{{\"約束\":{{\"A1\":{{\"約束ID\":\"A1\",\"期日\":{}}}}}}}", body);
    let reqs = vec![
        call_req("決める", "{\"new_約束_id\":\"A1\"}", "{\"期日\":{\"year\":2026,\"month\":10}}", "{}"),
        call_req("決める", "{\"new_約束_id\":\"A1\"}", "{\"期日\":{\"month\":13}}", "{}"),
        call_req("誕生日にする", "{\"new_約束_id\":\"A1\"}", "{}", "{}"),
        call_req("月を見る", "{}", "{\"約束ID\":\"A1\"}", &rec("{\"month\":null}")),
        call_req("月を見る", "{}", "{\"約束ID\":\"A1\"}", &rec("{\"month\":3}")),
        call_req("月を見る", "{}", "{\"約束ID\":\"A1\"}", &rec("{\"month\":9}")),
    ];
    let got = host_calls(VDT, &reqs);
    assert!(got[0].contains("\\\"month\\\":10") && got[0].contains("\\\"day\\\":null") && got[0].contains("\\\"year\\\":2026"), "{}", got[0]);
    assert!(got[1].contains("範囲外") && got[1].contains("期日.month"), "{}", got[1]);
    assert!(got[2].contains("\\\"month\\\":10") && got[2].contains("\\\"day\\\":26") && got[2].contains("\\\"year\\\":null"), "{}", got[2]);
    assert!(got[3].contains("未指定") && got[4].contains("前半") && got[5].contains("後半"), "{:?}", &got[3..6]);
}

fn convert_src() -> String {
    PART.replace("library 税込(x: 非負金額) -> 非負金額 from rust \"std\" \"\" call \"x + x / 10\" guarantee result >= x trust check \"整数の計算だけで、panic も繰り返しもしない\"\nexample 税込(100) == 110", "convert 税込(x: 非負金額) -> 非負金額 = x + x")
}

#[test]
fn convertは式が定義で_外の部品も保証も要らない() {
    with_check(&convert_src(), false, |ck| {
        assert!(ck.ready(), "{:?}", ck.diags);
        assert_eq!(build(ck)["部品"], json!([["税込", "x: 金額(円) 0..", "金額(円) 0..", "= x + x"]]));
        assert!(!generate(ck).contains("trait Parts"));
    });
}

#[test]
fn convertの式の型が戻り値と違うと止まる() {
    let m = errs(&convert_src().replace("= x + x", "= int(x)"));
    assert!(m.iter().any(|x| x.contains("戻り値の型と違います") || x.contains("戻り値の型（")), "{:?}", m);
}

#[test]
fn convertは生成コードの中で計算され_実行時の範囲検査は出さない() {
    build_only(&convert_src(), "fn main() {}\n");
}

#[test]
fn convertの結果が範囲に収まると示せなければ検査で止まる() {
    let bad = convert_src().replace("convert 税込(x: 非負金額) -> 非負金額", "convert 税込(x: 非負金額) -> 税抜の限り");
    assert!(any(&errs(&bad), "収まると示せません"));
    let good = convert_src().replace("convert 税込(x: 非負金額) -> 非負金額 = x + x", "convert 税込(x: 税抜の限り) -> 非負金額 = x + x");
    assert!(ready(&good));
    assert!(!with_check(&good, false, |ck| generate(ck)).contains("保証違反(\"税込\""));
}

const DIVIDE: &str = r####"ltype 日数 = int 0..
ltype 週数 = int 0..
ltype 余り日 = int 0..6

convert 週(d: 日数) -> 週数 = d / 7
convert 端(d: 日数) -> 余り日 = d % 7
"####;

#[test]
fn 整数の割り算は正の定数で切り捨て_範囲は端を割って示す() {
    assert!(ready(&DIVIDE.replace("convert 端(d: 日数) -> 余り日 = d % 7\n", "")));
    assert!(pe(&DIVIDE.replacen("d / 7", "d / 0", 1)).contains("正の整数"));
    // 変数で割るのは、割る数が 1 以上と示せるか or で 0 のときの値を書くときだけ
    assert!(any(&errs(&DIVIDE.replacen("d / 7", "d / d", 1)), "0 になる道"));
    assert!(any(&errs(&DIVIDE.replace("ltype 日数 = int 0..", "ltype 日数 = money(JPY) 0..").replace("convert 端(d: 日数) -> 余り日 = d % 7\n", "")), "割り算できるのは整数だけ"));
}

#[test]
fn 余りの範囲は静的に示せる() {
    assert!(ready(DIVIDE));
    assert!(any(&errs(&DIVIDE.replace("ltype 余り日 = int 0..6", "ltype 余り日 = int 0..5")), "収まると示せません"));
}

#[test]
fn 割り算と余りは生成コードで動く() {
    build_only(DIVIDE, "fn main() {}\n");
}

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

#[test]
fn vtimeを補うルールつきで実在日時へ変える() {
    assert!(ready(UTC));
}

#[test]
fn 補うルールが無い成分を読むと違反になる() {
    let bad = UTC.replace("fill { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 }", "fill { year: 1970, month: 1, day: 1, hour: 0, minute: 0 }");
    assert!(any(&errs(&bad), "未指定かもしれない成分"));
    assert!(any(&errs(&UTC.replace("hour: 0,", "hour: 99,")), "収まりません"));
}

#[test]
fn vtimeの変換は生成コードで動く() {
    build_only(UTC, "fn main() {}\n");
}

const T_MAIN: &str = "use geas_app::*;\nfn t(日時: vtime) -> i64 {\n    let ctx = 予約Ctx { new_予定_id: \"e1\".to_string() };\n    let o = 予約(&ctx, &Store::default(), &予約Input { 日時 }).unwrap();\n    match &o.Write[0] { Write::予定_New(r) => r.時刻 }\n}\n";

#[test]
fn vtimeの変換は暦どおりの秒になり_未指定は補うルールの値になる() {
    let main = format!("{}fn main() {{\n    println!(\"{{}}\", t(vtime {{ year: Some(2026), month: Some(10), day: Some(26), hour: Some(3), minute: Some(4), second: Some(5) }}));\n    println!(\"{{}}\", t(vtime {{ year: Some(2000), month: None, day: None, hour: None, minute: None, second: None }}));\n    println!(\"{{}}\", t(vtime {{ year: None, month: None, day: None, hour: None, minute: None, second: None }}));\n}}\n", T_MAIN);
    assert_eq!(run_plain(UTC, &main), vec!["1792983845", "946684800", "0"]);
}

fn tz_src() -> String {
    format!("timezone +09:00\nltype Offset = int -50400..50400\n{}", UTC.replace("fill { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 })", "fill { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 }, offset: Offset = tz_offset)").replace("+ v.second)", "+ v.second - offset)"))
}

#[test]
fn timezoneの宣言と_デフォルト引数で_標準のずれを使える() {
    let tz = tz_src();
    with_check(&tz, false, |ck| {
        assert!(ck.ready(), "{:?}", ck.diags);
        assert_eq!(ck.p.tz_offset, 9 * 3600);
    });
    assert_eq!(with_check(UTC, false, |ck| ck.p.tz_offset), 0); // 書かなければ UTC
    assert_eq!(geas_core::parser::parse(&tz.replace("+09:00", "-05:30"), None).unwrap().tz_offset, -(5 * 3600 + 30 * 60));
    assert!(ready(&tz.replace("+09:00", "-05:30"))); // 負のずれも、そのままの範囲で扱える
}

#[test]
fn デフォルト引数は_省ける_渡せる_決まりに反すると止まる() {
    let tz = tz_src();
    assert!(ready(&tz.replace("実在日時(日時)", "実在日時(日時, 0)"))); // 渡して上書き
    assert!(any(&errs(&tz.replace("実在日時(日時)", "実在日時(日時, 0, 0)")), "引数は 2 個です"));
    assert!(any(&errs(&tz.replace("offset: Offset = tz_offset", "offset: Offset = 99999")), "デフォルト値が、引数の範囲"));
    assert!(any(&errs(&tz.replace("v: vtime fill", "w: Offset = 0, v: vtime fill")), "にもデフォルト値が要ります"));
}

#[test]
fn timezoneの書き方の誤りは止まる() {
    assert!(pe("timezone +09:00\ntimezone +01:00\n").contains("1つだけ"));
    assert!(pe("timezone Asia/Tokyo\n").contains("時:分で書きます"));
    assert!(pe("timezone +15:00\n").contains("±14:00 まで"));
    assert!(pe("timezone +09:60\n").contains("0〜59"));
}

fn mod_dir(dir: &Path, files: &[(&str, &str)]) {
    for (n, text) in files {
        std::fs::write(dir.join(format!("{}.geas", n)), text).unwrap();
    }
}

fn parse_at_err(dir: &Path, name: &str) -> String {
    let p = dir.join(format!("{}.geas", name));
    let src = std::fs::read_to_string(&p).unwrap();
    geas_core::parser::parse(&src, Some(&p)).err().map(|e| format!("{}", e)).expect("取り込みの誤りになるはず")
}

fn load_strict<R>(dir: &Path, f: impl FnOnce(&geas_core::check::Checker) -> R) -> R {
    let p = dir.join("app.geas");
    with_check_at(&std::fs::read_to_string(&p).unwrap(), Some(&p), true, f)
}

#[test]
fn モジュールにはtimezoneを書けない() {
    let t = Tmp::new("modtz");
    mod_dir(&t.0, &[("geo", "module Geo\ntimezone +09:00\n"), ("app", "use \"geo.geas\"\n")]);
    assert!(parse_at_err(&t.0, "app").contains("timezone を書けません"));
}

#[test]
fn 標準のタイムゾーンで暦の秒がずれ_引数で上書きできる() {
    let main = format!("{}fn main() {{\n    println!(\"{{}}\", t(vtime {{ year: Some(1970), month: Some(1), day: Some(2), hour: None, minute: None, second: None }}));\n}}\n", T_MAIN);
    assert_eq!(run_plain(&tz_src(), &main), vec![(86400 - 9 * 3600).to_string()]); // 日本時間の1970-01-02 0:00 は、UTC の前日 15:00
}

// ---- 待ち合わせ all・仕様書のフロー図・表示名 ----
fn shop_src() -> String {
    code()
}

fn join_src() -> String {
    shop_src().replace("reply PaymentSucceeded → apply_payment_result", "all reply PaymentSucceeded, StockReserved → apply_payment_result")
}

#[test]
fn 待ち合わせallは全部の応答がそろったら進む行として書ける() {
    with_check(&join_src(), false, |ck| {
        assert!(ck.ready(), "{:?}", ck.diags);
        let tg = ck.p.triggers.iter().find(|t| t.name == "PaymentPending").unwrap();
        assert!(tg.causes.contains(&("all".to_string(), "PaymentSucceeded,StockReserved".to_string(), "apply_payment_result".to_string())));
        assert!(build(ck)["トリガ"].as_array().unwrap().contains(&json!(["決済待ち", "全部の応答 決済成功 ＋ StockReserved", "決済結果の反映"]))); // 仕様書は表示名で出る
        assert!(generate(ck).contains("(\"all\", \"PaymentSucceeded,StockReserved\", \"apply_payment_result\")"));
    });
}

#[test]
fn 待ち合わせの書き方の誤りは止まる() {
    let one = shop_src().replace("reply PaymentSucceeded → apply_payment_result", "all reply PaymentSucceeded → apply_payment_result");
    assert!(any(&errs(&one), "2つ以上"));
    assert!(any(&errs(&join_src().replace("deadline now", "reply StockReserved → expire_payment\n  deadline now")), "両方にあります"));
    assert!(any(&errs(&join_src().replace("deadline now", "all reply StockReserved, Other → expire_payment\n  deadline now")), "複数の all の行"));
    assert!(pe(&shop_src().replace("reply PaymentSucceeded → apply_payment_result", "all PaymentSucceeded, X → apply_payment_result")).contains("all の後ろには"));
}

const NOTIFY_FAN: &str = "\neffect 通知.メール(order_id: id) no_wait \"通知は結果を待たない\"\neffect 通知.プッシュ(order_id: id) no_wait \"通知は結果を待たない\"\nresult 通知結果 {\n  Requested\n}\ndeed メール通知 {\n  input order_id: id required\n  result 通知結果\n  effects 通知.メール\n\n  request 通知.メール(order_id, idempotency_key = order_id)\n  return Requested\n}\ndeed プッシュ通知 {\n  input order_id: id required\n  result 通知結果\n  effects 通知.プッシュ\n\n  request 通知.プッシュ(order_id, idempotency_key = order_id)\n  return Requested\n}\nnexus メール {\n  メール通知: Requested → reply\n}\nnexus プッシュ {\n  プッシュ通知: Requested → reply\n}\n";

#[test]
fn 仕様書にフロー図が出る_並列は_プラスのゲートウェイ() {
    let src = mutate(&format!("{}{}", code(), NOTIFY_FAN), "place_order: Received → request_payment, OutOfStock", "place_order: Received → par { メール通知, プッシュ通知 }, OutOfStock");
    with_check(&src, false, |ck| {
        let md = render(ck, &build(ck), "");
        assert!(md.contains("```mermaid") && md.contains("flowchart LR"));
        assert_eq!(md.matches("((\"＋\"))").count(), 1);
        assert!(md.contains("-- \"受付\" -->") && md.contains("応答（reply）"));
    });
    with_check(&code(), false, |ck| {
        let plain = render(ck, &build(ck), "");
        assert!(!plain.contains("((\"＋\"))") && plain.contains("((\"×\"))") && plain.contains("待つ 決済待ち"));
    });
}

#[test]
fn 説明コメントは意味を変えず_仕様書の名前と表示名の表になる() {
    let src = mutate(&code(), "/// 顧客\nentity Customer {", "/// 買う人\n/// @en Customer\nentity Customer {");
    with_check(&src, false, |ck| {
        assert!(ck.ready(), "{:?}", ck.diags);
        assert!(build(ck)["名前"].as_array().unwrap().contains(&json!(["Customer", "既定: 買う人 / en: Customer"])));
        assert!(render(ck, &build(ck), "").contains("## 名前と表示名"));
        assert!(generate(ck) == with_check(&code(), false, |c2| generate(c2))); // 説明コメントがあってもなくても、生成されるコードは同じ
    });
    // 説明コメントが無ければ、表は出ない
    with_check("ltype 点数 = int 0..\n", false, |ck| assert!(!render(ck, &build(ck), "").contains("名前と表示名")));
}

#[test]
fn 表示名の表は覚えておく() {
    let src = mutate(&code(), "/// 顧客\nentity Customer {", "/// 買う人\nentity Customer {");
    let p = geas_core::parser::parse(&src, None).unwrap();
    assert_eq!(p.show_names("Customer の数"), "買う人 の数");
    assert!(std::ptr::eq(p.show_table(), p.show_table())); // 仕様書を作る間は、同じ表を引く
}

// ---- 名前の書き方（半角英数。種類は大文字小文字で見分ける）----
#[test]
fn お店の例は名前の書き方の規則に合っている() {
    with_check(&code(), true, |ck| assert!(ck.ok(), "{:?}", ck.diags));
}

#[test]
fn 名前に日本語は使えない_コマンドは常に検査する() {
    let m = serrs(&mutate(&code(), "ltype StockCount = int 0..", "ltype 在庫数 = int 0.."));
    assert!(m.iter().any(|x| x.contains("UpperCamel") && x.contains("在庫数")), "{:?}", m);
    let r = Command::new(BIN).env("GEAS_LANG", "ja").arg("check").arg(root().join("tests/fixtures/shop/shop.geas")).output().unwrap();
    assert!(r.status.success(), "{}{}", String::from_utf8_lossy(&r.stdout), String::from_utf8_lossy(&r.stderr));
    let t = Tmp::new("badnames");
    let bad = t.0.join("bad_names.geas");
    std::fs::write(&bad, "ltype 数量 = int 1..99\n").unwrap();
    let r = Command::new(BIN).env("GEAS_LANG", "ja").arg("check").arg(&bad).output().unwrap();
    let out = String::from_utf8_lossy(&r.stdout).to_string() + &String::from_utf8_lossy(&r.stderr);
    assert!(!r.status.success() && out.contains("UpperCamel"), "{}", out);
}

#[test]
fn 型は大文字始まり_項目と処理は小文字始まり() {
    let m = serrs(&mutate(&code(), "stock: StockCount required", "Stock: StockCount required"));
    assert!(m.iter().any(|x| x.contains("lower_snake") && x.contains("Stock")), "{:?}", m);
    let m = serrs(&mutate(&code(), "deed place_order {", "deed PlaceOrder {").replace("place_order:", "PlaceOrder:"));
    assert!(m.iter().any(|x| x.contains("lower_snake") && x.contains("PlaceOrder")), "{:?}", m);
    let m = serrs(&mutate(&code(), "state OrderStatus {", "state order_status {").replace("OrderStatus", "order_status"));
    assert!(m.iter().any(|x| x.contains("UpperCamel") && x.contains("order_status")), "{:?}", m);
}

#[test]
fn 小文字の名前は組み込みや予約語と同じにできない() {
    let m = serrs(&mutate(&code(), "  let subtotal = sum", "  let int = sum"));
    assert!(m.iter().any(|x| x.contains("予約語") && x.contains("int")), "{:?}", m);
    assert!(!any(&serrs(&code()), "予約語"));
}

#[test]
fn 名前の規則は_チェックの関数では既定で見ない() {
    assert!(okk("ltype 点数 = int 0..\n"));
}

// ---- id → 値（キーが entity のキーに限られない、読み取り専用の値の入れ物）----
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

#[test]
fn idから値への写像は_entityを介さず書ける() {
    with_check(IDMAP, true, |ck| assert!(ck.ok(), "{:?}", ck.diags));
}

#[test]
fn idの写像もltypeで名前を付けてから使う() {
    assert!(pe(&mutate(IDMAP, "input table: Table required", "input table: id → Pos required")).contains("ltype"));
}

#[test]
fn idの写像の値は合成型でも_ホストから受け取って足せる() {
    with_check(IDMAP, true, |ck| assert!(ck.ok(), "{:?}", ck.diags));
    let req = "{\"name\":\"show\",\"ctx\":{},\"input\":{\"table\":{\"a\":{\"x\":3,\"y\":null},\"b\":{\"x\":4,\"y\":5}}},\"store\":{}}";
    let out = run_app_host(IDMAP, &format!("fn main() {{\n    println!(\"{{}}\", geas_app::host::call(r#\"{}\"#).unwrap());\n}}\n", req)).join("\n");
    assert!(out.contains("Big"), "{}", out); // 3 + 4 = 7 > 6
}

// ---- モジュール（状態を持たない部品の集まり。ソースのまま取り込む）----
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

#[test]
fn モジュールを取り込んで_名前空間つきで使える() {
    let t = Tmp::new("mod1");
    mod_dir(&t.0, &[("geo", GEO), ("app", GEO_APP)]);
    load_strict(&t.0, |ck| {
        assert!(ck.ok(), "{:?}", ck.diags);
        assert_eq!(ck.p.origin["Geo_Pos"], ("Geo".to_string(), "Pos".to_string()));
        let names = build(ck)["名前"].clone();
        let get = |k: &str| names.as_array().unwrap().iter().find(|r| r[0] == k).unwrap()[1].clone();
        assert!(get("Geo_Pos") == json!("既定: 位置") && get("Geo_same") == json!("既定: そのまま")); // 表示名はモジュールの /// から
    });
}

#[test]
fn モジュールの名前は_表示名が無ければ_モジュール名と定義名で出る() {
    let t = Tmp::new("mod2");
    mod_dir(&t.0, &[("geo", &GEO.replace("/// 位置\n", "")), ("app", GEO_APP)]);
    load_strict(&t.0, |ck| assert_eq!(ck.p.show_names("Geo_Pos"), "Geo.Pos"));
}

#[test]
fn モジュールを使ったコードがビルドでき_動く() {
    let t = Tmp::new("mod3");
    mod_dir(&t.0, &[("geo", GEO), ("app", GEO_APP)]);
    let out = t.0.join("crate");
    std::fs::create_dir_all(&out).unwrap();
    let req = "{\"name\":\"show\",\"ctx\":{},\"input\":{\"table\":{\"a\":{\"x\":3,\"y\":null},\"b\":{\"x\":4,\"y\":5}},\"limit\":6},\"store\":{}}";
    load_strict(&t.0, |ck| write_crate(ck, &generate(ck), &format!("fn main() {{\n    println!(\"{{}}\", geas_app::host::call(r#\"{}\"#).unwrap());\n}}\n", req), &out, true).unwrap());
    let r = Command::new("cargo").args(["run", "--quiet", "--offline"]).env("CARGO_TARGET_DIR", common::serve::target_dir()).current_dir(&out).output().unwrap();
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 1500));
    assert!(String::from_utf8_lossy(&r.stdout).contains("Big")); // 3 + 4 = 7 > 6
}

#[test]
fn モジュールには状態を持つ宣言を書けない() {
    for decl in ["entity Item {\n  item_id: id unique\n}", "invariant Item: item_id == 1", "nexus Flow {\n  show: Big → reply\n}"] {
        let t = Tmp::new("modstate");
        mod_dir(&t.0, &[("geo", &format!("{}\n{}\n", GEO, decl)), ("app", GEO_APP)]);
        assert!(parse_at_err(&t.0, "app").contains("状態は持てません"), "{}", decl);
    }
}

#[test]
fn モジュールの取り込みの誤りは止まる() {
    let t = Tmp::new("moderr");
    mod_dir(&t.0, &[("geo", GEO), ("app", &GEO_APP.replace("Geo.same(limit)", "Geo.nothing(limit)"))]);
    assert!(parse_at_err(&t.0, "app").contains("Geo に「nothing」はありません"));
}
