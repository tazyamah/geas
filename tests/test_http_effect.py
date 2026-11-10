"""外部作用の実通信（over http）の動作テスト：つぶやきの見本（tests/fixtures/solo_tweet）から、サーバーまで生成して組み、
手元の模擬の X（HTTP サーバー）に向けて動かす。宣言した形で送られること（メソッド・認証・JSON の本文）、状態が結果の種類に結ばれること、
シークレットが無いときは送らないこと、リダイレクトに従わないことを確かめる。cargo が無い環境では飛ばす。"""
import json
import os
import shutil
import socket
import subprocess
import sys
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "tests" / "fixtures" / "solo_tweet" / "solo_tweet.geas"

pytestmark = pytest.mark.skipif(shutil.which("cargo") is None, reason="cargo が無い")


@pytest.fixture(scope="module")
def built(tmp_path_factory):
    out = tmp_path_factory.mktemp("solo_tweet")
    subprocess.run([sys.executable, "-m", "geas", "rust", str(SRC), "-o", str(out), "--host", "--runtime"], check=True, capture_output=True, text=True, cwd=ROOT)
    assert "ureq" in (out / "server" / "Cargo.toml").read_text()  # over http を宣言したアプリだけに、通信の部品が入る
    r = subprocess.run(["cargo", "build", "--release", "--offline", "--quiet"], cwd=out / "server", capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    return out


class FakeX:
    """手元の模擬の X。来た要求を記録し、決めた状態で返す"""

    def __init__(self):
        self.seen, self.status, self.redirect_to = [], 201, None
        outer = self

        class H(BaseHTTPRequestHandler):
            def do_POST(self):
                body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
                outer.seen.append({"path": self.path, "auth": self.headers.get("Authorization"), "type": self.headers.get("Content-Type"), "body": json.loads(body or b"{}")})
                if outer.redirect_to:
                    self.send_response(302)
                    self.send_header("Location", outer.redirect_to)
                    self.end_headers()
                    return
                out = json.dumps({"data": {"id": "1999", "text": "x"}} if outer.status == 201 else {"title": "Forbidden"}).encode()
                self.send_response(outer.status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(out)))
                self.end_headers()
                self.wfile.write(out)

            def log_message(self, *a):
                pass

        self.httpd = HTTPServer(("127.0.0.1", 0), H)
        self.url = f"http://127.0.0.1:{self.httpd.server_address[1]}/2/tweets"
        threading.Thread(target=self.httpd.serve_forever, daemon=True).start()

    def stop(self):
        self.httpd.shutdown()


@pytest.fixture
def x():
    f = FakeX()
    yield f
    f.stop()


class App:
    def __init__(self, built, tmp_path, x_url, token="tok-123"):
        s = socket.socket()
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
        s.close()
        env = {k: v for k, v in os.environ.items() if not k.startswith("GEAS_SECRET_")}
        if token:
            env["GEAS_SECRET_TWITTER_TOKEN"] = token
        self.base = f"http://127.0.0.1:{port}"
        self.proc = subprocess.Popen([str(built / "server" / "target" / "release" / "geas_server"), "--port", str(port), "--db", str(tmp_path / "t.sqlite"),
                                      "--pwa", str(built / "ui"), "--test", "--effect-url", f"Twitter.post={x_url}"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        for _ in range(100):
            try:
                urllib.request.urlopen(self.base + "/api/health", timeout=1)
                return
            except OSError:
                time.sleep(0.1)
        raise RuntimeError("サーバーが起きません")

    def call(self, method, path, body=None):
        req = urllib.request.Request(self.base + path, method=method, data=json.dumps(body).encode() if body is not None else None, headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=10) as r:
                return json.loads(r.read())
        except urllib.error.HTTPError as e:
            return json.loads(e.read())

    def tweet(self, body="こんにちは"):
        return self.call("POST", "/tweets", {"body": body})["body"]["tweet_id"]

    def row(self, tid):
        return next(r for r in self.call("GET", "/tweets")["rows"] if r["tweet_id"] == tid)

    def wait_status(self, tid, want, secs=8):
        for _ in range(int(secs * 10)):
            if self.row(tid)["status"] == want:
                return True
            time.sleep(0.1)
        return False

    def stop(self):
        self.proc.terminate()
        self.proc.wait(timeout=10)


@pytest.fixture
def app(built, tmp_path, x):
    a = App(built, tmp_path, x.url)
    yield a
    a.stop()


def test_宣言どおりに送られ_201は投稿済みになる(app, x):
    tid = app.tweet("宣言どおり")
    assert app.call("POST", "/x_postings", {"tweet_id": tid})["result"] == "Asked"
    assert app.wait_status(tid, "Posted")
    assert x.seen == [{"path": "/2/tweets", "auth": "Bearer tok-123", "type": "application/json", "body": {"text": "宣言どおり"}}]  # 本文のキーは宣言どおり（text）
    detail = app.call("GET", f"/tweets/{tid}/details")["rows"][0]
    assert detail["x_ref"] == "1999"  # 返事の data.id が、応答の中身になった


def test_4xxは拒否として結ばれ_失敗になる(app, x):
    x.status = 403
    tid = app.tweet()
    app.call("POST", "/x_postings", {"tweet_id": tid})
    assert app.wait_status(tid, "PostFailed")
    assert len(x.seen) == 1


def test_シークレットが無ければ送らず_返事を待つ先の期限で失敗になる(built, tmp_path, x):
    a = App(built, tmp_path, x.url, token=None)
    try:
        tid = a.tweet()
        a.call("POST", "/x_postings", {"tweet_id": tid})
        time.sleep(1.5)
        assert x.seen == [] and a.row(tid)["status"] == "Posting"  # 認証が無いまま送らない
        a.call("POST", "/api/test/advance", {"secs": 130})  # 返事が来ないまま、トリガの期限（2 分）
        assert a.wait_status(tid, "PostFailed")
    finally:
        a.stop()


def test_リダイレクトには従わない(app, x):
    other = FakeX()
    try:
        x.redirect_to = other.url
        tid = app.tweet()
        app.call("POST", "/x_postings", {"tweet_id": tid})
        assert app.wait_status(tid, "PostFailed")  # 3xx は status other → Rejected
        time.sleep(0.5)
        assert other.seen == []  # 宣言していない先へは出ない
    finally:
        other.stop()


def test_試験用の通信先の差し替えは_test_なしでは開かない(built, tmp_path):
    r = subprocess.run([str(built / "server" / "target" / "release" / "geas_server"), "--port", "0", "--db", str(tmp_path / "z.sqlite"), "--pwa", str(built / "ui"),
                        "--effect-url", "Twitter.post=http://127.0.0.1:1/"], capture_output=True, text=True, timeout=20)
    assert r.returncode == 2 and "--test" in r.stderr
