# SPDX-License-Identifier: GPL-3.0-or-later
"""海外店のアプリ（tests/fixtures/overseas_shop/app）の通し確認。
Geas から生成した Rust をサーバーに組み、localhost で動かして、JSON の口と、PWA・Chrome 拡張を確かめる。
cargo が無い環境では飛ばす。画面と拡張の確認は、node と Playwright があるときだけ行う。"""
import json
import os
import shutil
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
APP = ROOT / "tests" / "fixtures" / "overseas_shop" / "app"

pytestmark = pytest.mark.skipif(shutil.which("cargo") is None, reason="cargo が無い")


@pytest.fixture(scope="module")
def server(tmp_path_factory):
    subprocess.run(["sh", str(APP / "build.sh")], check=True, capture_output=True, text=True, cwd=ROOT)
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    work = tmp_path_factory.mktemp("shop")
    proc = subprocess.Popen(
        [str(APP / "server" / "target" / "release" / "shop_server"), "--port", str(port), "--db", str(work / "shop.sqlite"),
         "--pwa", str(APP / "server" / "generated" / "ui"), "--dev", str(APP / "server" / "generated" / "dev"), "--seed", str(APP / "seed.json")],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    base = f"http://127.0.0.1:{port}"
    for _ in range(50):
        try:
            urllib.request.urlopen(base + "/api/health", timeout=1)
            break
        except OSError:
            time.sleep(0.1)
    else:
        proc.kill()
        raise RuntimeError("サーバーが起動しません")
    yield {"base": base, "work": work, "db": work / "shop.sqlite", "port": port}
    proc.kill()
    proc.wait()


def call(base, method, path, body=None):
    req = urllib.request.Request(base + "/api/" + path, method=method, data=None if body is None else json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=10) as r:
            return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read())


def order(base, customer, items, currency="JPY", country="JP"):
    return call(base, "POST", "deeds/place_order", {
        "input": {"customer_id": customer, "items": items, "ship_to": {"country": country, "postal_code": "100-0001", "line": "x"}},
        "slots": {"currency": currency}})[1]


def status_of(base, order_id, customer):
    rows = call(base, "GET", f"queries/orders_of?customer={customer}")[1]["rows"]
    return next(r["status"] for r in rows if r["order_id"] == order_id)


def settle(base, secs=3.0):
    """模擬の返事が返るのを待つ（返事までの秒数は1秒）"""
    deadline = time.time() + secs
    while time.time() < deadline:
        time.sleep(0.2)


def reset_mock(base):
    call(base, "POST", "test/settings", {"delay_secs": 1, "replies": {"PaymentSettled": {"outcome": "Approved", "drop": False}, "StockSettled": {"outcome": "Held", "drop": False}}})


def test_注文は決済と倉庫の返事がそろって確定し_在庫とポイントが動く(server):
    b = server["base"]
    reset_mock(b)
    r = order(b, "c-001", {"tea-jp": 2, "knife-jp": 1})
    assert r["result"] == "Received"
    assert [t["deed"] for t in r["trace"]][:2] == ["place_order", "request_payment"]  # 受注のあと、並列の枝が動く
    oid = r["body"]["order_id"]
    assert status_of(b, oid, "c-001") == "PaymentRequested"  # 返事がそろうまでは待つ
    settle(b)
    assert status_of(b, oid, "c-001") == "Completed"
    rows = call(b, "GET", "queries/orders_of?customer=c-001")[1]["rows"]
    assert next(x for x in rows if x["order_id"] == oid)["total"] == 10160 + 1016 + 500  # 小計 + 税10% + 国内送料
    stock = {p["product_id"]: p["stock"] for p in call(b, "GET", "queries/product_list")[1]["rows"]}
    assert stock["tea-jp"] == 118 and stock["knife-jp"] == 14
    names = [e["name"] for e in call(b, "GET", "test/state")[1]["effects"]]
    assert "Shipping.ship" in names


