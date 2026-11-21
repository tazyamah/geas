// SPDX-License-Identifier: GPL-3.0-or-later
//! 言語機能ごとの動作テスト（数と集合）。負の数・min・max・abs・if 式・in・len を、生成した Rust で動かす（Python 版 tests/test_numbers.py の移し）。
//! どの機能にどのテストがあるかは docs/language-test-map.md。
mod common;
use common::*;
use geas_core::check::ERROR;

const CLAMP: &str = r####"ltype Qty = int 0..99
ltype Big = int -200..200
ltype Yen = money(JPY) -5000..5000

entity Item {
  item_id: id unique
  stock: Qty required
  total: Big required
  fee: Yen required
}

result Res { Done(stock: Qty, total: Big, fee: Yen), NoItem }

convert span(a: Big, b: Big) -> Big = max(min(abs(a - b), 200), -200)

deed adjust {
  input item_id: id required, m: Big required, y: Yen required
  reads Item.stock, Item.total, Item.fee
  writes Item.stock, Item.total, Item.fee
  result Res
  effects none

  let it = Item[item_id] else { return NoItem }
  it.stock = max(min(m, 99), 0)
  it.total = if m > 100 or m < -100 then -m else span(m, -7)
  it.fee = -max(y, -10, 0 - abs(y))
  return Done(it.stock, it.total, it.fee)
}
"####;

const MEMBERS: &str = r####"ltype Tags = set of id
ltype Name = text 1..20
ltype Qty = int 0..99
ltype Stock = Item → Qty max 3

entity Item {
  item_id: id unique
  tags: Tags required
  extra: Tags
  name: Name required
  size: Qty required
}

result Res { Tagged, Untagged(count: Qty, letters: Qty), InStock, NoItem }

deed look {
  input item_id: id required, tag: id required, stock: Stock required
  reads Item.tags, Item.extra, Item.name
  writes Item.size
  result Res
  effects none

  let it = Item[item_id] else { return NoItem }
  if item_id in stock { return InStock }
  if tag in it.tags { return Tagged }
  if it.extra is null { return Untagged(min(len(it.tags), 99), len(it.name)) }
  if tag in it.extra { return Tagged }
  it.size = len(stock) + len(it.name) + min(len(it.tags) + len(it.extra), 50)
  return Untagged(min(len(it.extra), 99), len(it.name))
}
"####;

#[test]
fn 負の数とminmaxabsは生成したrustでも式どおりに動く() {
    let main = "use geas_app::*;\nfn main() {\n    let mut s = Store::default();\n    s.Item.insert(\"a\".into(), Item { item_id: \"a\".into(), stock: 1, total: 1, fee: 1 });\n    for (m, y) in [(150, 30), (-150, -30), (50, -3), (-200, 5000), (0, 0)] {\n        let o = adjust(&adjustCtx {}, &s, &adjustInput { item_id: \"a\".into(), m, y }).unwrap();\n        println!(\"{}\", o.result.show());\n    }\n}\n";
    assert_eq!(run_app(CLAMP, main), strs(&["Done(99, -150, -30)", "Done(0, 150, 10)", "Done(50, 57, 3)", "Done(0, 200, -5000)", "Done(0, 7, 0)"]));
}

#[test]
fn inとlenは生成したrustでも動く() {
    let main = "use geas_app::*;\nuse std::collections::{BTreeMap, BTreeSet};\nfn set(xs: &[&str]) -> BTreeSet<String> { xs.iter().map(|x| x.to_string()).collect() }\nfn main() {\n    let mut s = Store::default();\n    s.Item.insert(\"a\".into(), Item { item_id: \"a\".into(), tags: set(&[\"red\", \"new\"]), extra: None, name: \"りんご\".into(), size: 0 });\n    s.Item.insert(\"b\".into(), Item { item_id: \"b\".into(), tags: set(&[\"red\"]), extra: Some(set(&[\"sale\", \"big\", \"old\"])), name: \"pear\".into(), size: 0 });\n    let stock: BTreeMap<String, u8> = [(\"z\".to_string(), 1u8), (\"y\".to_string(), 2u8)].into_iter().collect();\n    for (id, tag) in [(\"a\", \"red\"), (\"a\", \"blue\"), (\"b\", \"sale\"), (\"b\", \"blue\"), (\"z\", \"red\"), (\"q\", \"red\")] {\n        let o = look(&lookCtx {}, &s, &lookInput { item_id: id.into(), tag: tag.into(), stock: stock.clone() }).unwrap();\n        let wrote: Vec<String> = o.Write.iter().map(|w| format!(\"{:?}\", w)).collect();\n        println!(\"{} {}\", o.result.show(), wrote.join(\";\"));\n    }\n}\n";
    assert_eq!(run_app(MEMBERS, main), strs(&["Tagged ", "Untagged(2, 3) ", "Tagged ", "Untagged(3, 4) Item_size(\"b\", 10)", "NoItem ", "NoItem "]));
}

