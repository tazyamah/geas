# SPDX-License-Identifier: GPL-3.0-or-later
"""言語機能ごとの動作テスト：小さな .geas を書き、生成した Rust を実際に動かして、書いたとおりに動くことを確かめる。
どの機能にどのテストがあるかは docs/language-test-map.md。生成器が正しければ、どのアプリも .geas どおりに動く、という保証の置き場所。
cargo が無い環境では飛ばす。"""
import shutil
import subprocess

import pytest

from geas.check import check
from geas.examples import write_crate
from geas.parser import parse
from geas.rustgen import generate

pytestmark = pytest.mark.skipif(shutil.which("cargo") is None, reason="cargo が無い")


def run_app(src, main, tmp_path, host=False):
    """検査を通った .geas から crate を生成し、main（Rust）を動かして標準出力の行を返す"""
    ck = check(parse(src))
    assert ck.ok and not [d for d in ck.diags if d.level == "問い"], ck.diags
    write_crate(ck, generate(ck), main, tmp_path, host=host)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    return r.stdout.splitlines()


# ---- 状態の遷移・数量・金額・不変条件 ----
ORDERS = """ltype Qty = int 1..99
ltype Stock = int 0..
ltype Price = money(JPY) 0..
ltype ItemQty = Item → Qty

entity Item {
  item_id: id unique
  stock: Stock required
  price: Price required
}

entity Order {
  order_id: id unique
  items: ItemQty required
  total: Price required
  status: OrderStatus
}

state OrderStatus {
  initial Received
  middle Paid
  final Completed, Canceled
  reject Completed → Canceled reason "完了後は返品で扱う"
}

invariant Item: stock >= 0

result PlaceResult { Placed(order_id: id), OutOfStock, NoItem }

deed place {
  input order_id: id required, items: ItemQty required
  reads Item.stock, Item.price
  writes Order, Item.stock
  transition Order: (new) → Received
  result PlaceResult
  effects none

  for (k, v) in items {
    let it = Item[k] else { return NoItem }
  }
  let short = { k | (k, v) in items, Item[k].stock < v }
  if short is nonempty { return OutOfStock }
  let subtotal = sum { Item[k].price * v | (k, v) in items }
  let o = create Order { order_id: order_id, items: items, total: subtotal, status: Received }
  update { Item[k].stock -= v | (k, v) in items }
  return Placed(o.order_id)
}

result PayResult { Paid, NoOrder, WrongState }
deed pay {
  input order_id: id required
  reads Order.status
  writes Order.status
  transition Order: Received → Paid
  result PayResult
  effects none
  let o = Order[order_id] else { return NoOrder }
  if o.status != Received { return WrongState }
  o.status = Paid
  return Paid
}

result DoneResult { Done, NoOrder, WrongState }

result RestockResult { Restocked, NoItem }
deed restock {
  input item_id: id required, amount: Qty required
  reads Item.stock
  writes Item.stock
  result RestockResult
  effects none
  let it = Item[item_id] else { return NoItem }
  it.stock += amount
  return Restocked
}
deed complete {
  input order_id: id required
  reads Order.status
  writes Order.status
  transition Order: Paid → Completed
  result DoneResult
  effects none
  let o = Order[order_id] else { return NoOrder }
  if o.status != Paid { return WrongState }
  o.status = Completed
  return Done
}
"""

