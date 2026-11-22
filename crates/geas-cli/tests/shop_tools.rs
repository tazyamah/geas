// SPDX-License-Identifier: GPL-3.0-or-later
//! 変数で割る割り算・足し引きの人の引き受け（saturating・trust assume）・storage・資源枯渇・標準部品・bool・変異検査・レシート・check --json・bench の試験。
//! 元は Python 版の tests/test_geas.py の終わり（タグ python-final）。
//! 標準部品の束の生成器（stdgen：rustdoc の JSON から num.geas を作る）は Python 版にしか無く、Rust には移していない（コミットした num.geas の台帳は parts.rs が見張る）。
mod common;
use common::*;
use geas_core::check::ERROR;
use geas_core::crategen::write_crate;
use geas_core::rustgen::generate;
use geas_core::spec::{build, render};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

/// geas::mutate が CARGO_TARGET_DIR をプロセス全体に設定して消すので、このファイルの試験は 1 つずつ動かす
static L: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn lock() -> std::sync::MutexGuard<'static, ()> {
    L.lock().unwrap_or_else(|e| e.into_inner())
}

const BIN: &str = env!("CARGO_BIN_EXE_geas");

fn code() -> String {
    std::fs::read_to_string(root().join("tests/fixtures/shop/shop.geas")).unwrap()
}

fn errs(src: &str) -> Vec<String> {
    with_check(src, false, |ck| ck.diags.iter().filter(|d| d.level == ERROR).map(|d| d.msg.clone()).collect())
}

fn any(v: &[String], needle: &str) -> bool {
    v.iter().any(|m| m.contains(needle))
}

fn pe(src: &str) -> String {
    parse_err(src).unwrap_or_else(|| panic!("構文の誤りになるはず: {}", src))
}

fn ready(src: &str) {
    with_check(src, false, |ck| assert!(ck.ready(), "{:?}", ck.diags.iter().map(|d| d.msg.clone()).collect::<Vec<_>>()));
}

fn gen(src: &str) -> String {
    with_check(src, false, |ck| generate(ck))
}

// ---- 変数で割る割り算：割る数が 1 以上と示せるか、0 のときの値（or）を書く ----
const DIV_SRC: &str = r####"ltype Count = int 1..1000
ltype Total = int 0..1000000
entity Stat {
  stat_id: id unique
  average: Total required
  rest: Total required
}
result Done {
  Saved(stat_id: id)
}
deed save_average {
  input total: Total required, n: Count required
  writes Stat
  result Done
  effects none

  let stat = create Stat { average: total / n, rest: total % n }
  return Saved(stat.stat_id)
}
nexus Flow {
  save_average: Saved → reply
}
"####;

fn div_guard() -> String {
    DIV_SRC.replace("ltype Count = int 1..1000", "ltype Count = int 0..1000").replace("result Done {\n  Saved(stat_id: id)\n}", "result Done {\n  Saved(stat_id: id)\n  NoCount\n}").replace("  let stat = create", "  if n == 0 {\n    return NoCount\n  }\n  let stat = create").replace("save_average: Saved → reply", "save_average: Saved → reply, NoCount → reply")
}

fn div_or() -> String {
    DIV_SRC.replace("ltype Count = int 1..1000", "ltype Count = int 0..1000").replace("total / n, rest: total % n", "total / n or 0, rest: total % n or total")
}

#[test]
fn 変数で割れるのは_割る数が1以上と示せるか_0のときの値を書いたときだけ() {
    let _g = lock();
    for src in [DIV_SRC.to_string(), div_guard(), div_or()] {
        ready(&src);
    }
    // 範囲が 0 を含み、止めも or も無ければ止まる
    assert!(any(&errs(&mutate(DIV_SRC, "ltype Count = int 1..1000", "ltype Count = int 0..1000")), "0 になる道"));
    // 止める条件が足りなければ止まる（n < 0 は n == 0 を除かない）
    assert!(any(&errs(&div_guard().replace("if n == 0", "if n < 0")), "0 になる道"));
    // 1 以上と分かる別の書き方でも通る
    ready(&div_guard().replace("if n == 0", "if n < 1"));
    ready(&div_guard().replace("if n == 0", "if n <= 0"));
    // 割る数・0 のときの値は整数
    assert!(any(&errs(&mutate(&div_or(), "total / n or 0", "total / n or (n == 1)")), "0 のときの値"));
    // 商の範囲も求める：total / n は total 以下なので Small(0..10) には入らないが、total % n は n 未満
    let small = mutate(DIV_SRC, "rest: Total required", "rest: Total required\n  small: Tiny required").replace("total % n }", "total % n, small: total / n }").replace("ltype Total", "ltype Tiny = int 0..10\nltype Total");
    assert!(any(&errs(&small), "収まると示せません"));
    ready(&DIV_SRC.replacen("ltype Total", "ltype Tiny = int 0..999\nltype Total", 1).replace("rest: Total required", "rest: Tiny required"));
}

