import json
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

from geas.ast import show_type
from geas.check import ERROR, QUESTION, WARN, check
from geas.examples import HOST_MOD, compare_examples, run_crate, runner_source, write_crate
from geas.hostgen import host_source
from geas.spec import render
from geas.modules import ModuleError
from geas.parser import ParseError, parse
from geas.plan import A_ONLY, B_ONLY, DIFF, compare_static, read_plan
from geas.rustgen import generate
from geas.spec import build

ROOT = Path(__file__).parent.parent
EX = ROOT / "tests" / "fixtures" / "shop"
CODE = (EX / "shop.geas").read_text()
PLAN = (EX / "shop_plan.md").read_text()
EFFECT = "effect Payment. /// 請求\n  charge(order_id: id, /// 金額\n    amount: money(JPY))"
needs_cargo = pytest.mark.skipif(shutil.which("cargo") is None, reason="cargo が無い")


def chk(src):
    return check(parse(src))


def msgs(ck, level):
    return [d.msg for d in ck.diags if d.level == level]


def mutate(src, old, new):
    assert old in src, old
    return src.replace(old, new, 1)


# ---- 正常系 ----
def test_お店の例は検査を通る():
    ck = chk(CODE)
    assert ck.ok, ck.diags
    assert not msgs(ck, QUESTION)


def test_不変条件は証明済みと出る():
    ck = chk(CODE)
    text = " ".join(s for _, rows in ck.invariant_status for _, s in rows)
    assert "証明できない" not in text and "減算の前に不足を確認" in text


def test_仕様書は読み書きと遷移をコードから取り出す():
    spec = build(chk(CODE))
    shop = {x["処理"]: x for x in spec["処理"]}
    assert shop["受注"]["書く"] == ["注文", "商品.在庫"]
    assert ("注文", "受付", "決済要求済", "決済要求") in spec["遷移"]
    assert shop["決済要求"]["外部"] == ["決済.請求(冪等キー=注文ID)"]


def test_物理数は範囲から選ばれる():
    spec = build(chk(CODE))
    assert dict(spec["実現"])["商品.在庫"] == "u64"
    assert ("明細.値", "u8") in next(x for x in spec["処理"] if x["処理"] == "受注")["物理数"]


# ---- 検査が止める例（構文のスケッチ §3 の表） ----
def test_丸めの指定が無いと問いになる():
    ck = chk(mutate(CODE, "let total = subtotal + round(subtotal * tax_rate, floor, JPY)", "let total = subtotal + subtotal * tax_rate"))
    assert any("端数をどう丸めますか" in m for m in msgs(ck, QUESTION))


def test_結果に行き先が無いと止まる():
    ck = chk(mutate(CODE, ", NoSuchCustomer → reply\n", "\n"))
    assert any("顧客なし" in m and "行き先" in m for m in msgs(ck, ERROR))


def test_冪等キーが無いと止まる():
    ck = chk(mutate(CODE, ", idempotency_key = order_id)", ")"))
    assert any("冪等キー" in m for m in msgs(ck, ERROR))


def test_宣言に無い遷移は止まる():
    ck = chk(mutate(CODE, "  order.status = PaymentRequested", "  order.status = Completed"))
    assert any("遷移" in d.code and "完了" in d.msg for d in ck.diags if d.level == ERROR)


def test_元の状態の確認が無いと止まる():
    ck = chk(mutate(CODE, "  if order.status != Received { return WrongStatus }\n", ""))
    assert any("確認" in m for m in msgs(ck, ERROR))


def test_在庫不足の判定を削ると不変条件が示せない():
    ck = chk(mutate(CODE, "  if short is nonempty { return OutOfStock(short) }\n", ""))
    assert any("反例" in m for m in msgs(ck, ERROR))


def test_書く項目の宣言漏れは止まる():
    ck = chk(mutate(CODE, "  writes Product.stock, Order\n", "  writes Order\n"))
    assert any("商品.在庫" in m for m in msgs(ck, ERROR))


def test_合計ゼロの確認を削ると不変条件が示せない():
    ck = chk(mutate(CODE, "  if order.total <= 0 { return ZeroTotal }\n", ""))
    assert any("合計 > 0" in m for m in msgs(ck, ERROR))


def test_存在を確かめない参照は問いになる():
    ck = chk(mutate(CODE, "  let customer = Customer[customer_id] else { return NoSuchCustomer }", "  let customer = Customer[customer_id]"))
    assert any("存在しない" in m for m in msgs(ck, QUESTION))


def test_構文エラーは行番号つき():
    with pytest.raises(ParseError, match="行目"):
        parse("entity Product {\n  stock int\n}\n")


# ---- Rust 生成 ----
def test_エラーのあるコードからは生成しない():
    with pytest.raises(AssertionError):
        generate(chk(mutate(CODE, ", idempotency_key = order_id)", ")")))


def test_生成は決定的で_コミットした生成物と一致する():
    ck = chk(CODE)
    assert generate(ck) == generate(chk(CODE))
    committed = EX / "shop_rust" / "src" / "lib.rs"
    assert committed.read_text() == generate(ck), "python -m geas rust で生成物を作り直してください"


# ---- 突合 ----
def static(plan_text):
    return compare_static(build(chk(CODE)), read_plan(plan_text))


def test_お店の例は計画と静的に一致する():
    assert [f for f in static(PLAN) if f.kind != "未規定"] == []


def test_計画にある遷移がコードに無いと検出する():
    plan = mutate(PLAN, "| TR2 |", "| TR9 | 注文 | 受付 | 完了 | 即時完了 | 仮 |\n| TR2 |")
    fs = static(plan)
    assert any(f.kind == A_ONLY and "受付→完了" in f.where for f in fs)


def test_コードが計画に無いものを書き込むと検出する():
    plan = mutate(PLAN, "| 商品.在庫, 注文 |", "| 商品.在庫 |")
    fs = static(plan)
    assert any(f.kind == DIFF and "書く" in f.where and "注文" in f.msg for f in fs)


def test_範囲の食い違いを検出する():
    fs = static(mutate(PLAN, "| 在庫 | 整数 0.. |", "| 在庫 | 整数 1.. |"))
    assert any(f.kind == DIFF and "商品.在庫" in f.where for f in fs)


def test_計画に無い項目がコードにあると検出する():
    fs = static(mutate(PLAN, "| DA3 | 商品 | 単価 | 金額(円) 0.. | ○ |  | 仮 |\n", ""))
    assert any(f.kind == B_ONLY and "商品.単価" in f.where for f in fs)


@needs_cargo
def test_具体例を実際に動かして計画と一致する(tmp_path):
    ck = chk(CODE)
    rows = read_plan(PLAN)["具体例"]
    src, problems = runner_source(ck, rows)
    assert [p for p in problems if p.kind != "未規定"] == []
    write_crate(ck, generate(ck), src, tmp_path)
    actual = run_crate(tmp_path, ck.p.show_names)
    assert actual["EX1"][0] == "受付(N1)"
    assert compare_examples(rows, actual) == []


@needs_cargo
def test_具体例が計画と違えば検出する(tmp_path):
    ck = chk(CODE)
    rows = read_plan(mutate(PLAN, "| 在庫不足({A}) | なし |", "| 受付(N1) | なし |"))["具体例"]
    src, _ = runner_source(ck, rows)
    write_crate(ck, generate(ck), src, tmp_path)
    fs = compare_examples(rows, run_crate(tmp_path, ck.p.show_names))
    assert any("EX2" in f.where and "結果" in f.where for f in fs)


def test_cli_compare(tmp_path):
    r = subprocess.run([sys.executable, "-m", "geas", "compare", str(EX / "shop.geas"), str(EX / "shop_plan.md"), "--no-run"],
                       capture_output=True, text=True, cwd=ROOT)
    assert r.returncode == 0, r.stdout + r.stderr


def test_計画と必須の入力が違うと突合で出る(tmp_path):
    plan = (EX / "shop_plan.md").read_text().replace("| 決済要求 | 注文ID（必須） |", "| 決済要求 | 注文ID |")
    (tmp_path / "plan.md").write_text(plan)
    r = subprocess.run([sys.executable, "-m", "geas", "compare", str(EX / "shop.geas"), str(tmp_path / "plan.md"), "--no-run"],
                       capture_output=True, text=True, cwd=ROOT)
    assert r.returncode != 0 and "注文ID" in r.stdout, r.stdout


# ---- トリガ ----
def test_トリガの要因が仕様書に出る():
    spec = build(chk(CODE))
    assert ("決済待ち", "応答 決済成功", "決済結果の反映") in spec["トリガ"]
    assert ("決済待ち", "期限 現在+30分", "決済の期限切れ") in spec["トリガ"]


def test_期限の無いトリガは止まる():
    ck = chk(mutate(CODE, "  deadline now + 30 min → expire_payment\n", ""))
    assert any("期限がありません" in m for m in msgs(ck, ERROR))


def test_待つ先のトリガが未定義だと止まる():
    ck = chk(mutate(CODE, "Requested → wait PaymentPending", "Requested → wait 承認待ち"))
    assert any("承認待ち" in m for m in msgs(ck, ERROR))


def test_トリガの行き先が未定義の処理だと止まる():
    ck = chk(mutate(CODE, "→ expire_payment\n}", "→ 期限切れ処理\n}"))
    assert any("期限切れ処理" in m for m in msgs(ck, ERROR))


def test_トリガの引数と処理の入力が合わないと止まる():
    ck = chk(mutate(CODE, "trigger PaymentPending(order_id: id)", "trigger PaymentPending(order: id)"))
    assert any("合いません" in m or "入力にもっていません" in m for m in msgs(ck, ERROR))


def test_計画にあるトリガの行き先がコードと違えば検出する():
    plan = mutate(PLAN, "| 期限 現在+30分 | 決済の期限切れ |", "| 期限 現在+30分 | 決済結果の反映 |")
    fs = compare_static(build(chk(CODE)), read_plan(plan))
    assert any(f.kind == DIFF and "トリガ" in f.where for f in fs)


def test_入力の範囲が項目より広いと止まる():
    src = mutate(CODE, "ltype IdSet = set of id\n", "ltype IdSet = set of id\nltype 広い数量 = int 1..200\nltype 広い商品数量 = Product → 広い数量\n")
    ck = chk(mutate(src, "customer_id: id required, items: ProductQuantity required", "customer_id: id required, items: 広い商品数量 required"))
    assert any("範囲の広い入力" in m for m in msgs(ck, ERROR))


def test_トリガの型の食い違いと期限の誤りは止まる():
    assert any("型" in m for m in msgs(chk(mutate(CODE, "trigger PaymentPending(order_id: id)", "trigger PaymentPending(order_id: int)")), ERROR))
    assert any("期限が複数" in m for m in msgs(chk(mutate(CODE, "  deadline now + 30 min → expire_payment", "  deadline now + 30 min → expire_payment\n  deadline now + 1 hour → expire_payment")), ERROR))
    assert any("0より長く" in m for m in msgs(chk(mutate(CODE, "deadline now + 30 min", "deadline now + 0 min")), ERROR))


# ---- 初回の対話の台帳 ----
def test_台帳は次に聞くことを示し_揃うまで開始不可():
    from geas import interview as iv

    led = iv.new_ledger("メモ")
    todo, ready = iv.status(led)
    assert not ready and any("一言" in t for t in todo)
    led.update(summary="メモを残す", out_of_scope=["共有"])
    led["usecases"].append({"id": "UC1", "actor": "本人", "goal": "メモを書く", "level": "利用者目標",
                            "main": ["書く", "保存する"], "extensions": [{"at": "2", "cond": "容量不足", "handling": "エラー表示"}],
                            "examples": [{"given": "空", "when": "書く", "then": "1件"}]})
    todo, ready = iv.status(led)
    assert not ready and any("R1" in t for t in todo)
    led["runtime"] = {k: {"value": "x"} for k in iv.RUNTIME}
    assert iv.status(led) == ([], True)


def test_未解決の問いと未仕分けの付加情報は開始を止める():
    from geas import interview as iv

    led = iv.new_ledger("メモ")
    led.update(summary="a", out_of_scope=["b"], runtime={k: {"value": "x"} for k in iv.RUNTIME})
    led["usecases"].append({"id": "UC1", "level": "利用者目標", "main": ["a"], "extensions": [{"at": "1", "cond": "c", "handling": "h"}], "examples": [{"given": "g", "when": "w", "then": "t"}]})
    led["questions"].append({"id": "Q1", "text": "空のとき", "status": "open"})
    assert not iv.status(led)[1]
    led["questions"][0]["status"] = "unspec"
    led["extras"].append({"id": "X1", "text": "スマホで使いたい"})
    todo, ready = iv.status(led)
    assert any("仕分け" in t for t in todo) and not ready


def test_台帳の状態の打ち間違いと_R1の未規定は止まる():
    from geas import interview as iv

    led = iv.new_ledger("メモ")
    led.update(summary="a", out_of_scope=["b"])
    led["usecases"].append({"id": "UC1", "level": "利用者目標", "main": ["a"], "extensions": [{"at": "1", "cond": "c", "handling": "h"}], "examples": [{"given": "g", "when": "w", "then": "t"}]})
    led["questions"].append({"id": "Q1", "text": "x", "status": "Open"})
    assert any("不正" in t for t in iv.status(led)[0])
    led["questions"][0].update(status="unspec", answer="結果に影響しないため")
    led["runtime"] = {k: {"status": "answered", "value": "x"} for k in iv.RUNTIME}
    led["runtime"]["R1"] = {"status": "unspec", "value": "決めない"}
    todo, ready = iv.status(led)
    assert not ready and any("R1" in t and "未規定にできません" in t for t in todo)
    led["runtime"]["R1"] = {"status": "answered", "value": "ブラウザ"}
    led["runtime"]["R4"] = {"status": "unspec", "value": "決めない"}
    assert iv.status(led) == ([], True)


def test_台帳から出したplanの表は突合の対象にならない():
    from geas import interview as iv

    led = iv.new_ledger("メモ")
    led["usecases"].append({"id": "UC1", "goal": "書く", "main": ["書く"]})
    assert read_plan(iv.render(led)) == {}



# ---- 構文（英語キーワード・Rust 風の本体） ----
def test_flowはnexusの別名():
    ck = chk(mutate(CODE, "nexus OrderIntake", "flow OrderIntake"))
    assert not msgs(ck, ERROR)


def test_日本語の古いキーワードや_body_は受け付けない():
    with pytest.raises(ParseError):
        parse("データ 商品 {\n  在庫: int\n}\n")
    with pytest.raises(ParseError):
        parse(mutate(CODE, "  let customer = Customer[customer_id] else", "  body {\n  let customer = Customer[customer_id] else"))


def test_if_の本体は改行して書いてもよい():
    ck = chk(mutate(CODE, "  if order.total <= 0 { return ZeroTotal }\n", "  if order.total <= 0 {\n    return ZeroTotal\n  }\n"))
    assert not msgs(ck, ERROR)


