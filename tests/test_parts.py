# SPDX-License-Identifier: GPL-3.0-or-later
"""部品（library）の境界値の検査（geas/partcheck.py）：宣言を、根の言語で実際に呼んで確かめる。
標準部品（geas/std/num.geas）は、全部が組めて、panic せず、範囲に収まり、別に書いた手本（geas/stdref.py）と合うこと。
cargo が無い環境では飛ばす。"""
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

from geas.check import check
from geas.parser import parse
from geas.partcheck import check_parts
from geas.stdref import REF, reference

pytestmark = pytest.mark.skipif(shutil.which("cargo") is None, reason="cargo が無い")

NUM = Path(__file__).resolve().parent.parent / "geas" / "std" / "num.geas"


def std_parts(tmp_path):
    ck = check(parse(f'use "{NUM.as_posix()}"\n', str(tmp_path / "all.geas")))
    assert ck.ok, ck.diags
    return ck.p.parts


def test_標準部品は全部が組めて_端の値で_panic_せず_範囲に収まり_手本と合う(tmp_path):
    parts = std_parts(tmp_path)
    rep = check_parts(parts, tmp_path / "crate", reference(parts))
    assert rep.ok, [(f.part, f.kind, f.detail) for f in rep.findings][:10]
    assert not rep.skipped, rep.skipped
    # 対応表：標準部品のどれにも手本がある（部品を足して手本を足し忘れると、ここで止まる）
    assert set(rep.calls) == {pt.name for pt in parts} == rep.compared
    # 使われていない手本も残さない
    used = {pt.name.split("_", 2)[2] for pt in parts}
    assert set(REF) == used, set(REF) ^ used


BAD = """ltype Count = int 0..100
ltype Small = int 0..9

library good(x: Count) -> Count from rust "std" "" call "x / 2" guarantee result <= x trust check "半分にするだけ"
library overflow(x: Count) -> Count from rust "std" "" call "x * 3" trust check "3 倍するだけ"
library too_wide(x: Count) -> Small from rust "std" "" call "x / 2" trust check "半分にするだけ"
library broken_promise(x: Count) -> Count from rust "std" "" call "x / 2" guarantee result >= x trust check "半分にするだけ"
library no_such(x: Count) -> Count from rust "std" "" call "x.no_such_method()" trust check "無い関数"
library by_zero(x: Count, y: Count) -> Count from rust "std" "" call "x / y" trust check "割るだけ"
"""


def test_宣言と違う部品を_種類ごとに見つける(tmp_path):
    rep = check_parts(check(parse(BAD)).p.parts, tmp_path)
    kinds = {}
    for f in rep.findings:
        kinds.setdefault(f.part, set()).add(f.kind)
    assert "good" not in kinds and rep.calls["good"] > 0
    assert "panic" in kinds["overflow"]            # 100 * 3 は u8 に入らない（デバッグビルドで呼ぶので、あふれが panic になる）
    assert "範囲外" in kinds["overflow"]             # 50 * 3 = 150 は u8 に入るが、宣言の 0..100 を出る
    assert kinds["too_wide"] == {"範囲外"}           # 100 / 2 = 50 は 0..9 を出る
    assert kinds["broken_promise"] == {"保証違反"}
    assert kinds["no_such"] == {"組めない"} and "no_such" not in rep.calls
    assert kinds["by_zero"] == {"panic"}
    detail = next(f.detail for f in rep.findings if f.part == "by_zero")
    assert detail.endswith(",0)"), detail          # どの引数で起きたかが分かる


def test_標準部品の手本が違えば_手本と違うと出る(tmp_path):
    parts = [pt for pt in std_parts(tmp_path) if pt.name == "Num_u8_wrapping_add"]
    rep = check_parts(parts, tmp_path / "crate", {"Num_u8_wrapping_add": lambda a, b: a + b})  # あふれを切り捨てない、誤った手本
    assert {f.kind for f in rep.findings} == {"手本と違う"}
    assert any("255,1" in f.detail for f in rep.findings)


def test_parts_コマンドは_食い違いがあれば終了コード_1(tmp_path):
    src = tmp_path / "bad.geas"
    src.write_text(BAD, encoding="utf-8")
    r = subprocess.run([sys.executable, "-m", "geas", "parts", str(src), "--all"], capture_output=True, text=True, encoding="utf-8")
    assert r.returncode == 1, r.stdout + r.stderr
    assert "overflow：panic" in r.stdout and "食い違う部品が 5 個" in r.stdout
    r = subprocess.run([sys.executable, "-m", "geas", "parts", str(src)], capture_output=True, text=True, encoding="utf-8")
    assert r.returncode == 0 and "確かめる部品（library）がありません" in r.stdout  # 呼ばれていない部品は、--all を付けたときだけ


