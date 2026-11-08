# SPDX-License-Identifier: GPL-3.0-or-later
"""前置きの証明（proofs/prelude_proofs.rs）を、Kani で実行する。

使い方: python proofs/run.py [--timeout 秒]    （cargo install --locked kani-verifier && cargo kani setup が済んでいること）
生成物の前置き（geas.rustgen.PRELUDE）に証明を連結した crate を、一時フォルダに作って `cargo kani` を回す。
通常のテスト（pytest）には入れない。Kani が無い環境では、tests/test_proofs.py が飛ばされる。
"""
import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
from geas.rustgen import PRELUDE  # noqa: E402

CARGO = """[package]
name = "prelude_proofs"
version = "0.1.0"
edition = "2021"
[lints.rust]
unexpected_cfgs = { level = "warn", check-cfg = ['cfg(kani)'] }
"""


def crate(dest: Path) -> None:
    (dest / "src").mkdir(parents=True)
    (dest / "Cargo.toml").write_text(CARGO)
    (dest / "src/lib.rs").write_text(PRELUDE + "\n" + (ROOT / "proofs/prelude_proofs.rs").read_text())


def harnesses() -> list[str]:
    import re
    return re.findall(r"#\[kani::proof\]\s*fn (\w+)", (ROOT / "proofs/prelude_proofs.rs").read_text())


def run(timeout: int = 300) -> int:
    """証明を 1 つずつ実行する。戻り値は、失敗した（または時間内に終わらなかった）証明の数"""
    bad = 0
    with tempfile.TemporaryDirectory() as d:
        crate(Path(d))
        for h in harnesses():
            try:
                r = subprocess.run(["cargo", "kani", "--harness", h], cwd=d, capture_output=True, text=True, timeout=timeout)
                ok = r.returncode == 0 and "VERIFICATION:- SUCCESSFUL" in r.stdout
                note = "成功" if ok else "失敗\n" + "\n".join(x for x in r.stdout.splitlines() if "Failed Checks" in x or "VERIFICATION" in x)
            except subprocess.TimeoutExpired:
                ok, note = False, f"{timeout} 秒で終わらず"
            print(f"{h}: {note}")
            bad += 0 if ok else 1
    print("前置きの証明: " + ("すべて成功" if not bad else f"{bad} 件が成功していません"))
    return bad


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--timeout", type=int, default=300, help="証明 1 つあたりの上限（秒）")
    sys.exit(1 if run(ap.parse_args().timeout) else 0)
