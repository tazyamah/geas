# SPDX-License-Identifier: GPL-3.0-or-later
"""数の基本の書き方：負の数と符号の反転（-5、-x）、min・max・abs、and・or・not をまたいだ条件の読み取り、
条件の式（if … then … else …）の範囲、let の付け直しの禁止。検査（範囲の証明）と、生成した Rust の動きの両方を確かめる。"""
import shutil

import pytest

from geas.check import check
from geas.parser import ParseError, parse

from test_behavior import run_app

needs_cargo = pytest.mark.skipif(shutil.which("cargo") is None, reason="cargo が無い")

BASE = """ltype Qty = int 0..99
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
"""


def errors(body, writes="Item.total", reads="Item"):
    ck = check(parse(BASE.replace("BODY", body).replace("WRITES", writes).replace("READS", reads)))
    return [d.msg for d in ck.diags if d.level == "エラー" and d.code != "宣言"]  # reads・writes の宣言のずれは、ここでは見ない


def test_負の数と符号の反転が書け_範囲も符号を返して示す():
    assert errors("  it.total = -5") == []
    assert errors("  it.total = -n") == []            # 0..99 の反転は -99..0
    assert any("3..201" in m for m in errors("  it.total = 3 - -n * 2"))  # 引く数が負でも、符号を返して足す
    assert errors("  it.total = 2 - -n * 2") == []
    assert any("収まると示せません" in m and "-198..0" in m for m in errors("  it.stock = -n * 2", "Item.stock"))
    assert errors("  it.fee = -y", "Item.fee") == []   # 金額の反転は金額
    assert any("-201" in m for m in errors("  it.total = -201"))
    with pytest.raises(ParseError):
        parse(BASE.replace("BODY", "  it.total = n / -2"))  # 負の定数では割れない（前からの決まり）


def test_min_max_abs_の範囲を示せる():
    assert errors("  it.stock = min(m, 99)", "Item.stock") != []            # 下が -200 のまま
    assert errors("  it.stock = max(min(m, 99), 0)", "Item.stock") == []    # 0..99 に挟む
    assert errors("  it.stock = min(abs(m), 99)", "Item.stock") == []       # abs は 0..200
    assert errors("  it.total = abs(m)") == []
    assert errors("  it.total = max(n, m, 5)") == []                         # 3 つ以上も書ける
    assert any("0..400" in m for m in errors("  it.total = abs(m) * 2"))
    assert errors("  it.fee = max(y, 0)", "Item.fee") == []                  # 数字は金額としても使える
    assert any("整数どうしか金額どうし" in m for m in errors("  it.fee = max(y, n)", "Item.fee"))
    assert any("引数は2つ以上" in m for m in errors("  it.total = min(m)"))
    assert any("引数は1つ" in m for m in errors("  it.total = abs(m, n)"))


def test_or_と_not_と_and_をまたいで_確かめた範囲を引き継ぐ():
    assert any("-8000..8000" in m for m in errors("  it.total = m * 40"))
    assert errors("  if m > 5 or m < 0 { return Bad }\n  it.total = m * 40") == []
    assert errors("  if not (m <= 5 and m >= 0) { return Bad }\n  it.total = m * 40") == []
    assert errors("  if m >= 0 and m <= 5 {\n    it.total = m * 40\n  }") == []
    assert errors("  if not (m > 5 or m < 0) {\n    it.total = m * 40\n  }") == []
    # 片方としか言えない形からは、何も引き出さない
    assert errors("  if m > 5 and m < 0 { return Bad }\n  it.total = m * 40") != []
    assert errors("  if m >= 0 or m <= 5 {\n    it.total = m * 40\n  }") != []


def test_条件の式は_then_と_else_の側で_条件を使って範囲を出す():
    assert errors("  it.stock = if m > 99 then 99 else if m < 0 then 0 else m", "Item.stock") == []
    assert any("-8000..200" in m for m in errors("  it.total = (if m > 5 then 5 else m) * 40"))


def test_let_で同じ名前は付け直せない():
    # 前の m について確かめた範囲（0..5）が、付け直した m（-200..200）に残ると、収まらない値を示せたことにしてしまう
    hole = "  if m > 5 { return Bad }\n  if m < 0 { return Bad }\n  let m = it.total\n  it.total = m * 40"
    assert any("「m」は、すでにある名前です" in m for m in errors(hole))
    assert any("「it」は、すでにある名前です" in m for m in errors("  let it = Item[item_id] else { return NoItem }"))
    assert any("「k」は、すでにある名前です" in m for m in errors("  let k = 1\n  let k = 2"))
    assert any("「it」は、すでにある名前です" in m for m in errors("  let it = create Item { stock: 1, total: 1, fee: 0 }", "Item"))
    assert errors("  let k = it.total\n  if k > 5 or k < 0 { return Bad }\n  it.total = k * 40") == []