// ---- 検査（範囲の証明）----
const BASE: &str = r####"ltype Qty = int 0..99
ltype Big = int -200..200
ltype Yen = money(JPY) -5000..5000

entity Item {
  item_id: id unique
  stock: Qty required
  total: Big required
  fee: Yen required
}

result Res { Done, NoItem, Bad }

deed adjust {
  input item_id: id required, n: Qty required, m: Big required, y: Yen required
  reads READS
  writes WRITES
  result Res
  effects none

  let it = Item[item_id] else { return NoItem }
BODY
  return Done
}
"####;

fn base_src(body: &str, writes: &str, reads: &str) -> String {
    BASE.replace("BODY", body).replace("WRITES", writes).replace("READS", reads)
}

/// reads・writes の宣言のずれ（コード「宣言」）はここでは見ない
fn errs_with(src: &str) -> Vec<String> {
    with_check(src, false, |ck| ck.diags.iter().filter(|d| d.level == ERROR && d.code != "宣言").map(|d| d.msg.clone()).collect())
}

fn errs(body: &str) -> Vec<String> {
    errs_with(&base_src(body, "Item.total", "Item"))
}

fn errs_w(body: &str, writes: &str) -> Vec<String> {
    errs_with(&base_src(body, writes, "Item"))
}

fn any(v: &[String], needles: &[&str]) -> bool {
    v.iter().any(|m| needles.iter().all(|n| m.contains(n)))
}

#[test]
fn 負の数と符号の反転が書け_範囲も符号を返して示す() {
    assert!(errs("  it.total = -5").is_empty());
    assert!(errs("  it.total = -n").is_empty()); // 0..99 の反転は -99..0
    assert!(any(&errs("  it.total = 3 - -n * 2"), &["3..201"])); // 引く数が負でも、符号を返して足す
    assert!(errs("  it.total = 2 - -n * 2").is_empty());
    assert!(any(&errs_w("  it.stock = -n * 2", "Item.stock"), &["収まると示せません", "-198..0"]));
    assert!(errs_w("  it.fee = -y", "Item.fee").is_empty()); // 金額の反転は金額
    assert!(any(&errs("  it.total = -201"), &["-201"]));
    assert!(parse_err(&base_src("  it.total = n / -2", "Item.total", "Item")).is_some()); // 負の定数では割れない（前からの決まり）
}

#[test]
fn minmaxabsの範囲を示せる() {
    assert!(!errs_w("  it.stock = min(m, 99)", "Item.stock").is_empty()); // 下が -200 のまま
    assert!(errs_w("  it.stock = max(min(m, 99), 0)", "Item.stock").is_empty()); // 0..99 に挟む
    assert!(errs_w("  it.stock = min(abs(m), 99)", "Item.stock").is_empty()); // abs は 0..200
    assert!(errs("  it.total = abs(m)").is_empty());
    assert!(errs("  it.total = max(n, m, 5)").is_empty()); // 3 つ以上も書ける
    assert!(any(&errs("  it.total = abs(m) * 2"), &["0..400"]));
    assert!(errs_w("  it.fee = max(y, 0)", "Item.fee").is_empty()); // 数字は金額としても使える
    assert!(any(&errs_w("  it.fee = max(y, n)", "Item.fee"), &["整数どうしか金額どうし"]));
    assert!(any(&errs("  it.total = min(m)"), &["引数は2つ以上"]));
    assert!(any(&errs("  it.total = abs(m, n)"), &["引数は1つ"]));
}

