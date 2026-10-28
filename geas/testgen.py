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


# ---- 画面の具体例：ユースケースの手順（画面・操作・結果）と「画面の具体例」の表から、画面の通しテスト（Playwright）を作る ----

SCREEN_SEP = re.compile(r"\s*[;；]\s*")


def _kv(text: str) -> list[tuple[str, str]]:
    """「名前 = 値; 名前 = 値」を組にする（名前だけの項目は値が空）"""
    out = []
    for part in SCREEN_SEP.split(text.strip().strip("`")):
        part = part.strip()
        if not part or part in ("－", "-"):
            continue
        m = re.search(r"[=:：]", part)  # 最初に出てくる区切りで、名前と値に分ける
        k, v = (part[:m.start()], part[m.end():]) if m else (part, "")
        out.append((k.strip(), v.strip().strip("`")))
    return out


def _find(items, label: str, key="label"):
    """人が書いた名前（表示名でもコードの名前でもよい）で、定義の中から1つ探す"""
    for it in items:
        if label in (it.get(key), it.get("name"), it.get("deed")):
            return it
    return None


def _field_target(where: str, sc: dict, name: str, whole: bool = False) -> dict:
    """画面の入力欄・slot・行の個数を指す名前を、画面部品の指し方にする"""
    head, _, rest = name.partition(".")
    f = _find(sc["fields"], head)
    if f is not None:
        t = f["type"]
        if t["kind"] == "group":
            part = _find(t["parts"], rest)
            if part is None:
                raise ValueError(f"{where}: 「{name}」の「{rest}」は、入力欄「{f['label']}」の部品にありません")
            return {"as": "field", "field": f"{f['name']}.{part['name']}", "map": False}
        if t["kind"] == "map" and whole and not rest:  # 差し替えでは、写像の欄ぜんぶを値（JSON）で置き換える
            return {"as": "field", "field": f["name"]}
        if t["kind"] == "map":
            if not rest:
                raise ValueError(f"{where}: 「{name}」は 明細.行のID の形で個数を書きます")
            return {"as": "count", "row": rest}
        return {"as": "field", "field": f["name"], "map": False}
    s = _find(sc["slots"], head)
    if s is not None:
        return {"as": "slot", "slot": s["name"]}
    raise ValueError(f"{where}: 「{name}」は、この画面の入力欄にも選ばせる slot にもありません")


def _origin(pattern: str) -> str:
    """拡張の page パターン（*://host/*）から、テストが差し替えるオリジンを作る"""
    m = re.fullmatch(r"(?:\*|https?)://([^/*]+)/.*", pattern)
    return "https://" + m.group(1) if m else ""


def _columns(defs: dict, sc: dict) -> list[dict]:
    return [c for q in sc["shows"] for c in defs["queries"][q]["columns"]]