MAIN_ORDERS = """use geas_app::*;
use std::collections::BTreeMap;
fn items(xs: &[(&str, u8)]) -> BTreeMap<String, u8> { xs.iter().map(|(k, v)| (k.to_string(), *v)).collect() }
fn main() {
    let mut s = Store::default();
    s.Item.insert("tea".into(), Item { item_id: "tea".into(), stock: 5, price: 300 });
    s.Item.insert("cup".into(), Item { item_id: "cup".into(), stock: 1, price: 1200 });
    let ctx = placeCtx { new_Order_id: "O1".into() };
    // 在庫が足りないと、何も書かずに止まる
    let o = place(&ctx, &s, &placeInput { order_id: "O1".into(), items: items(&[("tea", 2), ("cup", 2)]) });
    println!("{}|{}", o.result.show(), o.Write.len());
    // 知らない商品
    let o = place(&ctx, &s, &placeInput { order_id: "O1".into(), items: items(&[("none", 1)]) });
    println!("{}|{}", o.result.show(), o.Write.len());
    // 受注できると、合計は 数量×単価 の和、在庫はその分だけ減る
    let o = place(&ctx, &s, &placeInput { order_id: "O1".into(), items: items(&[("tea", 2), ("cup", 1)]) });
    println!("{}", o.result.show());
    s.apply(o.Write);
    println!("total={} stock={},{} status={}", s.Order["O1"].total, s.Item["tea"].stock, s.Item["cup"].stock, s.Order["O1"].status.name());
    // 残りの在庫を超える注文は、もう通らない（在庫は 0 を下回らない）
    let o = place(&ctx, &s, &placeInput { order_id: "O2".into(), items: items(&[("cup", 1)]) });
    println!("{}", o.result.show());
    // 状態は 受付 → 支払済 → 完了 の順にしか進めない。順を飛ばす・戻す・繰り返す操作は、何も書かずに断る
    let o = complete(&completeCtx {}, &s, &completeInput { order_id: "O1".into() });
    println!("先に完了 {}|{}", o.result.show(), o.Write.len());
    let o = pay(&payCtx {}, &s, &payInput { order_id: "O1".into() });
    println!("支払い {}", o.result.show());
    s.apply(o.Write);
    let o = pay(&payCtx {}, &s, &payInput { order_id: "O1".into() });
    println!("二重の支払い {}|{}", o.result.show(), o.Write.len());
    let o = complete(&completeCtx {}, &s, &completeInput { order_id: "O1".into() });
    println!("完了 {}", o.result.show());
    s.apply(o.Write);
    let o = pay(&payCtx {}, &s, &payInput { order_id: "O1".into() });
    println!("完了後の支払い {}|{}", o.result.show(), o.Write.len());
    println!("最後 {}", s.Order["O1"].status.name());
    let o = pay(&payCtx {}, &s, &payInput { order_id: "O9".into() });
    println!("{}", o.result.show());
    // 項目への加算
    let o = restock(&restockCtx {}, &s, &restockInput { item_id: "cup".into(), amount: 4 });
    s.apply(o.Write);
    println!("{} stock={}", o.result.show(), s.Item["cup"].stock);
    println!("{}", restock(&restockCtx {}, &s, &restockInput { item_id: "none".into(), amount: 1 }).result.show());
}
"""


def test_状態は宣言した順にしか進まず_在庫と合計は式どおりに動く(tmp_path):
    got = run_app(ORDERS, MAIN_ORDERS, tmp_path)
    assert got == [
        "OutOfStock|0", "NoItem|0", "Placed(O1)",
        "total=1800 stock=3,0 status=Received",
        "OutOfStock",
        "先に完了 WrongState|0", "支払い Paid", "二重の支払い WrongState|0", "完了 Done", "完了後の支払い WrongState|0",
        "最後 Completed", "NoOrder", "Restocked stock=4", "NoItem"], got


# ---- 金額の丸めと外部作用の冪等キー ----
BILLS = """ltype Rate = ratio 0..1
ltype Yen = money(JPY) 0..
ltype Parts = set of id

entity Bill {
  bill_id: id unique
  base: Yen required
  floored: Yen required
  ceiled: Yen required
  rounded: Yen required
}

effect Notice. send(bill_id: id, piece: id) no_wait "通知の失敗は人が見る"

result BillResult { Billed }

deed bill {
  input bill_id: id required, base: Yen required, parts: Parts required
  slot rate: Rate
  writes Bill
  result BillResult
  effects Notice.send
  let b = create Bill { bill_id: bill_id, base: base, floored: round(base * rate, floor, JPY), ceiled: round(base * rate, ceil, JPY), rounded: round(base * rate, half_up, JPY) }
  for p in parts {
    request Notice.send(bill_id, p, idempotency_key = (bill_id, p))
  }
  return Billed
}
"""

