"""前置きの証明（proofs/）。Kani が入っている環境でだけ実際に証明を回す。入っていなければ、証明の置き場所だけを確かめる"""
import re
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

from geas.rustgen import PRELUDE

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "proofs"))
import run as proofs_run  # noqa: E402


def kani_ready() -> bool:
    if not shutil.which("cargo"):
        return False
    return subprocess.run(["cargo", "kani", "--version"], capture_output=True).returncode == 0


def test_証明が使う関数は前置きにあり_証明は1件以上ある():
    src = (ROOT / "proofs/prelude_proofs.rs").read_text()
    names = proofs_run.harnesses()
    assert len(names) >= 8 and {"gtin_ok_exact_13", "first_bad_char_exact", "fold_width_kana_exact"} <= set(names)  # unwind などの属性が付いた証明も拾う
    for f in sorted(set(re.findall(r"\b(range_ok|ratio_ok|floor|ceil|half_up|mul_ratio|scale|first_bad_in|gtin_chars_ok|fold_width_kana)\(", src))):
        assert re.search(rf"pub fn {f}[(<]", PRELUDE), f  # ジェネリクスの関数は < が続く


@pytest.mark.skipif(not kani_ready(), reason="Kani が入っていない（cargo install --locked kani-verifier && cargo kani setup）")
def test_前置きの証明がすべて通る():
    assert proofs_run.run(timeout=300) == 0
