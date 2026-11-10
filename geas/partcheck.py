# SPDX-License-Identifier: GPL-3.0-or-later
"""部品（library）の境界値の検査：宣言だけを頼りに、引数の範囲の端の値で実際に呼んで、宣言どおりかを確かめる。

library は「人の保証」で、検査器は宣言（引数と戻り値の範囲、guarantee、panic しないという理由の文章）を信じて証明に使う。
ここでは、その宣言を、根の言語（Rust）で実際に呼んで確かめる。見るのは次の 5 つ：
- 組めるか（いまの toolchain に、その関数があるか）
- panic しないか（デバッグビルドで呼ぶので、計算のあふれも panic として見つかる）
- 戻り値が、宣言した範囲に収まるか
- guarantee が成り立つか
- 同じ引数で 2 回呼んで、同じ値が返るか（純粋な関数か）
手本（reference：名前 → Python の関数）を渡すと、戻り値を手本とも比べる（標準部品の手本は geas/stdref.py）。

端の値を試すだけなので、「間のどこかで外れる」ことは見つけられない。証明ではなく、宣言の書き間違いを早く見つける網。
引数が整数・金額・真偽でない部品（文字列など）は、まだ確かめられない（skipped に入る）。

使い方: python -m geas parts ファイル.geas
"""
import hashlib
import json
import re
import shutil
import subprocess
from dataclasses import dataclass, field
from pathlib import Path

from .ast import Bin, BoolLit, Call, Cond, Name, Not, Num, show
from .check import phys
from .rustgen import dependencies, rust_type


@dataclass
class Finding:
    part: str
    kind: str  # 組めない｜panic｜範囲外｜保証違反｜結果が変わる｜手本と違う
    detail: str


@dataclass
class Report:
    findings: list = field(default_factory=list)
    calls: dict = field(default_factory=dict)  # 部品名 → 呼んだ回数
    skipped: dict = field(default_factory=dict)  # 部品名 → 確かめられなかった理由
    compared: set = field(default_factory=set)  # 手本と比べた部品

    @property
    def ok(self) -> bool:
        return not self.findings


def bounds(t) -> tuple | None:
    """型の値の範囲（下限, 上限）。整数と金額だけ。範囲を書いていない側は、物理型の端"""
    if t.kind not in ("整数", "金額"):
        return None
    p = phys(t)
    bits = int(p[1:])
    lo, hi = (0, 2**bits - 1) if p[0] == "u" else (-(2 ** (bits - 1)), 2 ** (bits - 1) - 1)
    if t.range is not None:
        lo = lo if t.range.lo is None else max(lo, t.range.lo)
        hi = hi if t.range.hi is None else min(hi, t.range.hi)
    return lo, hi


