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
    def __init__(self, built, db, extra=(), test_api=True):
        """test_api：テスト用の口（/api/test/…）を開く --test を付けて起こすか"""
        self.built, self.db, self.proc, self.base, self.extra = built, db, None, None, list(extra)
        self.test_api = test_api

    def start(self):
        s = socket.socket()
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
        s.close()
        self.proc = subprocess.Popen(
            [str(self.built / "server" / "target" / "release" / "geas_server"), "--port", str(port), "--db", str(self.db),
             "--pwa", str(self.built / "ui"), *(["--test"] if self.test_api else []), "--dev", str(self.built / "dev"), *self.extra],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
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


def test_最初のデータは_DB_が空のときだけ読む(built, tmp_path):
    """行のある DB で起こすときは、最初のデータ（埋め込みか --seed）を読まない（大きいと、起動のたびに時間とメモリを使う）。
    読めない最初のデータを渡しても、行のある DB なら起動し、空の DB なら入れられずに止まる"""
    broken = tmp_path / "broken_seed.json"
    broken.write_text("{ JSON ではない", encoding="utf-8")
    s = Server(built, tmp_path / "seeded.sqlite")
    s.start()
    pid, _ = s.accept()
    s.stop()
    s.extra = ["--seed", str(broken)]
    s.start()
    try:
        assert list(s.parcels()) == [pid]
    finally:
        s.stop()
    r = subprocess.run([str(built / "server" / "target" / "release" / "geas_server"), "--port", "0", "--db", str(tmp_path / "empty.sqlite"), "--seed", str(broken)],
                       capture_output=True, text=True, timeout=60)
    assert r.returncode != 0 and "seed.json" in r.stderr, r.stderr[-1000:]


def test_型の範囲の外の行は_最初のデータからも_DBからも入らない(built, tmp_path):
    """検査器は「保存の値は型の範囲に入っている」を前提に証明する。だから保存の行も、読むときに型の範囲で確かめる。
    範囲の外の最初のデータは入れずに止まり（DB は空のまま）、外で書き換えた DB の行は、起動のときに止まる"""
    import json
    import sqlite3

    exe = str(built / "server" / "target" / "release" / "geas_server")
    row = {"parcel_id": "p-1", "kind": "light", "fee": 300, "status": "Received", "refund_ref": None}
    for bad, word in (({"fee": -1}, "Parcel.fee"), ({"kind": "x" * 11}, "Parcel.kind")):
        seed = tmp_path / "bad_seed.json"
        seed.write_text(json.dumps({"Parcel": {"p-1": {**row, **bad}}}), encoding="utf-8")
        db = tmp_path / f"bad_{word}.sqlite"
        r = subprocess.run([exe, "--port", "0", "--db", str(db), "--seed", str(seed)], capture_output=True, text=True, timeout=60)
        assert r.returncode != 0 and word in r.stderr and "範囲外" in r.stderr, r.stderr[-1000:]
        assert sqlite3.connect(db).execute('SELECT count(*) FROM "Parcel"').fetchone()[0] == 0, "範囲の外の行が DB に入っている"
    # 行を置くキーと、行の中の ID が違う最初のデータも入らない
    seed = tmp_path / "key_seed.json"
    seed.write_text(json.dumps({"Parcel": {"p-2": row}}), encoding="utf-8")
    r = subprocess.run([exe, "--port", "0", "--db", str(tmp_path / "key.sqlite"), "--seed", str(seed)], capture_output=True, text=True, timeout=60)
    assert r.returncode != 0 and "キー「p-2」と、行の中の ID「p-1」が違います" in r.stderr, r.stderr[-1000:]
    # 範囲の中の行は入る
    seed = tmp_path / "good_seed.json"
    seed.write_text(json.dumps({"Parcel": {"p-1": row}}), encoding="utf-8")
    s = Server(built, tmp_path / "good.sqlite", extra=["--seed", str(seed)])
    s.start()
    try:
        assert s.parcels()["p-1"]["fee"] == 300
    finally:
        s.stop()
    # DB の行を外から書き換えると、次の起動で止まる
    with sqlite3.connect(s.db) as c:
        c.execute("""UPDATE "Parcel" SET v = replace(v, '"fee":300', '"fee":-5')""")
    r = subprocess.run([exe, "--port", "0", "--db", str(s.db)], capture_output=True, text=True, timeout=60)
    assert r.returncode != 0 and "Parcel.fee=-5 は範囲外" in r.stderr, r.stderr[-1000:]


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


def test_テスト用の口と_宣言を通らない口は__test_を付けたときだけ開く(built, tmp_path):
    """--test なしで外から呼べるのは、仕様書の URL の表にある口と、画面が使う /api/defs・/api/health だけ。
    時計を進める・模擬を変える・処理を名前で呼ぶ（resource の宣言を通らない）口は 404"""
    s = Server(built, tmp_path / "plain.sqlite", test_api=False)
    s.start()
    try:
        for method, path, body in [("GET", "/api/test/state", None), ("POST", "/api/test/advance", {"secs": 3600}), ("POST", "/api/test/settings", {"delay_secs": 0}),
                                   ("POST", "/api/test/tick", {}), ("GET", "/api/queries/parcel_list", None), ("POST", "/api/deeds/accept", {"input": {"kind": "light"}}),
                                   ("POST", "/api/events/Reconnected", {}), ("GET", "/test.html", None), ("GET", "/test.js", None)]:
            assert s.call(method, path, body)[0] == 404, (method, path)
        assert s.call("GET", "/parcels")[1]["rows"] == []  # 名前で呼ぶ口からは、何も作られていない
        assert s.call("POST", "/parcels", {"kind": "light"})[0] == 201
        assert s.call("GET", "/api/defs")[0] == 200 and s.call("GET", "/api/health")[0] == 200
        assert urllib.request.urlopen(s.base + "/", timeout=5).status == 200
    finally:
        s.stop()


def test_流れの途中の処理の_URL_は__test_なしでは答えず_流れの中では動く(built, tmp_path):
    """仕様書の URL の表で「内部」の URL（返事・期限・前の処理の結果から進む処理）は、外から呼ぶと 404。
    呼べると、決済の返事を待たずに先へ進められる。流れの中（返事がそろったら settle → ship）では、ふつうに動く"""
    s = Server(built, tmp_path / "inner.sqlite", test_api=False)
    s.start()
    try:
        pid, _ = s.accept()
        for path in ("/settlements", "/cancellations"):
            code, j = s.call("POST", path, {"parcel_id": pid})
            assert code == 404 and "result" not in j, (path, j)  # 処理の結果（NoParcel など）ではなく、口が無い
        assert s.parcels()[pid]["status"] == "Waiting"  # 外からは進められない
        until = time.time() + 20
        while s.parcels()[pid]["status"] == "Waiting" and time.time() < until:  # 模擬の返事（既定で 1 秒後）がそろうのを待つ
            time.sleep(0.2)
        assert s.parcels()[pid]["status"] == "Shipped"  # 流れの中では settle → ship が動く
    finally:
        s.stop()


def test__test_を付けると開発用の模擬パネルとテスト用の口が開く(server):
    assert urllib.request.urlopen(server.base + "/test.html", timeout=5).status == 200
    assert server.call("GET", "/api/test/state")[0] == 200


def test_POST_は_JSON_と名乗るものだけを受け_UTF8_でない本文は断る(server):
    """別のサイトのページは、フォームや text/plain の POST なら事前の確認なしに送らせられる（CSRF）。JSON と名乗る POST だけを受ける"""
    body = json.dumps({"kind": "light"}).encode()
    for ct in ("text/plain", "application/x-www-form-urlencoded", "multipart/form-data; boundary=x", "application/jsonp"):
        req = urllib.request.Request(server.base + "/parcels", method="POST", data=body, headers={"Content-Type": ct})
        with pytest.raises(urllib.error.HTTPError) as e:
            urllib.request.urlopen(req, timeout=5)
        assert e.value.code == 415, ct
    assert server.parcels() == {}  # 断った要求では、何も作られていない
    req = urllib.request.Request(server.base + "/parcels", method="POST", data=body, headers={"Content-Type": "Application/JSON; charset=utf-8"})
    assert urllib.request.urlopen(req, timeout=5).status == 201
    req = urllib.request.Request(server.base + "/parcels", method="POST", data=b'{"kind": "\xff"}', headers={"Content-Type": "application/json"})
    with pytest.raises(urllib.error.HTTPError) as e:
        urllib.request.urlopen(req, timeout=5)
    assert e.value.code == 400 and "UTF-8" in json.loads(e.value.read())["error"]
    assert len(server.parcels()) == 1
    for path in ("/api/health", "/", "/ui.js"):
        h = urllib.request.urlopen(server.base + path, timeout=5).headers
        assert h.get("X-Content-Type-Options") == "nosniff" and h.get("Referrer-Policy") == "same-origin", path


def test_Host_が手元の名前でも足した名前でもなければ断る(built, tmp_path):
    """悪意のあるページが DNS の付け替え（DNS rebinding）で 127.0.0.1 に届いても、ブラウザは Host にそのページの名前を書く。
    その名前の要求は、読むのも書くのも断る。localhost・IP の書き方・--allow-host で足した名前は受ける"""
    s = Server(built, tmp_path / "host.sqlite", ["--allow-host", "shop.example"])
    s.start()
    try:
        def send(method, path, host, body=None):
            req = urllib.request.Request(s.base + path, method=method, data=None if body is None else json.dumps(body).encode(),
                                         headers={"Content-Type": "application/json", "Host": host})
            try:
                with urllib.request.urlopen(req, timeout=5) as r:
                    return r.status
            except urllib.error.HTTPError as e:
                return e.code
        port = s.base.rsplit(":", 1)[1]
        for host in (f"evil.example:{port}", f"127.0.0.1.evil.example:{port}"):
            assert send("GET", "/parcels", host) == 403, host
            assert send("POST", "/parcels", host, {"kind": "light"}) == 403, host
            assert send("GET", "/api/defs", host) == 403, host
        assert s.call("GET", "/parcels")[1]["rows"] == []  # 断った要求では、何も作られていない
        for host in (f"localhost:{port}", f"127.0.0.1:{port}", f"[::1]:{port}", "shop.example", f"SHOP.example:{port}"):
            assert send("GET", "/parcels", host) == 200, host
        assert send("POST", "/parcels", "shop.example", {"kind": "light"}) == 201
    finally:
        s.stop()


def test_期限の待ちが1件失敗し続けても_ほかの期限は発火する(server):
    """発火に失敗する待ち（ここでは、無いトリガの待ち）が先にあっても、うしろの待ちの期限は止まらない"""
    import sqlite3
    with sqlite3.connect(server.db) as c:
        c.execute("INSERT INTO geas_waits (trigger, args, collected, deadline) VALUES ('Nowhere', '{}', '{}', 0)")
    server.mock(PaySettled={"drop": True})
    pid, _ = server.accept()
    server.advance(6)
    fired = server.advance(30 * 60 + 10)
    assert any(f.get("deadline") == "Nowhere" and "error" in f for f in fired), fired
    assert any(f.get("deadline") == "Pending" and f["outcome"].get("fired") == "expire" for f in fired), fired
    assert server.parcels()[pid]["status"] == "Canceled"


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


def test_同じDBファイルを使うサーバーが2つあっても_期限の発火は待ち1件につき1回だけになる(built, server):
    import sqlite3
    from concurrent.futures import ThreadPoolExecutor

    n = 40
    server.mock(PaySettled={"drop": True}, DepotSettled={"drop": True})
    other = Server(built, server.db)
    other.start()
    try:
        pids = [server.accept()[0] for _ in range(n)]
        for pid in pids:
            assert server.call("POST", "/nudge_requests", {"parcel_id": pid})[0] == 201
        # 時計は 1 台ごとに進むので、2 台を同時に期限の先へ進める。期限の一覧を両方が読む
        with ThreadPoolExecutor(2) as ex:
            list(ex.map(lambda s: s.advance(10 * 60 + 30), [server, other]))
        time.sleep(3)  # ticker（300ms ごと）が両方の台で回るのを待つ
        # /api/test/state の effects は件数が切れるので、DB を直接数える
        keys = [k for (k,) in sqlite3.connect(server.db).execute("SELECT idem_key FROM geas_effects WHERE name = 'Courier.send'")]
        assert sorted(keys) == sorted(pids), f"期限の発火は {n} 回のはずが {len(keys)} 回"
    finally:
        other.stop()


def test_大きすぎる本文は読み込まずに413で断り_上限ちょうどの文字列は通る(server):
    req = urllib.request.Request(server.base + "/parcels", method="POST", data=b"x" * (16 * 1024 * 1024 + 1), headers={"Content-Type": "application/json"})
    with pytest.raises(urllib.error.HTTPError) as e:
        urllib.request.urlopen(req, timeout=30)
    assert e.value.code == 413
    assert server.call("POST", "/parcels", {"kind": "light"})[0] == 201  # そのあとも普通に動く


def test_workerの本数を指定でき_範囲外の指定は起動前に断る(built, tmp_path):
    one = Server(built, tmp_path / "one.sqlite", ["--workers", "1"])
    one.start()
    try:
        for _ in range(3):  # worker が 1 本でも、続けて処理できる
            assert one.call("POST", "/parcels", {"kind": "light"})[0] == 201
    finally:
        one.stop()
    exe = str(built / "server" / "target" / "release" / "geas_server")
    for bad in ("0", "257", "many"):
        r = subprocess.run([exe, "--workers", bad, "--db", str(tmp_path / "bad.sqlite")], capture_output=True, text=True, timeout=30)
        assert r.returncode == 2 and "--workers" in r.stderr, (bad, r.returncode, r.stderr)


def test_サーバーのログを_geas_trace_で仕様書の行に結べる(built, tmp_path):
    """構造化ログ（本文は載らない）が、仕様書のルート表の行・処理・結果の種類に戻れる。仕様書にない経路・状態は「結べなかったもの」に出る"""
    log = tmp_path / "server.log"
    with open(log, "w") as f:
        # Server.start は stderr を捨てるので、ここでは同じ起動をログつきで行う
        s = socket.socket()
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
        s.close()
        proc = subprocess.Popen([str(built / "server" / "target" / "release" / "geas_server"), "--port", str(port), "--db", str(tmp_path / "t.sqlite"),
                                 "--pwa", str(built / "ui"), "--dev", str(built / "dev")], stdout=subprocess.DEVNULL, stderr=f)
        base = f"http://127.0.0.1:{port}"
        try:
            for _ in range(100):
                try:
                    urllib.request.urlopen(base + "/api/health", timeout=1)
                    break
                except OSError:
                    time.sleep(0.1)
            req = urllib.request.Request(base + "/parcels", method="POST", data=json.dumps({"kind": "light", "secret_note": "個人の値"}).encode(), headers={"Content-Type": "application/json"})
            urllib.request.urlopen(req, timeout=10).read()
        finally:
            proc.kill()
            proc.wait()
    text = log.read_text()
    assert '"log":"geas.route"' in text and "個人の値" not in text
    r = subprocess.run([sys.executable, "-m", "geas", "trace", str(SRC), str(log)], capture_output=True, text=True, cwd=ROOT)
    assert r.returncode == 0 and "POST" in r.stdout and "/parcels" in r.stdout and "仕様書に結べたもの" in r.stdout, r.stdout + r.stderr
    bad = tmp_path / "bad.log"
    bad.write_text('{"log":"geas.route","ts_ms":1,"method":"GET","route":"/nowhere","target":"x","result":null,"status":200,"ms":1}\n')
    r = subprocess.run([sys.executable, "-m", "geas", "trace", str(SRC), str(bad)], capture_output=True, text=True, cwd=ROOT)
    assert r.returncode == 1 and "ルート表にない経路" in r.stdout


def test_保存はメモリに持つが_別のサーバーやDBを直接書き換えた内容は次のリクエストで見える(built, server):
    """サーバーは保存をメモリに持ち、処理が変えた行だけを DB に書く。別の接続（同じ DB を使う別のサーバー・人の手）が
    書いたことは DB の data_version で分かり、そのときだけ読み直す。古い内容で上書きしない"""
    import sqlite3

    other = Server(built, server.db)
    other.start()
    try:
        pid, _ = server.accept("light")
        assert other.parcels()[pid]["kind"] == "light"  # 別のサーバーが作った行が見える
        assert other.call("PUT", f"/parcels/{pid}", {"kind": "heavy"})[0] == 200
        assert server.parcels()[pid]["kind"] == "heavy"  # 別のサーバーが書き換えた行が見える
        db = sqlite3.connect(server.db)
        row = json.loads(db.execute("SELECT v FROM Parcel WHERE k = ?", (pid,)).fetchone()[0])
        db.execute("UPDATE Parcel SET v = ? WHERE k = ?", (json.dumps(dict(row, kind="medium")), pid))
        db.commit()
        db.close()
        assert server.parcels()[pid]["kind"] == "medium"  # DB を直接書き換えても見える
        assert server.call("PUT", f"/parcels/{pid}", {"kind": "small"})[0] == 200
        got = other.parcels()[pid]
        assert (got["kind"], got["fee"]) == ("small", row["fee"])
        # DB にも、処理が変えた行の新しい内容が書かれている
        db = sqlite3.connect(server.db)
        stored = json.loads(db.execute("SELECT v FROM Parcel WHERE k = ?", (pid,)).fetchone()[0])
        db.close()
        assert (stored["kind"], stored["fee"]) == ("small", row["fee"])
    finally:
        other.stop()


def test_1KBを超える応答も_書き込みの待ちなしで返る(server):
    """tiny_http の respond は 1KB ずつ書くので、1KB を超える応答は 2 回目の書き込みが Nagle と相手の遅延 ACK に挟まれ、
    毎回 40ms ほど待っていた。応答は 1 回の書き込みで送る"""
    import http.client

    for _ in range(40):
        server.accept("light")
    conn = http.client.HTTPConnection("127.0.0.1", int(server.base.rsplit(":", 1)[1]), timeout=10)
    times = []
    for _ in range(10):
        t = time.perf_counter()
        conn.request("GET", "/parcels")
        r = conn.getresponse()
        body = r.read()
        times.append(time.perf_counter() - t)
    conn.close()
    assert r.status == 200 and len(body) > 2048 and len(json.loads(body)["rows"]) == 40
    assert sorted(times)[len(times) // 2] < 0.02, [round(x * 1000, 1) for x in times]


def test_問い合わせの応答には版が付き_保存が変わっていなければ304で本文を送らない(built, server):
    """画面は、見せている一覧を 2 秒ごとに引き直す。保存が変わっていなければ（処理が行を書いていない・別の接続も書いていない）、
    サーバーは問い合わせを実行せずに 304 を返す（変わっていない一覧を毎回作って送らない）。版は URL ごとに違う"""
    import http.client
    import sqlite3

    def get(path, tag=None, accept="application/json"):
        conn = http.client.HTTPConnection("127.0.0.1", int(server.base.rsplit(":", 1)[1]), timeout=10)
        conn.request("GET", path, headers={"Accept": accept, **({"If-None-Match": tag} if tag else {})})
        r = conn.getresponse()
        body = r.read()
        conn.close()
        return r.status, r.getheader("ETag"), body

    pid, _ = server.accept("light")
    code, tag, body = get("/parcels")
    assert code == 200 and tag and pid in {p["parcel_id"] for p in json.loads(body)["rows"]}
    assert get("/parcels", tag) == (304, tag, b"")  # 変わっていない：本文なし
    code, other, _ = get(f"/parcels/{pid}/details")
    assert code == 200 and other != tag and get(f"/parcels/{pid}/details", tag)[0] == 200  # 別の URL の版では 304 にならない
    code, _, page = get(f"/parcels/{pid}/details", other, accept="text/html")  # 画面の URL を開く（殻の HTML）ときは、版を見ない
    assert code == 200 and page.lstrip().lower().startswith(b"<!doctype html")
    server.accept("heavy")  # 処理が行を書いた
    code, tag2, body = get("/parcels", tag)
    assert code == 200 and tag2 != tag and len(json.loads(body)["rows"]) == 2
    assert get("/parcels", tag2)[0] == 304
    db = sqlite3.connect(server.db)  # 別の接続（人の手・別のサーバー）が書いた
    row = json.loads(db.execute("SELECT v FROM Parcel WHERE k = ?", (pid,)).fetchone()[0])
    db.execute("UPDATE Parcel SET v = ? WHERE k = ?", (json.dumps(dict(row, kind="medium")), pid))
    db.commit()
    db.close()
    code, tag3, body = get("/parcels", tag2)
    assert code == 200 and tag3 != tag2 and {p["parcel_id"]: p["kind"] for p in json.loads(body)["rows"]}[pid] == "medium"
    server.stop()  # 再起動したら、前の版では 304 にならない（DB が止まっている間に書き換えられていても、古い一覧を使わせない）
    server.start()
    assert get("/parcels", tag3)[0] == 200


def test_test_なしの_api_defs_は_画面が使う分だけを返す(built, tmp_path):
    """--test なしの /api/defs は、画面と外から呼べる URL だけ。流れの途中の処理・処理の一覧・外部作用・模擬の設定は出さない。
    --test のときは、開発用のパネルと通しの試験が使うので全部を返す"""
    s = Server(built, tmp_path / "plain.sqlite", test_api=False)
    s.start()
    try:
        defs = s.call("GET", "/api/defs")[1]
        assert sorted(defs) == ["routes", "screens"]
        assert defs["routes"] and not any(r.get("internal") for r in defs["routes"])
    finally:
        s.stop()
    t = Server(built, tmp_path / "test.sqlite")
    t.start()
    try:
        full = t.call("GET", "/api/defs")[1]
        assert {"deeds", "effects", "mock", "flows"} <= set(full) and any(r.get("internal") for r in full["routes"])
    finally:
        t.stop()


def test_HEAD_は_GET_と同じ状態で_本文なしで答える(built, tmp_path):
    s = Server(built, tmp_path / "head.sqlite", test_api=False)
    s.start()
    try:
        with urllib.request.urlopen(urllib.request.Request(s.base + "/parcels", method="HEAD"), timeout=5) as r:
            assert r.status == 200 and r.read() == b""
    finally:
        s.stop()


def test_保存の行が今の定義で読めない_DB_では_起動のときに理由を言って止まる(built, tmp_path):
    """.geas の実体の項目を変えたあとに、前の版の DB を開いた場合にあたる。起動してから全部のリクエストが失敗するのでなく、起動で止まる"""
    import sqlite3
    db = tmp_path / "old.sqlite"
    s = Server(built, db, test_api=False)
    s.start()
    try:
        assert s.call("POST", "/parcels", {"kind": "light"})[0] == 201
    finally:
        s.stop()
    con = sqlite3.connect(db)
    table = next(t for (t,) in con.execute("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'geas_%' AND name NOT LIKE 'sqlite_%'")
                 if con.execute(f'SELECT count(*) FROM "{t}"').fetchone()[0])
    con.execute(f'UPDATE "{table}" SET v = ?', ("{}",))  # 項目の無い行（前の版の形のつもり）
    con.commit()
    con.close()
    r = subprocess.run([str(built / "server" / "target" / "release" / "geas_server"), "--port", "0", "--db", str(db), "--pwa", str(built / "ui")],
                       capture_output=True, text=True, encoding="utf-8", timeout=30)
    assert r.returncode == 1 and "今の定義と合いません" in r.stderr and "panicked" not in r.stderr, r.stderr
