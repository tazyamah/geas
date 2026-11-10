"""外部作用のファイル操作（over file）の動作テスト：ノートの見本（tests/fixtures/file_demo）から、サーバーまで生成して組み、実際のフォルダに書く。
宣言どおりのファイルができること、すでにあれば書かないこと、場所が無ければ結果の種類に結ばれること、OS の書式でない場所は入力不正で止まることを確かめる。"""
import json
import os
import shutil
import socket
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "tests" / "fixtures" / "file_demo" / "file_demo.geas"

pytestmark = pytest.mark.skipif(shutil.which("cargo") is None, reason="cargo が無い")


@pytest.fixture(scope="module")
def built(tmp_path_factory):
    out = tmp_path_factory.mktemp("file_demo")
    subprocess.run([sys.executable, "-m", "geas", "rust", str(SRC), "-o", str(out), "--host", "--runtime"], check=True, capture_output=True, text=True, cwd=ROOT)
    assert "ureq" not in (out / "server" / "Cargo.toml").read_text()  # over http を書かなければ、通信の部品は入らない
    r = subprocess.run(["cargo", "build", "--release", "--offline", "--quiet"], cwd=out / "server", capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    t = subprocess.run(["cargo", "test", "--release", "--offline", "--quiet", "fileio::"], cwd=out / "server", capture_output=True, text=True)
    assert t.returncode == 0 and "3 passed" in t.stdout, (t.stdout + t.stderr)[-2500:]  # fileio の単体試験（engine の単体試験は、runtime_demo の小包に固有なので走らせない）
    return out


class App:
    def __init__(self, built, tmp_path):
        s = socket.socket()
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
        s.close()
        self.base = f"http://127.0.0.1:{port}"
        self.proc = subprocess.Popen([str(built / "server" / "target" / "release" / "geas_server"), "--port", str(port), "--db", str(tmp_path / "t.sqlite"),
                                      "--pwa", str(built / "ui"), "--test"], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
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

    def note(self, body="ひとこと"):
        return self.call("POST", "/notes", {"body": body})["body"]["note_id"]

    def row(self, nid):
        return next(r for r in self.call("GET", "/notes")["rows"] if r["note_id"] == nid)

    def wait_status(self, nid, want, secs=8):
        for _ in range(int(secs * 10)):
            if self.row(nid)["status"] == want:
                return True
            time.sleep(0.1)
        return False

    def stop(self):
        self.proc.terminate()
        self.proc.wait(timeout=10)


@pytest.fixture
def app(built, tmp_path):
    a = App(built, tmp_path)
    yield a
    a.stop()


@pytest.fixture
def out_dir(tmp_path):
    d = tmp_path / "out"
    d.mkdir()
    return d


def test_宣言した場所に_宣言したファイルが_できる(app, out_dir):
    nid = app.note("ファイルに書く")
    assert app.call("POST", "/note_exports", {"note_id": nid, "dir": str(out_dir)})["result"] == "Asked"
    assert app.wait_status(nid, "Exported")
    assert (out_dir / "note.txt").read_text(encoding="utf-8") == "ファイルに書く"
    assert app.row(nid)["size"] == len("ファイルに書く".encode("utf-8"))  # 書いたバイト数が、応答の中身になった
    assert sorted(p.name for p in out_dir.iterdir()) == ["note.txt"]  # 宣言していないファイルは作らない


def test_すでにあれば書かない(app, out_dir):
    (out_dir / "note.txt").write_text("元の中身", encoding="utf-8")
    nid = app.note("上書きされない")
    app.call("POST", "/note_exports", {"note_id": nid, "dir": str(out_dir)})
    assert app.wait_status(nid, "ExportFailed")  # exists → Refused
    assert (out_dir / "note.txt").read_text(encoding="utf-8") == "元の中身"


def test_場所が無ければ失敗になる(app, tmp_path):
    nid = app.note()
    app.call("POST", "/note_exports", {"note_id": nid, "dir": str(tmp_path / "nothing")})
    assert app.wait_status(nid, "ExportFailed")  # not_found → Refused
    assert not (tmp_path / "nothing").exists()  # 場所を作らない


def test_OSの書式でない場所は_入力不正で止まる(app, out_dir):
    nid = app.note()
    for bad in ["relative/dir", str(out_dir) + "/../x", "$HOME/x", ""]:
        r = app.call("POST", "/note_exports", {"note_id": nid, "dir": bad})
        assert r.get("result") != "Asked", (bad, r)
    assert app.row(nid)["status"] == "Draft" and not list(out_dir.iterdir())


def test_ospath_は_生成した変換と同じ本文():
    from geas.rustgen import OS_PATH_RS
    mine = (ROOT / "geas" / "server" / "src" / "ospath.rs").read_text(encoding="utf-8")
    assert OS_PATH_RS.replace("\nfn os_path_expand", "\npub fn os_path_expand", 1) in mine