#[test]
fn 変数で割る割り算は_0のときの値を返し_それ以外は商と余りを返す() {
    let _g = lock();
    let rs = gen(&div_or());
    assert!(rs.contains("checked_div_euclid") && rs.contains("checked_rem_euclid"));
    let main = "use geas_app::host::call;\nfn main() {\n    for n in [0, 1, 7, 1000] {\n        let req = format!(r#\"{{\"name\":\"save_average\",\"ctx\":{{\"new_Stat_id\":\"A\"}},\"input\":{{\"total\":100,\"n\":{}}},\"store\":{{}}}}\"#, n);\n        println!(\"{:?}\", call(&req));\n    }\n}\n";
    let out = run_app_host(&div_or(), main);
    assert!(out.len() == 4 && out.iter().all(|o| o.starts_with("Ok(")), "{:?}", out);
    for (o, (q, m)) in out.iter().zip([(0, 100), (100, 0), (14, 2), (0, 100)]) {
        assert!(o.contains(&q.to_string()) && o.contains(&m.to_string()), "{} {} {}", o, q, m);
    }
}

// ---- 足し引きの範囲を人が引き受ける：saturating（端で止める）と trust assume（届かないと承知する）----
const ACC_SRC: &str = r####"ltype Hits = int 0..1000000
ltype Step = int 1..100
entity Counter {
  counter_id: id unique
  hits: Hits required
}
result Done {
  Counted
  NoCounter
}
deed bump {
  input counter_id: id required, step: Step required
  reads Counter.hits
  writes Counter.hits
  result Done
  effects none

  let c = Counter[counter_id] else { return NoCounter }
  c.hits += step
  return Counted
}
nexus Flow {
  bump: Counted → reply, NoCounter → reply
}
"####;

fn acc_sat() -> String {
    ACC_SRC.replace("c.hits += step", "c.hits += step saturating \"アクセス数は頭打ちでよい\"")
}

fn acc_assume() -> String {
    ACC_SRC.replace("c.hits += step", "c.hits += step trust assume \"1 件ずつ足すだけで上限には届かない\"")
}

#[test]
fn 足し引きの範囲は_saturatingかtrust_assumeで人が引き受けるまで示せないとして止まる() {
    let _g = lock();
    assert!(any(&errs(ACC_SRC), "収まると示せません"));
    for (src, mode) in [(acc_sat(), "saturating"), (acc_assume(), "assume")] {
        with_check(&src, false, |ck| {
            assert!(ck.ready(), "{:?}", ck.diags);
            let g = &ck.infos["bump"].guards;
            assert_eq!(g.len(), 1);
            assert_eq!((g[0].0.as_str(), g[0].1.as_str(), g[0].2.as_str()), ("Counter.hits", "+=", mode));
        });
    }
    // 理由は必須。assume 以外の trust と、= の代入には付けられない
    assert!(pe(&ACC_SRC.replace("c.hits += step", "c.hits += step saturating")).contains("理由"));
    assert!(pe(&ACC_SRC.replace("c.hits += step", "c.hits += step trust dangerous \"x\"")).contains("assume"));
    // 2026-11-16 から：prove・check・verify は「途中の計算」の trust として読む。項目の範囲の確認は外れず、計算が示せているなら要らないと言う
    let calc = errs(&ACC_SRC.replace("c.hits += step", "c.hits += step trust verify \"x\""));
    assert!(any(&calc, "収まると示せません") && any(&calc, "trust verify は要りません"), "{:?}", calc);
    // 下限の 0 で止める -= も同じ形で書ける
    ready(&acc_sat().replace("+=", "-="));
    // 人の保証の一覧に、場所と理由つきで出る
    with_check(&acc_sat(), false, |ck| {
        let rows = build(ck)["人の保証"].clone();
        assert!(rows[0][0] == "bump" && rows[0][1] == "Counter.hits +=" && rows[0][3].as_str().unwrap().contains("端で止まる") && rows[0][4] == "アクセス数は頭打ちでよい", "{}", rows);
    });
    with_check(&acc_assume(), false, |ck| assert!(build(ck)["人の保証"][0][3].as_str().unwrap().contains("届くと異常")));
    // 範囲の無い型（int 0..）は assume で足せる
    ready(&acc_assume().replace("ltype Hits = int 0..1000000", "ltype Hits = int 0.."));
}

#[test]
fn saturatingは端で止まり_trust_assumeは届いたら異常で止まる() {
    let _g = lock();
    let main = "use geas_app::host::call;\nfn main() {\n    for step in [10, 100] {\n        let req = format!(r#\"{{\"name\":\"bump\",\"ctx\":{{}},\"input\":{{\"counter_id\":\"K\",\"step\":{}}},\"store\":{{\"Counter\":{{\"K\":{{\"counter_id\":\"K\",\"hits\":999950}}}}}}}}\"#, step);\n        println!(\"{:?}\", call(&req));\n    }\n}\n";
    let sat = run_app_host(&acc_sat(), main);
    let asm = run_app_host(&acc_assume(), main);
    assert!(sat[0].contains("999960") && sat[1].contains("1000000"), "{:?}", sat);
    assert!(asm[0].contains("999960") && asm[1].starts_with("Err("), "{:?}", asm);
}

// ---- storage：保存先の定義。保存先ごとに、リクエストの直列を誰が保証するか ----
fn iso_src() -> String {
    format!("{}store Stat in ExternalDb\n", DIV_SRC)
}

