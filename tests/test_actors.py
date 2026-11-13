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


# 範囲：小包のうち、返金の控え（refund_ref）が、呼んだ本人の ID と同じもの（own_parcels）と、状態で分けたもの
SCOPE = """
scope own_parcels on Parcel {
  filter refund_ref equals caller
}

scope live on Parcel {
  where status != Canceled
}

scope canceled on Parcel {
  where status == Canceled
}

/// 自分の小包
query my_parcels {
  from Parcel.own_parcels
  select parcel_id, kind
  reads Parcel.refund_ref, Parcel.parcel_id, Parcel.kind
}

/// 自分の、取り消していない小包
query my_live_parcels {
  from Parcel.own_parcels.live
  select parcel_id
  reads Parcel.refund_ref, Parcel.status, Parcel.parcel_id
}

/// 自分の、取り消した小包
query my_canceled_parcels {
  from Parcel.canceled.own_parcels
  select parcel_id
  reads Parcel.refund_ref, Parcel.status, Parcel.parcel_id
}

/// 取り消していない小包
query live_parcels {
  from Parcel.live
  select parcel_id
  reads Parcel.status, Parcel.parcel_id
}

resource MyParcels {
  index my_parcels for Customer
}

resource MyLiveParcels {
  index my_live_parcels for Customer
}

resource MyCanceledParcels {
  index my_canceled_parcels for Customer
}

resource LiveParcels {
  index live_parcels for anyone
}

result ClaimResult { Claimed, NoParcel }

/// 持ち主を決める
deed claim {
  input parcel_id: id required
  input owner: id required
  reads Parcel
  writes Parcel.refund_ref
  result ClaimResult
  effects none

  let p = Parcel[parcel_id] else { return NoParcel }
  p.refund_ref = owner
  return Claimed
}

nexus Claiming {
  claim: Claimed → reply, NoParcel → reply
}

resource Claims {
  create claim for Staff {
    Claimed → 201
    NoParcel → 404
  }
}
"""


def scoped(scope: str = SCOPE) -> str:
    return with_actors() + scope


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
    code.write_text(scoped(), encoding="utf-8")
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


def test_範囲の検査と仕様書():
    from geas.envreq import items
    from geas.hostgen import definitions
    from geas.spec import build, render

    assert errors(scoped()) == []
    mine = "from Parcel.own_parcels\n"
    assert has(scoped(SCOPE.replace(mine, "from Parcel.own_parcel\n")), "範囲「own_parcel」が未定義です")
    assert has(scoped(SCOPE.replace(mine, "from Parcel.own_parcels.own_parcels\n")), "重ねて書いています")
    assert has(scoped(SCOPE.replace("scope own_parcels on Parcel", "scope own_parcels on Package")), "データ「Package」が未定義です")
    assert has(scoped(SCOPE.replace("filter refund_ref equals", "filter kind equals")), "ID か参照")
    assert has(scoped(SCOPE.replace("where status == Canceled", "where status == Gone")), "「Gone」は項目 status の状態ではありません")
    assert has(scoped(SCOPE.replace("scope live on", "scope kind on").replace("Parcel.live", "Parcel.kind")), "項目か範囲か分からなくなります")
    assert has(scoped(SCOPE.replace("reads Parcel.refund_ref, Parcel.status, Parcel.parcel_id", "reads Parcel.refund_ref, Parcel.parcel_id", 1)), "宣言（読む）にありません")  # 範囲の項目も、読む項目に数える
    assert has(scoped(SCOPE.replace("index my_parcels for Customer", "index my_parcels for anyone")), "呼べるのは役割のある人だけです")
    assert has(scoped(SCOPE.replace(mine, "input caller: id required\n  from Parcel.own_parcels\n  filter parcel_id equals caller\n")), "入力の名前に caller は使えません")
    # 出来事には呼んだ本人がいないので、本人で絞る範囲の問い合わせは each に使えない（本人で絞らない範囲は使える）
    ids = "\nquery {n} {{\n  from Parcel.{s}\n  reads {r}\n}}\n\nevent Sweep{n} {{\n  each {n} → ask_nudge\n}}\n"
    assert has(scoped(SCOPE + ids.format(n="mine_ids", s="own_parcels", r="Parcel.refund_ref")), "出来事からは呼べません")
    assert not has(scoped(SCOPE + ids.format(n="live_ids", s="live", r="Parcel.status")), "出来事からは呼べません")
    for bad in ("filter refund_ref == caller", "filter refund_ref equals me", "filter refund_ref equals caller\n  filter parcel_id equals caller", "order fee desc"):
        with pytest.raises(ParseError):
            parse(scoped(SCOPE.replace("filter refund_ref equals caller", bad)), str(SRC))
    with pytest.raises(ParseError, match="範囲は in ではなく"):
        parse(scoped(SCOPE.replace(mine, "from Parcel in own_parcels\n")), str(SRC))
    ck = ck_of(scoped())
    spec = build(ck)
    md = render(ck, spec, "")
    who = {x[1]: x[6] for x in spec["ルート"]}
    assert who["/my_parcels"] == "客（本人の分だけ）" and who["/my_live_parcels"] == "客（本人の分だけ）" and who["/live_parcels"] == "誰でも"
    assert "## 範囲" in md and "X-Geas-Actor-Id" in md and "すべての範囲に入る行だけ" in md
    rows = {x[0]: x for x in spec["範囲"]}
    assert rows["own_parcels"][2].endswith("が、呼んだ本人のID と同じ") and "!=" in rows["live"][2]
    assert rows["live"][3] == "自分の、取り消していない小包、取り消していない小包"
    conds = {x["問い合わせ"]: x["条件"] for x in spec["問い合わせ"]}
    assert len(conds["自分の、取り消していない小包"]) == 2 and any("（範囲 own_parcels）" in c for c in conds["自分の、取り消していない小包"]) and any("（範囲 live）" in c for c in conds["自分の、取り消していない小包"])
    defs = definitions(ck)["queries"]
    assert defs["my_parcels"] == {"inputs": [], "refs": [], "caller": True} and defs["my_canceled_parcels"].get("caller") is True
    assert "caller" not in defs["parcel_list"] and "caller" not in defs["live_parcels"]  # caller は、呼ぶ側の入力にしない。本人で絞らない範囲には要らない
    assert "X-Geas-Actor-Id" in next(x for x in items(ck.p) if x["id"] == "actor")["要求"]
    assert "## 範囲" not in render(ck_of(with_actors()), build(ck_of(with_actors())), "")