# ---- 条件つきの状態変更 ----
GUARD = "  if order.status != Received { return WrongStatus }\n  if order.total <= 0 { return ZeroTotal }\n  order.status = PaymentRequested\n"
COND = "  if order.total <= 0 { return ZeroTotal }\n  if order.status == Received {\n    order.status = PaymentRequested\n  }\n"


def test_if_の中で元の状態を確かめてから変えるのは通る():
    ck = chk(mutate(CODE, GUARD, COND))
    assert not [d for d in ck.diags if d.level == ERROR and d.code == "遷移"]


def test_if_の中でも元の状態を確かめていないと止まる():
    ck = chk(mutate(CODE, GUARD, "  if order.total > 0 {\n    order.status = PaymentRequested\n  }\n"))
    assert any("遷移" in d.code and "受付" in d.msg for d in ck.diags if d.level == ERROR)


def test_if_の中の確認は外へ持ち出さない():
    ck = chk(mutate(CODE, GUARD, "  if order.status == Received {\n    request Payment.charge(order_id, order.total, idempotency_key = order_id)\n  }\n  order.status = PaymentRequested\n"))
    assert any("遷移" in d.code and "受付" in d.msg for d in ck.diags if d.level == ERROR)


def test_if_の中に_let_は書けない():
    ck = chk(mutate(CODE, GUARD, "  if order.status == Received {\n    let x = Order[order_id] else { return NoSuchOrder }\n  }\n"))
    assert any("if の中" in m for m in msgs(ck, ERROR))


def test_条件つきの変更は_Rust_の_if_の中に出る():
    prog = parse(mutate(CODE, GUARD, COND))
    src = generate(check(prog))
    assert "    if (order.status == OrderStatus::Received) {\n        writes.push(Write::Order_status(order.order_id.clone(), OrderStatus::PaymentRequested));\n    }" in src


# ---- 問い合わせの条件（または・数との比較） ----
QUERY = """ltype 重さ値 = int 0..

entity メモ {
  メモID: id unique
  状態: メモ状態
  重さ: 重さ値
}
state メモ状態 {
  initial 下書き
  middle 公開
  final 削除
}
query 探す {
  from メモ
  where 状態 == 公開 or 状態 == 下書き
  where 重さ >= 10
  reads メモ.状態, メモ.重さ
}
"""


def test_問い合わせの条件に_or_と数の比較を書ける():
    ck = chk(QUERY)
    assert not msgs(ck, ERROR)
    assert build(ck)["問い合わせ"][0]["条件"] == ["状態 == 公開 または 状態 == 下書き", "重さ >= 10"]


def test_数との比較は数の項目にだけ使える():
    ck = chk(QUERY.replace("重さ >= 10", "状態 >= 10"))
    assert any("数との比較" in m for m in msgs(ck, ERROR))


def test_問い合わせの条件は_Rust_で_or_になる():
    src = generate(chk(QUERY))
    assert "if !(r.状態 == メモ状態::公開 || r.状態 == メモ状態::下書き) { return false; }" in src
    assert "if !(r.重さ.map_or(false, |v| (v as i128) >= 10)) { return false; }" in src  # 重さは required でないので、null は満たさない


# ---- 問い合わせの select（結合と集計） ----
def test_selectで参照先の項目と集計を返せる():
    spec = build(chk(CODE))
    row = next(q for q in spec["問い合わせ"] if q["問い合わせ"] == "注文一覧")
    assert row["列"] == ["注文ID", "顧客", "点数"]


def test_selectの参照先が参照でないと止まる():
    ck = chk(mutate(CODE, "customer.customer_id as customer", "total.customer_id as customer"))
    assert any("参照（ref）ではありません" in m for m in msgs(ck, ERROR))


def test_sumはキーつきの集まりにだけ使える():
    ck = chk(mutate(CODE, "sum(items) as /// 点数\n    item_count", "sum(total) as /// 点数\n    item_count"))
    assert any("集計できません" in m for m in msgs(ck, ERROR))


def test_selectに書いた参照先は読むの宣言に要る():
    ck = chk(mutate(CODE, "Order.customer, Customer.customer_id, Order.items\n}", "Order.customer, Order.items\n}"))
    assert any("顧客.顧客ID" in m for m in msgs(ck, ERROR))


def test_selectのある問い合わせは_each_に使えない():
    ck = chk(mutate(CODE, "invariant Product: stock >= 0", "event 開いた {\n  each order_list → request_payment\n}\n\ninvariant Product: stock >= 0"))
    assert any("select" in m for m in msgs(ck, ERROR))


# ---- 文字列検索の同一視（contains の軸） ----
MEMO = """entity メモ {
  メモID: id unique
  本文: text
}
query 探す {
  input 単語: text
  from メモ
  filter 本文 contains 単語 ignore_case ignore_kana ignore_width
  reads メモ.本文
}
"""


def test_containsの同一視する軸は書いたものだけ():
    ck = chk(MEMO)
    assert not msgs(ck, ERROR)
    row = build(ck)["問い合わせ"][0]
    assert row["条件"] == ["本文 含む(大小無視,かな無視,幅無視) 単語（単語が無ければ絞らない）"]
    plain = build(chk(MEMO.replace(" ignore_case ignore_kana ignore_width", "")))["問い合わせ"][0]
    assert plain["条件"] == ["本文 含む 単語（単語が無ければ絞らない）"]
    req = build(chk(MEMO.replace("input 単語: text", "input 単語: text required")))["問い合わせ"][0]
    assert req["条件"] == ["本文 含む(大小無視,かな無視,幅無視) 単語"]  # 必須の入力なら、無いときの動きは無い


def test_生成した照合は大小_かな_幅をそろえる(tmp_path):
    if shutil.which("rustc") is None:
        pytest.skip("rustc が無い")
    src = generate(chk(MEMO))
    start = src.index("pub fn fold_text")
    end = src.index("pub fn show_set")
    main = src[start:end] + '''
fn main() {
    assert_eq!(fold_text("ＡＢＣ　abc", true, false, true), "abc　abc");
    assert_eq!(fold_text("カタカナ", false, true, false), "かたかな");
    assert_eq!(fold_text("Ａｂ", false, false, false), "Ａｂ");
}
'''
    f = tmp_path / "t.rs"
    f.write_text(main)
    r = subprocess.run(["rustc", "--edition", "2021", "-o", str(tmp_path / "t"), str(f)], capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]
    assert subprocess.run([str(tmp_path / "t")]).returncode == 0