# ---- 台帳：検査の結果を、仕様書の「人の保証」に出す ----
APP = """ltype Count = int 0..100

entity Tray {
  tray_id: id unique
  n: Count required
}
result PutResult {
  Done(tray_id: id)
}
library half(x: Count) -> Count from rust "std" "" call "x / 2" guarantee result <= x trust check "半分にするだけ"
deed put_tray {
  input n: Count required
  writes Tray
  result PutResult
  effects none

  let t = create Tray { n: half(n) }
  return Done(t.tray_id)
}
nexus Filling {
  put_tray: Done → reply
}
"""


def geas(*args):
    return subprocess.run([sys.executable, "-m", "geas", *args], capture_output=True, text=True, encoding="utf-8")


def test_仕様書の人の保証に_宣言を実際に呼んで確かめたかが出る(tmp_path):
    src = tmp_path / "app.geas"
    src.write_text(APP, encoding="utf-8")
    row = lambda: next(ln for ln in geas("spec", str(src)).stdout.splitlines() if "ライブラリ half" in ln)
    assert "宣言は実際に呼んで確かめていない（geas parts --record）" in row()
    r = geas("parts", str(src), "--record")
    assert r.returncode == 0 and (tmp_path / "app.parts.json").exists(), r.stdout + r.stderr
    assert "宣言は端の値で 14 回呼んで確かめた" in row()   # 両端とその隣、0 の周り、真ん中、ビット幅の境目のうち、0..100 に入る 14 個
    # 宣言（範囲・式・guarantee）を変えたら、確かめ直すまで「通過」とは出さない。理由の文章だけの変更は、宣言の変更に数えない
    src.write_text(APP.replace("半分にするだけ", "2 で割るだけ"), encoding="utf-8")
    assert "宣言は端の値で 14 回呼んで確かめた" in row()
    src.write_text(APP.replace('call "x / 2"', 'call "x / 3"'), encoding="utf-8")
    assert "宣言を変えたあと、確かめ直していない" in row()
    # 食い違いのある部品は、台帳に載らない
    src.write_text(APP.replace("guarantee result <= x", "guarantee result > x"), encoding="utf-8")
    assert geas("parts", str(src), "--record").returncode == 1
    assert "宣言は実際に呼んで確かめていない" in row()


def test_標準部品は_同梱の台帳から_確かめた内容が仕様書に出る(tmp_path):
    import json
    from geas.partcheck import STD_LEDGER, fingerprint
    ledger = json.loads(STD_LEDGER.read_text(encoding="utf-8"))["parts"]
    parts = std_parts(tmp_path)
    # 台帳は、いまの宣言の全部と合っている（num.geas を作り直して台帳を忘れると、ここで止まる）
    assert {pt.name: fingerprint(pt) for pt in parts} == {n: row["fingerprint"] for n, row in ledger.items()}
    assert all(row["reference"] and row["calls"] > 0 for row in ledger.values())
    from test_geas import STD_APP
    shutil.copy(NUM, tmp_path / "num.geas")
    (tmp_path / "app.geas").write_text(STD_APP, encoding="utf-8")
    out = geas("spec", str(tmp_path / "app.geas")).stdout
    row = next(ln for ln in out.splitlines() if "u8_wrapping_add" in ln and "ライブラリ" in ln)
    assert "宣言は全部の入力で証明した（Kani：式どおりで、panic しない）" in row, row


def test_標準部品の証明は_部品ごとに作られ_式は実在のメソッドにだけある(tmp_path):
    sys.path.insert(0, str(NUM.parent.parent.parent / "proofs"))
    import std_proofs
    parts = std_proofs.std_parts()
    src = std_proofs.source(parts)
    methods = {pt.name.split("_", 2)[2] for pt in parts}
    assert set(std_proofs.SPEC) <= methods, set(std_proofs.SPEC) - methods
    assert all(f"fn safe_{pt.name}()" in src for pt in parts)
    assert src.count("fn eq_") == sum(1 for pt in parts if pt.name.split("_", 2)[2] in std_proofs.SPEC)
    assert "fn eq_Num_i16_saturating_add()" in src and "(A + B).clamp(LO, HI)" in src