/// 直列（isolation）に関する誤りだけ。同期の状態「記録」の扱いは、この試験の対象外
fn iso_errors(src: &str) -> Vec<String> {
    with_check(src, false, |ck| ck.diags.iter().filter(|d| d.code == "直列").map(|d| d.msg.clone()).collect())
}

#[test]
fn storageは保存先を定義し_直列の担い手を持ち_人の保証の一覧に出る() {
    let _g = lock();
    let iso = iso_src();
    assert!(iso_errors(&iso).join("").contains("ExternalDb")); // 定義の漏れは止まる
    with_check(DIV_SRC, false, |ck| assert_eq!(build(ck)["人の保証"], json!([]))); // store が無ければ標準の保存先（確かめてある）で、書かなくてよい
    let sqlite = format!("{}storage ExternalDb isolation store(sqlite)\n", iso);
    assert!(iso_errors(&sqlite).is_empty()); // 確かめてある種類は理由が要らず、人の保証でもない
    with_check(&sqlite, false, |ck| assert_eq!(build(ck)["人の保証"], json!([])));
    let rows = with_check(&format!("{}storage ExternalDb isolation process\n", iso), false, |ck| build(ck)["人の保証"].clone());
    assert!(rows[0][0] == "（保存先 ExternalDb）" && rows[0][1] == "リクエストの直列化" && rows[0][2].as_str().unwrap().contains("プロセスの中だけ"), "{}", rows);
    let other = format!("{}storage ExternalDb isolation store(corp_db) \"社内の DB サーバー。serializable で動かす\"\n", iso);
    let rows = with_check(&other, false, |ck| build(ck)["人の保証"].clone());
    assert!(rows[0][2] == "保存先 corp_db に任せる" && rows[0][4] == "社内の DB サーバー。serializable で動かす", "{}", rows);
    // 確かめていない種類は理由が必須。使われていない保存先の宣言も止まる
    assert!(any(&iso_errors(&format!("{}storage ExternalDb isolation store(corp_db)\n", iso)), "理由"));
    assert!(any(&iso_errors(&format!("{}storage ExternalDb isolation process\nstorage Other isolation process\n", iso)), "どの entity"));
    for (bad, pat) in [("storage ExternalDb isolation redis", "process"), ("storage ExternalDb process", "storage は"), ("storage ExternalDb isolation store", "("), ("storage ExternalDb isolation process\nstorage ExternalDb isolation process", "2つ以上")] {
        let e = pe(&format!("{}{}\n", iso, bad));
        assert!(e.contains(pat), "{} {}", bad, e);
    }
}

#[test]
fn 仕様書に_entityごとの保存先と直列の担い手が出る() {
    let _g = lock();
    let src = format!("{}storage ExternalDb isolation store(sqlite)\n", iso_src());
    with_check(&src, false, |ck| {
        assert_eq!(build(ck)["保存先"], json!([["Stat", "ExternalDb", "保存先が直列にする（sqlite）"]]));
        assert!(render(ck, &build(ck), "").contains("## 保存先"));
    });
    with_check(DIV_SRC, false, |ck| {
        assert!(!render(ck, &build(ck), "").contains("## 保存先"));
        assert_eq!(build(ck)["保存先"], json!([]));
    });
}

// ---- 資源枯渇：長さの上限を書かない文字列は、既定の上限で境界に止める ----
const TEXT_SRC: &str = r####"ltype Title = text 1..10
entity Note {
  note_id: id unique
  body: text required
  title: Title required
}
result Done {
  Saved(note_id: id)
}
deed save_note {
  input body: text required, title: Title required
  writes Note
  result Done
  effects none

  let n = create Note { body: body, title: title }
  return Saved(n.note_id)
}
nexus Flow {
  save_note: Saved → reply
}
"####;

#[test]
fn 長さを書かない文字列は既定の上限で止まり_深すぎる入れ子も止まる() {
    let _g = lock();
    ready(TEXT_SRC);
    assert!(gen(TEXT_SRC).contains("TEXT_LIMIT"));
    let main = "use geas_app::host::call;\nfn main() {\n    let ok = \"あ\".repeat(1_048_576);\n    let big = \"あ\".repeat(1_048_577);\n    for body in [ok.as_str(), big.as_str()] {\n        let req = format!(r#\"{{\"name\":\"save_note\",\"ctx\":{{\"new_Note_id\":\"A\"}},\"input\":{{\"body\":\"{}\",\"title\":\"T\"}},\"store\":{{}}}}\"#, body);\n        let r = call(&req);\n        println!(\"{}\", match &r { Ok(_) => \"ok\".to_string(), Err(e) => format!(\"err {}\", e.chars().take(40).collect::<String>()) });\n    }\n    let deep = format!(\"{}{}\", \"[\".repeat(100_000), \"]\".repeat(100_000));\n    let req = format!(r#\"{{\"name\":\"save_note\",\"ctx\":{{}},\"input\":{{\"body\":\"x\",\"title\":\"T\",\"extra\":{}}},\"store\":{{}}}}\"#, deep);\n    println!(\"{}\", match call(&req) { Ok(_) => \"ok\".to_string(), Err(e) => format!(\"err {}\", e.chars().take(40).collect::<String>()) });\n}\n";
    let got = run_app_host(TEXT_SRC, main);
    assert_eq!(got[0], "ok");
    assert!(got[1].starts_with("err") && got[1].contains("長さ"), "{}", got[1]);
    assert!(got[2].starts_with("err") && got[2].contains("入れ子"), "{}", got[2]);
}

