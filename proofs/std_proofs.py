# SPDX-License-Identifier: GPL-3.0-or-later
"""標準部品（geas/std/num.geas）の証明を、Kani で実行する。

部品の境界値の検査（python -m geas parts）は、端の値で呼ぶだけ。ここでは、引数の型の全部の値について示す。
部品ごとに、証明を 2 段で試す：
- 式どおり：戻り値が、桁の限りを気にしない計算（i128）で書いた式と、全部の入力で等しい（下の SPEC に式がある部品だけ）
- panic しない：全部の入力で、panic も計算のあふれも起きない
時間内に終わらない証明は、成功に数えない（その部品は、境界値の検査だけが根拠のまま）。

使い方: python proofs/std_proofs.py [--timeout 秒] [--jobs 数] [--record]
        （cargo install --locked kani-verifier && cargo kani setup が済んでいること）
--record を付けると、境界値の検査も回して、台帳（geas/std/num.parts.json）に証明した内容を書く。仕様書の「人の保証」に出る。
通常のテスト（pytest）には入れない。
"""
import argparse
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))
from geas.check import check, phys  # noqa: E402
from geas.parser import parse  # noqa: E402
from geas.stdref import NAME  # noqa: E402

NUM = ROOT / "geas" / "std" / "num.geas"
EQ, SAFE = "式どおりで、panic しない", "panic しない"

# メソッド名 → 戻り値 R が満たす式（Rust）。A・B は引数、R は戻り値で、どれも i128（真偽の戻り値は bool のまま）。
# LO・HI は self の型の範囲、M は 2 のビット数乗。Rust の整数のメソッドとは別の書き方（桁の限りを気にしない計算）で書く
WRAP = "(({v}) - LO).rem_euclid(M) + LO"
SPEC = {
    "abs_diff": "R == (A - B).abs()",
    "cast_signed": "R == if A >= M / 2 { A - M } else { A }",
    "cast_unsigned": "R == A.rem_euclid(M)",
    "is_negative": "R == (A < 0)",
    "is_positive": "R == (A > 0)",
    "is_power_of_two": "R == (A > 0 && (A & (A - 1)) == 0)",
    "isqrt": "R >= 0 && R * R <= A && (R + 1) * (R + 1) > A",
    "midpoint": "R == (A + B) / 2",  # i128 の / は 0 の側へ丸める
    "saturating_abs": "R == A.abs().clamp(LO, HI)",
    "saturating_add": "R == (A + B).clamp(LO, HI)",
    "saturating_add_signed": "R == (A + B).clamp(LO, HI)",
    "saturating_add_unsigned": "R == (A + B).clamp(LO, HI)",
    "saturating_sub": "R == (A - B).clamp(LO, HI)",
    "saturating_sub_signed": "R == (A - B).clamp(LO, HI)",
    "saturating_sub_unsigned": "R == (A - B).clamp(LO, HI)",
    "saturating_mul": "R == (A * B).clamp(LO, HI)",
    "saturating_neg": "R == (-A).clamp(LO, HI)",
    "signum": "R == if A > 0 { 1 } else if A < 0 { -1 } else { 0 }",
    "to_le": "R == A || cfg!(target_endian = \"big\")",
    "wrapping_abs": "R == " + WRAP.format(v="A.abs()"),
    "wrapping_add": "R == " + WRAP.format(v="A + B"),
    "wrapping_add_signed": "R == " + WRAP.format(v="A + B"),
    "wrapping_add_unsigned": "R == " + WRAP.format(v="A + B"),
    "wrapping_sub": "R == " + WRAP.format(v="A - B"),
    "wrapping_sub_signed": "R == " + WRAP.format(v="A - B"),
    "wrapping_sub_unsigned": "R == " + WRAP.format(v="A - B"),
    "wrapping_mul": "R == " + WRAP.format(v="A * B"),
    "wrapping_neg": "R == " + WRAP.format(v="-A"),
}

CARGO = """[package]
name = "std_proofs"
version = "0.1.0"
edition = "2021"
[lints.rust]
unexpected_cfgs = { level = "warn", check-cfg = ['cfg(kani)'] }
"""


def std_parts() -> list:
    with tempfile.TemporaryDirectory() as tmp:
        ck = check(parse(f'use "{NUM.as_posix()}"\n', str(Path(tmp) / "all.geas")))
    return [pt for pt in ck.p.parts if pt.external]


def rust_ty(t) -> str:
    return "bool" if t.kind == "真偽" else phys(t)


