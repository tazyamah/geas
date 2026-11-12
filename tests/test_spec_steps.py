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
    assert "order.状態 ≠ 決済要求済 なら 結果 状態違い" in md
    assert "order.状態 ≠ 決済要求済" in html and "処理の中身 ポイントの加算" in html  # HTML はフローチャート（分かれ道と結果は別の箱）


def test_比較の向きを変えると仕様書と変更点に出る():
    s0, s1, md0, md1, html1 = changed("Product[k].stock < v }", "Product[k].stock <= v }")
    assert md0 != md1 and "商品[k].在庫 ≦ v" in html1
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


def test_式は読める言葉で出て_括弧は落ちない():
    _, md, _ = views(BASE)
    assert "合計 = subtotal + 丸め（subtotal * 税率、通貨 の単位に切り捨て） + 送料" in md
    assert "subtotal = 合計〔明細 の各 (k, v)について： 商品[k].価格 * v〕" in md
    assert "不足 = 集まり〔明細 の各 (k, v) のうち 商品[k].在庫 < v のものについて： k〕" in md
    # 括弧の位置で意味が変わる式は、表示も変わる
    from geas.ast import Bin, Name, Not, show
    a, b, c = Name("a"), Name("b"), Name("c")
    assert show(Bin("*", Bin("+", a, b), c)) == "(a + b) * c" and show(Bin("+", a, Bin("*", b, c))) == "a + b * c"
    assert show(Bin("-", a, Bin("-", b, c))) == "a - (b - c)" and show(Bin("-", Bin("-", a, b), c)) == "a - b - c"
    assert show(Not(Bin("かつ", a, b))) == "not (a かつ b)"


def test_並びだけの変更と_同じ行の追加も_変更点に出る():
    a = {"決め方": [("送料", "通貨 = JPY", "500"), ("送料", "上のどれでもない", "1500")]}
    b = {"決め方": list(reversed(a["決め方"]))}
    c = {"決め方": a["決め方"] + a["決め方"][:1]}
    same = changes_section(a, a, "前の版")
    assert "並びが変わりました" in changes_section(b, a, "前の版")
    assert changes_section(c, a, "前の版") != same and "＋" in changes_section(c, a, "前の版")


def test_同じ処理の行が流れに2つあると検査で止まる():
    line = next(ln for ln in BASE.split("\n") if ln.strip().startswith("place_order:"))
    ck = check(parse(BASE.replace(line, line + "\n" + line, 1), str(SRC)), strict_names=True)
    assert not ck.ok and any("同じ流れに 2 つあります" in d.msg for d in ck.diags)


def test_同じ宣言を2回書くと検査で止まる():
    """同じ行を 2 回書くと、2 回起こるもの（出来事の each）と、届かない行になるもの（rule）がある。黙って通さない"""
    import re

    cases = [("solo_tweet", r"^\s*each saved_tweets", "同じ処理が 2 回起こります"), ("solo_tweet", r"^\s*open TweetDetail", "が 2 つあります"),
             ("overseas_shop", r"^\s*currency == JPY: 2500", "同じ条件の行が 2 つあります")]
    for fx, pat, word in cases:
        src = Path(f"tests/fixtures/{fx}/{fx}.geas")
        text = src.read_text(encoding="utf-8")
        line = next(ln for ln in text.split("\n") if re.match(pat, ln))
        ck = check(parse(text.replace(line, line + "\n" + line, 1), str(src)), strict_names=True)
        assert not ck.ok and any(word in d.msg for d in ck.diags), (fx, pat, [d.msg for d in ck.diags])