def screen_cases(ck, plan: dict) -> list[dict]:
    """ユースケースの手順の表と画面の具体例の表から、通しテストの手順（データ）を作る。計画書の食い違いは ValueError"""
    from .plan import usecases
    from .screengen import screens_def
    if not plan.get("画面例"):
        return []
    defs = screens_def(ck)
    scr = defs["screens"]
    design = {}
    for r in plan.get("デザイン", []):
        name = ck.p.resolve(r["画面"].strip(), scr)
        design.setdefault(name, {})[r["指定"].strip()] = r["値"].strip()
    pages = {s.name: s.pages for s in ck.p.screens}
    examples = {(r["ユースケース"].strip(), r["手順"].strip()): r for r in plan["画面例"]}
    ucs = usecases(plan.get("ユースケース", []))
    cases = []
    for (uc, step) in examples:
        if uc not in ucs or not any(s["手順"].strip() == step for s in ucs[uc]):
            raise ValueError(f"画面の具体例 {uc} 手順 {step}: ユースケースの手順の表にありません")
    for uc, steps in ucs.items():
        if not any((uc, s["手順"].strip()) in examples for s in steps):
            continue
        out = []
        for i, st in enumerate(steps):
            no = st["手順"].strip()
            where = f"画面の具体例 {uc} 手順 {no}"
            name = ck.p.resolve(st["画面"].strip(), scr)
            if name not in scr:
                raise ValueError(f"{where}: 画面「{st['画面'].strip()}」がありません")
            sc = scr[name]
            ex = examples.get((uc, no), {})
            op = st["操作"].strip()
            step = {"no": no, "screen": name, "label": sc["label"], "url": st.get("URL", "").strip() or sc["url"], "ext": bool(pages[name]), "origin": _origin(pages[name][0]) if pages[name] else "",
                    "set": [], "replace": [], "expect": [], "text": f"{uc} 手順 {no}（{sc['label']}／{op}）"}
            if op == "見る":
                step["op"] = "look"
            elif op == "行を選ぶ":
                nxt = ck.p.resolve(steps[i + 1]["画面"].strip(), scr) if i + 1 < len(steps) else None
                if nxt not in sc["opens"] if nxt else True:
                    raise ValueError(f"{where}: 「行を選ぶ」の次の手順の画面が、この画面から開けません")
                step["op"], step["opens"] = "open", nxt
            else:
                a = _find(sc["actions"], ck.p.resolve(op, [x["deed"] for x in sc["actions"]]), "deed") or _find(sc["actions"], op)
                if a is None:
                    raise ValueError(f"{where}: 操作「{op}」は、画面「{sc['label']}」にありません")
                step["op"], step["deed"] = "act", a["deed"]
                res = st.get("結果", "").strip()
                if res:
                    rr = _find(a["routes"], ck.p.resolve(res, [r["result"] for r in a["routes"]]), "result") or _find(a["routes"], res)
                    if rr is None:
                        raise ValueError(f"{where}: 結果「{res}」は、操作「{op}」の結果にありません")
                    step["result"] = rr["result"]
                    step["to"] = rr["to"]
            for k, v in _kv(ex.get("入れる・選ぶ", "")):
                if step["op"] == "open":
                    if k != "行":
                        raise ValueError(f"{where}: 行を選ぶ手順で指せるのは「行 = ID」だけです")
                    step["row"] = v
                    continue
                if step["ext"]:
                    if k not in ("ページ", "ページの表"):
                        raise ValueError(f"{where}: 拡張の画面で指せるのは「ページ = 経路」と「ページの表 = 見出し: 値」です")
                    step["set"].append({"as": "page" if k == "ページ" else "table", "value": v})
                    continue
                if k.startswith("差し替え "):
                    t = _field_target(where, sc, k[len("差し替え "):].strip(), whole=True)
                    t["value"] = v
                    step["replace"].append(t)
                    continue
                t = _field_target(where, sc, k)
                if t["as"] == "count":
                    v = int(v)
                if t["as"] == "slot":
                    sl = _find(sc["slots"], t["slot"], "name")
                    v = next((x["value"] for x in sl["values"] if v in (x["value"], x["label"])), v)
                t["value"] = v
                step["set"].append(t)
            tsc = scr[step["opens"]] if step["op"] == "open" else sc  # 行を選んだあとは、開いた画面に見えるもの
            cols = _columns(defs, tsc)
            for k, v in _kv(ex.get("見えること", "")):
                if k in ("含む", "含まない", "行数", "行", "行なし", "表示なし", "通知"):
                    step["expect"].append({"as": k, "value": v})
                elif k.startswith("選択 "):
                    t = _field_target(where, tsc, k[3:].strip())
                    sl = _find(tsc["slots"], t.get("slot", ""), "name")
                    if sl is None:
                        raise ValueError(f"{where}: 「{k}」は slot ではありません")
                    step["expect"].append({"as": "選択", "slot": t["slot"], "value": next((x["value"] for x in sl["values"] if v in (x["value"], x["label"])), v)})
                else:
                    m = re.fullmatch(r"行\s+(\S+)\s+の\s+(\S+)", k)
                    c = _find(cols, m.group(2)) if m else None
                    if c is None:
                        raise ValueError(f"{where}: 見えること「{k}」が読めません（含む・含まない・通知・行数・行・行なし・表示なし・選択 slot・行 ID の 列 のどれか）")
                    step["expect"].append({"as": "列", "row": m.group(1), "col": c["name"], "value": v})
            out.append(step)
        for s in out:
            s["design"] = design.get(s["screen"], {})
        cases.append({"name": uc, "steps": out})
    return cases


