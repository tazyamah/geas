# SPDX-License-Identifier: GPL-3.0-or-later
"""画面（screen）の宣言：検査・仕様書・計画書との突合"""
from pathlib import Path

from geas.check import ERROR, WARN, check
from geas.parser import parse
from geas.plan import compare_static, read_plan
from geas.spec import build, render

DIR = Path(__file__).parent / "fixtures" / "overseas_shop"
CODE = (DIR / "overseas_shop.geas").read_text()


def chk(src):
    return check(parse(src), strict_names=True)


def msgs(ck, level):
    return [d.msg for d in ck.diags if d.level == level]


def mutate(src, old, new):
    assert old in src, old
    return src.replace(old, new, 1)


def test_海外店の画面は検査を通り_仕様書に画面と遷移とデザインが出る():
    ck = chk(CODE)
    assert ck.ok, ck.diags
    spec = build(ck)
    assert [x[0] for x in spec["画面"]] == ["商品一覧の画面", "注文の画面", "注文の履歴の画面", "価格の確認の画面（Chrome 拡張）", "商品の管理の画面"]
    assert [x[4] for x in spec["画面"]] == ["○", "", "", "○", "○"]
    assert ("商品一覧の画面", "行を選ぶ", "", "注文の画面") in spec["画面遷移"]
    assert ("注文の画面", "注文", "受付", "注文の履歴の画面") in spec["画面遷移"]
    md = render(ck, spec)
    assert "## 画面デザイン（突合しない。配置と色）" in md and "| 商品一覧の画面 | accent | #0b5cad |" in md


def test_結果の行き先に漏れがあるとエラー():
    ck = chk(mutate(CODE, "    OtherCurrency → stay\n", ""))
    assert any("結果「OtherCurrency」の行き先がありません" in m or "違う商品" in m or "通貨が違う" in m for m in msgs(ck, ERROR))


def test_処理の入力を埋めるものが無いとエラー():
    ck = chk(mutate(CODE, "field customer_id from customer_list, items from product_list, ship_to", "field customer_id from customer_list, items from product_list"))
    assert any("入力「" in m and "埋めるものがありません" in m for m in msgs(ck, ERROR))


def test_行き先が実在しないとエラー_入口から行けない画面は警告():
    assert any("行き先" in m for m in msgs(chk(mutate(CODE, "Received → MyOrders", "Received → Nowhere")), ERROR))
    ck = chk(mutate(CODE, "screen PriceCheck entry {", "screen PriceCheck {"))
    assert any("入口の画面から行けません" in m for m in msgs(ck, WARN))


def test_designの値の形を検査する():
    assert any("accent" in m for m in msgs(chk(mutate(CODE, 'accent "#e47911"', 'accent "red"')), ERROR))
    assert any("layout" in m for m in msgs(chk(mutate(CODE, "layout stack", "layout wild")), ERROR))
    assert any("使えません" in m for m in msgs(chk(mutate(CODE, "layout stack", "font big")), ERROR))


def test_計画書との突合は画面と遷移を比べ_デザインは比べない():
    ck = chk(CODE)
    spec = build(ck)
    plan = "\n".join(
        ["| 画面 | ◆見せる | ◆入力欄 | ◆受け取る値 | ◆入口 | ◆選ばせる slot |", "|---|---|---|---|---|---|"]
        + [f"| {a} | {b} | {c} | {d} | {e} | {f} |" for a, b, c, d, e, f, _ in spec["画面"]]
        + ["", "| 画面 | ◆操作 | ◆結果 | ◆行き先 |", "|---|---|---|---|"]
        + [f"| {a} | {b} | {c} | {d} |" for a, b, c, d in spec["画面遷移"]]
    )
    only = [f for f in compare_static(spec, read_plan(plan)) if f.where.startswith(("画面", "画面遷移"))]
    assert only == []
    bad = plan.replace("| 注文の画面 | 注文 | 受付 | 注文の履歴の画面 |", "| 注文の画面 | 注文 | 受付 | 商品一覧の画面 |")
    assert any("行き先" in f.where for f in compare_static(spec, read_plan(bad)))