def test_ドルの注文は税なしで_通貨の違う商品を混ぜると止まる(server):
    b = server["base"]
    reset_mock(b)
    assert order(b, "c-001", {"tea-jp": 1}, "USD")["result"] == "OtherCurrency"
    r = order(b, "c-001", {"tea-us": 1}, "USD", country="US")
    assert r["result"] == "Received"
    rows = call(b, "GET", "queries/orders_of?customer=c-001")[1]["rows"]
    last = next(x for x in rows if x["order_id"] == r["body"]["order_id"])
    assert (last["currency"], last["total"]) == ("USD", 699 + 3000)  # 税0%、海外送料30ドル（セント）
    settle(b)


def test_決済の返事が落ちても_つながりが戻ると同じ冪等キーで引き直して確定する(server):
    b = server["base"]
    reset_mock(b)
    call(b, "POST", "test/settings", {"replies": {"PaymentSettled": {"drop": True}}})
    oid = order(b, "c-002", {"bowl-jp": 1})["body"]["order_id"]
    settle(b)
    assert status_of(b, oid, "c-002") == "PaymentRequested"  # 倉庫の返事だけが届いて、待ち続ける
    call(b, "POST", "test/settings", {"replies": {"PaymentSettled": {"drop": False}}})
    ev = call(b, "POST", "events/Reconnected", {})[1]
    assert [t["deed"] for run in ev["runs"] for t in run["trace"]] == ["recheck_payment"]
    settle(b)
    assert status_of(b, oid, "c-002") == "Completed"
    charges = [e for e in call(b, "GET", "test/state")[1]["effects"] if e["name"] == "Payment.charge" and e["args"]["order_id"] == oid]
    assert len(charges) == 2 and len({c["idem_key"] for c in charges}) == 1  # 請求は2回出したが、冪等キーは同じ
    assert call(b, "POST", "events/Reconnected", {})[1]["runs"] == []  # 返事待ちはもう無い


def test_返事がそろわないまま30分たつと取り消す(server):
    b = server["base"]
    reset_mock(b)
    call(b, "POST", "test/settings", {"replies": {"PaymentSettled": {"drop": True}}})
    oid = order(b, "c-002", {"bowl-jp": 1})["body"]["order_id"]
    settle(b, 1.5)
    fired = call(b, "POST", "test/advance", {"secs": 1801})[1]["fired"]
    assert any(f.get("deadline") == "OrderPending" for f in fired)
    assert status_of(b, oid, "c-002") == "Canceled"
    reset_mock(b)


def test_倉庫が在庫不足を返すと返金して取り消し_決済が拒否されると取り消す(server):
    b = server["base"]
    reset_mock(b)
    call(b, "POST", "test/settings", {"replies": {"StockSettled": {"outcome": "Short"}}})
    oid = order(b, "c-002", {"bowl-jp": 1})["body"]["order_id"]
    settle(b)
    assert status_of(b, oid, "c-002") == "Canceled"
    assert any(e["name"] == "Payment.refund" and e["args"]["order_id"] == oid for e in call(b, "GET", "test/state")[1]["effects"])
    reset_mock(b)
    call(b, "POST", "test/settings", {"replies": {"PaymentSettled": {"outcome": "Declined"}}})
    oid2 = order(b, "c-002", {"bowl-jp": 1})["body"]["order_id"]
    settle(b)
    assert status_of(b, oid2, "c-002") == "Canceled"
    assert not any(e["name"] == "Payment.refund" and e["args"]["order_id"] == oid2 for e in call(b, "GET", "test/state")[1]["effects"])
    reset_mock(b)


def test_外から来る値は境界で確かめる(server):
    b = server["base"]
    assert call(b, "GET", "queries/price_by_jan?jan=4901234567894")[1]["rows"] == [
        {"currency": "JPY", "price": 680, "product_id": "tea-jp"}, {"currency": "USD", "price": 699, "product_id": "tea-us"}]
    code, j = call(b, "GET", "queries/price_by_jan?jan=4901234567895")
    assert code == 400 and "チェックディジット" in j["error"]
    assert call(b, "GET", "queries/price_by_jan?jan=12345")[0] == 400
    assert order(b, "c-999", {"tea-jp": 1})["result"] == "NoSuchCustomer"
    code, j = call(b, "POST", "deeds/place_order", {"input": {"customer_id": "c-001", "items": {"tea-jp": 0}, "ship_to": {"country": "JP", "postal_code": "1", "line": "x"}}, "slots": {"currency": "JPY"}})
    assert code == 400 and "範囲外" in j["error"]
    code, j = call(b, "POST", "deeds/place_order", {"input": {"customer_id": "c-001", "items": {"tea-jp": 1}, "ship_to": {"country": "JP", "postal_code": "1", "line": "x"}}, "slots": {"currency": "GBP"}})
    assert code == 400
    assert call(b, "POST", "deeds/nothing", {})[0] == 400


