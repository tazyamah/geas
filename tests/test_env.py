"""動かす前提（needs と env-checks）：アプリが実行環境に要求すること。導出する項目と、人が理由つきで書く項目。"""
import json
import subprocess
import sys
from pathlib import Path

import pytest

from geas.check import ERROR, WARN, check
from geas.envreq import items
from geas.parser import ParseError, parse
from geas.spec import build, render

ROOT = Path(__file__).parent.parent
SHOP = (ROOT / "tests" / "fixtures" / "shop" / "shop.geas").read_text()
DEMO = (ROOT / "tests" / "fixtures" / "runtime_demo" / "runtime_demo.geas").read_text()


def chk(src):
    return check(parse(src))


def env_errors(src):
    return [d.msg for d in chk(src).diags if d.code == "環境" and d.level == ERROR]


def ids(src):
    return [x["id"] for x in items(parse(src))]


def test_導出される項目は_宣言から決まり_番号は導出元の名前になる():
    got = {x["id"]: x for x in items(parse(SHOP))}
    assert list(got) == ["instances", "durable.default", "reach.Payment"]
    assert got["instances"]["観点"] == "安全性" and "同じ機械" in got["instances"]["要求"]
    assert got["reach.Payment"]["確かめ方"] == ["実環境"] and "導出" in got["reach.Payment"]["由来"]
    assert not any(x["人の保証"] for x in got.values())


def test_isolation_process_の保存先があると_同時に動くのは1台になる():
    src = SHOP + "\nstorage Orders isolation process\nstore Order in Orders\n"
    got = {x["id"]: x for x in items(parse(src))}
    assert "1 台" in got["instances"]["要求"] and "isolation process" in got["instances"]["由来"]
    assert {"durable.default", "durable.Orders"} <= set(got)


def test_needs_secret_は理由つきで書き_外部作用の名前空間だけが秘密を使える():
    src = SHOP + '\nneeds secret Payment.api_key "決済事業者の API 鍵"\n'
    got = {x["id"]: x for x in items(parse(src))}
    x = got["secret.Payment.api_key"]
    assert x["観点"] == "機密" and "人が書いた" in x["由来"] and x["理由"] == "決済事業者の API 鍵"
    assert not env_errors(src)
    assert any("名前空間がありません" in m for m in env_errors(SHOP + '\nneeds secret Shipping.api_key "x"\n'))
    assert any("lower_snake" in m for m in env_errors(SHOP + '\nneeds secret Payment.鍵 "x"\n'))  # 識別子は半角英数
    assert any("2つ以上" in m for m in env_errors(SHOP + '\nneeds secret Payment.api_key "a"\nneeds secret Payment.api_key "b"\n'))


def test_needs_は理由が必須で_書き方の誤りは止まる():
    for bad, pat in (("needs secret Payment.api_key", "理由"), ('needs exposure open "x"', "public"), ('needs exposure public', "理由"),
                     ('needs instances at_most 0 "x"', "1 以上"), ('needs instances 3 "x"', "at_most"), ('needs cpu 2 "x"', "needs は")):
        with pytest.raises(ParseError, match=pat):
            parse(SHOP + "\n" + bad + "\n")


def test_instances_は導出した上限より緩く書けず_厳しくは書ける():
    base = SHOP + "\nstorage Orders isolation process\nstore Order in Orders\n"
    assert any("より緩い" in m for m in env_errors(base + 'needs instances at_most 3 "x"\n'))
    assert not env_errors(base + 'needs instances at_most 1 "二重に動かさない"\n')
    free = SHOP + 'needs instances at_most 3 "決済事業者の上限が 3 並列"\n'  # 導出の上限が無ければ、人が書いた上限がそのまま使われる
    assert not env_errors(free)
    got = {x["id"]: x for x in items(parse(free))}
    assert "3 台以下" in got["instances"]["要求"] and got["instances"]["理由"] == "決済事業者の上限が 3 並列"


def test_公開する口があるアプリは_公開範囲を書かないと警告し_書けば項目と前段の人の保証が出る():
    warns = lambda src: [d for d in chk(src).diags if d.code == "環境" and d.level == WARN]
    assert len(warns(DEMO)) == 1 and "公開範囲" in warns(DEMO)[0].msg
    src = DEMO + '\nneeds exposure internal "社内の運送担当だけが使う"\n'
    assert not warns(src)
    got = {x["id"]: x for x in items(parse(src))}
    assert "届く口が無い" in got["exposure"]["要求"] and got["exposure"]["観点"] == "到達性"
    assert got["limits"]["人の保証"] and got["limits"]["確かめ方"] == ["人の保証"]  # 前段の約束は外から確かめられない
    assert "時間切れ" in got["limits"]["要求"] and "本文" not in got["limits"]["要求"]  # 本文の上限はサーバーが 413 で守るので、前段には求めない
    assert "limits" not in ids(SHOP)  # 公開する口が無ければ前段の約束も出ない


def test_仕様書に動かす前提の節が出て_行は一覧と同じ():
    ck = chk(SHOP + '\nneeds secret Payment.api_key "決済事業者の API 鍵"\n')
    md = render(ck, build(ck))
    assert "## 動かす前提" in md and "secret.決済.api_key" in md and "照合する器はまだ無い" in md  # 名前は表示名に置き換わって出る
    assert [r[0] for r in build(ck)["動かす前提"]] == [ck.p.show_names(x["id"]) for x in items(ck.p)]  # 仕様書は表示名、一覧（JSON）はコードの名前


def test_env_checks_は_JSON_で一覧を出し_検査のエラーがあれば止まる(tmp_path):
    f = tmp_path / "a.geas"
    f.write_text(SHOP + '\nneeds secret Payment.api_key "決済事業者の API 鍵"\n')
    r = subprocess.run([sys.executable, "-m", "geas", "env-checks", str(f), "--json"], capture_output=True, text=True, cwd=ROOT)
    assert r.returncode == 0, r.stderr
    j = json.loads(r.stdout)
    assert [x["id"] for x in j["items"]] == ["instances", "durable.default", "reach.Payment", "secret.Payment.api_key"]
    assert set(j["items"][0]) == {"id", "観点", "要求", "由来", "確かめ方", "環境の事実", "人の保証", "理由"}
    t = subprocess.run([sys.executable, "-m", "geas", "env-checks", str(f)], capture_output=True, text=True, cwd=ROOT)
    assert "secret.Payment.api_key（機密）" in t.stdout
    f.write_text(SHOP + '\nneeds secret Shipping.api_key "x"\n')
    r = subprocess.run([sys.executable, "-m", "geas", "env-checks", str(f), "--json"], capture_output=True, text=True, cwd=ROOT)
    assert r.returncode == 1 and r.stdout == "" and "名前空間がありません" in r.stderr
