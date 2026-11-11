# SPDX-License-Identifier: GPL-3.0-or-later
"""処理の中身（どの条件でどの結果になるか、何をいくつ書くか）が、人が読む仕様書と変更点に出ること。
出ていないと、コードの決まりを変えても仕様書が 1 文字も変わらず、コードを読まないレビューで見つけられない。"""
from pathlib import Path

from geas.check import check
from geas.parser import parse
from geas.review import changes_section, review
from geas.spec import build, render

SRC = Path("tests/fixtures/overseas_shop/overseas_shop.geas")
BASE = SRC.read_text(encoding="utf-8")


def views(text):
    ck = check(parse(text, str(SRC)), strict_names=True)
    assert ck.ok
    spec = build(ck)
    return spec, render(ck, spec, ""), review(ck, None, [], "")


def changed(old, new):
    assert old in BASE
    s0, md0, _ = views(BASE)
    s1, md1, html1 = views(BASE.replace(old, new, 1))
    return s0, s1, md0, md1, html1


def test_処理の中身が仕様書に出る():
    _, md, html = views(BASE)
    for text in (md, html):
        assert "顧客.ポイント に足す： 10" in text  # HTML では図の中の文字
        assert "顧客.ポイント に足す： 5" in text
    assert "order.状態 != 決済要求済 なら 結果 状態違い" in md
    assert "order.状態 != 決済要求済" in html and "処理の中身 ポイントの加算" in html  # HTML はフローチャート（分かれ道と結果は別の箱）


def test_比較の向きを変えると仕様書と変更点に出る():
    s0, s1, md0, md1, html1 = changed("Product[k].stock < v }", "Product[k].stock <= v }")
    assert md0 != md1 and "商品[k].在庫 &lt;= v" in html1
    assert changes_section(s1, s0, "前") != changes_section(s0, s0, "前")


def test_足す数を変えると仕様書と変更点に出る():
    s0, s1, md0, md1, _ = changed("customer.points += 10", "customer.points += 9")
    assert md0 != md1
    assert "に足す： 9" in changes_section(s1, s0, "前")


def test_条件を逆にすると仕様書と変更点に出る():
    s0, s1, md0, md1, _ = changed("if order.status != PaymentRequested { return WrongStatus }", "if order.status == PaymentRequested { return WrongStatus }")
    assert md0 != md1
    assert changes_section(s1, s0, "前") != changes_section(s0, s0, "前")


def test_仕様書に出ていなかった宣言が出る():
    _, md, html = views(BASE)
    for text in (md, html):
        assert "海外にも売るお店" in text  # アプリの名前
        assert "（最大 100 件）" in text  # 集まりの要素数の上限
        assert "B0SHOPTEA1→4901234567894" in text  # 拡張の対応表の中身
        assert "[A-Z0-9]{10}" in text  # 拡張が要素を読むときの形
        assert "送料: 金額(通貨:通貨) 0..100000" in text  # slot に使う型の範囲


def test_返事から値を取り出す場所が出る():
    src = Path("tests/fixtures/solo_tweet/solo_tweet.geas")
    ck = check(parse(src.read_text(encoding="utf-8"), str(src)), strict_names=True)
    spec = build(ck)
    assert "data.id" in render(ck, spec, "") and "data.id" in review(ck, None, [], "")


def test_エラーのあるコードでも仕様書は落ちずに出る():
    for drop in ("ltype Quantity = int 1..99", "  restock_product: Restocked → reply, NoSuchProduct → reply, StockFull → reply"):
        assert drop in BASE
        ck = check(parse(BASE.replace(drop, "", 1), str(SRC)), strict_names=True)
        assert not ck.ok
        spec = build(ck)
        assert render(ck, spec, "") and review(ck, None, [], "")
