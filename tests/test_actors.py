# SPDX-License-Identifier: GPL-3.0-or-later
"""呼べる人の区別（actor 名前 ／ resource の動作の for 役割, … ／ for anyone）。
本人の確かめは前に置くプロキシの仕事で、サーバーは、プロキシが載せたヘッダ X-Geas-Actor の値と、動作の for を比べる。
cargo が無い環境では、組んで動かす試験を飛ばす。"""
import json
import shutil
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

import pytest

from geas.check import check
from geas.parser import ParseError, parse
from geas.rest import internal_targets

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "tests" / "fixtures" / "runtime_demo" / "runtime_demo.geas"
PLAIN = SRC.read_text(encoding="utf-8")
ACTORS = "\n/// 客\nactor Customer\n/// お店の人\nactor Staff\n"
# 呼ぶもの → for に書く役割。ここに無い動作は Staff だけ
WHO = {"accept": "Customer", "parcel_list": "anyone", "restamp": "Customer, Staff", "parcel_of": "Customer, Staff"}


def with_actors(who: dict = WHO, default: str = "Staff", actors: str = ACTORS) -> str:
    """見本に役割を足した版。外から呼べる動作すべてに for を書く（流れの途中の処理には書かない）"""
    p = parse(PLAIN, str(SRC))
    inner = internal_targets(p)
    lines = PLAIN.split("\n")
    for res in p.resources:
        for a in res.actions:
            if a.target in inner:
                continue
            role = who.get(a.target, default)
            if role:
                head = f"{a.kind} {a.target}"
                i = a.line - 1
                assert head in lines[i], (head, lines[i])
                lines[i] = lines[i].replace(head, f"{head} for {role}", 1)
    return "\n".join(lines) + actors


def ck_of(text: str):
    return check(parse(text, str(SRC)), strict_names=True)


def errors(text: str) -> list[str]:
    return [d.msg for d in ck_of(text).diags if d.level == "エラー"]


def has(text: str, word: str) -> bool:
    return any(word in m for m in errors(text))


def test_役割の検査():
    assert errors(with_actors()) == []
    assert errors(PLAIN) == []  # 役割の無いアプリは、今までどおり
    # 役割を宣言したら、外から呼べる動作すべてに for が要る（書き忘れで、誰でも呼べる口を作らない）
    assert has(with_actors({**WHO, "accept": ""}), "呼べる役割がありません")
    assert has(with_actors({**WHO, "accept": "Custmer"}), "役割「Custmer」が未定義です")
    assert has(with_actors({**WHO, "accept": "anyone, Staff"}), "ほかの役割と並べて書けません")
    assert has(with_actors({**WHO, "accept": "Staff, Staff"}), "重ねて書いています")
    assert has(with_actors(actors=""), "for に書く役割が宣言されていません")
    assert has(with_actors(actors=ACTORS + "actor Staff\n"), "同じ名前の役割が2つ以上")
    assert has(with_actors(actors=ACTORS + "actor anyone\n"), "役割の名前にできません")
    # 流れの途中の処理（外から呼べない）には書けない
    assert has(with_actors().replace("create settle {", "create settle for Staff {", 1), "流れの途中の処理は外から呼べないので")
    # 使っていない役割は警告
    assert any("どの動作の for にも書かれていません" in d.msg for d in ck_of(with_actors(actors=ACTORS + "actor Auditor\n")).diags)
    with pytest.raises(ParseError):
        parse(with_actors().replace("for Customer, Staff", "for", 1), str(SRC))


def test_役割は仕様書と定義と動かす前提に出る():
    from geas.envreq import items
    from geas.hostgen import definitions
    from geas.review import review
    from geas.spec import build, render

    ck = ck_of(with_actors())
    spec = build(ck)
    md, html = render(ck, spec, ""), review(ck, None, [], "")
    assert "| 外から | 呼べる人 |" in md
    row = lambda path, m: next(x for x in spec["ルート"] if x[0] == m and x[1] == path)
    assert row("/parcels", "POST")[6] == "客" and row("/parcels", "GET")[6] == "誰でも"
    assert row("/parcels/:id", "PUT")[6] == "客、お店の人" and row("/parcels/:id", "DELETE")[6] == "お店の人"
    assert row("/settlements", "POST")[6] == "（外から呼べない）"
    roles = {a: c for a, _, c in spec["役割"]}
    assert [x[:2] for x in spec["役割"]] == [("客", "Customer"), ("お店の人", "Staff"), ("誰でも", "（要らない）")]  # ヘッダに載せるのは、コードの名前
    assert "POST /parcels" in roles["客"] and "DELETE /parcels/:id" not in roles["客"] and "DELETE /parcels/:id" in roles["お店の人"]
    assert roles["誰でも"] == "GET /parcels"
    assert "## 役割" in md and "X-Geas-Actor" in md and "401" in md and "403" in md
    assert "呼べる人" in html and "役割（呼べる人の区別）" in html
    # 生成物の定義：役割のある口だけに for が付く（誰でも呼べる口と、流れの途中の処理には付かない）
    routes = {(r["method"], r["path"]): r for r in definitions(ck)["routes"]}
    assert routes[("POST", "/parcels")]["for"] == ["Customer"] and routes[("PUT", "/parcels/:id")]["for"] == ["Customer", "Staff"]
    assert "for" not in routes[("GET", "/parcels")] and "for" not in routes[("POST", "/settlements")]
    # 動かす前提：プロキシが役割を確かめる、という人の保証が出る
    item = next(x for x in items(ck.p) if x["id"] == "actor")
    assert item["人の保証"] and "X-Geas-Actor" in item["要求"] and "Customer・Staff" in item["要求"]
    # 役割の無いアプリの仕様書は、今までどおり（列も節も出ない）
    plain = ck_of(PLAIN)
    md0 = render(plain, build(plain), "")
    assert "呼べる人" not in md0 and "## 役割" not in md0 and not any(x["id"] == "actor" for x in items(plain.p))


