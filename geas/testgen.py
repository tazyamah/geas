# SPDX-License-Identifier: GPL-3.0-or-later
"""計画書の「URL の具体例」から、HTTP の通しテスト（pytest）を作る。
行は上から順に、1つの新しいサーバー（最初のデータ入り）に対して実行する。入力は実際に送る JSON で、
前の行の応答から「控える」で取った値を {名前} で経路と入力に差し込める。
生成するテストはアプリを知らず、データ（ROWS）と汎用の実行部だけでできている。"""
import json
import re
from pathlib import Path

from .plan import n, route_match
from .rest import routes

HEADER = '''# SPDX-License-Identifier: GPL-3.0-or-later
"""計画書の「URL の具体例」から geas が生成したテスト。手で直さない（計画書を直して作り直す）。
サーバーは、生成物の server/ をビルドしたもの（cargo build --release）を、新しい DB で起こす。
環境変数 GEAS_BASE_URL があれば、起こさずにそのサーバーに対して実行する。"""
import json
import os
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

import pytest

APP = Path(__file__).resolve().parent.parent

# (ID, メソッド, 経路, 入力, 期待する結果, 期待する状態, 期待する本文, 控える)
ROWS = %s


@pytest.fixture(scope="module")
def base():
    if os.environ.get("GEAS_BASE_URL"):
        yield os.environ["GEAS_BASE_URL"].rstrip("/")
        return
    exe = APP / "server" / "target" / "release" / "geas_server"
    if not exe.exists():
        pytest.skip("サーバーがビルドされていません（cd server && cargo build --release）")
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
    with tempfile.TemporaryDirectory() as work:
        proc = subprocess.Popen([str(exe), "--port", str(port), "--db", str(Path(work) / "app.sqlite"),
                                 "--pwa", str(APP / "ui"), "--dev", str(APP / "dev")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        url = f"http://127.0.0.1:{port}"
        for _ in range(100):
            try:
                urllib.request.urlopen(url + "/api/health", timeout=1)
                break
            except OSError:
                time.sleep(0.1)
        yield url
        proc.terminate()
        proc.wait(timeout=10)


@pytest.fixture(scope="module")
def vars_():
    return {}


def call(base, method, path, body):
    req = urllib.request.Request(base + path, method=method, data=None if body is None else json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=10) as r:
            return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read())


def fill(text, vars_):
    for k, v in vars_.items():
        text = text.replace("{" + k + "}", str(v))
    return text


def dig(j, path):
    for p in path.split("."):
        j = j[int(p)] if isinstance(j, list) else j[p]
    return j


def subset(want, got):
    if isinstance(want, dict):
        return isinstance(got, dict) and all(k in got and subset(v, got[k]) for k, v in want.items())
    if isinstance(want, list):
        return isinstance(got, list) and len(want) == len(got) and all(subset(a, b) for a, b in zip(want, got))
    return want == got


@pytest.mark.parametrize("row", ROWS, ids=[r[0] for r in ROWS])
def test_url_example(base, vars_, row):
    rid, method, path, body, result, status, expect, keep = row
    path = fill(path, vars_)
    code, j = call(base, method, path, None if body is None else json.loads(fill(body, vars_)))
    assert code == status, (rid, method, path, code, j)
    if result:
        assert j.get("result") == result, (rid, j)
    if expect is not None:
        assert subset(json.loads(fill(expect, vars_)), j), (rid, expect, j)
    for name, src in keep:
        vars_[name] = dig(j, src)
'''


def cell(r, *names):
    for k in names:
        if k in r:
            return r[k].strip().strip("`").strip()
    return ""


def url_rows(ck, plan: dict) -> list[tuple]:
    """計画書の URL 例の行を、テストに渡す形にする。結果の種類は、表示名でもコードの名前でもよく、コードの名前にして渡す"""
    p = ck.p
    rs = routes(p)
    out = []
    for r in plan.get("URL例", []):
        rt = route_match([(m, path) for m, path, *_ in rs], r["メソッド"], r["経路"])
        if rt is None:
            raise ValueError(f"URL例 {r.get('ID', '').strip()}: {r['メソッド'].strip()} {r['経路'].strip()} に当たるルートがありません")
        names = [v for m, path, _k, _t, _r, st in rs if (m, path) == rt for v, _ in st]
        res = p.resolve(cell(r, "期待する結果"), names) if names else ""
        if res in ("", "－", "-"):
            res = ""
        elif names and res not in names:
            raise ValueError(f"URL例 {r.get('ID', '').strip()}: 結果「{res}」は、ルート {rt[0]} {rt[1]} の結果にありません")
        keep = []
        for part in filter(None, (x.strip() for x in cell(r, "控える").split(";"))):
            m = re.fullmatch(r"([A-Za-z_]\w*)\s*=\s*(\S+)", part)
            if not m:
                raise ValueError(f"URL例 {r.get('ID', '').strip()}: 控え方「{part}」は 名前 = 応答の道すじ（例 order_id = body.order_id）で書きます")
            keep.append((m.group(1), m.group(2)))
        body = cell(r, "入力")
        expect = cell(r, "期待する本文")
        for label, text in (("入力", body), ("期待する本文", expect)):
            if text and text not in ("－", "-"):
                try:
                    json.loads(re.sub(r"\{[A-Za-z_]\w*\}", "x", text))
                except ValueError as e:
                    raise ValueError(f"URL例 {r.get('ID', '').strip()}: {label} が JSON ではありません（{e}）")
        out.append((r["ID"].strip(), r["メソッド"].strip(), r["経路"].strip(), body if body not in ("", "－", "-") else None,
                    res, int(r["期待する状態"].strip()), expect if expect not in ("", "－", "-") else None, keep))
    return out


def write_url_tests(ck, plan: dict, dest) -> Path | None:
    rows = url_rows(ck, plan)
    if not rows:
        return None
    out = Path(dest) / "tests"
    out.mkdir(parents=True, exist_ok=True)
    f = out / "test_url_examples.py"
    f.write_text(HEADER % ("[\n" + "".join(f"    {r!r},\n" for r in rows) + "]"))
    return f
