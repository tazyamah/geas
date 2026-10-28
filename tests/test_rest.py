# SPDX-License-Identifier: GPL-3.0-or-later
"""リソース（resource）：URL の決め方・検査（状態の対応が必須、クエリ文字列を使わない、重なりなし）"""
from pathlib import Path

from geas.check import ERROR, WARN, check
from geas.parser import parse
from geas.rest import routes, singular, snake
from geas.spec import build, render

CODE = (Path(__file__).parent / "fixtures" / "overseas_shop" / "overseas_shop.geas").read_text()


def chk(src):
    return check(parse(src), strict_names=True)


def msgs(ck, level=ERROR):
    return [d.msg for d in ck.diags if d.level == level]


def mutate(src, old, new):
    assert old in src, old
    return src.replace(old, new, 1)


def test_名前から_URL_が決まる():
    assert snake("ProductPrices") == "product_prices" and singular("customers") == "customer" and singular("categories") == "category"
    table = {(m, p): (k, t) for m, p, k, t, _, _ in routes(chk(CODE).p)}
    assert table[("POST", "/orders")] == ("create", "place_order")
    assert table[("GET", "/orders")] == ("index", "order_list")
    assert table[("GET", "/customers/:customer_id/orders")] == ("index", "orders_of")  # CustomerOrders under Customers は、親の名前を落とす
    assert table[("GET", "/product_prices/:jan")] == ("show", "price_by_jan")
    assert table[("POST", "/order_settlements")] == ("create", "settle_order")
    assert table[("POST", "/order_recheck_requests")] == ("create", "recheck_payment")


def test_海外店は検査を通り_仕様書にルートが出る():
    ck = chk(CODE)
    assert ck.ok, ck.diags
    assert not [m for m in msgs(ck, WARN) if "リソース" in m or "割り当てて" in m]
    text = render(ck, build(ck))
    assert "## ルート" in text and "| POST | /orders | create | 注文 | 受付→201," in text and "/customers/:customer_id/orders" in text


def test_結果の状態が抜けるとエラー():
    ck = chk(mutate(CODE, "    NoSuchCustomer → 404\n  }\n  index order_list", "  }\n  index order_list"))
    assert any("結果「顧客なし」の HTTP の状態がありません" in m for m in msgs(ck))


def test_存在しない結果や範囲外の状態はエラー():
    assert any("結果の種類ではありません" in m for m in msgs(chk(mutate(CODE, "    Received → 201\n", "    Received → 201\n    Nothing → 404\n"))))
    assert any("200〜599" in m for m in msgs(chk(mutate(CODE, "Received → 201", "Received → 99"))))


def test_クエリ文字列になる入力はエラー():
    # 入れ子にしないと、問い合わせ orders_of の入力 customer はパスで渡せない
    assert any("クエリ文字列にしない" in m for m in msgs(chk(mutate(CODE, "resource CustomerOrders under Customers", "resource CustomerOrders"))))
    # show にキーが無い
    assert any("key" in m for m in msgs(chk(mutate(CODE, "resource ProductPrices key jan", "resource ProductPrices"))))


def test_同じ処理を2か所に割り当てられない_URLの重なりもエラー():
    ck = chk(CODE + "\nresource Orders2 {\n  index order_list\n}\n")
    assert any("すでに" in m for m in msgs(ck))


def test_割り当てのない処理は警告():
    ck = chk(mutate(CODE, "/// 商品\nresource Products {\n  index product_list\n}\n", ""))
    assert any("product_list" in d.where or "商品一覧" in d.where for d in ck.diags if d.level == WARN)


def test_画面はリソースの動作のURLに付き_割り当て漏れと重なりは誤り():
    from geas.rest import screen_url
    ck = chk(CODE)
    urls = {sc.name: screen_url(ck.p, sc) for sc in ck.p.screens}
    assert urls  # 画面があるなら URL が付く
    assert all(u.startswith("/") for u in urls.values() if u)


PLAN = (Path(__file__).parent / "fixtures" / "overseas_shop" / "overseas_shop_plan.md").read_text()


def _compare(plan):
    from geas.plan import compare_static, read_plan
    return compare_static(build(chk(CODE)), read_plan(plan))


def test_計画書のルートの表と突合する():
    assert _compare(PLAN) == []
    # 経路が違う／状態が違う／計画に無いルート
    f = _compare(PLAN.replace("| POST | /order_settlements |", "| POST | /order_settlement |"))
    assert any("ルート POST /order_settlement" in x.where and "コードから生成した仕様書に無い" in x.msg for x in f)
    f = _compare(PLAN.replace("注文なし→404, 状態違い→409 |\n| POST | /order_completions", "注文なし→400, 状態違い→409 |\n| POST | /order_completions", 1))
    assert any("結果→状態" in x.where for x in f)
    f = _compare(PLAN.replace("| GET | /products | index | 商品一覧 |  |\n", ""))
    assert any("ルート GET /products" in x.where for x in f)


def test_計画書の画面のURLが違えば食い違う():
    f = _compare(PLAN.replace("| /orders/new |", "| /orders/add |"))
    assert any("画面 注文の画面 のURL" in x.where for x in f)