#[test]
fn orとnotとandをまたいで_確かめた範囲を引き継ぐ() {
    assert!(any(&errs("  it.total = m * 40"), &["-8000..8000"]));
    assert!(errs("  if m > 5 or m < 0 { return Bad }\n  it.total = m * 40").is_empty());
    assert!(errs("  if not (m <= 5 and m >= 0) { return Bad }\n  it.total = m * 40").is_empty());
    assert!(errs("  if m >= 0 and m <= 5 {\n    it.total = m * 40\n  }").is_empty());
    assert!(errs("  if not (m > 5 or m < 0) {\n    it.total = m * 40\n  }").is_empty());
    // 片方としか言えない形からは、何も引き出さない
    assert!(!errs("  if m > 5 and m < 0 { return Bad }\n  it.total = m * 40").is_empty());
    assert!(!errs("  if m >= 0 or m <= 5 {\n    it.total = m * 40\n  }").is_empty());
}

#[test]
fn 条件の式は_thenとelseの側で_条件を使って範囲を出す() {
    assert!(errs_w("  it.stock = if m > 99 then 99 else if m < 0 then 0 else m", "Item.stock").is_empty());
    assert!(any(&errs("  it.total = (if m > 5 then 5 else m) * 40"), &["-8000..200"]));
}

#[test]
fn letで同じ名前は付け直せない() {
    // 前の m について確かめた範囲（0..5）が、付け直した m（-200..200）に残ると、収まらない値を示せたことにしてしまう
    let hole = "  if m > 5 { return Bad }\n  if m < 0 { return Bad }\n  let m = it.total\n  it.total = m * 40";
    assert!(any(&errs(hole), &["「m」は、すでにある名前です"]));
    assert!(any(&errs("  let it = Item[item_id] else { return NoItem }"), &["「it」は、すでにある名前です"]));
    assert!(any(&errs("  let k = 1\n  let k = 2"), &["「k」は、すでにある名前です"]));
    assert!(any(&errs_w("  let it = create Item { stock: 1, total: 1, fee: 0 }", "Item"), &["「it」は、すでにある名前です"]));
    assert!(errs("  let k = it.total\n  if k > 5 or k < 0 { return Bad }\n  it.total = k * 40").is_empty());
}

#[test]
fn 結果の中身の数も_型の範囲に収まると示す() {
    let src = BASE.replace("result Res { Done, NoItem, Bad }", "result Res { Done(left: Qty), NoItem, Bad }").replace("READS", "Item.stock").replace("WRITES", "Item.stock");
    let m = |body: &str, ret: &str| errs_with(&src.replace("BODY", body).replace("return Done\n", &format!("return Done({})\n", ret)));
    assert!(m("  it.stock = n", "it.stock").is_empty());
    assert!(m("  it.stock = n", "max(min(m, 99), 0)").is_empty());
    assert!(any(&m("  it.stock = n", "m"), &["結果「Done」の中身「left」", "-200..200"]));
}

fn member_errors(src: &str) -> Vec<String> {
    with_check(src, false, |ck| ck.diags.iter().filter(|d| d.level == ERROR).map(|d| d.msg.clone()).collect())
}

#[test]
fn inとlenの型と範囲を検査する() {
    assert!(member_errors(MEMBERS).is_empty()); // 3 + 20 + 50 = 73 は 0..99 に収まる
    assert!(any(&member_errors(&MEMBERS.replace("ltype Qty = int 0..99", "ltype Qty = int 0..72")), &["収まると示せません", "1..73"]));
    // 上限を書けない集合の件数は、そのままでは収まると示せない（min で挟む）
    assert!(any(&member_errors(&MEMBERS.replace("min(len(it.extra), 99)", "len(it.extra)")), &["結果「Untagged」の中身「count」"]));
    assert!(any(&member_errors(&MEMBERS.replace("tag in it.tags", "tag in it.name")), &["in の右は、集合か写像です"]));
    assert!(any(&member_errors(&MEMBERS.replace("tag in it.tags", "it.name in it.tags")), &["in の左は、id"]));
    assert!(any(&member_errors(&MEMBERS.replace("len(it.name))\n}", "len(it.size))\n}")), &["len( ) の対象は"]));
    // null になりうる集合は、確かめてからでないと使えない（前からの決まり）
    assert!(any(&member_errors(&MEMBERS.replace("  if it.extra is null { return Untagged(min(len(it.tags), 99), len(it.name)) }\n", "")), &["無い（null の）ことがあります"]));
}