def test_呼べる人が広がった変更は_レシートの弱くなったに出る(tmp_path):
    from geas.receipt import build_receipt

    def weak(new_text, old_text):
        f = tmp_path / "new.geas"
        f.write_text(new_text, encoding="utf-8")
        r = build_receipt(ck_of(new_text), ck_of(old_text), str(f), None, "前の版")
        return [w for w in r.weakened if "呼べる人" in w[3]], [s for s in r.strengthened if "呼べる人" in s[3]]

    base = with_actors()
    assert weak(base, base) == ([], [])
    w, s = weak(with_actors({**WHO, "remove_parcel": "Customer, Staff"}), base)  # 消せる人に、客が増えた
    assert [x[0] for x in w] == ["URL DELETE /parcels/:id"] and s == []
    w, s = weak(with_actors({**WHO, "accept": "anyone"}), base)  # 誰でも呼べるようになった
    assert [x[0] for x in w] == ["URL POST /parcels"] and s == []
    w, s = weak(with_actors({**WHO, "restamp": "Staff"}), base)  # 狭まった
    assert w == [] and [x[0] for x in s] == ["URL PUT /parcels/:id"]


@pytest.fixture(scope="module")
def served(tmp_path_factory):
    if shutil.which("cargo") is None:
        pytest.skip("cargo が無い")
    out = tmp_path_factory.mktemp("actors")
    code = out / "actors.geas"
    code.write_text(with_actors(), encoding="utf-8")
    subprocess.run([sys.executable, "-m", "geas", "rust", str(code), "-o", str(out / "app"), "--host", "--runtime"], check=True, capture_output=True, text=True, cwd=ROOT)
    r = subprocess.run(["cargo", "build", "--release", "--offline", "--quiet"], cwd=out / "app" / "server", capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    proc = subprocess.Popen([str(out / "app" / "server" / "target" / "release" / "geas_server"), "--port", str(port), "--db", str(out / "a.sqlite"),
                             "--pwa", str(out / "app" / "ui")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    base = f"http://127.0.0.1:{port}"
    for _ in range(100):
        try:
            urllib.request.urlopen(base + "/api/health", timeout=1)
            break
        except OSError:
            time.sleep(0.1)
    yield base
    proc.kill()
    proc.wait()


def call(base, method, path, actor=None, body=None):
    headers = {"Content-Type": "application/json", **({"X-Geas-Actor": actor} if actor is not None else {})}
    req = urllib.request.Request(base + path, method=method, data=None if body is None else json.dumps(body).encode(), headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=10) as r:
            return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read())


def test_役割の無い呼び出しは401_合わない役割は403_合えば通る(served):
    new = {"kind": "light"}
    # 誰でも呼べる口は、ヘッダが無くても通る
    assert call(served, "GET", "/parcels")[0] == 200
    # 役割のある口：ヘッダが無い（空も）なら 401、役割が合わなければ 403。どちらも、処理は動かない
    assert call(served, "POST", "/parcels", None, new)[0] == 401
    assert call(served, "POST", "/parcels", "", new)[0] == 401
    assert call(served, "POST", "/parcels", "Staff", new)[0] == 403
    assert call(served, "POST", "/parcels", "Nobody", new)[0] == 403
    assert call(served, "POST", "/parcels", "customer", new)[0] == 403  # 大文字と小文字は区別する
    assert call(served, "GET", "/parcels")[1]["rows"] == []
    code, made = call(served, "POST", "/parcels", "Customer", new)
    assert code == 201, made
    pid = call(served, "GET", "/parcels")[1]["rows"][0]["parcel_id"]
    # 消せるのは、お店の人だけ
    assert call(served, "DELETE", f"/parcels/{pid}", "Customer")[0] == 403
    assert call(served, "DELETE", f"/parcels/{pid}")[0] == 401
    assert len(call(served, "GET", "/parcels")[1]["rows"]) == 1
    # 役割を複数持つ人（カンマ区切り）は、どれかが合えば通る
    assert call(served, "GET", f"/parcels/{pid}/details", "Auditor, Staff")[0] == 200
    assert call(served, "GET", f"/parcels/{pid}/details", "Auditor")[0] == 403
    # 流れの途中の処理は、役割があっても外から呼べない
    assert call(served, "POST", "/settlements", "Staff", {"parcel_id": pid})[0] == 404