def edge_values(t) -> list | None:
    """引数に渡す値：範囲の両端とその隣、0 の周り、真ん中、ビット幅の境目。確かめられない型は None"""
    if t.kind == "真偽":
        return [False, True]
    b = bounds(t)
    if b is None:
        return None
    lo, hi = b
    picks = {lo, lo + 1, hi - 1, hi, -1, 0, 1, 2, (lo + hi) // 2, 7, 8, 15, 16, 31, 32, 63, 64, 255, 256}
    return sorted(v for v in picks if lo <= v <= hi)


def cases(pt) -> list | None:
    """引数ごとの、渡す値の並び（全部の組み合わせで呼ぶ）。確かめられない型の引数があれば None"""
    cols = [edge_values(t) for _, t in pt.params]
    return None if any(c is None for c in cols) else cols


def lit(v, t) -> str:
    return ("true" if v else "false") if t.kind == "真偽" else f"{v}{phys(t)}"


def harness(parts: list, case_map: dict) -> tuple[str, dict]:
    """確かめる Rust のプログラムと、行番号 → 部品名（組めない部品を見分けるため、部品 1 つを 1 行に書く）"""
    lines = ["#![allow(warnings)]", "use std::panic::{catch_unwind, set_hook};", ""]
    where = {}
    for pt in parts:
        ps = ", ".join(f"{n}: {rust_type(t)}" for n, t in pt.params)
        where[len(lines) + 1] = pt.name
        lines.append(f"fn p_{pt.name}({ps}) -> {rust_type(pt.ret)} {{ {pt.lib[2]} }}")
    lines += ["", "fn run<T: std::fmt::Display + PartialEq>(name: &str, shown: String, f: impl Fn() -> T + std::panic::RefUnwindSafe) {",
              "    match (catch_unwind(&f), catch_unwind(&f)) {",
              '        (Ok(a), Ok(b)) => println!("{}\t{}\t{}\t{}", name, shown, a, a == b),',
              '        _ => println!("{}\t{}\tPANIC\ttrue", name, shown),',
              "    }", "}", "", "fn main() {", "    set_hook(Box::new(|_| {}));"]
    for pt in parts:  # 部品ごとに、引数ごとの端の値を入れ子の for で回す（呼び出しを 1 つずつ書くと、組むのに時間がかかりすぎる）
        names = [n for n, _ in pt.params]
        fmt = ",".join("{}" for _ in names)
        heads = "".join(f"for {n} in [{', '.join(lit(v, t) for v in vals)}] {{ " for (n, t), vals in zip(pt.params, case_map[pt.name]))
        lines.append(f'    {heads}run("{pt.name}", format!("{fmt}", {", ".join(names)}), || p_{pt.name}({", ".join(names)})); {"}" * len(names)}')
    lines.append("}")
    return "\n".join(lines) + "\n", where


def holds(e, env: dict):
    """guarantee の式を Python で計算する。計算できない形は None"""
    if isinstance(e, Num):
        return e.v
    if isinstance(e, BoolLit):
        return e.v
    if isinstance(e, Name):
        return env.get(e.n)
    if isinstance(e, Not):
        v = holds(e.e, env)
        return None if v is None else not v
    if isinstance(e, Cond):
        c = holds(e.c, env)
        return None if c is None else holds(e.a if c else e.b, env)
    if isinstance(e, Call) and e.n in ("min", "max", "abs"):
        vs = [holds(a, env) for a in e.args]
        return None if None in vs else {"min": min, "max": max}[e.n](vs) if e.n != "abs" else abs(vs[0])
    if isinstance(e, Bin):
        a, b = holds(e.l, env), holds(e.r, env)
        if a is None or b is None:
            return None
        ops = {"+": lambda: a + b, "-": lambda: a - b, "*": lambda: a * b, "<": lambda: a < b, "<=": lambda: a <= b,
               ">": lambda: a > b, ">=": lambda: a >= b, "==": lambda: a == b, "!=": lambda: a != b,
               "かつ": lambda: a and b, "または": lambda: a or b,
               "/": lambda: None if b == 0 else a // b, "%": lambda: None if b == 0 else a % b}
        return ops[e.op]() if e.op in ops else None
    return None


def build_and_run(dest: Path, src: str, deps: dict) -> subprocess.CompletedProcess:
    (dest / "src").mkdir(parents=True, exist_ok=True)
    (dest / "Cargo.toml").write_text('[package]\nname = "geas_parts"\nversion = "0.1.0"\nedition = "2021"\n\n'
                                     + dependencies(deps) + "[profile.dev]\noverflow-checks = true\n", encoding="utf-8")
    (dest / "src" / "main.rs").write_text(src, encoding="utf-8")
    return subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=dest, capture_output=True, text=True,
                          encoding="utf-8", errors="replace")


def check_parts(parts: list, dest: Path, reference: dict | None = None) -> Report:
    """library の部品を、境界値で呼んで確かめる。reference は 部品名 → 手本の関数（引数は geas の値、戻り値は期待する値）"""
    if shutil.which("cargo") is None:
        raise RuntimeError("cargo が無いので、部品を呼んで確かめられません")
    rep = Report()
    todo, case_map = [], {}
    for pt in parts:
        if not pt.external:
            continue
        cs = cases(pt)
        if cs is None or (pt.ret.kind != "真偽" and bounds(pt.ret) is None):
            rep.skipped[pt.name] = "引数か戻り値が、整数・金額・真偽ではない"
            continue
        todo.append(pt)
        case_map[pt.name] = cs
    deps = {}
    for pt in todo:
        if pt.lib[0] != "std":
            deps[pt.lib[0]] = pt.lib[1]
    r = None
    for _ in range(len(todo) + 1):  # 組めない部品を外して、組めるまでやり直す
        if not todo:
            return rep
        src, where = harness(todo, case_map)
        r = build_and_run(dest, src, deps)
        if r.returncode == 0:
            break
        bad = {}  # 部品名 → rustc の最初のエラーの見出し
        for block in re.split(r"\n(?=error)", r.stderr):
            m = re.search(r"--> src/main\.rs:(\d+):", block)
            if m and int(m.group(1)) in where:
                bad.setdefault(where[int(m.group(1))], block.splitlines()[0])
        if not bad:
            raise RuntimeError("部品を確かめるプログラムを組めませんでした:\n" + r.stderr[-2000:])
        for name in sorted(bad):
            rep.findings.append(Finding(name, "組めない", bad[name]))
        todo = [pt for pt in todo if pt.name not in bad]
    by_name = {pt.name: pt for pt in todo}
    for line in r.stdout.splitlines():
        name, shown, got, same = line.split("\t")
        pt = by_name[name]
        rep.calls[name] = rep.calls.get(name, 0) + 1
        call = f"{name}({shown})"
        if got == "PANIC":
            rep.findings.append(Finding(name, "panic", call))
            continue
        if same != "true":
            rep.findings.append(Finding(name, "結果が変わる", call))
        val = (got == "true") if pt.ret.kind == "真偽" else int(got)
        args = [(v == "true") if t.kind == "真偽" else int(v) for v, (_, t) in zip(shown.split(","), pt.params)]
        if pt.ret.kind != "真偽":
            lo, hi = bounds(pt.ret)
            if not lo <= val <= hi:
                rep.findings.append(Finding(name, "範囲外", f"{call} = {val}（宣言は {lo}..{hi}）"))
        if pt.guarantee is not None:
            ok = holds(pt.guarantee, {**{n: v for (n, _), v in zip(pt.params, args)}, "result": val})
            if ok is None:
                rep.skipped.setdefault(name + " の guarantee", f"この形の式は計算できない（{show(pt.guarantee)}）")
            elif not ok:
                rep.findings.append(Finding(name, "保証違反", f"{call} = {val}（guarantee {show(pt.guarantee)}）"))
        if reference and name in reference:
            rep.compared.add(name)
            want = reference[name](*args)
            if want != val:
                rep.findings.append(Finding(name, "手本と違う", f"{call} = {val}（手本は {want}）"))
    return rep


def render(rep: Report) -> str:
    L = [f"部品の境界値の検査：{len(rep.calls)} 個の部品を、合わせて {sum(rep.calls.values())} 回呼びました"
         + (f"（うち {len(rep.compared)} 個は手本とも比べました）" if rep.compared else "")]
    for f in rep.findings:
        L.append(f"  ✕ {f.part}：{f.kind}　{f.detail}")
    for name, why in rep.skipped.items():
        L.append(f"  － {name}：確かめられませんでした（{why}）")
    L.append("宣言と食い違う部品はありません（端の値で確かめた範囲）" if rep.ok else f"宣言と食い違う部品が {len({f.part for f in rep.findings})} 個あります")
    return "\n".join(L)


# ---- 台帳：検査の結果を残し、仕様書の「人の保証」に出す ----
STD_LEDGER = Path(__file__).resolve().parent / "std" / "num.parts.json"


def fingerprint(pt) -> str:
    """宣言の指紋。引数・戻り値の型と範囲、呼ぶ先、式、guarantee のどれかが変わると変わる（名前と理由の文章は含めない）"""
    from .ast import show_type
    text = json.dumps([[show_type(t) for _, t in pt.params], [n for n, _ in pt.params], show_type(pt.ret), pt.lib[0], pt.lib[1], pt.lib[2],
                       "" if pt.guarantee is None else show(pt.guarantee)], ensure_ascii=False)
    return hashlib.sha256(text.encode("utf-8")).hexdigest()[:16]


def toolchain() -> str:
    try:
        return subprocess.run(["rustc", "--version"], capture_output=True, text=True).stdout.strip()
    except OSError:
        return ""


def ledger_of(rep: Report, parts: list, proved: dict | None = None) -> dict:
    """検査の結果を台帳の形にする。食い違いのあった部品と、確かめられなかった部品は「通過」にしない。
    proved は 部品名 → 証明した内容（全部の入力で示したもの。proofs/std_proofs.py が作る）"""
    bad = {f.part for f in rep.findings}
    out = {}
    for pt in parts:
        if pt.name in rep.calls and pt.name not in bad:
            out[pt.name] = {"fingerprint": fingerprint(pt), "calls": rep.calls[pt.name], "reference": pt.name in rep.compared}
            if proved and pt.name in proved:
                out[pt.name]["proved"] = proved[pt.name]
    return {"toolchain": toolchain(), "parts": out}


def ledger_path(code) -> Path:
    return Path(code).with_suffix(".parts.json")


def load_ledger(code=None) -> dict:
    """部品名 → 台帳の行。標準部品の台帳（geas に同梱）に、アプリの台帳（コードの隣の 名前.parts.json）を重ねる"""
    out = {}
    for path in [STD_LEDGER, *([ledger_path(code)] if code else [])]:
        try:
            out.update(json.loads(path.read_text(encoding="utf-8")).get("parts", {}))
        except (OSError, ValueError):
            pass
    return out


def status_text(pt, ledger: dict) -> str:
    """仕様書の「人の保証」に出す、宣言の確かめの状態"""
    row = ledger.get(pt.name)
    if row is None:
        return "宣言は実際に呼んで確かめていない（geas parts --record）"
    if row.get("fingerprint") != fingerprint(pt):
        return "宣言を変えたあと、確かめ直していない（geas parts --record）"
    if row.get("proved"):
        return f"宣言は全部の入力で証明した（Kani：{row['proved']}）"
    return f"宣言は端の値で {row.get('calls', 0)} 回呼んで確かめた" + ("（手本とも一致）" if row.get("reference") else "")
