# SPDX-License-Identifier: GPL-3.0-or-later
"""Rust の標準ライブラリの公開 API（rustdoc の JSON）から、標準部品（library の宣言）を機械で作る。

選ぶ関数は、型に写せて、止まらず、panic しないと「宣言から」言えるものだけ：
- 整数型（i8 i16 i32 i64 u8 u16 u32）の固有メソッドで、self を値で受け、引数と戻り値が整数型の仲間（Self か上の型）か bool のもの
- 安定版（stable）で、廃止（deprecated）でなく、unsafe でなく、型引数を持たない
- ドキュメントに panic の記述も Safety の節もなく（strict_・unchecked_ で始まる名前も除く）、オーバーフローで panic する印（RustcInheritOverflowChecks）が付いていない
戻り値が Option・タプルのものは、geas の型に写せないので入れない（bool は bool 型に写す）。
戻り値の範囲は Rust の型から決まる（u8 なら 0..255）。引数にも同じ範囲を課すので、呼ぶ側は範囲に収まると示すか、収めて渡す。

使い方: python -m geas.stdgen [core.json] > geas/std/num.geas
"""
import json
import subprocess
import sys
from pathlib import Path

# geas の整数は i64 に収まる範囲。u64・u128・i128・usize・isize は、範囲が収まらないか実行環境で変わるので入れない
RANGES = {
    "i8": (-(2**7), 2**7 - 1), "i16": (-(2**15), 2**15 - 1), "i32": (-(2**31), 2**31 - 1), "i64": (-(2**63), 2**63 - 1),
    "u8": (0, 2**8 - 1), "u16": (0, 2**16 - 1), "u32": (0, 2**32 - 1),
}
LETTERS = "abcdefgh"


def default_json() -> Path:
    """nightly の rust-docs-json が入れる core.json（rustup component add rust-docs-json --toolchain nightly）"""
    for toolchain in ["nightly", *sorted(p.name for p in (Path.home() / ".rustup/toolchains").glob("nightly-*"))]:
        try:
            sysroot = subprocess.check_output(["rustc", f"+{toolchain}", "--print", "sysroot"], text=True, stderr=subprocess.DEVNULL).strip()
        except (OSError, subprocess.CalledProcessError):
            continue
        path = Path(sysroot) / "share/doc/rust/json/core.json"
        if path.exists():
            return path
    raise FileNotFoundError("core.json がありません（rustup component add rust-docs-json --toolchain nightly）")


def ltype_name(prim: str) -> str:
    return "bool" if prim == "bool" else prim.upper()


def eligible(item: dict) -> bool:
    """宣言だけから、止まらず panic しないと言える関数か"""
    fn = item["inner"].get("function") if isinstance(item["inner"], dict) else None
    if fn is None or item.get("visibility") != "public" or item.get("deprecation"):
        return False
    if fn["header"]["is_unsafe"] or fn["header"]["is_async"] or fn["generics"]["params"] or fn["generics"]["where_predicates"]:
        return False
    stability = (item.get("stability") or {}).get("level") or {}
    if "stable" not in stability:
        return False
    docs = item.get("docs") or ""
    if "panic" in docs.lower() or "# Safety" in docs or item["name"].startswith(("strict_", "unchecked_")):
        return False
    if any("OverflowChecks" in json.dumps(a) for a in item.get("attrs", [])):
        return False
    return True


def sig_types(fn: dict, self_prim: str):
    """(引数の型の並び, 戻り値の型)。写せなければ None。型は整数の名前（i32 など）"""
    def prim(t):
        if t.get("generic") == "Self":
            return self_prim
        p = t.get("primitive")
        return p if p in RANGES or p == "bool" else None

    inputs = fn["sig"]["inputs"]
    if not inputs or inputs[0][0] != "self" or inputs[0][1].get("generic") != "Self":
        return None
    ps = [prim(t) for _, t in inputs]
    out = fn["sig"]["output"]
    r = prim(out) if out else None
    if r is None or any(p is None for p in ps):
        return None
    return ps, r


def collect(doc: dict) -> list[tuple]:
    """[(整数型, メソッド名, 引数の型の並び, 戻り値の型, 説明の1行目)]"""
    idx = doc["index"]
    out = []
    for it in idx.values():
        im = it["inner"].get("impl") if isinstance(it["inner"], dict) else None
        if not im or im["trait"] is not None:
            continue
        prim = im["for"].get("primitive")
        if prim not in RANGES:
            continue
        for m in im["items"]:
            mi = idx[str(m)]
            if not eligible(mi):
                continue
            t = sig_types(mi["inner"]["function"], prim)
            if t is None:
                continue
            first = (mi.get("docs") or "").strip().split("\n")[0]
            out.append((prim, mi["name"], tuple(t[0]), t[1], first))
    return sorted(set(out), key=lambda r: (r[0], r[1]))


def render(rows: list[tuple]) -> str:
    L = ["module Num", "",
         "// 自動生成（python -m geas.stdgen）。手で編集しない。Rust の標準ライブラリ（core）の整数型のメソッドのうち、",
         "// 宣言から panic も繰り返しもしないと言えるものだけ。範囲は Rust の型から決める。", ""]
    for prim, (lo, hi) in RANGES.items():
        L.append(f"ltype {ltype_name(prim)} = int {lo}..{hi}")
    L.append("")
    for prim, name, args, ret, doc in rows:
        params = ", ".join(f"{LETTERS[i]}: {ltype_name(a)}" for i, a in enumerate(args))
        call_args = [f"({LETTERS[i]} as {a})" for i, a in enumerate(args)]
        call = f"{call_args[0]}.{name}({', '.join(call_args[1:])})"
        expr = call if ret == "bool" else f"({call}) as {ret}"
        why = f"Rust 標準ライブラリ core の {prim}::{name}（rustdoc から生成。Panics・unsafe・オーバーフロー検査の付く関数は除外済み）"
        L.append(f'library {prim}_{name}({params}) -> {ltype_name(ret)} from rust "std" "" call "{expr}" trust check "{why}"')
    return "\n".join(L) + "\n"


def main(argv):
    path = Path(argv[1]) if len(argv) > 1 else default_json()
    sys.stdout.write(render(collect(json.loads(path.read_text()))))


if __name__ == "__main__":
    main(sys.argv)
