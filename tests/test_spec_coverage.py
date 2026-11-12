# SPDX-License-Identifier: GPL-3.0-or-later
"""「漏れない」の常設の検査：コードがすることは、全部、人が読む仕様書に出る。

見本の `.geas` を 1 か所だけ変えた版をたくさん作る（数を 1 増やす・減らす、比較・加減の演算子を替える、語を替える、
文字列の頭に X を足す、コンマをはさんだ名前を入れ替える、1 行を消す・もう 1 つ足す・次の行と入れ替える）。検査を通って、生成される Rust 一式が変わったなら、
人が読む仕様書（review の HTML）と、更新のたびに見る「前の版からの変更」の両方が変わらなければならない。
変わらないものがあれば、仕様書に出ない動きがある（コードを読まないレビューで見つけられない）ので落とす。
エラーのあるコードでも、仕様書の生成は落ちないことも見る。"""
import contextlib
import hashlib
import io
import re
import shutil
from pathlib import Path

import pytest

from geas.check import ERROR
from geas.cli import load_text, main
from geas.lexer import lex
from geas.review import changes_section, review
from geas.spec import build, render

FIX = Path("tests/fixtures")
SWAP_OP = {"<": "<=", "<=": "<", ">": ">=", ">=": ">", "==": "!=", "!=": "==", "+": "-", "-": "+", "+=": "-=", "-=": "+=", "*": "+"}
SWAP_NAME = {"and": "or", "or": "and", "asc": "desc", "desc": "asc", "min": "max", "max": "min", "true": "false", "false": "true"}
ARROW = re.compile(r"arrow\d+")  # 図の矢印の id は、同じプロセスで作るたびに番号が進む（別プロセスなら同じ）


def mutants(text: str):
    """(行, 元の行, 変えた行, 変えた全文) の列"""
    lines = text.split("\n")
    for t in lex(text):
        if t.kind in ("NL", "EOF") or t.file:
            continue
        ln, at, new, old = lines[t.line - 1], t.col - 1, None, t.val
        if t.kind == "NUM":
            m = re.compile(r"\d+").match(ln, at)
            if m and m.group(0) == t.val:
                new = str(int(t.val) + 1)
        elif t.kind == "OP" and t.val in SWAP_OP and ln.startswith(t.val, at):
            new = SWAP_OP[t.val]
        elif t.kind == "NAME" and t.val in SWAP_NAME and ln.startswith(t.val, at):
            new = SWAP_NAME[t.val]
        elif t.kind == "STR" and ln.startswith('"', at):
            new, old = '"X', '"'
        if new is None:
            continue
        for new in [new] + ([str(int(t.val) - 1)] if t.kind == "NUM" and int(t.val) > 0 else []):  # 数は、1 増やすのと 1 減らすのと
            out = lines[:]
            out[t.line - 1] = ln[:at] + new + ln[at + len(old):]
            yield t.line, ln.strip(), out[t.line - 1].strip(), "\n".join(out)
    toks = [t for t in lex(text) if not t.file]
    for a, c, b in zip(toks, toks[1:], toks[2:]):  # コンマをはさんだ 2 つの名前を入れ替える
        if a.kind == b.kind == "NAME" and c.val == "," and a.line == b.line and a.val != b.val:
            ln = lines[a.line - 1]
            if ln.startswith(a.val, a.col - 1) and ln.startswith(b.val, b.col - 1):
                out = lines[:]
                out[a.line - 1] = ln[:a.col - 1] + b.val + ln[a.col - 1 + len(a.val):b.col - 1] + a.val + ln[b.col - 1 + len(b.val):]
                yield a.line, ln.strip(), out[a.line - 1].strip(), "\n".join(out)
    for i, ln in enumerate(lines):
        code = ln.split("//", 1)[0]
        if code.strip() and "{" not in code and "}" not in code:
            out = lines[:]
            out[i] = ""
            yield i + 1, ln.strip(), "（行を消す）", "\n".join(out)
            yield i + 1, ln.strip(), "（同じ行をもう 1 つ足す）", "\n".join(lines[:i + 1] + lines[i:])
            nxt = lines[i + 1].split("//", 1)[0] if i + 1 < len(lines) else ""
            if nxt.strip() and "{" not in nxt and "}" not in nxt and nxt.strip() != code.strip():
                yield i + 1, ln.strip(), "（次の行と入れ替える）", "\n".join(lines[:i] + [lines[i + 1], lines[i]] + lines[i + 2:])


def crate(path: Path, dest: Path) -> dict | None:
    """Rust 一式（lib・host・画面・拡張・サーバー・窓）を生成して、ファイルごとのハッシュを返す。生成できなければ None"""
    shutil.rmtree(dest, ignore_errors=True)
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(buf):
        try:
            rc = main(["rust", str(path), "-o", str(dest), "--host", "--runtime", "--standalone"])
        except SystemExit as e:
            rc = e.code
    if rc != 0:
        return None
    return {str(f.relative_to(dest)): hashlib.sha256(f.read_bytes()).hexdigest() for f in sorted(dest.rglob("*")) if f.is_file()}


def views(text: str, path: Path):
    ck = load_text(text, path)
    spec = build(ck)
    render(ck, spec, "")
    return ck, spec, ARROW.sub("arrowN", review(ck, None, [], ""))


@pytest.mark.parametrize("name", ["delete_demo", "file_demo", "overseas_shop", "runtime_demo", "shop", "solo_tweet"])
def test_コードの変更は_仕様書と変更点に必ず出る(name, tmp_path):
    work = tmp_path / f"{name}.geas"
    base = (FIX / name / f"{name}.geas").read_text(encoding="utf-8")
    work.write_text(base, encoding="utf-8")
    _, spec0, html0 = views(base, work)
    crate0 = crate(work, tmp_path / "out")
    assert crate0 is not None
    same = ARROW.sub("arrowN", changes_section(spec0, spec0, "前の版"))
    hidden, seen = [], 0
    for line, before, after, text in mutants(base):
        try:
            ck, spec, html = views(text, work)
        except ValueError:
            continue  # 構文エラー（仕様書の生成が落ちるのは ValueError 以外で、そのままテストの失敗になる）
        if any(d.level == ERROR for d in ck.diags) or not ck.ready:
            continue  # 検査が止める・問いが出る
        work.write_text(text, encoding="utf-8")
        if crate(work, tmp_path / "out") in (None, crate0):
            continue  # 生成で止まる・生成物が同じ（証明の前提だけの宣言や、既定値と同じ指定）
        seen += 1
        miss = [w for w, gone in (("仕様書", html == html0), ("変更点", ARROW.sub("arrowN", changes_section(spec, spec0, "前の版")) == same)) if gone]
        if miss:
            hidden.append(f"{line} 行目: {before}  →  {after}  〔{'・'.join(miss)}に出ない〕")
    assert seen > 0
    assert not hidden, "生成物が変わるのに、仕様書か変更点に出ない変更があります:\n" + "\n".join(hidden)