def test_範囲つきの問い合わせは_本人の分だけを返し_並べた範囲はすべてに入る行だけになる(served):
    def get(actor=None, who=None, tag=None, path="/my_parcels"):
        headers = {**({"X-Geas-Actor": actor} if actor else {}), **({"X-Geas-Actor-Id": who} if who is not None else {}), **({"If-None-Match": tag} if tag else {})}
        req = urllib.request.Request(served + path, headers=headers)
        try:
            with urllib.request.urlopen(req, timeout=10) as r:
                return r.status, [x["parcel_id"] for x in json.loads(r.read())["rows"]], r.headers.get("ETag")
        except urllib.error.HTTPError as e:
            return e.code, None, e.headers.get("ETag")

    made = [call(served, "POST", "/parcels", "Customer", {"kind": "light"}) for _ in range(2)]
    assert [c for c, _ in made] == [201, 201]
    a, b = sorted(x["parcel_id"] for x in call(served, "GET", "/parcels")[1]["rows"])[-2:]
    for pid, owner in ((a, "c-1"), (b, "c-2")):  # 控え（refund_ref）を「持ち主」に見立てて、お店の人が決める
        assert call(served, "POST", "/claims", "Staff", {"parcel_id": pid, "owner": owner})[0] == 201
    code, rows, tag_a = get("Customer", "c-1")
    assert (code, rows) == (200, [a])
    assert get("Customer", "c-2")[:2] == (200, [b])
    assert get("Customer", "nobody")[:2] == (200, [])
    # 本人の ID が無ければ 401、役割が無ければ 401、役割が違えば 403
    assert get("Customer")[0] == 401 and get("Customer", " ")[0] == 401
    assert get(None, "c-1")[0] == 401 and get("Staff", "c-1")[0] == 403
    # 別の人が、前の人の答えの版（ETag）を送っても、304 にならず自分の分が返る
    assert get("Customer", "c-1", tag_a)[0] == 304
    assert get("Customer", "c-2", tag_a)[:2] == (200, [b])
    # 並べた範囲は、すべてに入る行だけ（順は関係しない）。本人で絞らない範囲は、誰でも呼べる口にもできる
    assert get("Customer", "c-1", path="/my_live_parcels")[:2] == (200, [a])
    assert get("Customer", "c-1", path="/my_canceled_parcels")[:2] == (200, [])
    assert get("Customer", path="/my_canceled_parcels")[0] == 401
    assert {a, b} <= set(get(path="/live_parcels")[1])