def test_再起動しても注文と返事待ちは残る(server):
    """SQLite に保存してあるので、サーバーを落として起こしても、状態が続く"""
    b = server["base"]
    reset_mock(b)
    call(b, "POST", "test/settings", {"replies": {"PaymentSettled": {"drop": True}}})
    oid = order(b, "c-002", {"bowl-jp": 1})["body"]["order_id"]
    settle(b, 1.5)
    waits = call(b, "GET", "test/state")[1]["waits"]
    assert any(w["args"]["order_id"] == oid for w in waits)
    import sqlite3
    con = sqlite3.connect(server["db"])
    assert con.execute('select count(*) from "Order" where k = ?', (oid,)).fetchone()[0] == 1
    assert con.execute("select count(*) from geas_waits where args like ?", (f"%{oid}%",)).fetchone()[0] == 1
    con.close()
    call(b, "POST", "test/advance", {"secs": 1801})
    reset_mock(b)


_fresh = []


def _fresh_server(tmp_path, port=0):
    """状態のない新しい DB で、もう1つサーバーを起こす（テストの最後に止める）。port=0 なら空いているポート"""
    s = socket.socket()
    s.bind(("127.0.0.1", port))
    port = s.getsockname()[1]
    s.close()
    proc = subprocess.Popen([str(APP / "server" / "target" / "release" / "shop_server"), "--port", str(port), "--db", str(tmp_path / "ui.sqlite"),
                             "--pwa", str(APP / "server" / "generated" / "ui"), "--dev", str(APP / "server" / "generated" / "dev"), "--seed", str(APP / "seed.json")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    _fresh.append(proc)
    for _ in range(50):
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{port}/api/health", timeout=1)
            return port
        except OSError:
            time.sleep(0.1)
    raise RuntimeError("サーバーが起動しません")


@pytest.fixture(autouse=True, scope="module")
def _stop_fresh():
    yield
    for p in _fresh:
        p.kill()
        p.wait()


def _node_env():
    node = shutil.which("node")
    if node is None:
        return None
    root = subprocess.run(["npm", "root", "-g"], capture_output=True, text=True).stdout.strip() if shutil.which("npm") else ""
    env = dict(os.environ, NODE_PATH=root)
    ok = subprocess.run([node, "-e", "require('playwright')"], env=env, capture_output=True).returncode == 0
    return env if ok else None


@pytest.mark.skipif(_node_env() is None, reason="node と Playwright が無い")
def test_画面_PWA_で注文から完了まで進む(server, tmp_path):
    # 画面の確認は、状態を持たない新しい DB のサーバーで行う
    r = subprocess.run(["node", str(APP / "e2e" / "pwa.js"), "http://127.0.0.1:" + str(_fresh_server(tmp_path))], env=_node_env(), capture_output=True, text=True, timeout=120)
    assert r.returncode == 0, r.stdout + r.stderr


@pytest.mark.skipif(_node_env() is None, reason="node と Playwright が無い")
def test_Chrome拡張がAmazonの商品ページにお店の価格を出す(server, tmp_path):
    # 拡張が問い合わせてよいオリジンは manifest の host_permissions にある localhost:8787 だけなので、そのポートで起こす
    try:
        _fresh_server(tmp_path, 8787)
    except OSError:
        pytest.skip("ポート 8787 が使用中")
    r = subprocess.run(["node", str(APP / "e2e" / "extension.js"), str(tmp_path)], env=_node_env(), capture_output=True, text=True, timeout=180)
    assert r.returncode == 0, r.stdout + r.stderr


def rest(base, method, path, body=None):
    """resource 宣言の URL（/api は付かない）"""
    req = urllib.request.Request(base + path, method=method, data=None if body is None else json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=10) as r:
            return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read())


def test_RESTのURLで読み_結果の種類ごとの状態が返る(server):
    b = server["base"]
    reset_mock(b)
    code, j = rest(b, "GET", "/products")
    assert code == 200 and any(p["product_id"] == "tea-jp" for p in j["rows"])
    assert rest(b, "GET", "/product_prices/4901234567894")[1]["rows"][0]["price"] in (680, 699)
    assert rest(b, "GET", "/product_prices/12345")[0] == 400  # 境界の検査はそのまま効く
    body = {"customer_id": "c-001", "items": {"tea-jp": 1}, "ship_to": {"country": "JP", "postal_code": "100-0001", "line": "x"}}
    code, j = rest(b, "POST", "/orders", {**body, "slots": {"currency": "USD"}})
    assert (code, j["result"]) == (422, "OtherCurrency")  # 通貨が違う
    code, j = rest(b, "POST", "/orders", {**body, "customer_id": "c-999", "slots": {"currency": "JPY"}})
    assert (code, j["result"]) == (404, "NoSuchCustomer")
    code, j = rest(b, "POST", "/orders", {**body, "slots": {"currency": "JPY"}})
    assert (code, j["result"]) == (201, "Received")
    code, j = rest(b, "GET", "/customers/c-001/orders")
    assert code == 200 and any(r["order_id"] == j2 for j2 in [rest(b, "GET", "/customers/c-001/orders")[1]["rows"][-1]["order_id"]] for r in j["rows"])
    assert rest(b, "POST", "/order_settlements", {"order_id": "nothing"})[0] == 404  # 注文なし
    assert rest(b, "POST", "/reconnections", {})[0] == 201
    assert rest(b, "GET", "/nothing")[0] == 404


def test_RESTのupdateとdestroy_入荷と取り下げが状態つきで返り_取り下げた商品は注文の確定で商品なしになる(server, tmp_path):
    b = f"http://127.0.0.1:{_fresh_server(tmp_path)}"  # 商品を消すので、ほかのテストと状態を分ける
    stock = lambda: next(p["stock"] for p in rest(b, "GET", "/products")[1]["rows"] if p["product_id"] == "tea-jp")
    before = stock()
    # update（PUT /products/:id）：商品の入荷
    code, j = rest(b, "PUT", "/products/tea-jp", {"quantity": 5})
    assert (code, j["result"]) == (200, "Restocked") and stock() == before + 5
    code, j = rest(b, "PUT", "/products/nothing", {"quantity": 5})
    assert (code, j["result"]) == (404, "NoSuchProduct")
    assert rest(b, "PUT", "/products/tea-jp", {"quantity": 0})[0] == 400  # 境界の検査（数量は1以上）はそのまま効く
    # 取り下げる前に注文を受け付けておく（注文の明細には商品IDが残る）
    body = {"customer_id": "c-001", "items": {"tea-jp": 1}, "ship_to": {"country": "JP", "postal_code": "100-0001", "line": "x"}, "slots": {"currency": "JPY"}}
    code, j = rest(b, "POST", "/orders", body)
    assert (code, j["result"]) == (201, "Received")
    oid = j["body"]["order_id"]
    # destroy（DELETE /products/:id）：商品の取り下げ
    code, j = rest(b, "DELETE", "/products/tea-jp")
    assert (code, j["result"]) == (200, "Withdrawn")
    assert all(p["product_id"] != "tea-jp" for p in rest(b, "GET", "/products")[1]["rows"])
    code, j = rest(b, "DELETE", "/products/tea-jp")
    assert (code, j["result"]) == (404, "NoSuchProduct")  # 2回目は商品なし
    assert rest(b, "PUT", "/products/tea-jp", {"quantity": 1})[0] == 404
    # 参照は leave：注文は残り、確定のときに商品が無いので「在庫が足りなかった」
    code, j = rest(b, "POST", "/order_completions", {"order_id": oid})
    assert (code, j["result"]) == (422, "StockLacking")
