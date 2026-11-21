// SPDX-License-Identifier: GPL-3.0-or-later
//! 言語機能ごとの動作テスト（数と集合）。負の数・min・max・abs・if 式・in・len を、生成した Rust で動かす（Python 版 tests/test_numbers.py の移し）。
//! どの機能にどのテストがあるかは docs/language-test-map.md。
mod common;
use common::*;

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