PLAN = (DIR / "overseas_shop_plan.md").read_text()


def test_ユースケースの手順が画面遷移として実在しつながっているかを突合する():
    ck = chk(CODE)
    assert [f for f in compare_static(build(ck), read_plan(PLAN))] == []
    # 存在しない操作
    bad = PLAN.replace("| 商品を見つけて注文する | 3 | 注文の画面 | 注文 | 受付 |", "| 商品を見つけて注文する | 3 | 注文の画面 | 注文 | 決済済み |")
    assert any("コードの画面遷移にありません" in f.msg for f in compare_static(build(ck), read_plan(bad)))
    # つながっていない手順（行き先と次の手順の画面が違う）
    bad = PLAN.replace("| 商品を見つけて注文する | 4 | 注文の履歴の画面 | 見る |  |", "| 商品を見つけて注文する | 4 | 商品一覧の画面 | 見る |  |")
    assert any("前の手順のあとに見えている画面" in f.msg for f in compare_static(build(ck), read_plan(bad)))
    # どのユースケースにも使われない遷移
    bad = PLAN.replace("| 注文が受け付けられない理由を知る | 5 | 注文の画面 | 注文 | 顧客なし | /orders/new |\n", "")
    assert any("使われていません" in f.msg for f in compare_static(build(ck), read_plan(bad)))


def test_人向けの仕様書は画面の遷移とモックとデザインを含むHTMLになる():
    from geas.review import review
    ck = chk(CODE)
    html = review(ck, PLAN, compare_static(build(ck), read_plan(PLAN)), "（海外店）")
    for s in ("画面の遷移", "画面デザイン", "ユースケースを画面でたどる", 'id="screens-data"', "商品を見つけて注文する", "stateDiagram-v2", "flowchart LR"):
        assert s in html, s
    assert "計画書との食い違いはなく" in html


def test_画面の定義と_design_が生成物に載る():
    from geas.hostgen import definitions
    from geas.rustgen import generate
    ck = chk(CODE)
    d = definitions(ck)["screens"]
    assert d["title"] == "海外にも売るお店"
    assert d["screens"]["ProductList"]["design"] == {"layout": "grid", "accent": "#0b5cad", "mode": "light"}
    assert d["screens"]["PriceCheck"]["design"]["mode"] == "dark"
    form = d["screens"]["OrderForm"]
    assert [f["from"] for f in form["fields"]] == ["customer_list", "product_list", None]
    assert form["fields"][1]["type"]["kind"] == "map" and form["slots"][0]["name"] == "currency"
    price = next(c for c in d["queries"]["product_list"]["columns"] if c["name"] == "price")
    assert price["kind"] == "money" and price["unit"] == "currency"
    assert d["events_on"] == {"Reconnected": "online"}
    rs = generate(ck)
    assert 'pub const APP_TITLE: &str = "海外にも売るお店";' in rs
    assert 'ScreenDef { name: "PriceCheck"' in rs and 'accent: "#e47911", mode: "dark"' in rs


def test_画面の汎用UIとアイコンを生成物へ置く(tmp_path):
    from geas.screengen import write_ui
    write_ui(chk(CODE), tmp_path)
    ui = tmp_path / "ui"
    assert {"index.html", "ui.js", "ui.css", "sw.js", "icon.svg", "icon-192.png", "icon-512.png"} <= {p.name for p in ui.iterdir()}
    assert "#0b5cad" in (ui / "icon.svg").read_text()
    assert (ui / "icon-192.png").read_bytes()[:8] == b"\x89PNG\r\n\x1a\n"


def test_画面の定義はリソースのURLを持つ():
    from geas.hostgen import definitions
    d = definitions(chk(CODE))["screens"]["screens"]
    assert d["OrderForm"]["url"] == "/orders/new"
    assert d["MyOrders"]["url"] == "/customers/:customer_id/orders"
