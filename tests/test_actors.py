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

scope in_hand on Parcel {
  where status in [Received, Waiting]
}

/// 手元にある小包
query parcels_in_hand {
  from Parcel.in_hand
  where fee in [0, 300, 500]
  select parcel_id
  reads Parcel.status, Parcel.fee, Parcel.parcel_id
}

resource ParcelsInHand {
  index parcels_in_hand for anyone
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

scope of_kind(k: Kind) on Parcel {
  filter kind equals k
}

/// 種類ごとの、自分の小包
query my_parcels_of_kind {
  input kind: Kind required
  from Parcel.of_kind(kind).own_parcels
  select parcel_id
  reads Parcel.kind, Parcel.refund_ref, Parcel.parcel_id
}

resource MyParcelsOfKinds key kind {
  show my_parcels_of_kind for Customer
}

/// 種類と、料金の幅で絞った小包（種類はパスのキー。料金の下限は必ず、上限は指定したときだけ）
query parcels_by_kind_and_fee {
  input kind: Kind required
  input min_fee: Fee required
  input max_fee: Fee
  from Parcel.of_kind(kind)
  filter fee at_least min_fee
  filter fee at_most max_fee
  select parcel_id
  reads Parcel.kind, Parcel.fee, Parcel.parcel_id
}

resource ParcelsByKindAndFees key kind {
  show parcels_by_kind_and_fee for anyone
}

/// 小包を 2 件ずつ（料金の大きい順。同じ料金の中は ID の大きい順）
query parcels_paged {
  input kind: Kind
  from Parcel
  filter kind equals kind
  order fee desc
  page 2
  select parcel_id, kind
  reads Parcel.parcel_id, Parcel.kind, Parcel.fee
}

resource ParcelPages {
  index parcels_paged for anyone
}

/// 小包の ID を 1 件ずつ
query parcel_ids_paged {
  from Parcel
  order fee asc
  order fee desc
  page 1
  reads Parcel.fee
}

resource ParcelIdPages {
  index parcel_ids_paged for anyone
}

/// 種類と料金で小包を探す（条件を URL に載せない）
query parcels_quietly {
  input kind: Kind required
  input min_fee: Fee
  from Parcel.of_kind(kind)
  filter fee at_least min_fee
  order fee asc
  page 1
  select parcel_id
  reads Parcel.kind, Parcel.fee, Parcel.parcel_id
}

resource QuietParcelSearches {
  index parcels_quietly for Staff post "探している種類を、URL とログに残さない"
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

result RelabelResult { Relabeled, NotMine }

/// 自分の小包の種類を書き換える
deed relabel_mine {
  input parcel_id: id required, kind: Kind required
  reads Parcel.refund_ref, Parcel.status
  writes Parcel.kind
  result RelabelResult
  effects none

  let p = Parcel.own_parcels.live[parcel_id] else { return NotMine }
  p.kind = kind
  return Relabeled
}

nexus Relabeling {
  relabel_mine: Relabeled → reply, NotMine → reply
}

resource MyParcelLabels {
  update relabel_mine for Customer {
    Relabeled → 200
    NotMine → 404
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
    assert has(scoped(SCOPE.replace("filter refund_ref equals caller", "filter refund_ref equals me")), "範囲の引数にありません")
    for bad in ("filter refund_ref == caller", "filter refund_ref equals caller\n  filter parcel_id equals caller", "order fee desc"):
        with pytest.raises(ParseError):
            parse(scoped(SCOPE.replace("filter refund_ref equals caller", bad)), str(SRC))
    with pytest.raises(ParseError, match="範囲は in ではなく"):
        parse(scoped(SCOPE.replace(mine, "from Parcel in own_parcels\n")), str(SRC))
    # in [候補, …] は、候補ごとの == を or でつないだものと同じ（範囲の中でも、問い合わせの中でも書ける）
    p_in = parse(scoped(), str(SRC))
    assert next(x for x in p_in.scopes if x.name == "in_hand").conds == [[("status", "==", "Received"), ("status", "==", "Waiting")]]
    assert next(q for q in p_in.queries if q.name == "parcels_in_hand").conds == [[("fee", "==", "0"), ("fee", "==", "300"), ("fee", "==", "500")]]
    assert has(scoped(SCOPE.replace("in [Received, Waiting]", "in [Received, Gone]")), "「Gone」は項目 status の状態ではありません")
    for bad, why in (("status in Received", "候補を \\[ \\] で並べます"), ("status in [Received, Received]", "重ねて書いています"),
                     ("status == [Received, Waiting]", "in で書きます"), ("status = [Received, Waiting]", "in で書きます")):
        with pytest.raises(ParseError, match=why):
            parse(scoped(SCOPE.replace("status in [Received, Waiting]", bad)), str(SRC))
    # 引数つきの範囲：渡すのは問い合わせの必ずある入力で、数と型が合うこと
    use = "from Parcel.of_kind(kind).own_parcels"
    assert has(scoped(SCOPE.replace(use, "from Parcel.of_kind.own_parcels")), "範囲 of_kind の引数は 1 個です")
    assert has(scoped(SCOPE.replace(use, "from Parcel.of_kind(kind, kind).own_parcels")), "渡しているのは 2 個です")
    assert has(scoped(SCOPE.replace(use, "from Parcel.of_kind(kin).own_parcels")), "入力にありません")
    assert has(scoped(SCOPE.replace("input kind: Kind required\n  from Parcel.of_kind", "input kind: Kind\n  from Parcel.of_kind")), "required にしてください")
    assert has(scoped(SCOPE.replace("scope of_kind(k: Kind)", "scope of_kind(k: id)")), "同じ種類の文字列・ID・整数どうし")
    assert has(scoped(SCOPE.replace("filter kind equals k\n", "filter kind equals kk\n")), "範囲の引数にありません")
    assert has(scoped(SCOPE.replace("scope of_kind(k: Kind)", "scope of_kind(k: Kind, extra: Kind)").replace(use, "from Parcel.of_kind(kind, kind).own_parcels")), "どの filter にも使われていません")
    assert has(scoped(SCOPE.replace("filter kind equals k\n", "filter fee at_least k\n").replace("reads Parcel.kind, Parcel.refund_ref", "reads Parcel.fee, Parcel.refund_ref")), "同じ種類の数・日時どうし")
    with pytest.raises(ParseError, match="equals だけ"):
        parse(scoped(SCOPE.replace("filter refund_ref equals caller", "filter refund_ref at_least caller")), str(SRC))
    ck = ck_of(scoped())
    spec = build(ck)
    md = render(ck, spec, "")
    assert any(x[0] == "of_kind（k）" and "が、引数 k と同じ" in x[2] for x in spec["範囲"])
    assert any("一致 kind（範囲 of_kind）" in c for x in spec["問い合わせ"] if x["問い合わせ"] == "種類ごとの、自分の小包" for c in x["条件"])
    who = {x[1]: x[6] for x in spec["ルート"]}
    assert who["/my_parcels"] == "客（本人の分だけ）" and who["/my_live_parcels"] == "客（本人の分だけ）" and who["/live_parcels"] == "誰でも"
    assert "## 範囲" in md and "X-Geas-Actor-Id" in md and "すべての範囲に入る行だけ" in md
    rows = {x[0]: x for x in spec["範囲"]}
    assert rows["own_parcels"][2].endswith("が、呼んだ本人のID と同じ") and "!=" in rows["live"][2]
    assert rows["live"][3] == "自分の、取り消していない小包、取り消していない小包、自分の小包の種類を書き換える"
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
    # 引数つきの範囲：URL の種類（入力）を範囲に渡す。本人の分との積集合になる
    assert get("Customer", "c-1", path="/my_parcels_of_kinds/light")[:2] == (200, [a])
    assert get("Customer", "c-2", path="/my_parcels_of_kinds/light")[:2] == (200, [b])
    assert get("Customer", "c-1", path="/my_parcels_of_kinds/heavy")[:2] == (200, [])
    # 処理の 1 件引きも範囲を通す：自分の小包は書き換えられ、他人の小包は「無い」（404）。本人の ID が無ければ 401
    def relabel(pid, who, kind="mine"):
        req = urllib.request.Request(served + "/my_parcel_labels/" + pid, data=json.dumps({"kind": kind, "caller": "c-1"}).encode(), method="PUT",
                                     headers={"Content-Type": "application/json", "X-Geas-Actor": "Customer", **({"X-Geas-Actor-Id": who} if who else {})})
        try:
            with urllib.request.urlopen(req, timeout=10) as r:
                return r.status, json.loads(r.read()).get("result")
        except urllib.error.HTTPError as e:
            return e.code, json.loads(e.read()).get("result")
    assert relabel(a, "c-1") == (200, "Relabeled")
    assert relabel(b, "c-1") == (404, "NotMine")  # 本文に caller を書いても、ヘッダの本人で決まる
    assert relabel(a, "c-2") == (404, "NotMine") and relabel("nope", "c-1") == (404, "NotMine")
    assert relabel(a, None)[0] == 401
    kinds = {x["parcel_id"]: x["kind"] for x in call(served, "GET", "/parcels")[1]["rows"]}
    assert (kinds[a], kinds[b]) == ("mine", "light")
    assert relabel(a, "c-1", "light") == (200, "Relabeled")
    # 続きを読む：答えの next を辿ると、全部の行を 1 回ずつ、並びの順で読める。最後のページに next は無い
    def walk(path):
        seen, pages = [], 0
        while path:
            code, body = call(served, "GET", path)
            assert code == 200, (path, body)
            got = [x["parcel_id"] for x in body["rows"]] if "rows" in body else body["ids"]
            seen += got
            pages += 1
            path = body.get("next")
            assert pages < 50
        return seen, pages
    every = call(served, "GET", "/parcels")[1]["rows"]
    seen, pages = walk("/parcel_pages")
    want = [x["parcel_id"] for x in sorted(every, key=lambda x: (x["fee"], x["parcel_id"]), reverse=True)]
    assert seen == want and pages == (len(want) + 1) // 2
    ids, id_pages = walk("/parcel_id_pages")
    assert ids == [x["parcel_id"] for x in sorted(every, key=lambda x: (x["fee"], x["parcel_id"]))] and id_pages == len(ids)
    # 並びを選ぶ：書いた候補から q[order] で選ぶ。選ばなければ 1 行目。次のページの URL にも残る。候補に無い並びは 400
    down, down_pages = walk("/parcel_id_pages?q[order]=fee.desc")
    assert down == ids[::-1] and down_pages == len(ids) and walk("/parcel_id_pages?q[order]=fee.asc")[0] == ids
    assert "q[order]=fee.desc" in call(served, "GET", "/parcel_id_pages?q[order]=fee.desc")[1]["next"]
    assert [call(served, "GET", "/parcel_id_pages?q[order]=" + x)[0] for x in ("fee", "kind.desc", "fee.DESC", "")] == [400] * 4
    assert call(served, "GET", "/parcel_pages?q[order]=fee.desc")[0] == 400  # 並びが 1 行の問い合わせでは選べない
    # 条件は、次のページの URL にも残る
    lights = [x["parcel_id"] for x in every if x["kind"] == "light"]
    first = call(served, "GET", "/parcel_pages?q[kind]=light")[1]
    assert ("q[kind]=light" in first["next"] and "q[after]=" in first["next"]) if len(lights) > 2 else "next" not in first
    assert sorted(walk("/parcel_pages?q[kind]=light")[0]) == sorted(lights)
    # 無い行の印は 400。ページの無い問い合わせに q[after] は書けない
    assert call(served, "GET", "/parcel_pages?q[after]=nope")[0] == 400 and call(served, "GET", "/live_parcels?q[after]=" + a)[0] == 400
    # 事情のある POST：条件は本文の {"q": {…}}。その場で結果を返し（200）、続きは答えの after を q.after に入れて読む
    quiet = lambda body, actor="Staff", path="/quiet_parcel_searches": call(served, "POST", path, actor, body)
    code, page1 = quiet({"q": {"kind": "light", "min_fee": 300}})
    assert code == 200 and len(page1["rows"]) == 1 and "next" not in page1 and page1.get("after")
    code, page2 = quiet({"q": {"kind": "light", "after": page1["after"]}})
    assert code == 200 and page2["rows"] and page2["rows"] != page1["rows"]
    assert quiet({"q": {"kind": "heavy"}})[1]["rows"] == [] and quiet({"q": {"kind": "light", "min_fee": 301}})[1]["rows"] == []
    # 要る条件が無い・宣言に無い名前・q のほかのキー・q がオブジェクトでない・クエリ文字列・型違いは 400。役割も確かめる
    for wrong in ({}, {"q": {}}, {"q": {"kind": "light", "other": 1}}, {"kind": "light"}, {"q": "light"}, {"q": {"kind": "light", "min_fee": "300"}}, {"q": {"kind": "light"}, "x": 1}):
        assert quiet(wrong)[0] == 400, wrong
    assert quiet({"q": {"kind": "light"}}, path="/quiet_parcel_searches?q[kind]=light")[0] == 400
    assert quiet({"q": {"kind": "light"}}, actor="Customer")[0] == 403 and call(served, "GET", "/quiet_parcel_searches?q[kind]=light", "Staff")[0] != 200
    # 絞り込みの条件は q[名前]=値：種類はパス、料金の下限（必ず）と上限（省ける）はクエリ文字列
    two = "/parcels_by_kind_and_fees/light"
    assert {a, b} <= set(get(path=two + "?q[min_fee]=300")[1]) and get(path=two + "?q[min_fee]=301")[:2] == (200, [])
    assert {a, b} <= set(get(path=two + "?q[max_fee]=300&q[min_fee]=0")[1]) and get(path=two + "?q[min_fee]=0&q[max_fee]=299")[:2] == (200, [])
    assert {a, b} <= set(get(path=two + "?q%5Bmin_fee%5D=300")[1])  # 角括弧は %5B %5D でも同じ
    assert get(path="/parcels_by_kind_and_fees/heavy?q[min_fee]=0")[:2] == (200, [])
    # 要る条件が無い・数字でない・宣言に無い名前・q[…] でない形・同じ名前の重複は 400
    bad = ("", "?q[min_fee]=abc", "?q[min_fee]=3.5", "?q[min_fee]=300&q[other]=1", "?min_fee=300", "?q[min_fee]=300&q[min_fee]=300", "?q=300", "?q[min_fee]=300&x")
    assert [get(path=two + x)[0] for x in bad] == [400] * len(bad)
    # 条件を取らない URL にクエリ文字列を付けても 400（問い合わせでも、処理でも）
    assert get(path="/live_parcels?q[x]=1")[0] == 400 and get(path="/live_parcels?page=2")[0] == 400
    assert call(served, "POST", "/claims?x=1", "Staff", {"parcel_id": a, "owner": "c-1"})[0] == 400
    # 条件が違えば、答えの版（ETag）も別
    t300, t301 = get(path=two + "?q[min_fee]=300")[2], get(path=two + "?q[min_fee]=301")[2]
    assert t300 and t300 != t301 and get(path=two + "?q[min_fee]=301", tag=t300)[0] == 200 and get(path=two + "?q[min_fee]=300", tag=t300)[0] == 304
    # in [候補, …]：状態がどれかで、料金もどれかの小包（作ったばかりの 2 件は、どちらも当てはまる）
    hand = get(path="/parcels_in_hand")
    assert hand[0] == 200 and {a, b} <= set(hand[1])


def test_絞り込みの条件は_q名前で渡る():
    text = scoped()
    ck = ck_of(text)
    assert errors(text) == []
    from geas.hostgen import definitions
    from geas.mcp import tool_defs
    from geas.spec import build
    route = next(r for r in definitions(ck)["routes"] if r["target"] == "parcels_by_kind_and_fee")
    assert route["path"] == "/parcels_by_kind_and_fees/:kind" and route["path_inputs"] == [{"name": "kind"}]
    assert route["q_inputs"] == [{"name": "min_fee", "int": True}, {"name": "max_fee", "int": True, "optional": True}]
    # 仕様書の URL の表には、条件の名前まで出る（省ける条件は ?）
    assert any(row[1] == "/parcels_by_kind_and_fees/:kind?q[min_fee]=…&q[max_fee?]=…" for row in build(ck)["ルート"])
    tool = next(t for t in tool_defs(ck) if t["geas"]["path"] == "/parcels_by_kind_and_fees/:kind")
    assert tool["geas"]["q"] == ["min_fee", "max_fee"] and tool["inputSchema"]["required"] == ["kind", "min_fee"]
    # パスのキーは 1 つ。キーが入力に無い・URL に載せられない型は、検査で止まる
    for two in ("key kind, min_fee", "key kind key min_fee"):
        with pytest.raises(ParseError, match="パスのキーは 1 つです"):
            parse(text.replace("key kind {\n  show parcels_by_kind_and_fee", two + " {\n  show parcels_by_kind_and_fee"), "x")
    assert has(text.replace("key kind {\n  show parcels_by_kind_and_fee", "key nope {\n  show parcels_by_kind_and_fee"), "key「nope」が")
    assert has(text.replace("input max_fee: Fee\n", "input max_fee: bool\n").replace("  filter fee at_most max_fee\n", ""), "URL に載せられない型です")


def test_処理の1件引きは_範囲を通せる():
    text = scoped()
    ck = ck_of(text)
    assert errors(text) == []
    from geas.hostgen import definitions
    from geas.spec import build, render
    assert ck.infos["relabel_mine"].caller and ck.infos["relabel_mine"].scopes == ["own_parcels", "live"]
    assert definitions(ck)["deeds"]["relabel_mine"]["caller"] is True and "caller" not in definitions(ck)["deeds"]["claim"]
    spec = build(ck)
    assert any("（本人の分だけ）" in row[6] and row[1] == "/my_parcel_labels/:id" for row in spec["ルート"])
    assert "（範囲 own_parcels、live）[" in render(ck, spec, "")
    one = "let p = Parcel.own_parcels.live[parcel_id] else { return NotMine }"
    assert has(text.replace(one, "let p = Parcel.own_parcel[parcel_id] else { return NotMine }"), "範囲「own_parcel」が未定義です")
    assert has(text.replace(one, "let p = Parcel.of_kind[parcel_id] else { return NotMine }"), "引数つきの範囲")
    assert has(text.replace(one, "let p = Parcel.live.live[parcel_id] else { return NotMine }"), "重ねて書いています")
    assert has(text.replace("reads Parcel.refund_ref, Parcel.status\n  writes Parcel.kind", "reads Parcel.refund_ref\n  writes Parcel.kind"), "宣言")  # 範囲が比べる項目も、読む項目に数える
    assert has(text.replace("update relabel_mine for Customer", "update relabel_mine for anyone"), "呼べるのは役割のある人だけです")
    assert has(text.replace(one, "let p = Parcel[parcel_id] else { return NotMine }\n  let k = Parcel.live[parcel_id].kind"), "の形だけです")


def test_ページのある問い合わせは_続きの印を受ける():
    text = scoped()
    ck = ck_of(text)
    from geas.hostgen import definitions
    from geas.spec import build
    d = definitions(ck)
    assert d["queries"]["parcels_paged"]["paged"] is True and "paged" not in d["queries"]["live_parcels"]
    route = next(r for r in d["routes"] if r["target"] == "parcels_paged")
    assert route["q_inputs"] == [{"name": "kind", "optional": True}, {"name": "after", "optional": True}]
    assert any(row[1] == "/parcel_pages?q[kind?]=…&q[after?]=…" for row in build(ck)["ルート"])
    assert has(text.replace("query parcels_paged {\n  input kind: Kind", "query parcels_paged {\n  input kind: Kind, after: Kind"), "入力の名前に after は使えません")


def test_並びを2行以上書くと_呼ぶ側が選べる():
    text = scoped()
    ck = ck_of(text)
    assert errors(text) == []
    from geas.hostgen import definitions
    from geas.spec import build
    route = next(r for r in definitions(ck)["routes"] if r["target"] == "parcel_ids_paged")
    assert route["q_inputs"] == [{"name": "order", "optional": True}, {"name": "after", "optional": True}]
    spec = build(ck)
    assert any(row[1] == "/parcel_id_pages?q[order?]=…&q[after?]=…" for row in spec["ルート"])
    row = next(x for x in spec["問い合わせ"] if x["問い合わせ"] == "小包の ID を 1 件ずつ")
    assert row["並び"] == "fee 小さい順（q[order]=fee.asc。選ばないとき）、fee 大きい順（q[order]=fee.desc）"
    assert next(x for x in spec["問い合わせ"] if x["問い合わせ"].startswith("小包を 2 件ずつ"))["並び"] == "fee 大きい順"
    assert has(text.replace("  order fee asc\n  order fee desc\n", "  order fee asc\n  order fee asc\n"), "を 2 回書いています")
    assert has(text.replace("query parcel_ids_paged {\n", "query parcel_ids_paged {\n  input order: Kind\n  filter kind equals order\n"), "入力の名前に order は使えません")


def test_事情のあるPOSTで受ける問い合わせ():
    text = scoped()
    ck = ck_of(text)
    assert errors(text) == []
    from geas.hostgen import definitions
    from geas.mcp import tool_defs
    from geas.spec import build, render
    route = next(r for r in definitions(ck)["routes"] if r["target"] == "parcels_quietly")
    assert (route["method"], route["path"], route["kind"], route["post"], route["for"]) == ("POST", "/quiet_parcel_searches", "index", True, ["Staff"])
    spec = build(ck)
    assert spec["POSTの問い合わせ"] == [("/quiet_parcel_searches", "種類と料金で小包を探す（条件を URL に載せない）", "探している種類を、URL とログに残さない")]
    assert any(row[0] == "POST" and row[1] == '/quiet_parcel_searches（本文 {"q": {kind, min_fee?, after?}}）' for row in spec["ルート"])
    assert "## POST で受ける問い合わせ" in render(ck, spec, "") and "探している種類を、URL とログに残さない" in render(ck, spec, "")
    tool = next(t for t in tool_defs(ck) if t["geas"]["path"] == "/quiet_parcel_searches")
    assert tool["geas"]["post_q"] is True and tool["annotations"]["readOnlyHint"] is True
    one = 'index parcels_quietly for Staff post "探している種類を、URL とログに残さない"'
    with pytest.raises(ParseError, match="理由"):
        parse(text.replace(one, "index parcels_quietly for Staff post"), "x")
    assert errors(text.replace(one, 'index parcels_quietly post "理由" for Staff')) == []  # for は、post の後ろでもよい
    assert has(text.replace(one, one + "\n  create claim for Staff {\n    Claimed → 201\n    NoParcel → 404\n  }").replace("resource Claims {\n  create claim for Staff {\n    Claimed → 201\n    NoParcel → 404\n  }\n}", ""), "create は書けません")
    assert has(text.replace("show my_parcels_of_kind for Customer", 'show my_parcels_of_kind for Customer post "理由"'), "index だけです")
