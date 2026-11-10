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