MAIN_BILLS = """use geas_app::*;
fn main() {
    let s = Store::default();
    // 割合 × 金額 の端数は、切り捨て・切り上げ・四捨五入で別々の値になる
    for (base, num, den) in [(1000i64, 1i128, 3i128), (1000, 2, 3), (105, 1, 10), (100, 1, 10), (0, 1, 3), (7, 1, 2)] {
        let ctx = billCtx { rate: Ratio { num, den }, new_Bill_id: "B".into() };
        let o = bill(&ctx, &s, &billInput { bill_id: "B".into(), base, parts: ["x".to_string()].into() });
        let mut t = Store::default();
        t.apply(o.Write);
        let b = &t.Bill["B"];
        println!("{} {}/{} -> {} {} {}", base, num, den, b.floored, b.ceiled, b.rounded);
    }
    // for の中の外部作用は、回した値ごとに 1 回出て、冪等キーは (注文ID, 回した値) の組になる
    let ctx = billCtx { rate: Ratio { num: 1, den: 2 }, new_Bill_id: "B".into() };
    let o = bill(&ctx, &s, &billInput { bill_id: "B1".into(), base: 10, parts: ["p1".to_string(), "p2".to_string()].into() });
    for e in &o.effects { println!("{}", e.show()); }
    let o = bill(&ctx, &s, &billInput { bill_id: "B1".into(), base: 10, parts: Default::default() });
    println!("{}", o.effects.len());
}
"""


def test_金額の丸めは指定の方向になり_外部作用の冪等キーは回す値ごとに決まる(tmp_path):
    got = run_app(BILLS, MAIN_BILLS, tmp_path)
    assert got[:6] == [
        "1000 1/3 -> 333 334 333", "1000 2/3 -> 666 667 667", "105 1/10 -> 10 11 11",  # 10.5 は四捨五入で 11
        "100 1/10 -> 10 10 10", "0 1/3 -> 0 0 0", "7 1/2 -> 3 4 4"], got
    assert got[6:] == ['Notice.send(B1, p1, 冪等キー=["B1","p1"])', 'Notice.send(B1, p2, 冪等キー=["B1","p2"])', "0"], got


# ---- 削除：参照元の扱い ----
OWNERS = """entity Owner {
  owner_id: id unique
  on delete Receipt.owner → cascade
  on delete Log.owner → leave
}

entity Receipt {
  receipt_id: id unique
  owner: ref Owner required
}

entity Log {
  log_id: id unique
  owner: ref Owner required
}

result DropResult { Dropped, NotFound }

deed drop_owner {
  input owner_id: id required
  reads Owner
  writes Owner
  result DropResult
  effects none
  let o = Owner[owner_id] else { return NotFound }
  delete o
  return Dropped
}
"""

MAIN_OWNERS = """use geas_app::*;
fn main() {
    let mut s = Store::default();
    s.Owner.insert("o1".into(), Owner { owner_id: "o1".into() });
    s.Owner.insert("o2".into(), Owner { owner_id: "o2".into() });
    s.Receipt.insert("r1".into(), Receipt { receipt_id: "r1".into(), owner: "o1".into() });
    s.Receipt.insert("r2".into(), Receipt { receipt_id: "r2".into(), owner: "o2".into() });
    s.Log.insert("l1".into(), Log { log_id: "l1".into(), owner: "o1".into() });
    let o = drop_owner(&drop_ownerCtx {}, &s, &drop_ownerInput { owner_id: "o1".into() });
    println!("{}", o.result.show());
    s.apply(o.Write);
    // cascade は参照元も消し、leave は参照元をそのまま残す。関係のない行は触らない
    println!("owners={:?} receipts={:?} logs={:?}", s.Owner.keys().collect::<Vec<_>>(), s.Receipt.keys().collect::<Vec<_>>(), s.Log.keys().collect::<Vec<_>>());
    let o = drop_owner(&drop_ownerCtx {}, &s, &drop_ownerInput { owner_id: "o1".into() });
    println!("{}|{}", o.result.show(), o.Write.len());
}
"""