def test_結果の中身の数も_型の範囲に収まると示す():
    src = BASE.replace("result Res { Done, NoItem, Bad }", "result Res { Done(left: Qty), NoItem, Bad }").replace("READS", "Item.stock").replace("WRITES", "Item.stock")
    msgs = lambda body, ret: [d.msg for d in check(parse(src.replace("BODY", body).replace("return Done\n", f"return Done({ret})\n"))).diags if d.level == "エラー" and d.code != "宣言"]
    assert msgs("  it.stock = n", "it.stock") == []
    assert msgs("  it.stock = n", "max(min(m, 99), 0)") == []
    assert any("結果「Done」の中身「left」" in m and "-200..200" in m for m in msgs("  it.stock = n", "m"))


CLAMP = """ltype Qty = int 0..99
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
"""


@needs_cargo
def test_負の数と_min_max_abs_は_生成した_Rust_でも式どおりに動く(tmp_path):
    main = """use geas_app::*;
fn main() {
    let mut s = Store::default();
    s.Item.insert("a".into(), Item { item_id: "a".into(), stock: 1, total: 1, fee: 1 });
    for (m, y) in [(150, 30), (-150, -30), (50, -3), (-200, 5000), (0, 0)] {
        let o = adjust(&adjustCtx {}, &s, &adjustInput { item_id: "a".into(), m, y }).unwrap();
        println!("{}", o.result.show());
    }
}
"""
    got = run_app(CLAMP, main, tmp_path)
    assert got == ["Done(99, -150, -30)", "Done(0, 150, 10)", "Done(50, 57, 3)", "Done(0, 200, -5000)", "Done(0, 7, 0)"], got


# ---- 集合に入っているか（in）と、件数・文字数（len） ----
MEMBERS = """ltype Tags = set of id
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
"""


def member_errors(src):
    return [d.msg for d in check(parse(src)).diags if d.level == "エラー"]


def test_in_と_len_の型と範囲を検査する():
    assert member_errors(MEMBERS) == []   # 3 + 20 + 50 = 73 は 0..99 に収まる（写像の要素数の上限、文字列の長さ、min から示す）
    assert any("収まると示せません" in m and "1..73" in m for m in member_errors(MEMBERS.replace("ltype Qty = int 0..99", "ltype Qty = int 0..72")))
    # 上限を書けない集合の件数は、そのままでは収まると示せない（min で挟む）
    assert any("結果「Untagged」の中身「count」" in m for m in member_errors(MEMBERS.replace("min(len(it.extra), 99)", "len(it.extra)")))
    assert any("in の右は、集合か写像です" in m for m in member_errors(MEMBERS.replace("tag in it.tags", "tag in it.name")))
    assert any("in の左は、id" in m for m in member_errors(MEMBERS.replace("tag in it.tags", "it.name in it.tags")))
    assert any("len( ) の対象は" in m for m in member_errors(MEMBERS.replace("len(it.name))\n}", "len(it.size))\n}")))
    # null になりうる集合は、確かめてからでないと使えない（前からの決まり）
    assert any("無い（null の）ことがあります" in m for m in member_errors(MEMBERS.replace("  if it.extra is null { return Untagged(min(len(it.tags), 99), len(it.name)) }\n", "")))


@needs_cargo
def test_in_と_len_は_生成した_Rust_でも動く(tmp_path):
    main = """use geas_app::*;
use std::collections::{BTreeMap, BTreeSet};
fn set(xs: &[&str]) -> BTreeSet<String> { xs.iter().map(|x| x.to_string()).collect() }
fn main() {
    let mut s = Store::default();
    s.Item.insert("a".into(), Item { item_id: "a".into(), tags: set(&["red", "new"]), extra: None, name: "りんご".into(), size: 0 });
    s.Item.insert("b".into(), Item { item_id: "b".into(), tags: set(&["red"]), extra: Some(set(&["sale", "big", "old"])), name: "pear".into(), size: 0 });
    let stock: BTreeMap<String, u8> = [("z".to_string(), 1u8), ("y".to_string(), 2u8)].into_iter().collect();
    for (id, tag) in [("a", "red"), ("a", "blue"), ("b", "sale"), ("b", "blue"), ("z", "red"), ("q", "red")] {
        let o = look(&lookCtx {}, &s, &lookInput { item_id: id.into(), tag: tag.into(), stock: stock.clone() }).unwrap();
        let wrote: Vec<String> = o.Write.iter().map(|w| format!("{:?}", w)).collect();
        println!("{} {}", o.result.show(), wrote.join(";"));
    }
}
"""
    got = run_app(MEMBERS, main, tmp_path)
    assert got == ["Tagged ", "Untagged(2, 3) ", "Tagged ", 'Untagged(3, 4) Item_size("b", 10)', "NoItem ", "NoItem "], got
