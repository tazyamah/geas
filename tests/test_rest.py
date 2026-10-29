# SPDX-License-Identifier: GPL-3.0-or-later
"""リソース（resource）：URL の決め方・検査（状態の対応が必須、クエリ文字列を使わない、重なりなし）"""
from pathlib import Path

import pytest

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
    ck = chk(mutate(CODE, "/// 商品\nresource Products {\n  index product_list\n  update restock_product {\n    Restocked → 200\n    NoSuchProduct → 404\n  }\n  destroy withdraw_product {\n    Withdrawn → 200\n    NoSuchProduct → 404\n  }\n}\n", ""))
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


def test_ユースケースの手順のURLは画面のURLと突合する():
    assert _compare(PLAN) == []
    f = _compare(PLAN.replace("| 注文の画面 | 注文 | 受付 | /orders/new |", "| 注文の画面 | 注文 | 受付 | /orders |", 1))
    assert any("ユースケース" in x.where and "URL" in x.msg for x in f)


def test_内部URLごとの確認の網羅はユースケースと具体例から数える():
    from geas.plan import coverage, read_plan
    spec, plan = build(chk(CODE)), read_plan(PLAN)
    rows, got, total = coverage(spec, plan)
    by = {(r[0], r[1], r[2]): r for r in rows}
    assert by[("POST", "/orders", "受付")][6] == "○" and by[("POST", "/orders", "受付")][3] == "201"
    assert by[("POST", "/orders", "在庫不足")][5].startswith("在庫が足りなかった注文をやり直す")
    assert by[("POST", "/order_settlements", "承認")][6] == "未"
    assert by[("GET", "/products", "")][6] == "○"  # 画面を通る手順がある問い合わせ
    assert 0 < got < total
    # 具体例が当たれば、その行も確認済みになる
    plan["具体例"] = [{"ID": "EX1", "処理": "決済と在庫の見立て", "期待する結果": "承認(N1)"}]
    rows, got2, _ = coverage(spec, plan)
    assert got2 == got + 1 and [r for r in rows if r[1] == "/order_settlements" and r[2] == "承認"][0][4] == "EX1"


def test_ruleは仕様書と定義に出る():
    from geas.hostgen import definitions
    ck = chk(CODE)
    assert definitions(ck)["rules"]["tax_rate"] == [{"when": [["currency", "JPY"]], "value": "10/100"}, {"when": [], "value": "0/1"}]
    md = render(ck, build(ck))
    assert "slot の決め方" in md and "配送先.国 = JP" in md


def test_ruleの誤りは止まる():
    bad = {
        "最後が default でない": ("  default: 0/1\n", ""),
        "決められない型": ("rule tax_rate {", "rule currency {\n  default: 1\n}\nrule tax_rate {"),
        "値の種類違い": ("currency == JPY: 10/100", "currency == JPY: 10"),
        "条件の名前が未知": ("currency == JPY: 10/100", "nothing == JPY: 10/100"),
    }
    for why, (old, new) in bad.items():
        ck = chk(mutate(CODE, old, new))
        assert any("rule" in d.where for d in ck.diags if d.level == ERROR), why