def test_削除は_cascade_で参照元も消し_leave_で残す(tmp_path):
    got = run_app(OWNERS, MAIN_OWNERS, tmp_path)
    assert got == ["Dropped", 'owners=["o2"] receipts=["r2"] logs=["l1"]', "NotFound|0"], got


# ---- 問い合わせ：絞り込み・並び・ページ ----
MEMOS = """ltype Weight = int 0..
ltype Name = text 1..40

entity Memo {
  memo_id: id unique
  title: Name required
  weight: Weight required
  written_at: time required
  kind: id required
}

query heavy_titles {
  input min_weight: int required, max_weight: int required, word: text required, kind: id
  from Memo
  filter weight at_least min_weight
  filter weight at_most max_weight
  filter title contains word ignore_case
  filter kind equals kind
  order written_at newest
  page 2
  select memo_id, weight
  reads Memo.memo_id, Memo.weight, Memo.title, Memo.written_at, Memo.kind
}

query oldest_first {
  from Memo
  order written_at oldest
  select memo_id
  reads Memo.memo_id, Memo.written_at
}

query light_only {
  from Memo
  where weight == 0 or weight == 1
  order written_at newest
  select memo_id
  reads Memo.memo_id, Memo.weight, Memo.written_at
}
"""

MAIN_MEMOS = """use geas_app::*;
fn memo(id: &str, title: &str, weight: u64, at: i64, kind: &str) -> Memo {
    Memo { memo_id: id.into(), title: title.into(), weight, written_at: at, kind: kind.into() }
}
fn main() {
    let mut s = Store::default();
    for m in [memo("m1", "Red Tea", 5, 100, "a"), memo("m2", "red cup", 7, 300, "b"), memo("m3", "Blue tea", 9, 200, "a"),
              memo("m4", "RED knife", 1, 400, "a"), memo("m5", "red bowl", 6, 250, "a")] {
        s.Memo.insert(m.memo_id.clone(), m);
    }
    let show = |rows: Vec<heavy_titlesRow>| rows.iter().map(|r| r.memo_id.clone()).collect::<Vec<_>>().join(",");
    let q = |min, max, word: &str, kind: Option<&str>| show(heavy_titles(&heavy_titlesCtx {}, &s, &heavy_titlesInput { min_weight: min, max_weight: max, word: word.into(), kind: kind.map(|k| k.to_string()) }));
    println!("{}", q(0, 100, "RED", None));      // 大小文字を無視して含む。新しい順に 2 件（m4, m2 の次の m5 は 3 件目）
    println!("{}", q(5, 6, "red", None));        // 下限と上限は両端を含む
    println!("{}", q(0, 100, "red", Some("b")));  // equals で完全一致
    println!("{}", q(0, 100, "zzz", None));
    println!("{}", oldest_first(&oldest_firstCtx {}, &s, &oldest_firstInput {}).iter().map(|r| r.memo_id.clone()).collect::<Vec<_>>().join(","));
    println!("{}", light_only(&light_onlyCtx {}, &s, &light_onlyInput {}).iter().map(|r| r.memo_id.clone()).collect::<Vec<_>>().join(","));
}
"""


def test_問い合わせは絞り込み_並び_ページ_where_のとおりに返す(tmp_path):
    got = run_app(MEMOS, MAIN_MEMOS, tmp_path)
    assert got == ["m4,m2", "m5,m1", "m2", "", "m1,m3,m5,m2,m4", "m4"], got


