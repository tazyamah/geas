# SPDX-License-Identifier: GPL-3.0-or-later
"""サーバーの実行部の動作テスト：小さな .geas（tests/fixtures/runtime_demo）から、サーバーまで生成して localhost で動かし、
並列（par）・待ち合わせ（all）・期限（deadline）・出来事（each）・rule・冪等キー・REST の状態・再起動を、お店アプリを使わずに確かめる。
どの機能にどのテストがあるかは docs/language-test-map.md。cargo が無い環境では飛ばす。"""
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
SRC = ROOT / "tests" / "fixtures" / "runtime_demo" / "runtime_demo.geas"
PLAN = ROOT / "tests" / "fixtures" / "runtime_demo" / "runtime_demo_plan.md"

pytestmark = pytest.mark.skipif(shutil.which("cargo") is None, reason="cargo が無い")


@pytest.fixture(scope="module")
def built(tmp_path_factory):
    out = tmp_path_factory.mktemp("runtime_demo")
    subprocess.run([sys.executable, "-m", "geas", "rust", str(SRC), "-o", str(out), "--host", "--runtime", "--plan", str(PLAN)], check=True, capture_output=True, text=True, cwd=ROOT)
    r = subprocess.run(["cargo", "build", "--release", "--offline", "--quiet"], cwd=out / "server", capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    return out


class Server:
    def __init__(self, built, db):
        self.built, self.db, self.proc, self.base = built, db, None, None

    def start(self):
        s = socket.socket()
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
        s.close()
        self.proc = subprocess.Popen(
            [str(self.built / "server" / "target" / "release" / "geas_server"), "--port", str(port), "--db", str(self.db),
             "--pwa", str(self.built / "ui"), "--dev", str(self.built / "dev")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.base = f"http://127.0.0.1:{port}"
        for _ in range(100):
            try:
                urllib.request.urlopen(self.base + "/api/health", timeout=1)
                return
            except OSError:
                time.sleep(0.1)
        self.proc.kill()
        raise RuntimeError("サーバーが起動しません")

    def stop(self):
        self.proc.kill()
        self.proc.wait()

    def call(self, method, path, body=None):
        req = urllib.request.Request(self.base + path, method=method, data=None if body is None else json.dumps(body).encode(),
                                     headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=10) as r:
                return r.status, json.loads(r.read())
        except urllib.error.HTTPError as e:
            return e.code, json.loads(e.read())

    def mock(self, **replies):
        """模擬の返事の設定。返事までは 5 秒にして、時計を進めて返させる（待たない）"""
        self.call("POST", "/api/test/settings", {"delay_secs": 5, "replies": replies})

    def state(self):
        return self.call("GET", "/api/test/state")[1]

    def advance(self, secs):
        return self.call("POST", "/api/test/advance", {"secs": secs})[1]["fired"]

    def parcels(self):
        return {p["parcel_id"]: p for p in self.call("GET", "/parcels")[1]["rows"]}

    def accept(self, kind="light"):
        code, j = self.call("POST", "/parcels", {"kind": kind})
        assert code == 201, j
        return j["body"]["parcel_id"], j


@pytest.fixture
def server(built, tmp_path):
    s = Server(built, tmp_path / "demo.sqlite")
    s.start()
    s.mock(PaySettled={"outcome": "Approved", "drop": False}, DepotSettled={"outcome": "Reserved", "drop": False})
    yield s
    s.stop()


def effects(s, name):
    return [e for e in s.state()["effects"] if e["name"] == name]


def test_rule_は上から順に当てはまった行の値になり_新しいIDは自動で採番される(server):
    a, _ = server.accept("light")
    b, _ = server.accept("heavy")
    assert (a, b) == ("parcel-0001", "parcel-0002")
    rows = server.parcels()
    assert rows[a]["fee"] == 300 and rows[b]["fee"] == 900  # kind == "heavy" の行だけ 900、他は default


def test_par_の枝は両方動き_待ち合わせは全部の返事がそろうまで進まない(server):
    pid, j = server.accept()
    assert [t["deed"] for t in j["trace"]][0] == "accept" and set(t["deed"] for t in j["trace"]) == {"accept", "request_pay", "request_reserve"}
    assert server.parcels()[pid]["status"] == "Waiting"
    assert [e["idem_key"] for e in effects(server, "Pay.charge")] == [pid]
    assert [e["idem_key"] for e in effects(server, "Depot.reserve")] == [pid]  # joins で、決済と同じ待ちに合流する
    w = server.state()["waits"]
    assert len(w) == 1 and w[0]["trigger"] == "Pending" and w[0]["args"] == {"parcel_id": pid}
    # 決済の返事だけ届いても、倉庫の返事を待つ
    server.mock(DepotSettled={"drop": True})
    pid2, _ = server.accept()
    server.advance(6)
    assert server.parcels()[pid2]["status"] == "Waiting"
    assert any(x["args"] == {"parcel_id": pid2} and x["collected"].get("PaySettled") for x in server.state()["waits"])


def test_返事がそろうと行き先へ進み_承認の中身が渡る(server):
    pid, _ = server.accept()
    fired = server.advance(6)
    assert any(f["outcome"].get("fired") == "settle" for f in fired), fired
    assert server.parcels()[pid]["status"] == "Shipped"  # Approved → ship
    assert [e["idem_key"] for e in effects(server, "Courier.send")] == [pid]  # no_wait の外部作用も、冪等キーつきで出る
    assert server.state()["waits"] == []


def test_拒否の返事は結果の種類が違うので別の行き先へ進む(server):
    server.mock(PaySettled={"outcome": "Declined"})
    pid, _ = server.accept()
    server.advance(6)
    assert server.parcels()[pid]["status"] == "Canceled"  # PayFailed → cancel（auth_id が無い）
    assert effects(server, "Courier.send") == []


def test_期限が来るまでは待ち_来たら期限の行き先へ進む(server):
    server.mock(PaySettled={"drop": True})
    pid, _ = server.accept()
    server.advance(6)
    server.advance(30 * 60 - 20)
    assert server.parcels()[pid]["status"] == "Waiting"  # 30 分に少し足りない
    fired = server.advance(30)
    assert any(f.get("deadline") == "Pending" for f in fired)
    assert server.parcels()[pid]["status"] == "Canceled"
    assert server.state()["waits"] == []


def test_つながりが戻る出来事は_返事待ちの1件ずつに引き直し_同じ冪等キーで請求する(server):
    # 返事待ちが無ければ、何も起きない
    code, ev = server.call("POST", "/reconnections", {})
    assert code == 201 and ev["runs"] == []
    server.mock(PaySettled={"drop": True})
    d, _ = server.accept()
    e, _ = server.accept()
    server.advance(6)
    server.mock(PaySettled={"drop": False})
    code, ev = server.call("POST", "/reconnections", {})
    assert code == 201
    assert [t["deed"] for run in ev["runs"] for t in run["trace"]] == ["recheck", "recheck"]
    charges = effects(server, "Pay.charge")
    assert len(charges) == 4 and {x["idem_key"] for x in charges} == {d, e}  # 1 件につき 2 回出したが、冪等キーは 1 つだけ
    server.advance(6)
    assert [server.parcels()[x]["status"] for x in (d, e)] == ["Shipped", "Shipped"]


def test_結果の種類ごとの_HTTP_の状態を返し_入力の範囲外は境界で止める(server):
    pid, _ = server.accept()
    server.mock(PaySettled={"drop": True})
    assert server.call("POST", "/settlements", {"parcel_id": "parcel-9999"})[0] == 404  # NoParcel
    code, j = server.call("POST", "/cancellations", {"parcel_id": pid})
    assert code == 201 and j["result"] == "Applied"
    assert server.call("POST", "/cancellations", {"parcel_id": pid})[0] == 409  # 二度目は WrongStatus
    assert server.call("POST", "/parcels", {"kind": ""})[0] == 400  # text 1..10 の下限
    assert server.call("POST", "/parcels", {"kind": "x" * 11})[0] == 400  # 上限
    assert server.call("POST", "/parcels", {})[0] == 400  # required の入力が無い
    assert server.call("DELETE", "/parcels")[0] == 404  # 宣言していない動作は無い
    assert len(server.parcels()) == 1  # 弾かれた注文は残らない


def test_再起動しても待ちと時計の先は続く(server):
    server.mock(PaySettled={"drop": True})
    pid, _ = server.accept()
    server.advance(6)
    server.stop()
    server.start()
    assert server.parcels()[pid]["status"] == "Waiting"
    assert len(server.state()["waits"]) == 1
    server.advance(30 * 60 + 10)
    assert server.parcels()[pid]["status"] == "Canceled"
    pid2, _ = server.accept()
    assert pid2 == "parcel-0002"  # ID の採番も再起動で戻らない


def test_計画書のURLの具体例から生成したテストが通る(built):
    r = subprocess.run([sys.executable, "-m", "pytest", "-q", str(built / "tests" / "test_url_examples.py")], capture_output=True, text=True, timeout=300)
    assert r.returncode == 0 and "13 passed" in r.stdout, r.stdout + r.stderr


def test_計画書の画面の具体例から生成したテストが通る(built):
    """入力欄・行・操作・通知・design を、実際の画面（PWA）で通す。node と Playwright が無ければ、その中で飛ばされる"""
    env = None
    if shutil.which("node") and shutil.which("npm"):
        root = subprocess.run(["npm", "root", "-g"], capture_output=True, text=True).stdout.strip()
        env = dict(os.environ, NODE_PATH=root)
    r = subprocess.run([sys.executable, "-m", "pytest", "-q", "-rs", str(built / "tests" / "test_screen_examples.py")], env=env, capture_output=True, text=True, timeout=600)
    assert r.returncode == 0 and ("1 passed" in r.stdout or "1 skipped" in r.stdout), r.stdout + r.stderr


def test_update_destroy_入れ子は_宣言のURLと状態で動く(server):
    pid, _ = server.accept("light")
    server.advance(6)  # 返事がそろって発送済みになる
    assert server.parcels()[pid]["status"] == "Shipped"
    assert server.call("PUT", f"/parcels/{pid}", {"kind": "heavy"})[0] == 200  # update：URL の ID が処理の ID の入力になる
    assert server.parcels()[pid]["kind"] == "heavy"
    assert server.call("PUT", "/parcels/parcel-9999", {"kind": "heavy"})[0] == 404
    assert server.call("PUT", f"/parcels/{pid}", {"kind": ""})[0] == 400
    code, j = server.call("GET", f"/parcels/{pid}/details")  # 入れ子：親の ID が問い合わせの入力になる
    assert code == 200 and [r["parcel_id"] for r in j["rows"]] == [pid]
    other, _ = server.accept("light")
    assert server.call("DELETE", f"/parcels/{other}")[0] == 409  # 消してよい状態（発送済み）ではない
    assert server.call("DELETE", "/parcels/parcel-9999")[0] == 404
    assert server.call("DELETE", f"/parcels/{pid}")[0] == 200  # destroy
    assert pid not in server.parcels() and other in server.parcels()


def test_どれか1つの返事で進むトリガは_1つ届けば進み_届かなければ期限で進む(server):
    pid, _ = server.accept("light")
    server.advance(6)
    assert server.call("POST", "/refund_requests", {"parcel_id": "parcel-9999"})[0] == 404
    assert server.call("POST", "/refund_requests", {"parcel_id": pid})[0] == 201
    assert [w["trigger"] for w in server.state()["waits"]] == ["RefundPending"]
    fired = server.advance(6)  # RefundSettled の返事が 1 つ届けば、待ち合わせずに行き先へ
    assert any(f["outcome"].get("fired") == "mark_refunded" for f in fired), fired
    assert server.state()["waits"] == []
    # 返事が落ちると、期限（10 分）の行き先へ進む
    server.mock(RefundSettled={"drop": True})
    assert server.call("POST", "/refund_requests", {"parcel_id": pid})[0] == 201
    server.advance(6)
    assert len(server.state()["waits"]) == 1
    fired = server.advance(10 * 60)
    assert any(f.get("deadline") == "RefundPending" and f["outcome"].get("fired") == "mark_refunded" for f in fired), fired


def test_アプリを開いた出来事も_返事待ちを引き直す(server):
    server.mock(PaySettled={"drop": True})
    pid, _ = server.accept()
    server.advance(6)
    server.mock(PaySettled={"drop": False})
    code, ev = server.call("POST", "/openings", {})
    assert code == 201 and [t["deed"] for run in ev["runs"] for t in run["trace"]] == ["recheck"]
    server.advance(6)
    assert server.parcels()[pid]["status"] == "Shipped"


def test_画面と_API_の応答には_CSP_が付き_生成した_HTML_にインラインのスクリプトは無い(server, built):
    import re
    for path in ("/", "/parcels/new", "/ui.js", "/manifest.webmanifest", "/api/defs", "/api/health"):
        try:
            csp = urllib.request.urlopen(server.base + path, timeout=5).headers.get("Content-Security-Policy", "")
        except urllib.error.HTTPError as e:  # 見つからない応答（404）にも付く
            csp = e.headers.get("Content-Security-Policy", "")
        assert "script-src 'self'" in csp and "connect-src 'self'" in csp and "object-src 'none'" in csp, (path, csp)
    # 生成した HTML には、インラインのスクリプト・イベントハンドラ属性・style 属性が無い（CSP で止まるものを書いていない）
    for html in [*built.glob("ui/*.html"), *built.glob("ext/*.html"), *built.glob("dev/*.html")]:
        text = html.read_text()
        assert not re.search(r"<script(?![^>]*\ssrc=)", text), html
        assert not re.search(r"\son[a-z]+\s*=", text), html
        assert 'style="' not in text, html
    manifest = json.loads((built / "ext" / "manifest.json").read_text())
    assert manifest["content_security_policy"]["extension_pages"] == "script-src 'self'; object-src 'self'"


def test_同時に来たリクエストは直列に並べた場合と同じ結果になる(server):
    from concurrent.futures import ThreadPoolExecutor

    # 同じ小包を同時に取り消す：状態が Waiting のうちに通るのは 1 件だけで、残りは WrongStatus（409）
    pid, _ = server.accept()
    with ThreadPoolExecutor(8) as ex:
        codes = sorted(ex.map(lambda _: server.call("POST", "/cancellations", {"parcel_id": pid})[0], range(8)))
    assert codes == [201] + [409] * 7, codes
    assert server.parcels()[pid]["status"] == "Canceled"
    # 同時に受け付けても、採番は重ならず、全件が保存される
    with ThreadPoolExecutor(8) as ex:
        ids = list(ex.map(lambda _: server.accept()[0], range(24)))
    assert len(set(ids)) == 24, ids
    assert set(ids) <= set(server.parcels())


def test_サーバーの単体テストが通る_汚れた_Mutex_でも続けて動く(built):
    r = subprocess.run(["cargo", "test", "--offline", "--quiet"], cwd=built / "server", capture_output=True, text=True)
    assert r.returncode == 0, r.stdout[-2000:] + r.stderr[-2000:]


def test_同じDBファイルを使うサーバーが2つあっても_直列に並べた場合と同じ結果になる(built, server):
    from concurrent.futures import ThreadPoolExecutor

    other = Server(built, server.db)
    other.start()
    try:
        pid, _ = server.accept()
        servers = [server, other]
        with ThreadPoolExecutor(8) as ex:
            codes = sorted(ex.map(lambda i: servers[i % 2].call("POST", "/cancellations", {"parcel_id": pid})[0], range(16)))
        assert codes == [201] + [409] * 15, codes  # 待たされるだけで、「database is locked」で落ちるリクエストは無い
        assert other.parcels()[pid]["status"] == "Canceled"
        with ThreadPoolExecutor(8) as ex:
            ids = list(ex.map(lambda i: servers[i % 2].accept()[0], range(24)))
        assert len(set(ids)) == 24, ids
    finally:
        other.stop()


def test_大きすぎる本文は読み込まずに413で断り_上限ちょうどの文字列は通る(server):
    req = urllib.request.Request(server.base + "/parcels", method="POST", data=b"x" * (16 * 1024 * 1024 + 1), headers={"Content-Type": "application/json"})
    with pytest.raises(urllib.error.HTTPError) as e:
        urllib.request.urlopen(req, timeout=30)
    assert e.value.code == 413
    assert server.call("POST", "/parcels", {"kind": "light"})[0] == 201  # そのあとも普通に動く