#[test]
fn 処理の定義に_触れるentityが出る_サーバーはそれだけを読み込む() {
    let _g = lock();
    with_check(&code(), false, |ck| {
        let d = geas_core::hostgen::definitions(ck).unwrap()["deeds"].clone();
        let touches = |n: &str| -> std::collections::BTreeSet<String> { d[n]["touches"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect() };
        let t = touches("place_order");
        for e in ["Product", "Customer", "Order"] {
            assert!(t.contains(e), "{:?}", t);
        }
        // 宣言した読み書きと作る entity が、すべて入っている（足りないと、処理が空の entity を見て誤った結果を返す）
        for (name, info) in &ck.infos {
            let mut want: std::collections::BTreeSet<String> = info.reads().iter().map(|r| r.split('.').next().unwrap().to_string()).collect();
            want.extend(info.writes.iter().map(|w| w.split('.').next().unwrap().to_string()));
            want.extend(info.new_ids.iter().cloned());
            assert!(want.is_subset(&touches(name)), "{} {:?} {:?}", name, want, touches(name));
        }
    });
}

// ---- 標準部品 ----
const STD_APP: &str = r####"use "num.geas"

ltype Small = int 0..255
ltype Qty = int 0..1000000

entity Item {
  item_id: id unique
  stock: Qty required
}
result PutResult {
  Put(item_id: id)
}
deed put_item {
  input a: Small required, b: Small required
  writes Item
  result PutResult
  effects none

  let item = create Item { stock: Num.u8_wrapping_add(a, b) }
  return Put(item.item_id)
}
nexus Flow {
  put_item: Put → reply
}
"####;

fn num_path() -> PathBuf {
    root().join("crates/geas-core/assets/std/num.geas")
}

fn std_dir(dir: &Path, src: &str) -> PathBuf {
    std::fs::copy(num_path(), dir.join("num.geas")).unwrap();
    let f = dir.join("app.geas");
    std::fs::write(&f, src).unwrap();
    f
}

fn std_check<R>(src: &str, tag: &str, f: impl FnOnce(&geas_core::check::Checker) -> R) -> R {
    let t = Tmp::new(tag);
    let p = std_dir(&t.0, src);
    with_check_at(src, Some(&p), false, f)
}

fn std_errs(src: &str, tag: &str) -> Vec<String> {
    std_check(src, tag, |ck| ck.diags.iter().filter(|d| d.level == ERROR).map(|d| d.msg.clone()).collect())
}

#[test]
fn 標準部品の束を取り込んで呼べ_呼んだものだけが人の保証に出る() {
    let _g = lock();
    std_check(STD_APP, "std1", |ck| {
        assert!(!ck.diags.iter().any(|d| d.level == ERROR), "{:?}", ck.diags);
        let sp = build(ck);
        let rows = sp["人の保証"].as_array().unwrap();
        assert_eq!(rows.iter().map(|r| r[0].as_str().unwrap()).collect::<Vec<_>>(), vec!["（ライブラリ Num.u8_wrapping_add）"], "{:?}", rows); // 束に 200 以上あっても、呼んだ1つだけ
        assert!(rows[0][1].as_str().unwrap().contains("std") && rows[0][1].as_str().unwrap().contains("wrapping_add"));
        assert_eq!(sp["部品"].as_array().unwrap().len(), 1);
        assert!(!generate(ck).contains("u8_wrapping_mul"));
    });
}

#[test]
fn 標準部品は_rustの型の範囲を引数に課し_戻り値の範囲も_rustの型から決まる() {
    let _g = lock();
    let wide = STD_APP.replace("ltype Small = int 0..255", "ltype Small = int 0..256");
    assert!(any(&std_errs(&wide, "std2a"), "範囲")); // u8 に収まると示せない値は渡せない
    let big = STD_APP.replace("ltype Qty = int 0..1000000", "ltype Qty = int 0..100");
    assert!(any(&std_errs(&big, "std2b"), "収まると示せません")); // 戻り値 0..255 は 0..100 に収まらない
}

fn run_std_host(src: &str, main: &str, tag: &str) -> Vec<String> {
    let t = Tmp::new(tag);
    let p = std_dir(&t.0, src);
    let out = t.0.join("crate");
    with_check_at(src, Some(&p), false, |ck| {
        assert!(!ck.diags.iter().any(|d| d.level == ERROR), "{:?}", ck.diags);
        write_crate(ck, &generate(ck), main, &out, true).unwrap();
    });
    let r = Command::new("cargo").args(["run", "--quiet", "--offline"]).env("CARGO_TARGET_DIR", common::serve::target_dir()).current_dir(&out).output().unwrap();
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 2500));
    String::from_utf8_lossy(&r.stdout).lines().map(|s| s.to_string()).collect()
}