# ---- バイト列・割合・終わりなく続く状態・「無いこと」の確認 ----
NOTES = """ltype Blob = bytes 1..8
ltype Share = ratio 0..1

entity Note {
  note_id: id unique
  at: time required
  data: Blob required
  share: Share required
  phase: LifeState
}

state LifeState {
  initial Fresh
  middle Seen
  endless
}

result NoteResult { Done, Missing, Gone }

deed touch {
  input note_id: id required, data: Blob required
  slot rate: Share
  reads Note.phase
  writes Note.data, Note.share, Note.phase
  transition Note: Fresh → Seen
  result NoteResult
  effects none

  let n = Note[note_id] else { return Missing }
  if n.phase != Fresh { return Gone }
  n.data = data
  n.share = rate
  n.phase = Seen
  return Done
}

result PeekResult { Absent, Present }
deed peek {
  input note_id: id required
  reads Note
  result PeekResult
  effects none

  if Note[note_id] is absent { return Absent }
  return Present
}
"""

MAIN_NOTES = """use geas_app::*;
fn main() {
    let mut s = Store::default();
    s.Note.insert("n1".into(), Note { note_id: "n1".into(), at: 0, data: vec![], share: Ratio { num: 0, den: 1 }, phase: LifeState::Fresh });
    // バイト列は長さ（1〜8）を境界で確かめる
    let ok = |n: usize| touchInput { note_id: "n1".into(), data: vec![7u8; n] }.validate().is_ok();
    println!("{} {} {} {}", ok(0), ok(1), ok(8), ok(9));
    // 割合の slot は範囲（0〜1）を境界で確かめる
    let r = |num, den| touchCtx { rate: Ratio { num, den } }.validate().is_ok();
    println!("{} {} {}", r(1, 4), r(1, 1), r(5, 4));
    // 書いたバイト列と割合がそのまま入る。終わりのない状態は、進んだあとは同じ操作を断る
    let o = touch(&touchCtx { rate: Ratio { num: 1, den: 4 } }, &s, &touchInput { note_id: "n1".into(), data: vec![1, 2, 3] });
    println!("{}", o.result.show());
    s.apply(o.Write);
    println!("{:?} {}/{} {}", s.Note["n1"].data, s.Note["n1"].share.num, s.Note["n1"].share.den, s.Note["n1"].phase.name());
    let o = touch(&touchCtx { rate: Ratio { num: 1, den: 2 } }, &s, &touchInput { note_id: "n1".into(), data: vec![9] });
    println!("{}|{}", o.result.show(), o.Write.len());
    println!("{}", touch(&touchCtx { rate: Ratio { num: 1, den: 2 } }, &s, &touchInput { note_id: "zz".into(), data: vec![9] }).result.show());
    // 「無い」の確認
    println!("{} {}", peek(&peekCtx {}, &s, &peekInput { note_id: "n1".into() }).result.show(), peek(&peekCtx {}, &s, &peekInput { note_id: "zz".into() }).result.show());
}
"""


def test_バイト列_割合_終わりなく続く状態_無いことの確認が動く(tmp_path):
    got = run_app(NOTES, MAIN_NOTES, tmp_path)
    assert got == ["false true true false", "true true false", "Done", "[1, 2, 3] 1/4 Seen", "Gone|0", "Missing", "Present Absent"], got


# ---- 既知の不具合：filter … within は、必要な参照の名前「現在」が、名前の規則（英小文字）に合わず書けない ----
WITHIN = """ltype Weight = int 0..
entity Memo {
  memo_id: id unique
  written_at: time required
}
query recent {
  slot current: time
  from Memo
  filter written_at within 1 hour
  select memo_id
  reads Memo.memo_id, Memo.written_at
}
"""


@pytest.mark.xfail(strict=True, reason="within の参照が「現在」固定で、名前の規則に合わない（直すとき、このテストを通常のテストにする）")
def test_within_は規則どおりの名前で書ける():
    ck = check(parse(WITHIN), strict_names=True)
    assert ck.ok, ck.diags