def source(parts: list) -> str:
    """証明の Rust。部品ごとに safe_名前（panic しない）と、式があれば eq_名前（式どおり）"""
    L = ["#![allow(warnings)]", "#[cfg(kani)]", "mod proofs {"]
    for pt in parts:
        m = NAME.match(pt.name)
        bits, signed, method = int(m.group(2)), m.group(1) == "i", m.group(3)
        lo, hi = (-(1 << (bits - 1)), (1 << (bits - 1)) - 1) if signed else (0, (1 << bits) - 1)
        anys = " ".join(f"let {n}: {rust_ty(t)} = kani::any();" for n, t in pt.params)
        call = f"let r: {rust_ty(pt.ret)} = {{ {pt.lib[2]} }};"
        L.append(f"    #[kani::proof]\n    fn safe_{pt.name}() {{ {anys} {call} }}")
        if method in SPEC:
            names = [n for n, _ in pt.params]
            binds = f"let A = {names[0]} as i128;" + (f" let B = {names[1]} as i128;" if len(names) > 1 else "")
            ret = "let R = r;" if pt.ret.kind == "真偽" else "let R = r as i128;"
            L.append(f"    #[kani::proof]\n    fn eq_{pt.name}() {{ {anys} {call} {binds} {ret} "
                     f"let LO: i128 = {lo}; let HI: i128 = {hi}; let M: i128 = {1 << bits}; assert!({SPEC[method]}); }}")
    L.append("}")
    return "\n".join(L) + "\n"


def run(timeout: int = 60, jobs: int = 4) -> dict:
    """部品名 → 証明できた内容（EQ か SAFE）。どちらも示せなかった部品は入らない"""
    parts = std_parts()
    with tempfile.TemporaryDirectory() as d:
        (Path(d) / "src").mkdir()
        (Path(d) / "Cargo.toml").write_text(CARGO)
        (Path(d) / "src/lib.rs").write_text(source(parts), encoding="utf-8")
        r = subprocess.run(["cargo", "kani", "-j", str(jobs), "--output-format", "terse", "-Z", "unstable-options",
                            "--harness-timeout", f"{timeout}s"], cwd=d, capture_output=True, text=True)
    done = re.search(r"Complete - (\d+) successfully verified harnesses, (\d+) failures, (\d+) total", r.stdout)
    if not done:
        raise RuntimeError("Kani が証明を実行できませんでした:\n" + (r.stdout + r.stderr)[-3000:])
    failed = set(re.findall(r"Verification failed for - proofs::(\w+)", r.stdout))
    out = {}
    for pt in parts:
        method = NAME.match(pt.name).group(3)
        safe = f"safe_{pt.name}" not in failed
        if safe and method in SPEC and f"eq_{pt.name}" not in failed:
            out[pt.name] = EQ
        elif safe:
            out[pt.name] = SAFE
    return out


def report(proved: dict, parts: list) -> str:
    eq = sum(1 for v in proved.values() if v == EQ)
    rest = sorted(pt.name for pt in parts if pt.name not in proved)
    L = [f"標準部品の証明：{len(parts)} 個のうち、式どおりと示せたもの {eq} 個、panic しないとだけ示せたもの {len(proved) - eq} 個、示せなかったもの {len(rest)} 個"]
    L += [f"  － {n}：時間内に示せませんでした（境界値の検査だけが根拠）" for n in rest]
    lost = sorted(pt.name for pt in parts if NAME.match(pt.name).group(3) in SPEC and proved.get(pt.name) == SAFE)
    L += [f"  － {n}：式どおりかは、時間内に示せませんでした（panic しないことは示した）" for n in lost]
    return "\n".join(L)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--timeout", type=int, default=60, help="証明 1 つあたりの上限（秒）")
    ap.add_argument("--jobs", type=int, default=4, help="同時に回す証明の数")
    ap.add_argument("--record", action="store_true", help="境界値の検査も回して、台帳（geas/std/num.parts.json）に書く")
    a = ap.parse_args()
    parts = std_parts()
    proved = run(a.timeout, a.jobs)
    print(report(proved, parts))
    if a.record:
        from geas.partcheck import STD_LEDGER, check_parts, ledger_of
        from geas.stdref import reference
        with tempfile.TemporaryDirectory() as tmp:
            rep = check_parts(parts, Path(tmp), reference(parts))
        if not rep.ok:
            print("境界値の検査で食い違いがあるので、台帳は書きません", file=sys.stderr)
            return 1
        STD_LEDGER.write_text(json.dumps(ledger_of(rep, parts, proved), ensure_ascii=False, indent=1, sort_keys=True) + "\n", encoding="utf-8")
        print(f"台帳を書きました: {STD_LEDGER}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