#[test]
fn 標準部品は生成物の中で実際に動く() {
    let _g = lock();
    let main = "use geas_app::host::call;\nfn main() { println!(\"{:?}\", call(r#\"{\"name\":\"put_item\",\"ctx\":{\"new_Item_id\":\"A\"},\"input\":{\"a\":200,\"b\":100},\"store\":{}}\"#)); }\n";
    let out = run_std_host(STD_APP, main, "std3").join("\n");
    assert!(out.starts_with("Ok(") && out.contains("\\\"stock\\\":44"), "{}", out); // 200u8 の wrapping_add(100) は 44
}

const BOOL_SRC: &str = r####"ltype Qty = int 0..100

entity Item {
  item_id: id unique
  stock: Qty required
  active: bool required
}
result Res {
  Done(item_id: id)
  Off
}
deed set_active {
  input item_id: id required, on: bool required
  reads Item.item_id
  writes Item.active
  result Res
  effects none

  let item = Item[item_id] else { return Off }
  item.active = on
  return Done(item.item_id)
}
nexus Flow {
  set_active: Done → reply, Off → reply
}
"####;

#[test]
fn boolの型と式の検査() {
    let _g = lock();
    with_check(BOOL_SRC, false, |ck| {
        assert!(ck.ready(), "{:?}", ck.diags);
        assert_eq!(geas_core::ast::show_type(&ck.data["Item"].fields[2].r#type).unwrap(), "真偽");
    });
    let ng = |old: &str, new: &str| !errs(&BOOL_SRC.replace(old, new)).is_empty();
    assert!(ng("item.active = on", "item.active = 1")); // 整数は bool の項目に書けない
    assert!(ng("item.active = on", "item.active = item.stock"));
    assert!(ng("let item = Item[item_id] else { return Off }", "let item = Item[item_id] else { return Off }\n  if not item.stock { return Off }")); // not は真偽だけ
    assert!(ng("let item = Item[item_id] else { return Off }", "let item = Item[item_id] else { return Off }\n  if item.stock and on { return Off }"));
    assert!(ng("let item = Item[item_id] else { return Off }", "let item = Item[item_id] else { return Off }\n  if item.active == 1 { return Off }")); // 違う型どうしの比較
    assert!(parse_err(&BOOL_SRC.replace("on: bool required", "on: bool 0..1 required")).is_some()); // bool に範囲は無い
    assert!(errs(&BOOL_SRC.replace("item.active = on", "item.active = not on or true")).is_empty());
}

#[test]
fn 標準部品の_boolを返す関数は条件に使える() {
    let _g = lock();
    let src = STD_APP.replace("  return Put(item.item_id)", "  if not Num.u8_is_power_of_two(a) { return Put(item.item_id) }\n  return Put(item.item_id)");
    std_check(&src, "std4", |ck| {
        assert!(!ck.diags.iter().any(|d| d.level == ERROR), "{:?}", ck.diags);
        assert!(build(ck)["人の保証"].as_array().unwrap().iter().any(|r| r[0].as_str().unwrap().contains("u8_is_power_of_two")));
    });
    assert!(std::fs::read_to_string(num_path()).unwrap().contains("-> bool"));
}

#[test]
fn 標準部品の_boolを返す関数は生成物の中で動く() {
    let _g = lock();
    let src = STD_APP.replace("let item = create Item { stock: Num.u8_wrapping_add(a, b) }", "let item = create Item { stock: if Num.u8_is_power_of_two(a) then 1 else Num.u8_wrapping_add(a, b) }");
    let main = "use geas_app::host::call;\nfn main() {\n    for a in [200, 64] {\n        println!(\"{:?}\", call(&format!(r#\"{{\"name\":\"put_item\",\"ctx\":{{\"new_Item_id\":\"A\"}},\"input\":{{\"a\":{},\"b\":100}},\"store\":{{}}}}\"#, a)));\n    }\n}\n";
    let lines = run_std_host(&src, main, "std5");
    assert!(lines[0].contains("\\\"stock\\\":44") && lines[1].contains("\\\"stock\\\":1"), "{:?}", lines); // 200 は 2 の冪でない（200+100 は 44）。64 は 2 の冪（1）
}

// ---- 変異検査（geas mutate）：約束を 1 つ崩した版を、検査器が止めるか ----
use geas_core::mutate as mt;

#[test]
fn 変異検査は_不変条件を守る確認を消した版を検出する() {
    let _g = lock();
    let t = Tmp::new("mut1");
    let f = t.0.join("shop.geas");
    std::fs::write(&f, code()).unwrap();
    let (ms, why) = mt::run(f.to_str().unwrap(), None).unwrap();
    assert!(why.is_none() && !ms.is_empty());
    let by = |before: &str| ms.iter().find(|m| m.kind == "確認を消す" && m.before.trim() == before).unwrap_or_else(|| panic!("{}", before)).result.clone();
    assert_eq!(by("if order.total <= 0 { return ZeroTotal }"), mt::KILLED); // 不変条件「合計 > 0」の守り
    assert_eq!(by("if short is nonempty { return OutOfStock(short) }"), mt::KILLED); // 不変条件「在庫 >= 0」の守り
    assert!(ms.iter().all(|m| [mt::KILLED, mt::SURVIVED, mt::INVALID].contains(&m.result.as_str())));
    let text = mt::report(&ms, "shop.geas", "すべての行");
    assert!(text.contains("変異検査") && text.contains("検出"));
}

#[test]
fn 変異検査は_前の版から変わった行だけを崩し_元が通らなければ始めない() {
    let _g = lock();
    let t = Tmp::new("mut2");
    let f = t.0.join("shop.geas");
    std::fs::write(&f, code()).unwrap();
    let line = "  if order.total <= 0 { return ZeroTotal }\n";
    let base = code().replace(line, "  if order.total < 0 { return ZeroTotal }\n"); // この 1 行だけが変わった
    let (ms, _) = mt::run(f.to_str().unwrap(), Some(&base)).unwrap();
    let befores: std::collections::BTreeSet<String> = ms.iter().map(|m| m.before.trim().to_string()).collect();
    assert!(!ms.is_empty() && befores == [line.trim().to_string()].into_iter().collect());
    std::fs::write(&f, code().replace(line, "")).unwrap(); // 元の版が不変条件を示せない
    let (ms, why) = mt::run(f.to_str().unwrap(), None).unwrap();
    assert!(ms.is_empty() && why.unwrap().contains("始めません"));
}

#[test]
fn 変異検査のコマンドは_結果を書き出す() {
    let _g = lock();
    let t = Tmp::new("mut3");
    let f = t.0.join("shop.geas");
    let out = t.0.join("m.md");
    std::fs::write(&f, code()).unwrap();
    let r = Command::new(BIN).env("GEAS_LANG", "ja").arg("mutate").arg(&f).arg("-o").arg(&out).output().unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert!(std::fs::read_to_string(&out).unwrap().starts_with("# 変異検査"));
}

#[test]
fn お店アプリの変異検査の結果は_コミットした見本と一致する() {
    let _g = lock();
    // 崩した版が増えた・生き残りが変わったときに落とす。直したときは、
    // `geas mutate tests/fixtures/overseas_shop/overseas_shop.geas -o tests/fixtures/overseas_shop/overseas_shop_mutation.md` で作り直す
    let f = root().join("tests/fixtures/overseas_shop/overseas_shop.geas");
    let (ms, why) = mt::run(f.to_str().unwrap(), None).unwrap();
    assert!(why.is_none());
    let want = std::fs::read_to_string(root().join("tests/fixtures/overseas_shop/overseas_shop_mutation.md")).unwrap();
    assert!(mt::report(&ms, "overseas_shop.geas", "すべての行") == want, "見本と違います。geas mutate で作り直してください");
}

const RATIO_SRC: &str = r####"ltype NonNegMoney = money(JPY) 0..
ltype Rate = ratio 0..1
entity Item {
  item_id: id unique
  price: NonNegMoney required
}
result PutResult {
  Put(item_id: id)
}
deed put_item {
  input base: NonNegMoney required, tax_rate: Rate required, discount: Rate required
  writes Item
  result PutResult
  effects none

  let item = create Item { price: round(base * tax_rate * discount, half_up, JPY) }
  return Put(item.item_id)
}
nexus Flow {
  put_item: Put → reply
}
"####;

fn trust_src(level: &str) -> String {
    RATIO_SRC.replace("tax_rate: Rate required,", &format!("tax_rate: Rate required{},", level))
}

#[test]
fn 前置きの範囲と割合の確認は_判定と文言に分かれている() {
    let _g = lock();
    // 判定（真偽だけ）は文言を組み立てないので、Kani で証明できる。check_* はそれを呼ぶだけ
    let src = gen(&trust_src(""));
    assert!(src.contains("pub fn range_ok(x: i128, lo: Option<i128>, hi: Option<i128>) -> bool"));
    assert!(src.contains("pub fn ratio_ok(x: &Ratio, lo: Option<i128>, hi: Option<i128>) -> bool"));
    assert!(src.contains("if !range_ok(x, lo, hi) {") && src.contains("if !ratio_ok(x, lo, hi) {"));
}

// ---- 変更ごとのレシート（geas receipt／差分 HTML の節）：前の版と比べた、信頼に関わる変化 ----
fn receipt_of(new: &str, base: &str, path: &str) -> geas_core::receipt::Receipt {
    with_check(new, false, |ck| with_check(base, false, |bck| geas_core::receipt::build_receipt(ck, bck, path, Some(base), "前の版", None)))
}

#[test]
fn レシートは_保証が弱くなったものと_増えた人の保証を先頭に出す() {
    let _g = lock();
    use geas_core::receipt::{to_html, to_markdown};
    let base_src = trust_src(" trust verify \"仕入れ先が保証\"");
    let new_src = trust_src(" trust assume \"契約 §4\"");
    let r = receipt_of(&new_src, &base_src, "app.geas");
    assert_eq!(r.weakened.iter().map(|w| (w.1.clone(), w.2.clone())).collect::<Vec<_>>(), vec![("検証ビルドだけ確かめる".to_string(), "確かめない（人の保証）".to_string())]); // verify → assume
    assert!(r.added_weak.is_empty() && r.strengthened.is_empty() && r.errors == 0);
    let md = to_markdown(&r);
    assert!(md.find("保証が弱くなったもの").unwrap() < md.find("保証の強さの件数").unwrap() && md.contains("契約 §4"));
    assert!(to_html(&r).contains("保証が弱くなったもの"));
    // 前に無かった人の保証は「増えた」として出る。何も変わらなければ、弱くなったものは無い
    let added = receipt_of(&new_src, &trust_src(""), "app.geas");
    assert_eq!(added.added_weak.iter().map(|a| (a.0.clone(), a.1.clone())).collect::<Vec<_>>(), vec![("確かめない（人の保証）".to_string(), "put_item tax_rate".to_string())]);
    let same = receipt_of(&new_src, &new_src, "app.geas");
    assert!(same.weakened.is_empty() && same.added_weak.is_empty() && to_markdown(&same).contains("ありません"));
}

#[test]
fn レシートのコマンドと差分htmlは_同じ内容を出す() {
    let _g = lock();
    let t = Tmp::new("receipt");
    let (f, base, out, html) = (t.0.join("app.geas"), t.0.join("base.geas"), t.0.join("r.md"), t.0.join("d.html"));
    std::fs::write(&base, trust_src(" trust verify \"仕入れ先が保証\"")).unwrap();
    std::fs::write(&f, trust_src(" trust assume \"契約 §4\"")).unwrap();
    let run = |args: &[&std::ffi::OsStr]| Command::new(BIN).env("GEAS_LANG", "ja").args(args).output().unwrap();
    use std::ffi::OsStr;
    let r = run(&[OsStr::new("receipt"), f.as_os_str(), OsStr::new("--base"), base.as_os_str(), OsStr::new("-o"), out.as_os_str()]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.starts_with("## 信頼のレシート") && text.contains("契約 §4"), "{}", text);
    assert_eq!(run(&[OsStr::new("receipt"), f.as_os_str()]).status.code(), Some(2)); // 前の版が無ければ断る
    let rh = t.0.join("r.html");
    let r = run(&[OsStr::new("review"), f.as_os_str(), OsStr::new("-o"), rh.as_os_str(), OsStr::new("--diff-o"), html.as_os_str(), OsStr::new("--base"), base.as_os_str()]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let h = std::fs::read_to_string(&html).unwrap();
    assert!(h.contains("id=\"receipt\"") && h.contains("信頼のレシート") && h.contains("検証ビルドだけ確かめる → 確かめない（人の保証）"));
    assert!(h.find("id=\"receipt\"").unwrap() < h.find("id=\"changes\"").unwrap()); // 変更点の前に出る
}

// ---- 開発ガイド対応：check --json・変異検査の --run ----
#[test]
fn check_jsonは_指摘に直すための手がかりを付ける() {
    let _g = lock();
    let t = Tmp::new("checkjson");
    let bad = t.0.join("bad.geas");
    std::fs::write(&bad, code().replace("  if order.total <= 0 { return ZeroTotal }\n", "")).unwrap();
    let r = Command::new(BIN).env("GEAS_LANG", "ja").arg("check").arg(&bad).arg("--json").output().unwrap();
    let d: Value = serde_json::from_slice(&r.stdout).unwrap();
    assert!(r.status.code() == Some(1) && d["ok"] == json!(false));
    let err = d["diagnostics"].as_array().unwrap().iter().find(|x| x["level"] == "error").unwrap().clone();
    assert!(!err["code"].as_str().unwrap().is_empty() && !err["message"].as_str().unwrap().is_empty());
    assert!(err["hint"]["ref"].as_str().unwrap().starts_with("docs/language-reference.md") && !err["hint"]["fix"].as_str().unwrap().is_empty());
}

#[test]
fn check_jsonは_構文エラーも_jsonで返す() {
    let _g = lock();
    let t = Tmp::new("checkjson2");
    let bad = t.0.join("syn.geas");
    std::fs::write(&bad, "entity A { x: int 0..5 }\n").unwrap();
    let r = Command::new(BIN).env("GEAS_LANG", "ja").arg("check").arg(&bad).arg("--json").output().unwrap();
    let d: Value = serde_json::from_slice(&r.stdout).unwrap();
    assert!(r.status.code() == Some(2) && d["diagnostics"][0]["level"] == "syntax");
}

#[test]
fn 検査器が出す指摘の種類は_すべて手がかりの表にある() {
    let _g = lock();
    let re = regex::Regex::new(r#"\.d\((?:ERROR|WARN|QUESTION), "([^"]+)""#).unwrap();
    let mut codes = std::collections::BTreeSet::new();
    for e in std::fs::read_dir(root().join("crates/geas-core/src/check")).unwrap().flatten() {
        let text = std::fs::read_to_string(e.path()).unwrap();
        for c in re.captures_iter(&text) {
            codes.insert(c[1].to_string());
        }
    }
    assert!(codes.len() > 20, "{:?}", codes);
    let default = geas_core::hints::table("（表にない種類）");
    let missing: Vec<&String> = codes.iter().filter(|c| geas_core::hints::table(c) == default).collect();
    assert!(missing.is_empty(), "geas-core の hints.rs の table に足してください: {:?}", missing);
}

#[test]
fn 変異検査の_runは_検査器が止めなかった崩しを_具体例で止める() {
    let _g = lock();
    let t = Tmp::new("mutrun");
    let f = t.0.join("shop.geas");
    std::fs::write(&f, code()).unwrap();
    let lines: Vec<String> = code().lines().map(|s| s.to_string()).collect();
    let i = lines.iter().position(|l| l.contains("let total = subtotal + round(")).unwrap() + 1;
    // 税を落とす崩しは、型の上では通る（検査器は止めない）が、具体例の合計が合わなくなる
    let mut mu = mt::Mutant { line: i, kind: "税を落とす（試験用）".into(), before: lines[i - 1].clone(), after: Some("  let total = subtotal".into()), result: mt::SURVIVED.into(), why: String::new() };
    assert!(!mt::stopped(&mt::apply(&code(), &mu), f.to_str().unwrap()).unwrap().0);
    let plan = std::fs::read_to_string(root().join("tests/fixtures/shop/shop_plan.md")).unwrap();
    let note = mt::run_examples(f.to_str().unwrap(), std::slice::from_mut(&mut mu), &plan).unwrap();
    assert!(note.is_none() && mu.result == mt::KILLED_EX, "{:?} {}", note, mu.result);
}

#[test]
fn レシートの影響範囲は_変わった処理から_urlと画面とユースケースをたどる() {
    let _g = lock();
    let ov = root().join("tests/fixtures/overseas_shop");
    let src = std::fs::read_to_string(ov.join("overseas_shop.geas")).unwrap();
    let old_src = src.replace("ltype Quantity = int 1..99", "ltype Quantity = int 1..50");
    let plan = geas_core::plan::read_plan(&std::fs::read_to_string(ov.join("overseas_shop_plan.md")).unwrap());
    let rows = plan.get("ユースケース").cloned().unwrap_or_default();
    let p = ov.join("overseas_shop.geas");
    let imp = |new: &str, old: &str| -> Vec<(String, Vec<String>)> {
        with_check_at(new, Some(&p), true, |ck| with_check_at(old, Some(&p), true, |bck| geas_core::receipt::build_receipt(ck, bck, p.to_str().unwrap(), Some(old), "前の版", Some(&rows)).impact))
    };
    let get = |v: &[(String, Vec<String>)], k: &str| v.iter().find(|(n, _)| n == k).map(|(_, x)| x.clone()).unwrap_or_default();
    let i = imp(&src, &old_src);
    assert!(get(&i, "処理").iter().any(|x| x == "注文") && get(&i, "データ").iter().any(|x| x == "注文"), "{:?}", i);
    assert!(get(&i, "ルート").iter().any(|x| x == "POST /orders") && get(&i, "画面").iter().any(|x| x == "注文の画面"), "{:?}", i);
    assert!(get(&i, "ユースケース").iter().any(|x| x == "商品を見つけて注文する"), "{:?}", i);
    assert!(imp(&src, &src).iter().all(|(_, v)| v.is_empty()));
}

#[test]
fn benchは_aiが書いたgeasを採点し_悪くなったケースを前回と比べて示す() {
    let _g = lock();
    use geas_core::bench;
    let t = Tmp::new("bench");
    let (cases, runs) = (t.0.join("cases"), t.0.join("runs"));
    let plan = std::fs::read_to_string(root().join("tests/fixtures/shop/shop_plan.md")).unwrap();
    for (name, c) in [("ok", code()), ("bad", code().replace("  if order.total <= 0 { return ZeroTotal }\n", "")), ("syn", "entity A { x: int 0..5 }\n".to_string())] {
        std::fs::create_dir_all(cases.join(name)).unwrap();
        std::fs::write(cases.join(name).join("plan.md"), &plan).unwrap();
        std::fs::create_dir_all(runs.join(name)).unwrap();
        std::fs::write(runs.join(name).join("app.geas"), c).unwrap();
        std::fs::write(runs.join(name).join("rounds.txt"), "3\n").unwrap();
    }
    std::fs::create_dir_all(cases.join("none")).unwrap();
    std::fs::write(cases.join("none").join("plan.md"), &plan).unwrap();
    let scores: Vec<bench::Score> = bench::run_bench(&cases, &runs, false);
    let by = |n: &str| scores.iter().find(|s| s.case == n).unwrap();
    assert!(by("ok").passed() && by("ok").rounds == Some(3));
    assert!(by("bad").status == "検査で止まる" && !by("bad").passed());
    assert!(by("syn").status == "構文エラー" && by("none").status == "書かれていない");
    let prev: Value = serde_json::from_str(&bench::to_json(&[bench::Score { case: "bad".into(), status: "通過".into(), examples: "合った".into(), ..Default::default() }])).unwrap();
    let md = bench::to_markdown(&scores, Some(&prev));
    assert!(md.contains("悪くなった（通っていたのに通らない）：bad"), "{}", md);
    let out = t.0.join("r.json");
    let r = Command::new(BIN).env("GEAS_LANG", "ja").arg("bench").arg(&cases).arg(&runs).args(["--no-run", "--json"]).arg(&out).output().unwrap();
    assert!(r.status.code() == Some(1) && !serde_json::from_str::<Value>(&std::fs::read_to_string(&out).unwrap()).unwrap()["cases"].as_array().unwrap().is_empty());
}