def test_URLの具体例はルートと突き合わされ_テストに変わる(tmp_path):
    from geas.plan import coverage, compare_static, read_plan
    from geas.testgen import url_rows, write_url_tests
    plan_md = (Path(__file__).parent / "fixtures" / "overseas_shop" / "overseas_shop_plan.md").read_text()
    ck = chk(CODE)
    sp = build(ck)
    plan = read_plan(plan_md)
    assert len(plan["URL例"]) == 16 and not [f for f in compare_static(sp, plan) if "URL例" in f.where]
    rows = {r[0]: r for r in url_rows(ck, plan)}
    assert rows["UX6"][1:6] == ("POST", "/orders", rows["UX6"][3], "Received", 201) and rows["UX6"][7] == [("order_id", "body.order_id")]
    assert rows["UX13"][4] == "Withdrawn" and rows["UX3"][4] == ""  # 表示名はコードの名前に、「－」は結果なしに
    f = write_url_tests(ck, plan, tmp_path)
    assert f == tmp_path / "tests" / "test_url_examples.py" and "def test_url_example" in f.read_text()
    cov = {(r[0], r[1], r[2]): r for r in coverage(sp, plan)[0]}
    assert "UX6" in cov[("POST", "/orders", "受付")][4] and cov[("PUT", "/products/:id", "商品なし")][6] == "○"  # 網羅の表に URL の具体例が数えられる
    for old, new, what in (("| UX4 | POST | /orders |", "| UX4 | POST | /nothing |", "ルートがない"), ("| 通貨が違う | 422 |", "| 通貨が違う | 200 |", "状態が違う"), ("| 受付 | 201 |", "| 受付済 | 201 |", "結果がない")):
        bad = read_plan(mutate(plan_md, old, new))
        assert [f for f in compare_static(sp, bad) if "URL例" in f.where], what


def test_画面の具体例はユースケースの手順と結ばれ_通しテストの手順に変わる(tmp_path):
    from geas.plan import read_plan
    from geas.testgen import screen_cases, write_screen_tests
    plan_md = (Path(__file__).parent / "fixtures" / "overseas_shop" / "overseas_shop_plan.md").read_text()
    ck = chk(CODE)
    plan = read_plan(plan_md)
    cases = {c["name"]: c["steps"] for c in screen_cases(ck, plan)}
    order = cases["商品を見つけて注文する"]
    assert [s["op"] for s in order] == ["look", "open", "act", "look"] and order[1]["row"] == "tea-jp" and order[1]["opens"] == "OrderForm"
    placed = order[2]
    assert placed["deed"] == "place_order" and placed["result"] == "Received" and placed["to"] == "MyOrders"
    assert {"as": "field", "field": "ship_to.country", "map": False, "value": "JP"} in placed["set"]  # 合成の部品は 欄.部品
    assert {"as": "count", "row": "knife-jp", "value": 1} in placed["set"]
    assert order[0]["design"]["layout"] == "grid"  # 画面デザインの表から
    rej = cases["注文が受け付けられない理由を知る"]
    assert rej[3]["replace"] == [{"as": "slot", "slot": "currency", "value": "USD"}] and rej[3]["result"] == "OtherCurrency"
    assert cases["取り下げた商品を扱おうとして失敗する"][1]["replace"][0]["field"] == "product_id"
    exp = cases["返事が来ない注文は、30分で取り消される"][2]
    assert exp["drop"] == ["PaymentSettled"] and exp["advance"] == 1800  # 返事を落とし、時計を進める（実際には待たない）
    ext = cases["商品ページでなければ、何も出さない"][0]
    assert ext["ext"] and ext["origin"] == "https://www.amazon.co.jp" and ext["expect"] == [{"as": "表示なし", "value": ""}]
    f = write_screen_tests(ck, plan, tmp_path)
    assert f == tmp_path / "tests" / "test_screen_examples.py" and (tmp_path / "tests" / "screen_runner.js").exists() and (tmp_path / "tests" / "screen_cases.json").exists()
    for old, new, what in (
        ("| 商品を見つけて注文する | 3 | 明細.knife-jp", "| 商品を見つけて注文する | 3 | 明細.knife-jp = 1; 備考 = x; 明細.knife-jp", "ない入力欄"),
        ("| 通知: 受付 |\n| 商品を見つけて注文する | 4", "| 通知: 受付; 色: 赤 |\n| 商品を見つけて注文する | 4", "読めない見えること"),
        ("| 商品を入荷する | 2 | 商品ID = bowl-jp", "| 商品を入荷する | 9 | 商品ID = bowl-jp", "手順がない"),
    ):
        with pytest.raises(ValueError):
            screen_cases(ck, read_plan(mutate(plan_md, old, new)))