SCREEN_TEST = '''# SPDX-License-Identifier: GPL-3.0-or-later
"""計画書の「ユースケースの手順」と「画面の具体例」から geas が生成したテスト。手で直さない（計画書を直して作り直す）。
手順は screen_cases.json、実行部は screen_runner.js（Playwright。アプリを知らない汎用のもの）。
サーバーは、生成物の server/ をビルドしたもの（cargo build --release）を新しい DB で起こし、上から順に実行する。
Chrome 拡張の手順は、拡張が問い合わせてよい先（manifest の host_permissions）のポート 8787 で行う。"""
import os
import shutil
import socket
import subprocess
import time
import urllib.request
from pathlib import Path

import pytest

APP = Path(__file__).resolve().parent.parent


def _node_env():
    node = shutil.which("node")
    if node is None:
        return None
    root = subprocess.run(["npm", "root", "-g"], capture_output=True, text=True).stdout.strip() if shutil.which("npm") else ""
    env = dict(os.environ, NODE_PATH=root)
    ok = subprocess.run([node, "-e", "require('playwright')"], env=env, capture_output=True).returncode == 0
    return env if ok else None


def _free(port):
    with socket.socket() as s:
        try:
            s.bind(("127.0.0.1", port))
            return True
        except OSError:
            return False


@pytest.mark.skipif(_node_env() is None, reason="node と Playwright が無い")
def test_ユースケースの手順を画面で通す(tmp_path):
    exe = APP / "server" / "target" / "release" / "geas_server"
    if not exe.exists():
        pytest.skip("サーバーがビルドされていません（cd server && cargo build --release）")
    ext_ok = _free(8787)
    if ext_ok:
        port = 8787
    else:
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            port = s.getsockname()[1]
    proc = subprocess.Popen([str(exe), "--port", str(port), "--db", str(tmp_path / "app.sqlite"), "--pwa", str(APP / "ui"), "--dev", str(APP / "dev")],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        url = f"http://127.0.0.1:{port}"
        for _ in range(100):
            try:
                urllib.request.urlopen(url + "/api/health", timeout=1)
                break
            except OSError:
                time.sleep(0.1)
        here = Path(__file__).resolve().parent
        r = subprocess.run(["node", str(here / "screen_runner.js"), str(here / "screen_cases.json"), url, str(tmp_path)] + ([] if ext_ok else ["--no-ext"]),
                           env=_node_env(), capture_output=True, text=True, timeout=600)
        assert r.returncode == 0, r.stdout + r.stderr
    finally:
        proc.terminate()
        proc.wait(timeout=10)
'''


def write_screen_tests(ck, plan: dict, dest) -> Path | None:
    cases = screen_cases(ck, plan)
    if not cases:
        return None
    import json as _json
    import shutil
    out = Path(dest) / "tests"
    out.mkdir(parents=True, exist_ok=True)
    (out / "screen_cases.json").write_text(_json.dumps(cases, ensure_ascii=False, indent=1) + "\n")
    shutil.copy(Path(__file__).parent / "e2e" / "screen_runner.js", out / "screen_runner.js")
    f = out / "test_screen_examples.py"
    f.write_text(SCREEN_TEST)
    return f