def test_外部作用の無いアプリでも生成した_crate_がビルドできる(tmp_path):
    if shutil.which("cargo") is None:
        pytest.skip("cargo が無い")
    ck = chk(MEMO)
    write_crate(ck, generate(ck), "fn main() {}\n", tmp_path, host=True)
    r = subprocess.run(["cargo", "build", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]


# ---- 確認は「どのキーで引いたものか」に効く（データ名には効かない） ----
PAY = "  let order = Order[order_id] else { return NoSuchOrder }\n  if order.status != Received { return WrongStatus }\n"
TWO = "deed request_payment {\n  input order_id: id required, 別ID: id required\n"


def two_orders(body):
    return mutate(mutate(CODE, "deed request_payment {\n  input order_id: id required\n", TWO), PAY, body)


def test_別の注文の状態を確かめても_こちらは遷移できない():
    ck = chk(two_orders("  let 別 = Order[別ID] else { return NoSuchOrder }\n  let order = Order[order_id] else { return NoSuchOrder }\n  if 別.status != Received { return WrongStatus }\n"))
    assert any(d.code == "遷移" and "受付" in d.msg for d in ck.diags if d.level == ERROR)


def test_同じ名前で引き直すと確認は消える():
    ck = chk(two_orders(PAY + "  let order = Order[別ID] else { return NoSuchOrder }\n"))
    assert any(d.code == "遷移" and "受付" in d.msg for d in ck.diags if d.level == ERROR)


def test_存在の確認は同じキーにだけ効く():
    ck = chk(two_orders(PAY + "  if Order[別ID].total <= 0 { return ZeroTotal }\n"))
    assert any("注文[別ID]" in d.msg for d in ck.diags if d.level == QUESTION)
    ck = chk(two_orders(PAY + "  if Order[別ID] is absent { return NoSuchOrder }\n  if Order[別ID].total <= 0 { return ZeroTotal }\n"))
    assert not any("注文[別ID]" in d.msg for d in ck.diags if d.level == QUESTION)


BULK = """
deed 一括完了 {
  input 他: IdSet
  writes Order.status
  result ReleaseResult
  effects none

  update { Order[k].status = Completed | k in 他 }
  return Applied
}
"""


def test_updateで状態の項目は変えられない():
    assert any("update で状態の項目" in m for m in msgs(chk(CODE + BULK), ERROR))


def test_状態は状態名で変える():
    ck = chk(two_orders(PAY + "  let 別 = Order[別ID] else { return NoSuchOrder }\n").replace("  order.status = PaymentRequested\n", "  order.status = 別.status\n", 1))
    assert any("状態名で書いてください" in m for m in msgs(ck, ERROR))


def test_新規作成の状態は状態名で書く():
    ck = chk(mutate(CODE, "status: Received }", "status: customer_id }"))
    assert any("状態名で書いてください" in m for m in msgs(ck, ERROR))


def test_空の_if_はエラーで止まる():
    ck = chk(mutate(CODE, "  if order.total <= 0 { return ZeroTotal }\n", "  if order.total <= 0 { }\n  if order.total <= 0 { return ZeroTotal }\n"))
    assert any("if の中が空" in m for m in msgs(ck, ERROR))


# ---- 不変条件と項目の範囲は、書き込むその場で、値の範囲から示す ----
REFLECT = ("  writes Order.status\n  transition Order: PaymentRequested → Completed", "  writes Order.status, Order.total\n  transition Order: PaymentRequested → Completed")


def test_項目の更新で在庫を減らすと範囲も不変条件も示せない():
    ck = chk(mutate(mutate(CODE, "  reads Order.total, Order.status\n  writes Order.status\n",
                           "  reads Order.total, Order.status, Product.stock\n  writes Order.status, Product.stock\n"),
                    "  order.status = PaymentRequested\n",
                    "  order.status = PaymentRequested\n  let 品 = Order[order_id] else { return NoSuchOrder }\n  let 商 = Product[order_id] else { return NoSuchOrder }\n  商.stock = 商.stock - 1\n"))
    assert any("反例" in m and "商品.在庫" in m for m in msgs(ck, ERROR))


def test_同じ減算を2回すると不足の確認は効かない():
    up = "  update { Product[k].stock -= v | (k, v) in items }\n"
    assert any("反例" in m for m in msgs(chk(mutate(CODE, up, up + up)), ERROR))


def test_状態がSでないと分かっていれば値を下げてよい():
    ck = chk(mutate(mutate(CODE, *REFLECT), "  order.status = Completed\n", "  order.status = Completed\n  order.total = 0\n"))
    assert ck.ok, ck.diags
    ck = chk(mutate(mutate(CODE, *REFLECT), "  order.status = Completed\n", "  order.total = 0\n  order.status = Completed\n"))
    assert any("合計 > 0" in m for m in msgs(ck, ERROR))


def test_Sで新規作成するなら値の確認が要る():
    ck = chk(mutate(mutate(CODE, "transition Order: (new) → Received", "transition Order: (new) → PaymentRequested"), "status: Received }", "status: PaymentRequested }"))
    assert any("合計 > 0" in m for m in msgs(ck, ERROR))


def test_分岐の中の書き込みで確認は崩れる():
    ck = chk(mutate(mutate(CODE, "  reads Order.total, Order.status\n  writes Order.status\n", "  reads Order.total, Order.status\n  writes Order.status, Order.total\n"),
                    "  order.status = PaymentRequested\n", "  if order.total > 100 {\n    order.total = 0\n  }\n  order.status = PaymentRequested\n"))
    assert any("合計 > 0" in m for m in msgs(ck, ERROR))


def test_遷移した後は元の状態の確認が消える():
    ck = chk(mutate(mutate(CODE, "transition Order: Received → PaymentRequested", "transition Order: Received → PaymentRequested\n  transition Order: Received → Canceled"),
                    "  order.status = PaymentRequested\n", "  order.status = PaymentRequested\n  order.status = Canceled\n"))
    assert any(d.code == "遷移" and "受付" in d.msg for d in ck.diags if d.level == ERROR)


def test_同じデータの別の束縛の確認も遷移で消える():
    ck = chk(mutate(two_orders(PAY + "  let 別 = Order[別ID] else { return NoSuchOrder }\n  if 別.status != Received { return WrongStatus }\n"),
                    "  order.status = PaymentRequested\n", "  order.status = PaymentRequested\n  別.status = PaymentRequested\n"))
    assert any(d.code == "遷移" and "受付" in d.msg for d in ck.diags if d.level == ERROR)


def test_書けない形の不変条件はエラーになる():
    ck = chk(CODE + "\ninvariant Product: stock <= 100\n")
    assert any("まだ検査できません" in m for m in msgs(ck, ERROR))


def test_項目の更新は型を確かめる():
    ck = chk(mutate(mutate(CODE, *REFLECT), "  order.status = Completed\n", "  order.status = Completed\n  order.total = order_id\n"))
    assert any("型の違う値" in m for m in msgs(ck, ERROR))


@needs_cargo
def test_項目へ数を書くと物理数へ変換される(tmp_path):
    ck = chk(mutate(mutate(CODE, *REFLECT), "  order.status = Completed\n", "  order.status = Completed\n  order.total = 0\n"))
    write_crate(ck, generate(ck), "fn main() {}\n", tmp_path)
    r = subprocess.run(["cargo", "build", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]


def test_減らした後に確かめた不足の集まりは古い():
    chk_line = "  if short is nonempty { return OutOfStock(short) }\n"
    up = "  update { Product[k].stock -= v | (k, v) in items }\n"
    src = mutate(CODE, up, up + chk_line + up)
    assert chk(mutate(CODE, up, up)).ok
    assert any("反例" in m for m in msgs(chk(src), ERROR))


# ---- 外部作用とフローの重複・矛盾 ----
FOR = """
deed 引当 {
  input items: ProductQuantity required, order_id: id required
  result ReleaseResult
  effects 倉庫.引当

  for (k, v) in items {
    request 倉庫.引当(k, v, idempotency_key = KEY)
  }
  return Applied
}

nexus 引当の流れ {
  引当: Applied → reply, NoSuchOrder → reply, WrongStatus → reply
}
"""


def with_for(key):
    src = mutate(CODE, EFFECT,
                 EFFECT + '\neffect 倉庫.引当(product_id: id, 数: Quantity) no_wait "試し"')
    return chk(src + FOR.replace("KEY", key))


def test_for_の中の冪等キーは回す値と外の値の組にする():
    assert any("for で回す値によりません" in m for m in msgs(with_for("order_id"), ERROR))
    assert any("for で回す値だけです" in m for m in msgs(with_for("k"), ERROR))  # 別の注文で同じ商品を回すとぶつかる
    assert not any("冪等" in m for m in msgs(with_for("(order_id, k)"), ERROR))
    assert not any("冪等" in m for m in msgs(with_for("(k, order_id)"), ERROR))
    assert any("冪等キーは ID か文字列" in m for m in msgs(with_for("(order_id, v)"), ERROR))


@needs_cargo
def test_join_idem_keyは区切りがあいまいにならない(tmp_path):
    ck = with_for("(order_id, k)")
    assert ck.ready, ck.diags
    main = """fn main() {
    println!("{}", geas_app::join_idem_key(&["a,b".to_string(), "c".to_string()]));
    println!("{}", geas_app::join_idem_key(&["a".to_string(), "b\\",\\"c".to_string()]));
    println!("{}", geas_app::join_idem_key(&["O1".to_string(), "P1".to_string()]));
}
"""
    write_crate(ck, generate(ck), main, tmp_path)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    a, b, c = r.stdout.splitlines()
    assert a != b and c == '["O1","P1"]', (a, b, c)


def test_冪等キーは_ID_か文字列():
    ck = chk(mutate(CODE, "idempotency_key = order_id)", "idempotency_key = 1)"))
    assert any("冪等キーは ID か文字列" in m for m in msgs(ck, ERROR))


def test_応答と処理を同じ結果に並べると止まる():
    ck = chk(mutate(CODE, "OutOfStock → reply, NoSuchProduct", "OutOfStock → par { reply, request_payment }, NoSuchProduct"))
    assert any("par に入れられません" in m for m in msgs(ck, ERROR))
    with pytest.raises(ParseError, match="par"):
        parse(mutate(CODE, "OutOfStock → reply, NoSuchProduct", "OutOfStock → reply, OutOfStock → request_payment, NoSuchProduct"))


NOTIFY = """
effect 通知.メール(order_id: id) no_wait "通知は結果を待たない"
effect 通知.プッシュ(order_id: id) no_wait "通知は結果を待たない"
result 通知結果 {
  Requested
}
deed メール通知 {
  input order_id: id required
  result 通知結果
  effects 通知.メール

  request 通知.メール(order_id, idempotency_key = order_id)
  return Requested
}
deed プッシュ通知 {
  input order_id: id required
  result 通知結果
  effects 通知.プッシュ

  request 通知.プッシュ(order_id, idempotency_key = order_id)
  return Requested
}
nexus メール {
  メール通知: Requested → reply
}
nexus プッシュ {
  プッシュ通知: Requested → reply
}
"""
FANOUT = ("place_order: Received → request_payment, OutOfStock", "place_order: Received → par { メール通知, プッシュ通知 }, OutOfStock")


def fanout(*edits):
    return chk(mutate(CODE + NOTIFY, *edits))


def test_同じ結果に処理を続けて書くと_書いた順に起こす行き先になる():
    ck = fanout(*FANOUT)
    assert ck.ready, ck.diags
    assert build(ck)["処理"][0]["結果"][0] == ("受付", "並列［メール通知 ＋ プッシュ通知］")
    assert any("2回書かれています" in m for m in msgs(fanout(FANOUT[0], "place_order: Received → par { メール通知, メール通知 }, OutOfStock"), ERROR))
    src = generate(ck)
    assert "vec![Dest::Deed(\"メール通知\"), Dest::Deed(\"プッシュ通知\")]" in src


def test_par_の枝は書く項目が重ならなければデータを書いてよい():
    ck = fanout(FANOUT[0], "place_order: Received → par { メール通知, request_payment }, OutOfStock")
    assert ck.ready, ck.diags


def test_par_の枝が同じ項目に書くか_書かれる項目を読むと止まる():
    ck = fanout(FANOUT[0], "place_order: Received → par { request_payment, expire_payment }, OutOfStock")
    assert any("同じ項目「注文.状態」に書いています" in m for m in msgs(ck, ERROR)), ck.diags
    assert any("どちらも 注文 の状態を動かします" in m for m in msgs(ck, ERROR))
    ck = fanout(FANOUT[0], "place_order: Received → par { request_payment, place_order }, OutOfStock")
    assert any("par の枝" in m and "新規作成する" in m for m in msgs(ck, ERROR)), ck.diags


STOCK_DEEDS = """
result 減らす結果 {
  Reduced
  Short
}
deed 在庫を減らすA {
  input items: ProductQuantity required
  reads Product.stock
  writes Product.stock
  result 減らす結果
  effects none

  let short = { k | (k, v) in items, Product[k].stock < v }
  if short is nonempty { return Short }
  update { Product[k].stock -= v | (k, v) in items }
  return Reduced
}
deed 在庫を減らすB {
  input items: ProductQuantity required
  reads Product.stock
  writes Product.stock
  result 減らす結果
  effects none

  let short = { k | (k, v) in items, Product[k].stock < v }
  if short is nonempty { return Short }
  update { Product[k].stock -= v | (k, v) in items }
  return Reduced
}
nexus 減らすA {
  在庫を減らすA: Reduced → reply, Short → reply
}
nexus 減らすB {
  在庫を減らすB: Reduced → reply, Short → reply
}
"""


def test_par_の枝は加算減算の更新だけなら同じ項目に書いてよい():
    edit = (FANOUT[0], "place_order: Received → par { 在庫を減らすA, 在庫を減らすB }, OutOfStock")
    ck = chk(mutate(CODE + STOCK_DEEDS, *edit))
    # place_order の結果の中身（受付の注文ID）と合わないので、行き先の受け渡しの誤りだけが出る。並列の誤りは出ない
    assert not any("par の枝" in m for m in msgs(ck, ERROR)), ck.diags
    # 片方が = で書くと、順序で結果が変わるので止まる
    assert any("同じ項目" in m for m in msgs(fanout(FANOUT[0], "place_order: Received → par { request_payment, expire_payment }, OutOfStock"), ERROR))


@needs_cargo
def test_行き先が複数のフローも生成コードがビルドできる(tmp_path):
    ck = fanout(*FANOUT)
    write_crate(ck, generate(ck), "fn main() {}\n", tmp_path)
    r = subprocess.run(["cargo", "build", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]


def test_同じ処理を違う行き先で2つのネクサスに書くと止まる():
    ck = chk(CODE + "\nnexus 二重 {\n  place_order: Received → reply, OutOfStock → reply, NoSuchProduct → reply, NoSuchCustomer → reply\n}\n")
    assert any("違う行き先" in m for m in msgs(ck, ERROR))


def test_失敗の理由を宣言して待たないのは矛盾():
    ck = chk(mutate(CODE, EFFECT, EFFECT + ' failures 残高不足 no_wait "試し"'))
    assert any("no_wait" in m for m in msgs(ck, ERROR))


# ---- 生成した Rust が必ずビルドできる ----
def test_定義の名前の重複は止まる():
    for old, new in [("entity Customer {", "entity Customer {\n  customer_id: id unique\n}\n\nentity Customer {"),
                     ("  stock: StockCount required\n", "  stock: StockCount required\n  stock: NonNegMoney\n"),
                     ("  input order_id: id required\n  reads Order.total", "  input order_id: id required\n  slot order_id: id\n  reads Order.total")]:
        assert any("2つ以上" in m for m in msgs(chk(mutate(CODE, old, new)), ERROR)), new


def test_未定義の型の名前は止まる():
    ck = chk(mutate(CODE, "  input order_id: id required\n  reads Order.total", "  input order_id: id required, 謎: 謎型\n  reads Order.total"))
    assert any("謎型" in m for m in msgs(ck, ERROR))


def test_最後に_return_が無いと止まる():
    ck = chk(mutate(CODE, "  request Payment.charge(order_id, order.total, idempotency_key = order_id)\n  return Requested\n",
                    "  request Payment.charge(order_id, order.total, idempotency_key = order_id)\n"))
    assert any("return で終えて" in m for m in msgs(ck, ERROR))


def test_if_の条件は真偽():
    ck = chk(mutate(CODE, "  if order.total <= 0 { return ZeroTotal }\n", "  if order.total <= 0 { return ZeroTotal }\n  if order.total { return ZeroTotal }\n"))
    assert any("if の条件" in m for m in msgs(ck, ERROR))


def test_update_の値の型を確かめる():
    src = CODE + BULK.replace("一括完了", "一括ゼロ").replace("writes Order.status", "writes Order.total").replace("Order[k].status = Completed", "Order[k].total = Completed")
    assert any("型の違う値" in m for m in msgs(chk(src), ERROR))


@needs_cargo
def test_割合の参照があってもホストつきでビルドできる(tmp_path):
    ck = chk(CODE)
    write_crate(ck, generate(ck), "fn main() {\n    let r = geas_app::host::read_ratio(\"1/10\", Some(0), Some(1)).unwrap();\n"
                "    assert_eq!(r.to_string(), \"1/10\");\n    assert!(geas_app::host::read_ratio(\"3/2\", Some(0), Some(1)).is_err());\n}\n", tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]


# ---- 誤検出と警告の漏れ ----
def test_update_の書き込みは_ほかで読んだ項目を消さない():
    src = CODE + BULK.replace("一括完了", "一括ゼロ").replace("writes Order.status", "reads Order.total\n  writes Order.total").replace(
        "  update { Order[k].status = Completed | k in 他 }\n", "  update { Order[k].total = 1 | k in 他, Order[k].total > 0 }\n")
    ck = chk(src)
    assert not any("注文.合計" in m and "使っていません" in m for m in msgs(ck, ERROR)), ck.diags


def test_使わない外部作用と届かない状態は警告():
    ck = chk(mutate(CODE, EFFECT, EFFECT + "\neffect 倉庫.引当(product_id: id) no_wait \"試し\""))
    assert any("どの処理からも要求されていません" in m for m in msgs(ck, "警告"))
    ck = chk(mutate(CODE, "transition Order: Received → PaymentRequested", "transition Order: Received → Received"))
    assert any("決済要求済" in m and "進む処理" in m for m in msgs(ck, "警告"))


# ---- ネクサスで次の処理へは、結果の中身を名前と型で渡す ----
def test_結果の中身には名前が要る():
    with pytest.raises(ParseError, match="名前を付けて"):
        parse(mutate(CODE, "  Received(order_id: id)\n", "  Received(id)\n"))


def test_結果の中身と次の処理の入力は名前と型で突き合わせる():
    assert any("中身「不足」を、処理 決済要求 は入力にもっていません" in m
               for m in msgs(chk(mutate(CODE, "OutOfStock → reply, NoSuchProduct", "OutOfStock → request_payment, NoSuchProduct")), ERROR))
    ck = chk(mutate(CODE, "  Received(order_id: id)\n", "  Received(番号: id)\n"))
    assert any("入力「注文ID」が、結果「受付」の中身にありません" in m for m in msgs(ck, ERROR))
    ck = chk(mutate(CODE, "  Received(order_id: id)\n", "  Received(order_id: IdSet)\n"))
    assert any("型" in m and "受付" in m for m in msgs(ck, ERROR))


@needs_cargo
def test_ホストは結果の中身を名前つきで返す(tmp_path):
    ck = chk(CODE)
    req = ('{"name":"place_order","ctx":{"tax_rate":"1/10","new_Order_id":"N1"},"input":{"customer_id":"C1","items":{"A":2}},'
           '"store":{"Product":{"A":{"product_id":"A","stock":5,"unit_price":100}},"Customer":{"C1":{"customer_id":"C1"}}}}')
    write_crate(ck, generate(ck), f'fn main() {{\n    println!("{{}}", geas_app::host::call(r#"{req}"#).unwrap());\n}}\n', tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]
    assert '"body":{"order_id":"N1"}' in r.stdout, r.stdout


# ---- 整数と金額は違う型。そのままでは混ぜず、money( )・int( ) で変えてから計算する ----
SUB = "  let total = subtotal + round(subtotal * tax_rate, floor, JPY)\n"


def test_整数と金額はそのままでは足し引きできない():
    ck = chk(mutate(CODE, SUB, SUB.replace("subtotal + round", "subtotal + total_count + round").replace("  let total", "  let total_count = sum { v | (k, v) in items }\n  let total")))
    assert any("整数と金額は、そのままでは足し引きできません" in m for m in msgs(ck, ERROR))


def test_金額どうしは掛けられない():
    ck = chk(mutate(CODE, "Product[k].unit_price * v |", "Product[k].unit_price * Product[k].unit_price |"))
    assert any("金額どうしは掛けられません" in m for m in msgs(ck, ERROR))


def test_整数の変数は金額の項目へそのまま入れられない():
    src = mutate(CODE, SUB, "  let total_count = sum { v | (k, v) in items }\n" + SUB)
    ck = chk(mutate(src, "total: total, status: Received }", "total: total_count, status: Received }"))
    assert any("型の違う値（Int）" in m for m in msgs(ck, ERROR))
    ck = chk(mutate(src, "total: total, status: Received }", "total: money(total_count), status: Received }"))
    assert ck.ok, ck.diags


def test_数字はどちらの型としても使える():
    assert chk(CODE).ok  # 合計 <= 0、在庫 < v など
    ck = chk(mutate(CODE, SUB, "  let total = subtotal + 100\n"))
    assert ck.ok, ck.diags


def test_変換の向きを取り違えると止まる():
    ck = chk(mutate(CODE, SUB, "  let total = money(subtotal)\n"))
    assert any("money( ) で変えられるのは整数だけ" in m for m in msgs(ck, ERROR))


@needs_cargo
def test_型の変換を使っても_Rust_がビルドできる(tmp_path):
    src = mutate(CODE, SUB, "  let total_count = sum { v | (k, v) in items }\n  let total = subtotal + money(total_count) + round(subtotal * tax_rate, floor, JPY)\n")
    ck = chk(src)
    assert ck.ok, ck.diags
    write_crate(ck, generate(ck), "fn main() {}\n", tmp_path)
    r = subprocess.run(["cargo", "build", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]


# ---- すべては原則「無いかもしれない」。必須の約束は required ----
NOTE = """ltype 重さ値 = int 0..

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
"""


def test_データの項目に_optional_は書かない():
    with pytest.raises(ParseError, match="required"):
        parse(NOTE.replace("  題: text\n", "  題: text optional\n"))


def test_無いかもしれない項目は確かめてから使う():
    assert chk(NOTE).ok, chk(NOTE).diags
    ck = chk(NOTE.replace("  let 題 = メモ.題 else { return 題なし }\n  return 題あり(題)", "  return 題あり(メモ.題)"))
    assert any("null の" in m for m in msgs(ck, ERROR))
    ck = chk(NOTE.replace("  let 題 = メモ.題 else { return 題なし }\n  return 題あり(題)",
                          "  if メモ.題 is null { return 題なし }\n  return 題あり(メモ.題)"))
    assert ck.ok, ck.diags


def test_必須の項目は新規作成で省けず_null_も入れられない():
    assert any("必須の項目「本文」" in m for m in msgs(chk(NOTE.replace("create メモ { 本文: 本文 }", "create メモ { 題: 本文 }")), ERROR))
    assert any("null は入れられません" in m for m in msgs(chk(NOTE.replace("create メモ { 本文: 本文 }", "create メモ { 本文: null }")), ERROR))
    assert any("is null" in m for m in msgs(chk(NOTE.replace("if メモ.題 is null", "if メモ.本文 is null").replace(
        "  let 題 = メモ.題 else { return 題なし }\n", "  if メモ.本文 is null { return 題なし }\n  let 題 = メモ.題 else { return 題なし }\n")), ERROR))


def test_null_を書いた後は確かめ直す():
    src = NOTE.replace("  reads メモ.題\n", "  reads メモ.題\n  writes メモ.題\n").replace(
        "  let 題 = メモ.題 else { return 題なし }\n  return 題あり(題)", "  let 題 = メモ.題 else { return 題なし }\n  メモ.題 = null\n  return 題あり(メモ.題)")
    assert any("null の" in m for m in msgs(chk(src), ERROR))


def test_null_は下限の不変条件を満たさない():
    ck = chk(NOTE + "\ninvariant メモ: 重さ >= 0\n")
    assert any("null は満たしません" in m for m in msgs(ck, ERROR))


def test_仕様書の必須は_required_とキーと状態():
    spec = build(chk(NOTE))
    req = {(d, f): r for d, f, _, r, _ in spec["データ"]}
    assert req[("メモ", "本文")] == "○" and req[("メモ", "メモID")] == "○" and req[("メモ", "題")] == ""


@needs_cargo
def test_ホストは_null_とキーが無いことを区別する(tmp_path):
    ck = chk(NOTE)
    call = lambda store: '{"name":"題を見る","ctx":{},"input":{"メモID":"M1"},"store":{"メモ":{"M1":' + store + '}}}'
    main = "fn main() {\n" + "".join(
        f'    println!("{{:?}}", geas_app::host::call(r#"{call(s)}"#));\n'
        for s in ['{"メモID":"M1","本文":"a","題":"t","重さ":null}', '{"メモID":"M1","本文":"a","題":null,"重さ":null}',
                  '{"メモID":"M1","本文":"a","重さ":null}']) + "}\n"
    write_crate(ck, generate(ck), main, tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]
    got = r.stdout.splitlines()
    assert '\\"result\\":\\"題あり\\"' in got[0] and '\\"body\\":{\\"題\\":\\"t\\"}' in got[0], got[0]
    assert '\\"result\\":\\"題なし\\"' in got[1], got[1]
    assert got[2].startswith("Err(") and "題" in got[2], got[2]


NULLIN = """ltype 重さ値 = int 0..
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
"""


def test_入力は既定で無いかもしれず_確かめてから使う():
    assert chk(NULLIN).ok, chk(NULLIN).diags
    ck = chk(NULLIN.replace("  if 重さ is null { return 無題 }\n", ""))
    assert any("入力「重さ」は無いことがあります" in m for m in msgs(ck, ERROR))
    with pytest.raises(ParseError, match="required"):
        parse(NULLIN.replace("題: text, 重さ", "題: text optional, 重さ"))


def test_トリガの引数には必ず要る入力を渡す():
    ck = chk(mutate(CODE, "deed request_payment {\n  input order_id: id required\n", "deed request_payment {\n  input order_id: id\n"))
    assert any("required の入力を渡してください" in m for m in msgs(ck, ERROR))


@needs_cargo
def test_ホストは無いかもしれない入力のキー無しと_null_を同じに扱う(tmp_path):
    ck = chk(NULLIN)
    calls = [("書く", '{"本文":"a","題":"t","重さ":3}'), ("書く", '{"本文":"a","重さ":3}'), ("書く", '{"本文":"a","題":null,"重さ":3}'),
             ("書く", '{"本文":"a","題":"","重さ":3}'), ("書く", '{"題":"t","重さ":3}'), ("書く", '{"本文":null,"題":"t","重さ":3}'),
             ("書く", '{"本文":"a","題":"t","重さ":10}'), ("下書き", '{"本文":"a"}')]
    main = "fn main() {\n" + "".join(
        f'    println!("{{:?}}", geas_app::host::call(r#"{{"name":"{n}","ctx":{{"new_メモ_id":"M1"}},"input":{i},"store":{{}}}}"#));\n'
        for n, i in calls) + "}\n"
    write_crate(ck, generate(ck), main, tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    got = r.stdout.splitlines()
    ok = lambda x, v: f'\\"result\\":\\"{v}\\"' in x
    assert ok(got[0], "書いた") and ok(got[1], "無題") and ok(got[2], "無題"), got[:3]
    assert ok(got[3], "書いた"), got[3]  # 空文字は「ある」
    assert all(g.startswith("Err(") for g in got[4:7]), got[4:7]  # 必須の入力が無い・null、範囲の外は入力不正
    assert ok(got[7], "書いた") and '\\"題\\":null' in got[7], got[7]  # 無い入力は、そのまま null の項目へ


def test_ホストは宣言にない入力の項目を無いものとして扱う(tmp_path):
    ck = chk(NULLIN)
    calls = ['{"本文":"a","重さ":3}', '{"本文":"a","重さ":3,"余分":"x","題名":"typo"}']
    main = "fn main() {\n" + "".join(
        f'    println!("{{:?}}", geas_app::host::call(r#"{{"name":"書く","ctx":{{"new_メモ_id":"M1"}},"input":{i},"store":{{}}}}"#));\n'
        for i in calls) + "}\n"
    write_crate(ck, generate(ck), main, tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    got = r.stdout.splitlines()
    assert got[0] == got[1] and '\\"result\\":\\"無題\\"' in got[0], got  # 宣言にない項目は、来ていないのと同じ


TEXT = """ltype 利用者名 = text 1..20 of "a-z0-9_"

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
"""


def test_文字列の長さと使える文字は仕様書の型に出る():
    from geas.ast import show_type
    ck = chk(TEXT)
    assert ck.ok, ck.diags
    t = ck.data["利用者"].fields[1].type
    assert show_type(t) == "文字列 1..20 文字種 [0-9_a-z]"


def 入力だけ型を変える(decl):
    """項目の型は利用者名のまま、入力の名前の型だけを、新しい論理型（decl）に変える"""
    src = mutate(TEXT, 'ltype 利用者名 = text 1..20 of "a-z0-9_"\n', 'ltype 利用者名 = text 1..20 of "a-z0-9_"\n' + decl + "\n")
    return mutate(src, "input 名: 利用者名", "input 名: 入力名")


def test_長さや文字が広い入力を項目へ入れると止まる():
    assert not msgs(chk(入力だけ型を変える('ltype 入力名 = text 1..10 of "a-z0-9_"')), ERROR)  # 狭いのはよい
    ck = chk(入力だけ型を変える('ltype 入力名 = text 1..30 of "a-z0-9_"'))
    assert any("長さ" in m and "収まる" in m for m in msgs(ck, ERROR)), ck.diags
    ck = chk(mutate(TEXT, "input 名: 利用者名", "input 名: text"))
    assert any("長さ" in m for m in msgs(ck, ERROR)), ck.diags
    ck = chk(入力だけ型を変える('ltype 入力名 = text 1..20 of "a-z0-9_A-Z"'))
    assert any("文字" in m and "収まる" in m for m in msgs(ck, ERROR)), ck.diags
    ck = chk(入力だけ型を変える("ltype 入力名 = text 1..20"))
    assert any("文字" in m for m in msgs(ck, ERROR)), ck.diags


def test_文字種の書き方の誤りは止まる():
    for bad in ('of "z-a"', 'of ""', 'of "a\\"'):
        with pytest.raises(ParseError):
            chk(mutate(TEXT, 'of "a-z0-9_"\n', bad + "\n"))


def test_ホストは入力の長さと文字を境界で確かめる(tmp_path):
    ck = chk(TEXT)
    calls = ['{"名":"abc_09"}', '{"名":""}', '{"名":"' + "a" * 21 + '"}', '{"名":"ABC"}', '{"名":"あ"}', '{"名":"' + "a" * 20 + '"}']
    main = "fn main() {\n" + "".join(
        f'    println!("{{:?}}", geas_app::host::call(r#"{{"name":"登録する","ctx":{{"new_利用者_id":"U1"}},"input":{i},"store":{{}}}}"#));\n'
        for i in calls) + "}\n"
    write_crate(ck, generate(ck), main, tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    got = r.stdout.splitlines()
    assert '登録した' in got[0] and '登録した' in got[5], (got[0], got[5])
    assert all(g.startswith("Err(") for g in got[1:5]), got[1:5]  # 空・長すぎ・使えない文字は入力不正


TIMEC = """entity 約束 {
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
"""


def test_時刻に期間を足し引きでき_時刻の差は秒数になる():
    assert chk(TIMEC).ok, chk(TIMEC).diags
    assert chk(mutate(TIMEC, "現在 + 24 hour", "現在 - 90 min")).ok
    assert chk(mutate(TIMEC, "現在 + 24 hour", "24 hour + 現在")).ok
    for bad in ("現在 + 開始", "現在 + 5", "24 hour", "現在 * 2"):
        ck = chk(mutate(TIMEC, "期限: 現在 + 24 hour", f"期限: {bad}"))
        assert msgs(ck, ERROR), bad
    ck = chk(mutate(TIMEC, "待ち秒: 現在 - 開始", "待ち秒: 24 hour - 現在"))
    assert msgs(ck, ERROR)
    for unit in ("day", "week", "month", "year"):  # 暦で数え方が変わる単位は、まだ書けない
        with pytest.raises(ParseError, match="暦で数え方が変わる"):
            chk(mutate(TIMEC, "24 hour", f"1 {unit}"))
    with pytest.raises(ParseError, match="暦で数え方が変わる"):  # 絞り込み・期限の単位も同じ
        chk(mutate(CODE, "deadline now + 30 min", "deadline now + 1 day"))
    with pytest.raises(ParseError, match="長すぎます"):
        chk(mutate(TIMEC, "24 hour", "99999999999 sec"))


def test_ホストは時刻の計算をして_範囲外の時刻は境界で止める(tmp_path):
    ck = chk(TIMEC)
    calls = [(1000, 400), (1000, 10**16), (10**16, 400)]
    main = "fn main() {\n" + "".join(
        f'    println!("{{:?}}", geas_app::host::call(r#"{{"name":"約束する","ctx":{{"new_約束_id":"K1","現在":{n}}},"input":{{"開始":{b}}},"store":{{}}}}"#));\n'
        for n, b in calls) + "}\n"
    write_crate(ck, generate(ck), main, tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    got = r.stdout.splitlines()
    assert "87400" in got[0] and "600" in got[0], got[0]  # 期限 = 1000 + 86400、待ち = 1000 - 400
    assert got[1].startswith("Err(") and got[2].startswith("Err("), got[1:]


SLOT = """ltype 千以下の数 = int 0..1000
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
"""


@needs_cargo
def test_slot_も境界で宣言の範囲を確かめる(tmp_path):
    ck = chk(SLOT)
    assert ck.ready, ck.diags
    call = lambda k: f'{{"name":"置く","ctx":{{"倍率":{k},"new_品_id":"A"}},"input":{{"数":10}},"store":{{}}}}'
    main = "fn main() {\n" + "".join(f'    println!("{{:?}}", geas_app::host::call(r#"{call(k)}"#));\n' for k in (100, 101, 0)) + "}\n"
    write_crate(ck, generate(ck), main, tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    got = r.stdout.splitlines()
    assert got[0].startswith("Ok(") and '\\"数\\":1000' in got[0], got[0]  # 範囲の端は通る
    assert got[1].startswith("Err(") and "倍率" in got[1], got[1]  # 宣言の外は処理を走らせない
    assert got[2].startswith("Err(") and "倍率" in got[2], got[2]


PART = """ltype 非負金額 = money(JPY) 0..
ltype 税抜の限り = money(JPY) 0..1000000

entity 品 {
  品ID: id unique
  価格: 非負金額 required
}
result 置いた結果 {
  置いた(品ID: id)
}
part 税込(x: 非負金額) -> 非負金額 guarantee result >= x
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
"""


def test_部品の保証は_geas_の式で書き_引数と_result_だけを使う():
    ck = chk(PART)
    assert ck.ready, ck.diags
    assert build(ck)["部品"] == [("税込", "x: 金額(円) 0..", "金額(円) 0..", "result >= x")]
    with pytest.raises(ParseError, match="保証がありません"):
        parse(PART.replace(" guarantee result >= x", ""))
    assert any("戻り値（result）を使っていません" in m for m in msgs(chk(PART.replace("result >= x", "x >= 0")), ERROR))
    assert any("真偽の式" in m for m in msgs(chk(PART.replace("result >= x", "result + x")), ERROR))
    assert any("定義されていません" in m for m in msgs(chk(PART.replace("result >= x", "result >= y")), ERROR))


def test_部品の呼び出しは宣言と型が合わないと止まる():
    assert any("引数は 1 個" in m for m in msgs(chk(PART.replace("税込(税抜)", "税込(税抜, 税抜)")), ERROR))
    assert any("型が違います" in m for m in msgs(chk(PART.replace("税込(税抜)", "税込(int(税抜))")), ERROR))
    assert any("組み込みの部品と同じ名前" in m for m in msgs(chk(PART.replace("税込", "round")), ERROR))


@needs_cargo
def test_部品の保証は呼ぶたびに確かめ_違反は保存を変えずに止まる(tmp_path):
    ck = chk(PART)
    main = """use geas_app::*;
use geas_app::host::call;
struct 良い;
impl Parts for 良い { fn 税込(&self, x: i64) -> i64 { x + x / 10 } }
struct 悪い;
impl Parts for 悪い { fn 税込(&self, x: i64) -> i64 { x - 1 } }
fn main() {
    let req = r#"{"name":"置く","ctx":{"new_品_id":"A"},"input":{"税抜":1000},"store":{}}"#;
    println!("{:?}", call(req, &良い));
    println!("{:?}", call(req, &悪い));
}
"""
    write_crate(ck, generate(ck), main, tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    ok, ng = r.stdout.splitlines()
    assert ok.startswith("Ok(") and '\\"価格\\":1100' in ok, ok
    assert ng.startswith("Err(") and "税込" in ng and "result >= x" in ng, ng


@needs_cargo
def test_組み込みの丸めは単位より近い値を返す(tmp_path):
    """組み込みの部品の保証は、宣言せず、ここで確かめる（切り捨て・切り上げは差が 1 未満、四捨五入は 1/2 以下）"""
    ck = chk(PART)
    main = """use geas_app::*;
fn main() {
    let mut bad = 0;
    for a in -50i128..50 {
        for b in 1i128..13 {
            let x = Ratio { num: a, den: b };
            let (lo, up, rn) = (floor(x), ceil(x), half_up(x));
            if !(lo * b <= a && a < (lo + 1) * b) { bad += 1; }
            if !((up - 1) * b < a && a <= up * b) { bad += 1; }
            if !(2 * (rn * b - a).abs() <= b) { bad += 1; }
        }
    }
    println!("{}", bad);
}
"""
    write_crate(ck, generate(ck), main, tmp_path)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    assert r.stdout.strip() == "0"


NULLQ = """ltype 重さ値 = int 0..
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
  order 重さ newest
  select メモID, 書いた人.名前 as 名前, sum(札) as 札数, count(札) as 札の種類
  reads メモ.メモID, メモ.書いた人, 人.名前, メモ.重さ, メモ.札
}
"""


@needs_cargo
def test_null_になりうる項目の問い合わせも_Rust_がビルドできる(tmp_path):
    ck = chk(NULLQ)
    assert ck.ok, ck.diags
    main = """use geas_app::*;
fn main() {
    let mut s = Store::default();
    s.人.insert("P".into(), 人 { 人ID: "P".into(), 名前: None });
    s.メモ.insert("A".into(), メモ { メモID: "A".into(), 書いた人: Some("P".into()), 重さ: None, 札: None });
    s.メモ.insert("B".into(), メモ { メモID: "B".into(), 書いた人: None, 重さ: Some(3), 札: Some([("P".to_string(), 2u8)].into()) });
    let all = 一覧(&一覧Ctx {}, &s, &一覧Input { 最小: None });
    println!("{}", all.iter().map(|x| x.show()).collect::<Vec<_>>().join(";"));
    println!("{}", 一覧(&一覧Ctx {}, &s, &一覧Input { 最小: Some(1) }).len());
}
"""
    write_crate(ck, generate(ck), main, tmp_path)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    rows, n = r.stdout.splitlines()
    assert rows == "メモID=B/名前=null/札数=2/札の種類=1;メモID=A/名前=null/札数=0/札の種類=0", rows  # null は並びの最後
    assert n == "1"  # 重さが null のものは絞り込みを満たさない


PAGEQ = """ltype 重さ値 = int 0..
ltype 件数値 = int 1..100

entity メモ {
  メモID: id unique
  重さ: 重さ値
}
query 一覧 {
  input 件数: 件数値
  from メモ
  order 重さ newest
  page 件数
  reads メモ.重さ
}
query 先頭 {
  from メモ
  order 重さ oldest
  page 2
  reads メモ.重さ
}
"""


def test_page_は_order_が無いとエラーで_limit_は使えない():
    assert chk(PAGEQ).ok, chk(PAGEQ).diags
    ck = chk(mutate(PAGEQ, "  order 重さ oldest\n  page 2", "  page 2"))
    assert any("order が要ります" in m for m in msgs(ck, ERROR)), ck.diags
    with pytest.raises(ParseError, match="page に替わりました"):
        chk(mutate(PAGEQ, "page 2", "limit 2"))
    with pytest.raises(ParseError):
        chk(mutate(PAGEQ, "page 2", "page 0"))


def test_page_の件数は仕様書に出る():
    from geas.spec import query_rows
    rows = {q.name: query_rows(q) for q in chk(PAGEQ).p.queries}
    assert rows["先頭"]["ページ"] == "2件" and rows["一覧"]["ページ"] == "件数（件数が無ければ全件）"


@needs_cargo
def test_page_は並びのうえで先頭の1ページを返す(tmp_path):
    ck = chk(PAGEQ)
    main = """use geas_app::*;
fn main() {
    let mut s = Store::default();
    for (id, w) in [("A", 1i64), ("B", 5), ("C", 3)] { s.メモ.insert(id.into(), メモ { メモID: id.into(), 重さ: Some(w as _) }); }
    println!("{:?}", 先頭(&先頭Ctx {}, &s, &先頭Input {}));
    println!("{:?}", 一覧(&一覧Ctx {}, &s, &一覧Input { 件数: Some(2) }));
}
"""
    write_crate(ck, generate(ck), main, tmp_path)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    a, b = r.stdout.splitlines()
    assert a == '["A", "C"]' and b == '["B", "C"]', (a, b)  # oldest の先頭2件／newest の先頭2件


def test_問いが残っているうちは生成しない():
    ck = chk(mutate(CODE, "let total = subtotal + round(subtotal * tax_rate, floor, JPY)", "let total = subtotal + subtotal * tax_rate"))
    assert ck.ok and not ck.ready
    with pytest.raises(AssertionError, match="問い"):
        generate(ck)


# ---- 論理型（ltype）と entity ----
LTYPE = """ltype Quantity = int 1..99
ltype ProductQuantity = Product → Quantity

entity Product {
  product_id: id unique
  stock: Quantity required
}

entity Order {
  order_id: id unique
  items: ProductQuantity required
}
"""


def test_ltype_は使った場所に定義が写される():
    p = parse(LTYPE)
    stock = next(f for d in p.data if d.name == "Product" for f in d.fields if f.name == "stock")
    assert stock.type.kind == "整数" and (stock.type.range.lo, stock.type.range.hi) == (1, 99)
    lines = next(f for d in p.data if d.name == "Order" for f in d.fields if f.name == "items")
    assert lines.type.kind == "写像" and lines.type.key == "Product" and lines.type.value.range.hi == 99


def test_ltype_の名前の重複と組み込み名は止まる():
    with pytest.raises(ParseError, match="2つ以上"):
        parse("ltype 数量 = int 1..99\nltype 数量 = int 1..9\n")
    with pytest.raises(ParseError, match="組み込み"):
        parse("ltype int = int 1..99\n")


def test_data_は_entity_に改名した():
    with pytest.raises(ParseError, match="entity"):
        parse("data 商品 {\n  商品ID: id unique\n}\n")


def test_範囲や組み合わせの直書きは止まり_名前を付けるよう案内する():
    for ty in ("int 1..99", "text 1..20", "money(JPY) 0..", "set of id", "product → Quantity"):
        with pytest.raises(ParseError, match="ltype"):
            parse(f"entity Product {{\n  product_id: id unique\n  項目: {ty}\n}}\n")
    parse("entity Product {\n  product_id: id unique\n  項目: int\n  名: text\n  金: money(JPY)\n}\n")  # 約束のない組み込みはそのまま書ける


# ---- store（保存先）は entity の外に書く ----
STORE = """entity メモ {
  メモID: id unique
  本文: text required
}
store メモ in Dropbox
"""


def test_store_を書くと_未同期の状態が足される():
    p = parse(STORE)
    assert p.data[0].store == "Dropbox"
    assert any(s.name == "記録状態" for s in p.states)
    assert any(f.name == "記録" for f in p.data[0].fields)


def test_store_が無ければ_保存の状態は足されない():
    p = parse(STORE.replace("store メモ in Dropbox\n", ""))
    assert p.data[0].store is None
    assert not any(s.name == "記録状態" for s in p.states)


def test_store_の誤りは止まる():
    with pytest.raises(ParseError, match="entity がありません"):
        parse(STORE.replace("store メモ", "store 謎"))
    with pytest.raises(ParseError, match="2つ以上"):
        parse(STORE + "store メモ in 端末\n")
    with pytest.raises(ParseError, match="別の行"):
        parse(STORE.replace("entity メモ {", "entity メモ store Dropbox {").replace("store メモ in Dropbox\n", ""))


# ---- 合成型 ----
COMP = """ltype 横 = int 0..100
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
"""


def test_合成型は項目と入力に使え_成分は平らに読める():
    ck = chk(COMP)
    assert ck.ok and ck.ready, ck.diags
    sp = build(ck)
    assert ("地図", "位置", "点（x: 整数 0..100、y: 整数 0..100）", "○", "") in sp["データ"]


def test_合成型の誤りは止まる():
    # 成分の書き漏れ・余計な成分・型の違い
    ck = chk(mutate(COMP, "点 { x: 位置.x, y: 5 }", "点 { x: 位置.x }"))
    assert any("成分「y」がありません" in m for m in msgs(ck, ERROR))
    ck = chk(mutate(COMP, "点 { x: 位置.x, y: 5 }", "点 { x: 位置.x, y: 5, z: 1 }"))
    assert any("成分「z」がありません" in m for m in msgs(ck, ERROR))
    ck = chk(mutate(COMP, "点 { x: 位置.x, y: 5 }", "点 { x: 位置.x, y: 位置 }"))
    assert any("型の違う値" in m for m in msgs(ck, ERROR)), ck.diags
    # 範囲が収まると示せない（入力は 0..1000 で、成分は 0..100）
    wide = mutate(COMP, "ltype 点 {", "ltype 広い横 = int 0..1000\nltype 点 {")
    ck = chk(mutate(mutate(wide, "deed 置く {\n  input 位置: 点 required", "deed 置く {\n  input 位置: 点 required, 幅: 広い横 required"), "点 { x: 位置.x, y: 5 }", "点 { x: 位置.x, y: 幅 }"))
    assert any("成分の範囲に収まると示せません" in m for m in msgs(ck, ERROR)), ck.diags
    # 存在しない成分を読む
    ck = chk(mutate(COMP, "地図.道.終点.x > 50", "地図.道.終点.z > 50"))
    assert any("成分「z」がありません" in m for m in msgs(ck, ERROR))


def test_合成型は問い合わせの入力と部品にはまだ使えない():
    ck = chk(mutate(COMP, "読んだ(地図ID: id)", "読んだ(地図ID: id, 場所: 点)"))
    assert not any("合成型" in m and "にだけ" in m for m in msgs(ck, ERROR)), ck.diags  # 結果の中身には使える
    ck = chk(COMP + "\nquery 点を探す {\n  input 位置: 点\n  from 地図\n  reads 地図.地図ID\n}\n")
    assert any("にだけ使えます" in m for m in msgs(ck, ERROR)), ck.diags


def test_合成型の宣言の誤りは止まる():
    for src, msg in [("ltype 点 {\n  x: int\n  x: int\n}\n", "2つ以上"),
                     ("ltype 点 {\n}\n", "成分がありません"),
                     ("ltype 横 = int 0..9\nltype 横束 = Product → 横\nltype 点 {\n  束: 横束\n}\n", "使えません")]:
        with pytest.raises(ParseError, match=msg):
            parse(src)


@needs_cargo
def test_合成型はホストの境界で成分ごとに確かめる(tmp_path):
    ck = chk(COMP)
    call = lambda n, ctx, i, store: ('    println!("{:?}", geas_app::host::call(r#"{"name":"' + n + '","ctx":' + ctx + ',"input":' + i + ',"store":' + store + '}"#));\n')
    rec = lambda x: '{"地図":{"G1":{"地図ID":"G1","位置":{"x":1,"y":2},"道":{"始点":{"x":1,"y":2},"終点":{"x":' + str(x) + ',"y":5}}}}}'
    main = "fn main() {\n" + "".join([
        call("置く", '{"new_地図_id":"G1"}', '{"位置":{"x":10,"y":20}}', "{}"),
        call("置く", '{"new_地図_id":"G1"}', '{"位置":{"x":10,"y":200}}', "{}"),
        call("置く", '{"new_地図_id":"G1"}', '{"位置":{"x":10}}', "{}"),
        call("置く", '{"new_地図_id":"G1"}', '{"位置":{"x":10,"y":20,"z":1}}', "{}"),
        call("横を見る", "{}", '{"地図ID":"G1"}', rec(5)),
        call("横を見る", "{}", '{"地図ID":"G1"}', rec(70)),
    ]) + "}\n"
    write_crate(ck, generate(ck), main, tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    got = r.stdout.splitlines()
    assert '\\"位置\\":{\\"x\\":10,\\"y\\":20}' in got[0] and '\\"終点\\":{\\"x\\":10,\\"y\\":5}' in got[0], got[0]
    assert "範囲外" in got[1] and "位置.y" in got[1], got[1]
    assert got[2].startswith("Err(") and "y" in got[2], got[2]
    assert got[3].startswith("Ok(") and "置いた" in got[3]  # 宣言にない成分は来ていないものとして扱う
    assert "読んだ" in got[4] and "右寄り" in got[5], got[4:6]


# ---- vtime（成分は未指定にできる）----
VDT = """entity 約束 {
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
"""


def test_vtimeの成分は未指定にでき_確かめずに読むと止まる():
    ck = chk(VDT)
    assert ck.ok and ck.ready, ck.diags
    sp = build(ck)
    assert any(r[1] == "期日" and r[2].startswith("vtime（year: 整数 1..9999") for r in sp["データ"]), sp["データ"]
    ck = chk(mutate(VDT, "  if 約束.期日.month is null { return 未指定 }\n", ""))
    assert any("未指定かもしれない成分" in m for m in msgs(ck, ERROR)), ck.diags
    # 必須の成分を持つ合成型では、書き漏れがエラー。null を書くのも止まる
    ck = chk("ltype 点 {\n  x: int\n  y: int optional\n}\n" + mutate(VDT, "vtime { month: 10, day: 26 }", "点 { y: 1 }").replace("期日: vtime required", "期日: 点 required").replace("input 期日: vtime required", "input 期日: 点 required").replace("約束.期日.month", "約束.期日.y"))
    assert any("成分「x」がありません" in m for m in msgs(ck, ERROR)), ck.diags


def test_vtimeの成分の範囲は構造の上限だけ():
    ck = chk(mutate(VDT, "vtime { month: 10, day: 26 }", "vtime { month: 13, day: 26 }"))
    assert any("成分の範囲に収まると示せません" in m for m in msgs(ck, ERROR)), ck.diags


@needs_cargo
def test_vtimeはホストの境界で成分ごとに確かめ_未指定はnull(tmp_path):
    ck = chk(VDT)
    call = lambda n, ctx, i, store: ('    println!("{:?}", geas_app::host::call(r#"{"name":"' + n + '","ctx":' + ctx + ',"input":' + i + ',"store":' + store + '}"#));\n')
    rec = lambda body: '{"約束":{"A1":{"約束ID":"A1","期日":' + body + '}}}'
    main = "fn main() {\n" + "".join([
        call("決める", '{"new_約束_id":"A1"}', '{"期日":{"year":2026,"month":10}}', "{}"),
        call("決める", '{"new_約束_id":"A1"}', '{"期日":{"month":13}}', "{}"),
        call("誕生日にする", '{"new_約束_id":"A1"}', "{}", "{}"),
        call("月を見る", "{}", '{"約束ID":"A1"}', rec('{"month":null}')),
        call("月を見る", "{}", '{"約束ID":"A1"}', rec('{"month":3}')),
        call("月を見る", "{}", '{"約束ID":"A1"}', rec('{"month":9}')),
    ]) + "}\n"
    write_crate(ck, generate(ck), main, tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    got = r.stdout.splitlines()
    assert '\\"month\\":10' in got[0] and '\\"day\\":null' in got[0] and '\\"year\\":2026' in got[0], got[0]
    assert "範囲外" in got[1] and "期日.month" in got[1], got[1]
    assert '\\"month\\":10' in got[2] and '\\"day\\":26' in got[2] and '\\"year\\":null' in got[2], got[2]
    assert "未指定" in got[3] and "前半" in got[4] and "後半" in got[5], got[3:6]


CONVERT = PART.replace("part 税込(x: 非負金額) -> 非負金額 guarantee result >= x", "convert 税込(x: 非負金額) -> 非負金額 = x + x")


def test_convert_は式が定義で_外の部品も保証も要らない():
    ck = chk(CONVERT)
    assert ck.ready, ck.diags
    assert build(ck)["部品"] == [("税込", "x: 金額(円) 0..", "金額(円) 0..", "= x + x")]
    assert not ck.infos["置く"].parts
    assert "trait Parts" not in generate(ck)


def test_convert_の式の型が戻り値と違うと止まる():
    bad = CONVERT.replace("= x + x", "= int(x)")
    assert any("戻り値の型と違います" in m or "戻り値の型（" in m for m in msgs(chk(bad), ERROR))


@needs_cargo
def test_convert_は生成コードの中で計算され_実行時の範囲検査は出さない(tmp_path):
    ck = chk(CONVERT)
    write_crate(ck, generate(ck), "fn main() {}\n", tmp_path)
    r = subprocess.run(["cargo", "build", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]


def test_convert_の結果が範囲に収まると示せなければ検査で止まる():
    bad = CONVERT.replace("convert 税込(x: 非負金額) -> 非負金額", "convert 税込(x: 非負金額) -> 税抜の限り")
    assert any("収まると示せません" in m for m in msgs(chk(bad), ERROR))
    ok = CONVERT.replace("convert 税込(x: 非負金額) -> 非負金額 = x + x", "convert 税込(x: 税抜の限り) -> 非負金額 = x + x")
    assert chk(ok).ready
    assert "保証違反(\"税込\"" not in generate(chk(ok))


DIVIDE = """ltype 日数 = int 0..
ltype 週数 = int 0..
ltype 余り日 = int 0..6

convert 週(d: 日数) -> 週数 = d / 7
convert 端(d: 日数) -> 余り日 = d % 7
"""


def test_整数の割り算は正の定数で切り捨て_範囲は端を割って示す():
    ck = chk(DIVIDE.replace("convert 端(d: 日数) -> 余り日 = d % 7\n", ""))
    assert ck.ready, ck.diags
    with pytest.raises(ParseError, match="正の整数の定数"):
        parse(DIVIDE.replace("d / 7", "d / 0", 1))
    with pytest.raises(ParseError, match="正の整数の定数"):
        parse(DIVIDE.replace("d / 7", "d / d", 1))
    assert any("割り算できるのは整数だけ" in m for m in msgs(chk(DIVIDE.replace("ltype 日数 = int 0..", "ltype 日数 = money(JPY) 0..").replace("convert 端(d: 日数) -> 余り日 = d % 7\n", "")), ERROR))


def test_余りの範囲は静的に示せる():
    assert chk(DIVIDE).ready
    bad = DIVIDE.replace("ltype 余り日 = int 0..6", "ltype 余り日 = int 0..5")
    assert any("収まると示せません" in m for m in msgs(chk(bad), ERROR))


@needs_cargo
def test_割り算と余りは生成コードで動く(tmp_path):
    ck = chk(DIVIDE)
    write_crate(ck, generate(ck), "fn main() {}\n", tmp_path)
    r = subprocess.run(["cargo", "build", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]


UTC = """convert 実在日時(v: vtime fill { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 }) -> time = time(
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
"""


def test_vtimeを補うルールつきで実在日時へ変える():
    ck = chk(UTC)
    assert ck.ready, ck.diags


def test_補うルールが無い成分を読むと違反になる():
    bad = UTC.replace("fill { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 }", "fill { year: 1970, month: 1, day: 1, hour: 0, minute: 0 }")
    assert any("未指定かもしれない成分" in m for m in msgs(chk(bad), ERROR))
    bad2 = UTC.replace("hour: 0,", "hour: 99,")
    assert any("収まりません" in m for m in msgs(chk(bad2), ERROR))


@needs_cargo
def test_vtimeの変換は生成コードで動く(tmp_path):
    ck = chk(UTC)
    write_crate(ck, generate(ck), "fn main() {}\n", tmp_path)
    r = subprocess.run(["cargo", "build", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]


@needs_cargo
def test_vtimeの変換は暦どおりの秒になり_未指定は補うルールの値になる(tmp_path):
    ck = chk(UTC)
    main = """use geas_app::*;
fn t(日時: vtime) -> i64 {
    let ctx = 予約Ctx { new_予定_id: "e1".to_string() };
    let o = 予約(&ctx, &Store::default(), &予約Input { 日時 });
    match &o.Write[0] { Write::予定_New(r) => r.時刻 }
}
fn main() {
    println!("{}", t(vtime { year: Some(2026), month: Some(10), day: Some(26), hour: Some(3), minute: Some(4), second: Some(5) }));
    println!("{}", t(vtime { year: Some(2000), month: None, day: None, hour: None, minute: None, second: None }));
    println!("{}", t(vtime { year: None, month: None, day: None, hour: None, minute: None, second: None }));
}
"""
    write_crate(ck, generate(ck), main, tmp_path)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    assert r.stdout.split() == ["1792983845", "946684800", "0"], r.stdout


TZ = ("timezone +09:00\nltype Offset = int -50400..50400\n"
      + UTC.replace("fill { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 })", "fill { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 }, offset: Offset = tz_offset)")
      .replace("+ v.second)", "+ v.second - offset)"))


def test_timezoneの宣言と_デフォルト引数で_標準のずれを使える():
    ck = chk(TZ)
    assert ck.ready, ck.diags
    assert ck.p.tz_offset == 9 * 3600
    assert chk(UTC).p.tz_offset == 0  # 書かなければ UTC
    assert parse(TZ.replace("+09:00", "-05:30")).tz_offset == -(5 * 3600 + 30 * 60)
    assert chk(TZ.replace("+09:00", "-05:30")).ready  # 負のずれも、そのままの範囲で扱える


def test_デフォルト引数は_省ける_渡せる_決まりに反すると止まる():
    assert chk(TZ.replace("実在日時(日時)", "実在日時(日時, 0)")).ready  # 渡して上書き
    assert any("引数は 2 個です" in m for m in msgs(chk(TZ.replace("実在日時(日時)", "実在日時(日時, 0, 0)")), ERROR))
    bad = TZ.replace("offset: Offset = tz_offset", "offset: Offset = 99999")
    assert any("デフォルト値が、引数の範囲" in m for m in msgs(chk(bad), ERROR))
    bad2 = TZ.replace("v: vtime fill", "w: Offset = 0, v: vtime fill")
    assert any("にもデフォルト値が要ります" in m for m in msgs(chk(bad2), ERROR))


def test_timezoneの書き方の誤りは止まる():
    with pytest.raises(ParseError, match="1つだけ"):
        parse("timezone +09:00\ntimezone +01:00\n")
    with pytest.raises(ParseError, match="時:分で書きます"):
        parse("timezone Asia/Tokyo\n")
    with pytest.raises(ParseError, match="±14:00 まで"):
        parse("timezone +15:00\n")
    with pytest.raises(ParseError, match="0〜59"):
        parse("timezone +09:60\n")


def test_モジュールにはtimezoneを書けない(tmp_path):
    mod_dir(tmp_path, geo="module Geo\ntimezone +09:00\n")
    (tmp_path / "app.geas").write_text('use "geo.geas"\n')
    with pytest.raises(ModuleError, match="timezone を書けません"):
        parse((tmp_path / "app.geas").read_text(), tmp_path / "app.geas")


@needs_cargo
def test_標準のタイムゾーンで暦の秒がずれ_引数で上書きできる(tmp_path):
    ck = chk(TZ)
    main = """use geas_app::*;
fn t(日時: vtime) -> i64 {
    let ctx = 予約Ctx { new_予定_id: "e1".to_string() };
    let o = 予約(&ctx, &Store::default(), &予約Input { 日時 });
    match &o.Write[0] { Write::予定_New(r) => r.時刻 }
}
fn main() {
    println!("{}", t(vtime { year: Some(1970), month: Some(1), day: Some(2), hour: None, minute: None, second: None }));
}
"""
    write_crate(ck, generate(ck), main, tmp_path)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    assert r.stdout.split() == [str(86400 - 9 * 3600)], r.stdout  # 日本時間の1970-01-02 0:00 は、UTC の前日 15:00


SHOP_SRC = (EX / "shop.geas").read_text()
JOIN = SHOP_SRC.replace("reply PaymentSucceeded → apply_payment_result", "all reply PaymentSucceeded, StockReserved → apply_payment_result")


def test_待ち合わせ_all_は全部の応答がそろったら進む行として書ける():
    ck = chk(JOIN)
    assert ck.ready, ck.diags
    tg = next(t for t in ck.p.triggers if t.name == "PaymentPending")
    assert ("all", "PaymentSucceeded,StockReserved", "apply_payment_result") in tg.causes
    assert ("決済待ち", "全部の応答 決済成功 ＋ StockReserved", "決済結果の反映") in build(ck)["トリガ"]  # 仕様書は表示名で出る
    assert '("all", "PaymentSucceeded,StockReserved", "apply_payment_result")' in generate(ck)


def test_待ち合わせの書き方の誤りは止まる():
    one = SHOP_SRC.replace("reply PaymentSucceeded → apply_payment_result", "all reply PaymentSucceeded → apply_payment_result")
    assert any("2つ以上" in m for m in msgs(chk(one), ERROR))
    both = JOIN.replace("deadline now", "reply StockReserved → expire_payment\n  deadline now")
    assert any("両方にあります" in m for m in msgs(chk(both), ERROR))
    twice = JOIN.replace("deadline now", "all reply StockReserved, Other → expire_payment\n  deadline now")
    assert any("複数の all の行" in m for m in msgs(chk(twice), ERROR))
    with pytest.raises(ParseError, match="all の後ろには"):
        parse(SHOP_SRC.replace("reply PaymentSucceeded → apply_payment_result", "all PaymentSucceeded, X → apply_payment_result"))


def test_仕様書にフロー図が出る_並列は_プラスのゲートウェイ():
    ck = fanout(*FANOUT)
    md = render(ck, build(ck))
    assert "```mermaid" in md and "flowchart LR" in md
    assert md.count('(("＋"))') == 1
    assert '-- "受付" -->' in md and "応答（reply）" in md
    plain = render(chk(CODE), build(chk(CODE)))
    assert '(("＋"))' not in plain and '(("×"))' in plain and "待つ 決済待ち" in plain


def test_説明コメントは意味を変えず_仕様書の名前と表示名の表になる():
    ck = chk(mutate(CODE, "/// 顧客\nentity Customer {", "/// 買う人\n/// @en Customer\nentity Customer {"))
    assert ck.ready, ck.diags
    assert ("Customer", "既定: 買う人 / en: Customer") in build(ck)["名前"]
    assert "## 名前と表示名" in render(ck, build(ck))
    assert generate(ck) == generate(chk(CODE))  # 説明コメントがあってもなくても、生成されるコードは同じ
    plain = chk("ltype 点数 = int 0..\n")  # 説明コメントが無ければ、表は出ない
    assert "名前と表示名" not in render(plain, build(plain))


# ---- 名前の書き方（半角英数。種類は大文字小文字で見分ける）----
def strict(src):
    return check(parse(src), strict_names=True)


def test_お店の例は名前の書き方の規則に合っている():
    ck = strict(CODE)
    assert ck.ok, ck.diags


def test_名前に日本語は使えない_コマンドは常に検査する(tmp_path):
    ck = strict(mutate(CODE, "ltype StockCount = int 0..", "ltype 在庫数 = int 0.."))
    assert any("UpperCamel" in m and "在庫数" in m for m in msgs(ck, ERROR))
    r = subprocess.run([sys.executable, "-m", "geas", "check", str(EX / "shop.geas")], capture_output=True, text=True, cwd=ROOT)
    assert r.returncode == 0, r.stdout + r.stderr
    bad = tmp_path / "bad_names.geas"
    bad.write_text("ltype 数量 = int 1..99\n")
    r = subprocess.run([sys.executable, "-m", "geas", "check", str(bad)], capture_output=True, text=True, cwd=ROOT)
    assert r.returncode != 0 and "UpperCamel" in r.stdout + r.stderr


def test_型は大文字始まり_項目と処理は小文字始まり():
    ck = strict(mutate(CODE, "stock: StockCount required", "Stock: StockCount required"))
    assert any("lower_snake" in m and "Stock" in m for m in msgs(ck, ERROR))
    ck = strict(mutate(CODE, "deed place_order {", "deed PlaceOrder {").replace("place_order:", "PlaceOrder:"))
    assert any("lower_snake" in m and "PlaceOrder" in m for m in msgs(ck, ERROR))
    ck = strict(mutate(CODE, "state OrderStatus {", "state order_status {").replace("OrderStatus", "order_status"))
    assert any("UpperCamel" in m and "order_status" in m for m in msgs(ck, ERROR))


def test_小文字の名前は組み込みや予約語と同じにできない():
    ck = strict(mutate(CODE, "let subtotal = ", "let round = ").replace("subtotal", "round", 1) if False else mutate(CODE, "  let subtotal = sum", "  let int = sum"))
    assert any("予約語" in m and "int" in m for m in msgs(ck, ERROR))
    assert not any("予約語" in m for m in msgs(strict(CODE), ERROR))


def test_名前の規則は_チェックの関数では既定で見ない():
    assert chk("ltype 点数 = int 0..\n").ok


# ---- id → 値（キーが entity のキーに限られない、読み取り専用の値の入れ物）----
IDMAP = """ltype Dots = int 0..100
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
"""


def test_id_から値への写像は_entity_を介さず書ける():
    ck = strict(IDMAP)
    assert ck.ok, ck.diags


def test_id_の写像も_ltype_で名前を付けてから使う():
    with pytest.raises(ParseError, match="ltype"):
        parse(mutate(IDMAP, "input table: Table required", "input table: id → Pos required"))


@needs_cargo
def test_id_の写像の値は合成型でも_ホストから受け取って足せる(tmp_path):
    ck = strict(IDMAP)
    assert ck.ok, ck.diags
    req = '{"name":"show","ctx":{},"input":{"table":{"a":{"x":3,"y":null},"b":{"x":4,"y":5}}},"store":{}}'
    write_crate(ck, generate(ck), f'fn main() {{\n    println!("{{}}", geas_app::host::call(r#"{req}"#).unwrap());\n}}\n', tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]
    assert "Big" in r.stdout, r.stdout  # 3 + 4 = 7 > 6


# ---- モジュール（状態を持たない部品の集まり。ソースのまま取り込む）----
GEO = """module Geo

/// 位置の数
ltype Dots = int 0..100
/// 位置
ltype Pos { x: Dots, y: Dots optional }

/// そのまま
convert same(d: Dots) -> Dots = d
"""
GEO_APP = """use "geo.geas"

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
"""


def mod_dir(tmp_path, **files):
    for n, text in files.items():
        (tmp_path / f"{n}.geas").write_text(text)
    return tmp_path


def load_strict(tmp_path, name="app"):
    return check(parse((tmp_path / f"{name}.geas").read_text(), tmp_path / f"{name}.geas"), strict_names=True)


def test_モジュールを取り込んで_名前空間つきで使える(tmp_path):
    mod_dir(tmp_path, geo=GEO, app=GEO_APP)
    ck = load_strict(tmp_path)
    assert ck.ok, ck.diags
    assert ck.p.origin["Geo_Pos"] == ("Geo", "Pos")
    names = dict(build(ck)["名前"])
    assert names["Geo_Pos"] == "既定: 位置" and names["Geo_same"] == "既定: そのまま"  # 表示名はモジュールの /// から


def test_モジュールの名前は_表示名が無ければ_モジュール名と定義名で出る(tmp_path):
    mod_dir(tmp_path, geo=GEO.replace("/// 位置\n", ""), app=GEO_APP)
    ck = load_strict(tmp_path)
    assert ck.p.show_names("Geo_Pos") == "Geo.Pos"


@needs_cargo
def test_モジュールを使ったコードがビルドでき_動く(tmp_path):
    mod_dir(tmp_path, geo=GEO, app=GEO_APP)
    ck = load_strict(tmp_path)
    req = '{"name":"show","ctx":{},"input":{"table":{"a":{"x":3,"y":null},"b":{"x":4,"y":5}},"limit":6},"store":{}}'
    out = tmp_path / "crate"
    out.mkdir()
    write_crate(ck, generate(ck), f'fn main() {{\n    println!("{{}}", geas_app::host::call(r#"{req}"#).unwrap());\n}}\n', out, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=out, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]
    assert "Big" in r.stdout, r.stdout  # 3 + 4 = 7 > 6


def test_モジュールには状態を持つ宣言を書けない(tmp_path):
    for decl in ("entity Item {\n  item_id: id unique\n}", "invariant Item: item_id == 1", "nexus Flow {\n  show: Big → reply\n}"):
        mod_dir(tmp_path, geo=GEO + "\n" + decl + "\n", app=GEO_APP)
        with pytest.raises(ModuleError, match="状態は持てません"):
            parse((tmp_path / "app.geas").read_text(), tmp_path / "app.geas")


def test_モジュールの取り込みの誤りは止まる(tmp_path):
    mod_dir(tmp_path, geo=GEO, app=GEO_APP.replace("Geo.same(limit)", "Geo.nothing(limit)"))
    with pytest.raises(ModuleError, match="Geo に「nothing」はありません"):
        parse((tmp_path / "app.geas").read_text(), tmp_path / "app.geas")
    mod_dir(tmp_path, geo=GEO.replace("module Geo\n", ""), app=GEO_APP)
    with pytest.raises(ModuleError, match="module 名前"):
        parse((tmp_path / "app.geas").read_text(), tmp_path / "app.geas")
    mod_dir(tmp_path, app=GEO_APP.replace('"geo.geas"', '"nothing.geas"'))
    with pytest.raises(ModuleError, match="がありません"):
        parse((tmp_path / "app.geas").read_text(), tmp_path / "app.geas")
    with pytest.raises(ModuleError, match="ファイルから読む"):
        parse(GEO_APP)


def test_モジュールが循環したり名前が重なったりすると止まる(tmp_path):
    mod_dir(tmp_path, a='use "b.geas"\nmodule A\nltype One = int 0..\n', b='use "a.geas"\nmodule B\nltype Two = int 0..\n', app='use "a.geas"\nresult R {\n  Done\n}\n')
    with pytest.raises(ModuleError, match="循環"):
        parse((tmp_path / "app.geas").read_text(), tmp_path / "app.geas")
    mod_dir(tmp_path, a="module Same\nltype One = int 0..\n", b="module Same\nltype Two = int 0..\n", app='use "a.geas"\nuse "b.geas"\nresult R {\n  Done\n}\n')
    with pytest.raises(ModuleError, match="2つのファイル"):
        parse((tmp_path / "app.geas").read_text(), tmp_path / "app.geas")


def test_モジュールがモジュールを取り込み_同じモジュールは1度だけ読む(tmp_path):
    mod_dir(tmp_path, base="module Base\nltype Count = int 0..100\n",
            mid='use "base.geas"\nmodule Mid\nltype Few = int 0..5\n',
            app='use "base.geas"\nuse "mid.geas"\n\nentity Box {\n  box_id: id unique\n  few: Mid.Few required\n  many: Base.Count required\n}\n')
    ck = load_strict(tmp_path)
    assert ck.ok, ck.diags
    assert [x for x in ck.p.ltype_names].count("Base_Count") == 1


def test_モジュールの処理は入力から結果を返すだけなら書け_アプリのフローにつなげられる(tmp_path):
    mod_dir(tmp_path, judge="""module Judge

ltype Score = int 0..100
/// 判定
result Verdict {
  Pass
  Fail
}
/// 合否
deed grade {
  input score: Score required
  result Verdict
  effects none

  if score >= 60 { return Pass }
  return Fail
}
""", app="""use "judge.geas"

nexus Flow {
  Judge.grade: Pass → reply, Fail → reply
}
""")
    ck = load_strict(tmp_path)
    assert ck.ok, ck.diags
    assert "Judge_grade" in ck.procs and ck.p.show_names("Judge_grade") == "合否"


def test_モジュールの処理がデータや外部作用に触れるとエラー(tmp_path):
    mod_dir(tmp_path, bad="""module Bad

result Done {
  Ok
}
deed touch {
  input item_id: id required
  result Done
  effects none

  let item = Item[item_id] else { return Ok }
  return Ok
}
""", app="""use "bad.geas"

ltype Digit = int 0..9
entity Item {
  item_id: id unique
  count: Digit
}
""")
    ck = load_strict(tmp_path)
    assert not ck.ok and any("モジュールの処理は" in d.msg and "データを読む" in d.msg for d in ck.diags), ck.diags


def test_表示名の無い名前は警告で教える(tmp_path):
    mod_dir(tmp_path, geo=GEO.replace("/// 位置\n", ""), app=GEO_APP)
    ck = load_strict(tmp_path)
    gaps = [d for d in ck.diags if d.code == "表示名"]
    assert ck.ok and any("「Pos」に表示名" in d.msg for d in gaps), gaps
    assert not any("「Dots」" in d.msg for d in gaps)  # /// 位置の数 がある
    # 表示名があれば警告しない。名前の書式を検査しない時（check の既定）も警告しない
    assert not [d for d in check(parse(open(EX / "shop.geas").read())).diags if d.code == "表示名"]


# ---- モジュールの version とハッシュの固定 ----
GEO_V = GEO.replace("module Geo\n", 'module Geo version "1.2.0"\n')


def sha_of(path):
    import hashlib

    return hashlib.sha256(path.read_bytes()).hexdigest()


def use_app(tmp_path, pins):
    (tmp_path / "app.geas").write_text(GEO_APP.replace('use "geo.geas"', f'use "geo.geas"{pins}'))
    return parse((tmp_path / "app.geas").read_text(), tmp_path / "app.geas")


def test_useで依存の_version_とソースのハッシュを明示でき_合っていれば取り込める(tmp_path):
    mod_dir(tmp_path, geo=GEO_V)
    p = use_app(tmp_path, f' version "1.2.0" sha256 "{sha_of(tmp_path / "geo.geas")}"')
    assert "Geo_Pos" in p.ltype_names
    assert "Geo_Pos" in use_app(tmp_path, "").ltype_names  # どちらも書かなくてよい
    assert "Geo_Pos" in use_app(tmp_path, ' version "1.2.0"').ltype_names


def test_versionが違う_ハッシュが違うと取り込めない(tmp_path):
    mod_dir(tmp_path, geo=GEO_V)
    with pytest.raises(ModuleError, match="version は「1.2.0」で、use に書いた「2.0.0」と違います"):
        use_app(tmp_path, ' version "2.0.0"')
    with pytest.raises(ModuleError, match="ハッシュが、固定した値と違います"):
        use_app(tmp_path, ' sha256 "' + "0" * 64 + '"')
    mod_dir(tmp_path, geo=GEO)  # モジュールに version が無い
    with pytest.raises(ModuleError, match="（無し）"):
        use_app(tmp_path, ' version "1.2.0"')
    mod_dir(tmp_path, geo=GEO_V)
    with pytest.raises(ModuleError, match="version の後に"):
        use_app(tmp_path, " version")


# ---- 応答の中身 ----
REPLY_SRC = SHOP_SRC.replace("/// 待ち解除結果", "reply PaymentDeclined(reason: id required)\nreply PaymentSucceeded(auth_id: id required)\n\n/// 待ち解除結果")


def test_応答は中身を宣言でき_行き先の処理の入力に足される():
    src = REPLY_SRC.replace("deed apply_payment_result {\n  input order_id: id required", "deed apply_payment_result {\n  input order_id: id required, auth_id: id required")
    ck = chk(src)
    assert ck.ready, ck.diags
    assert [r.name for r in ck.p.replies] == ["PaymentDeclined", "PaymentSucceeded"]
    assert ("決済成功", "auth_id（必須）") in build(ck)["応答"]
    assert 'ReplyDef { name: "PaymentSucceeded", fields: &[("auth_id", "ID", true)] }' in generate(ck)


def test_応答の中身が行き先の処理の入力と合わないと止まる():
    ck = chk(REPLY_SRC)  # apply_payment_result は auth_id を入力にもたない
    assert any("応答の中身" in m for m in msgs(ck, ERROR))


def test_待ち合わせでは全部の応答の中身が渡り_名前が重なると止まる():
    src = REPLY_SRC.replace("reply PaymentSucceeded → apply_payment_result", "all reply PaymentSucceeded, PaymentDeclined → apply_payment_result")
    dup = src.replace("reply PaymentDeclined(reason: id required)", "reply PaymentDeclined(auth_id: id required)")
    assert any("同じ名前" in m for m in msgs(chk(dup), ERROR))
    ok = src.replace("deed apply_payment_result {\n  input order_id: id required", "deed apply_payment_result {\n  input order_id: id required, auth_id: id required, reason: id required")
    assert chk(ok).ready


def test_使われない応答の宣言は警告():
    assert any("使われていません" in m for m in msgs(chk(REPLY_SRC.replace("reply PaymentDeclined", "reply Unused")), WARN))


# ---- 金額の単位 ----
USD_SRC = SHOP_SRC.replace("  unit_price: NonNegMoney required\n", "  unit_price: NonNegMoney required\n  price_usd: money(USD) required\n  note_usd: money(USD)\n")


def usd_deed(body, reads="Product.unit_price, Product.price_usd", writes=""):
    w = f"  writes {writes}\n" if writes else ""
    return USD_SRC + f"""
deed usd_probe {{
  input product_id: id required
  reads {reads}
{w}  result ReleaseResult
  effects none

  let p = Product[product_id] else {{ return NoSuchOrder }}
{body}
  return Applied
}}
"""


def unit_errors(src):
    return [m for m in msgs(chk(src), ERROR) if "金額" in m and "単位" in m or "換算" in m]


def test_円とドルの金額は足せず比べられない():
    assert unit_errors(usd_deed("  let t = p.unit_price + p.price_usd"))
    assert unit_errors(usd_deed("  if p.unit_price > p.price_usd { return WrongStatus }"))
    assert not unit_errors(usd_deed("  let t = p.price_usd + p.price_usd\n  if t > p.price_usd { return WrongStatus }"))


def test_金額を入れる先の単位が違うと止まる():
    bad = usd_deed("  p.note_usd = p.unit_price", writes="Product.note_usd")
    assert any("ドルの金額ですが、円の金額" in m for m in msgs(chk(bad), ERROR)), msgs(chk(bad), ERROR)
    assert not unit_errors(usd_deed("  p.note_usd = p.price_usd * 2", writes="Product.note_usd"))
    # let の変数、掛け算、丸めを通しても単位は引き継がれる
    assert unit_errors(usd_deed("  let t = p.unit_price * 3\n  p.note_usd = t", writes="Product.note_usd"))


def test_ドルの金額に_0以外の数字は直接書けず_money_で最小単位から作る():
    assert any("直接書けません" in m for m in msgs(chk(usd_deed("  p.note_usd = 500", writes="Product.note_usd")), ERROR))
    assert not unit_errors(usd_deed("  p.note_usd = money(500, USD)", writes="Product.note_usd"))
    assert any("直接書けません" in m for m in msgs(chk(usd_deed("  if p.price_usd > 100 { return WrongStatus }")), ERROR))
    assert not unit_errors(usd_deed("  if p.price_usd > 0 { return WrongStatus }"))
    assert any("単位" in m for m in msgs(chk(usd_deed("  p.note_usd = money(500, GBP)", writes="Product.note_usd")), ERROR))


def test_丸めの単位は金額の単位と合わせる():
    ok = usd_deed("  p.note_usd = round(p.price_usd * 0.5, floor, USD)".replace("0.5", "tax"), writes="Product.note_usd")
    ok = ok.replace("input product_id: id required", "input product_id: id required\n  slot tax: TaxRateValue")
    assert not unit_errors(ok)
    assert any("単位で丸めています" in m for m in msgs(chk(ok.replace("floor, USD", "floor, JPY")), ERROR))


# ---- 合成型を使える場所 ----
ADDR_SRC = SHOP_SRC.replace("/// 商品\nentity Product", 'ltype Zip = text 7..7 of "0-9"\nltype Line = text 1..50\nltype Addr { zip: Zip, line: Line }\n/// 商品\nentity Product', 1)
ADDR_SRC = ADDR_SRC.replace("  NoSuchCustomer\n}", "  NoSuchCustomer\n  Shipped(to: Addr)\n}", 1)
ADDR_SRC = ADDR_SRC.replace("/// 決済\neffect Payment.", 'effect Ship. send(order_id: id, to: Addr) no_wait "配送の失敗は人が見る"\n\n/// 決済\neffect Payment.', 1)
ADDR_SRC = ADDR_SRC.replace("place_order: Received → request_payment,", "place_order: Shipped → reply, Received → request_payment,")
ADDR_SRC += '''
deed ship_order {
  input order_id: id required, addr: Addr required
  reads Order.status
  result PlaceOrderResult
  effects Ship.send

  let order = Order[order_id] else { return NoSuchCustomer }
  if order.status != Received { return NoSuchCustomer }
  request Ship.send(order_id, addr, idempotency_key = order_id)
  return Shipped(addr)
}

nexus ShipFlow {
  ship_order: Shipped → reply, NoSuchCustomer → reply, Received → reply, OutOfStock → reply, NoSuchProduct → reply
}
'''


def test_合成型は結果の中身_外部作用の引数_応答の中身にも使える():
    ck = chk(ADDR_SRC)
    assert ck.ready, ck.diags
    assert "Shipped(" in generate(ck) and "pub to: Addr" in generate(ck).replace("\n", " ") or "to: Addr" in generate(ck)
    from geas.hostgen import host_source
    assert 'js_str("zip")' in host_source(ck)  # 画面とやりとりする JSON にも、成分が平らに出る
    with_reply = ADDR_SRC.replace("/// 待ち解除結果", "reply Delivered(to: Addr required)\n\n/// 待ち解除結果")
    assert not [m for m in msgs(chk(with_reply), ERROR) if "合成型" in m]


# ---- 海外にも売るお店（残件4件を使う題材） ----
from geas.check import route_map  # noqa: E402
OVERSEAS = (EX.parent / "overseas_shop" / "overseas_shop.geas").read_text()


def test_海外店の例は通り_4件の機能がそろって使われる():
    ck = chk(OVERSEAS)
    assert ck.ready, ck.diags
    assert not msgs(ck, WARN), msgs(ck, WARN)
    assert any(len(ds) > 1 for fl in ck.p.flows for _, routes in fl.entries for ds in route_map(routes).values())  # par
    assert any(i.writes for d in ("request_payment", "grant_points") for i in [ck.infos[d]])  # par の枝が書く
    assert [r.name for r in ck.p.replies] == ["PaymentSettled", "StockSettled"]  # 応答の中身
    assert {t.kind for t in ck.p.composites} == {"合成"}  # 合成型 Address を結果・外部作用・項目に使う
    assert len([p for p in ck.p.processes if p.name.startswith("place_order")]) == 1  # 通貨を slot で渡すので、円用・ドル用に分けない
    assert generate(ck) == generate(chk(OVERSEAS))


def test_海外店の例_通貨を取り違えると止まる():
    assert not any("単位" in m or "通貨" in m for m in msgs(chk(OVERSEAS), ERROR))
    no_check = OVERSEAS.replace("  let other_currency = { k | (k, v) in items, Product[k].currency != currency }\n  if other_currency is nonempty { return OtherCurrency(other_currency) }\n", "")
    assert any("別の通貨" in m for m in msgs(chk(no_check), ERROR)), msgs(chk(no_check), ERROR)
    wrong = OVERSEAS.replace("request Payment.charge(order_id, order.currency, order.total,", "request Payment.charge(order_id, USD, order.total,")
    assert any("金額ですが" in m for m in msgs(chk(wrong), ERROR)), msgs(chk(wrong), ERROR)


def test_海外店の例_par_の枝が同じ項目を書くと止まる():
    clash = OVERSEAS.replace("par { request_payment, request_stock_hold, grant_points, grant_campaign_points }", "par { request_payment, cancel_unpaid, request_stock_hold }")
    assert any("par の枝" in m for m in msgs(chk(clash), ERROR)), msgs(chk(clash), ERROR)


def test_海外店の例_応答の中身が行き先の入力と合わないと止まる():
    bad = OVERSEAS.replace("input order_id: id required, auth_id: id, short: IdSet\n  reads Order.status\n  result SettleResult", "input order_id: id required, short: IdSet\n  reads Order.status\n  result SettleResult")
    bad = bad.replace("  let auth = auth_id else { return PaymentFailed(order_id) }\n  let lacking = short else { return Approved(order_id) }\n  return StockShort(order_id, auth)", "  let lacking = short else { return Approved(order_id) }\n  return PaymentFailed(order_id)")
    assert any("応答の中身" in m for m in msgs(chk(bad), ERROR)), msgs(chk(bad), ERROR)


@needs_cargo
def test_海外店の例は生成したRustがビルドできる(tmp_path):
    ck = chk(OVERSEAS)
    write_crate(ck, generate(ck), "fn main() {}\n", tmp_path)
    r = subprocess.run(["cargo", "build", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]


def test_問い合わせは_equals_で文字列やIDの完全一致で絞れる():
    ck = chk(OVERSEAS)
    assert ck.ready, ck.diags
    assert "jan 一致 jan" in "".join(str(x) for x in build(ck)["問い合わせ"]) or any("JANコード" in str(x) for x in build(ck)["問い合わせ"])
    assert "r.jan == *x" in generate(ck)
    bad = OVERSEAS.replace("filter jan equals jan", "filter price equals jan")
    assert any("同じ種類" in m for m in msgs(chk(bad), ERROR))


# ---- 通貨コードを slot で渡す金額（money(currency)） ----
def shop_currency():
    s = SHOP_SRC
    s = s.replace("/// 単価\n  unit_price: NonNegMoney required\n", "/// 通貨\n  currency: currency required\n  /// 単価\n  unit_price: money(currency) required\n", 1)
    s = s.replace("  total: NonNegMoney required\n", "  total: money(currency) required\n  /// 通貨\n  currency: currency required\n", 1)
    s = s.replace("  slot tax_rate: TaxRateValue\n", "  slot tax_rate: TaxRateValue\n  slot currency: currency\n", 1)
    s = s.replace("  reads Product.stock, Product.unit_price, Customer\n", "  reads Product.stock, Product.unit_price, Product.currency, Customer\n", 1)
    s = s.replace("  let short = { k | (k, v) in items, Product[k].stock < v }\n",
                  "  let wrong = { k | (k, v) in items, Product[k].currency != currency }\n  if wrong is nonempty { return OutOfStock(wrong) }\n"
                  "  let short = { k | (k, v) in items, Product[k].stock < v }\n", 1)
    s = s.replace("create Order { customer: customer_id,", "create Order { currency: currency, customer: customer_id,")
    s = s.replace("round(subtotal * tax_rate, floor, JPY)", "round(subtotal * tax_rate, floor, currency)")
    s = s.replace("  reads Order.total, Order.status\n  writes Order.status\n  transition Order: Received → PaymentRequested",
                  "  reads Order.total, Order.currency, Order.status\n  writes Order.status\n  transition Order: Received → PaymentRequested")
    s = s.replace("amount: money(JPY))", "amount: money(currency), currency: currency)")
    s = s.replace("request Payment.charge(order_id, order.total, idempotency_key = order_id)", "request Payment.charge(order_id, order.total, order.currency, idempotency_key = order_id)")
    return s


SHOP_CUR = shop_currency()


def test_通貨コードの_slot_で金額を扱える_単位は同じ_slot_の通貨どうし():
    ck = chk(SHOP_CUR)
    assert ck.ready, ck.diags
    assert "pub enum Currency" in generate(ck)
    assert "金額(通貨:" in " ".join(str(x) for x in build(ck)["データ"])


def test_通貨が同じと確かめずに_集まりの金額を使うと止まる():
    no_check = SHOP_CUR.replace("  let wrong = { k | (k, v) in items, Product[k].currency != currency }\n  if wrong is nonempty { return OutOfStock(wrong) }\n", "")
    assert any("別の通貨の金額" in m or "別の通貨" in m for m in msgs(chk(no_check), ERROR)), msgs(chk(no_check), ERROR)


def test_保存した金額を_固定の円の引数へ渡すと止まる():
    jpy_only = SHOP_CUR.replace("amount: money(currency), currency: currency)", "amount: money(JPY))").replace("order.total, order.currency, idempotency_key", "order.total, idempotency_key")
    assert any("円の金額ですが、別の通貨の金額" in m for m in msgs(chk(jpy_only), ERROR)), msgs(chk(jpy_only), ERROR)


def test_通貨を取り違えて渡すと止まる_固定の通貨を渡すと別の単位():
    other = SHOP_CUR.replace("order.total, order.currency, idempotency_key", "order.total, JPY, idempotency_key")
    assert any("金額ですが" in m for m in msgs(chk(other), ERROR)), msgs(chk(other), ERROR)


def test_money_の通貨の名前が同じ並びに無いと止まる():
    bad = SHOP_CUR + "\neffect Foo. bar(a: money(currency)) no_wait \"試し\"\n"
    assert any("同じ並びにありません" in m for m in msgs(chk(bad), ERROR))


def test_保存した1件の通貨を_slot_と同じと確かめれば_その金額を使える():
    # 通貨の確認なしで slot の金額と足すと止まり、確認すれば通る（x.通貨 != 通貨 なら返す）
    base = SHOP_CUR.replace("deed apply_payment_result {\n  input order_id: id required\n  reads Order.status",
                            "deed apply_payment_result {\n  input order_id: id required, price: money(currency) required\n  slot currency: currency\n  reads Order.total, Order.currency, Order.status")
    base = base.replace("  let order = Order[order_id] else { return NoSuchOrder }\n  if order.status != PaymentRequested { return WrongStatus }\n  order.status = Completed",
                        "  let order = Order[order_id] else { return NoSuchOrder }\n  if order.status != PaymentRequested { return WrongStatus }\n  if order.total > price { return WrongStatus }\n  order.status = Completed")
    assert any("別の通貨" in m for m in msgs(chk(base), ERROR)), msgs(chk(base), ERROR)
    ok = base.replace("  if order.total > price { return WrongStatus }", "  if order.currency != currency { return WrongStatus }\n  if order.total > price { return WrongStatus }")
    assert not [m for m in msgs(chk(ok), ERROR) if "通貨" in m and "金額" in m], msgs(chk(ok), ERROR)


def test_海外店の例は計画書と突合して食い違いがない():
    ov = EX.parent / "overseas_shop"
    r = subprocess.run([sys.executable, "-m", "geas", "compare", str(ov / "overseas_shop.geas"), str(ov / "overseas_shop_plan.md"), "--no-run"],
                       capture_output=True, text=True, cwd=ROOT)
    assert r.returncode == 0, r.stdout + r.stderr
    plan = (ov / "overseas_shop_plan.md").read_text()
    for form in ("ブラウザ", "PC アプリ", "スマホアプリ", "Chrome 拡張"):
        assert form in plan  # 利用の形態の前提を、計画書に書いてある


# ---- 返事の合流先（joins）と 1件の可換な更新（+=） ----
def test_外部作用の返事は_joins_で同じ並列の枝が待つトリガに合流できる():
    ck = chk(OVERSEAS)
    assert ck.ready, ck.diags
    assert ("倉庫.在庫の確保", "注文待ち") in build(ck)["合流"]
    # 待つ枝が同じ par にいないと、返事の行き場が無い
    alone = OVERSEAS.replace("par { request_payment, request_stock_hold, grant_points, grant_campaign_points }", "par { request_stock_hold, grant_points, grant_campaign_points }")
    assert any("同じ並列" in m for m in msgs(chk(alone), ERROR)), msgs(chk(alone), ERROR)


def test_合流先の誤りは止まる():
    assert any("未定義" in m for m in msgs(chk(OVERSEAS.replace("joins OrderPending", "joins Nowhere")), ERROR))
    both = OVERSEAS.replace("joins OrderPending", 'joins OrderPending no_wait "二重"')
    assert any("どちらかにしてください" in m for m in msgs(chk(both), ERROR))
    other_key = OVERSEAS.replace("deed request_stock_hold {\n  input order_id: id required", "deed request_stock_hold {\n  input order_id: id")
    assert any("必須の入力にもっていません" in m for m in msgs(chk(other_key), ERROR)), msgs(chk(other_key), ERROR)


def test_加算減算は_par_の枝で重なってよく_代入は止まる():
    ck = chk(OVERSEAS)
    assert ck.infos["grant_points"].write_ops["Customer.points"] == {"+="}
    assert ck.infos["grant_campaign_points"].write_ops["Customer.points"] == {"+="}
    plain = OVERSEAS.replace("customer.points += 5", "customer.points = customer.points + 5")
    assert any("同じ項目" in m for m in msgs(chk(plain), ERROR)), msgs(chk(plain), ERROR)


# ---- クライアント側の出来事（つながりが戻った）と JAN のチェックディジット ----
def test_海外店の例_つながりが戻ったら返事待ちの注文ごとに決済を引き直す():
    ck = chk(OVERSEAS)
    assert ck.ready, ck.diags
    assert [(e.name, e.steps) for e in ck.p.events] == [("Reconnected", [("各", "awaiting_payment", "recheck_payment")])]
    bad = OVERSEAS.replace("each awaiting_payment → recheck_payment", "each awaiting_payment → settle_order")
    assert any("each" in m or "入力" in m for m in msgs(chk(bad), ERROR)), msgs(chk(bad), ERROR)
    gone = OVERSEAS.replace("recheck_payment: Rechecked → wait OrderPending, ", "recheck_payment: Rechecked → reply, ")
    assert any("失敗の扱い" in m for m in msgs(chk(gone), ERROR)), msgs(chk(gone), ERROR)


GTIN = """ltype Jan = text 13..13 of "0-9" check gtin
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
"""


def test_JANのチェックディジットは外から来る値を実行時に確かめ_確かめていない文字列は書けない():
    ck = chk(GTIN)
    assert not msgs(ck, ERROR), msgs(ck, ERROR)
    rs = generate(ck)
    assert 'check_gtin("jan"' in rs and "pub fn check_gtin" in rs
    bad = chk(GTIN.replace("item.jan = jan", "item.jan = raw"))
    assert any("検査数字" in m for m in msgs(bad, ERROR)), msgs(bad, ERROR)


def test_check_gtin_は長さと文字種が合わないと書けない():
    with pytest.raises(ParseError):
        chk(GTIN.replace('ltype Jan = text 13..13 of "0-9" check gtin', 'ltype Jan = text 5..13 of "0-9" check gtin'))
    with pytest.raises(ParseError):
        chk(GTIN.replace('ltype Jan = text 13..13 of "0-9" check gtin', 'ltype Jan = text 13..13 check gtin'))
    with pytest.raises(ParseError):
        chk(GTIN.replace("check gtin", "check luhn"))


@needs_cargo
def test_check_gtin_は正しい13桁だけ通す(tmp_path):
    ck = chk(OVERSEAS)
    main = ('use geas_app::*;\nfn main() {\n'
            '  assert!(check_gtin("j", "4901234567894").is_ok());\n'
            '  assert!(check_gtin("j", "4901234567895").is_err());\n'
            '  assert!(check_gtin("j", "490123456789a").is_err());\n}\n')
    write_crate(ck, generate(ck), main, tmp_path)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]


# ---- アプリ（サーバー・画面）向けの生成物 ----
def test_host_は列つきの問い合わせの行を列名つきの_JSON_で返し_定義に外部作用の合流先を出す():
    ck = chk(OVERSEAS)
    host = host_source(ck)
    assert "product_list_row_json" in host and 'js_str("price")' in host and '\\"rows\\"' in host
    defs = json.loads(host.split('DEFS_JSON: &str = r####"')[1].split('"####')[0])
    assert defs["effects"]["Warehouse.hold"]["joins"] == "OrderPending"
    assert defs["effects"]["Shipping.ship"]["no_wait"] is True
    assert defs["effects"]["Payment.charge"]["args"] == ["order_id", "currency", "amount"]


def test_問い合わせの_equals_は参照の項目をIDの入力で絞れる():
    ck = chk(OVERSEAS)
    assert ck.ready, ck.diags
    assert "r.customer == *x" in generate(ck)
    bad = OVERSEAS.replace("filter customer equals customer", "filter total equals customer")
    assert any("equals" in m for m in msgs(chk(bad), ERROR))
