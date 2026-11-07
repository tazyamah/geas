# SPDX-License-Identifier: GPL-3.0-or-later
"""仕様の変異検査：約束をわざと 1 つ崩した版を作り、検査器が止めるかを見る（GitHub Next の PDD Kit の変異検査に倣う）。

元の版が OK なら、崩した版は NG になるはずである。検査器が何も言わない崩し方（生き残り）は、
その約束が検査器では守られていない（テストか人の目が頼り）、または元から冗長な確認だったことを示す。
見るのは検査器の反応だけで、Rust のビルドや具体例の実行はしない（速い）。
"""
import difflib
import re
import subprocess
from dataclasses import dataclass
from pathlib import Path

from .check import ERROR, QUESTION, check
from .lexer import LexError
from .modules import ModuleError
from .parser import ParseError, parse

KILLED, SURVIVED, INVALID = "検出", "生き残り", "対象外"

GUARD = re.compile(r"^(\s*)if .+ \{ return [^{}]*\}\s*(//.*)?$")  # 1 行で書いた確認（満たさなければ結果を返す）
THRESHOLD = re.compile(r"^(\s*if [\w.]+ (?:>|>=) )(\d+)( \{ return .*)$")  # 上限の確認：数を緩める
TRANSITION = re.compile(r"^\s*transition .+$")
LTYPE_RANGE = re.compile(r"^(\s*ltype \w+ = (?:int|money\([\w]+\)) -?\d+\.\.)(\d+)(.*)$")  # 整数・金額の範囲の上限
LTYPE_MAX = re.compile(r"^(\s*ltype \w+ = .* max )(\d+)(.*)$")  # 写像の要素数の上限
REQUIRED = re.compile(r"^(.*\w+: [^,/\n]*?) required\b(.*)$")


@dataclass
class Mutant:
    line: int  # 1 始まり
    kind: str
    before: str
    after: str | None  # None は行を消す
    result: str = ""
    why: str = ""


def code_part(line: str) -> str:
    return line.split("//", 1)[0] if not line.lstrip().startswith("///") else ""


def mutants_of(text: str, only: set[int] | None = None) -> list[Mutant]:
    """1 行を 1 回だけ崩した版の一覧。only があれば、その行（1 始まり）だけを崩す"""
    out = []
    for i, line in enumerate(text.splitlines(), 1):
        if only is not None and i not in only:
            continue
        if line.lstrip().startswith("//"):
            continue
        m = THRESHOLD.match(line)
        if m:  # 上限の確認を緩める（10 倍と 1000 足しのうち、桁が大きいほう。範囲の外へ出す）
            n = int(m.group(2))
            out.append(Mutant(i, "上限の確認を緩める", line, f"{m.group(1)}{n * 10 + 1000}{m.group(3)}"))
        m = LTYPE_RANGE.match(line)
        if m and int(m.group(2)) >= 10:  # 範囲の上限を 1/10 に狭める。上限の確認（if x > N …）が、範囲に収まる根拠なら、示せなくなって止まるはず
            out.append(Mutant(i, "範囲の上限を狭める", line, f"{m.group(1)}{int(m.group(2)) // 10}{m.group(3)}"))
        m = LTYPE_MAX.match(line)
        if m:  # 要素数の上限を 10 倍に広げる。合計などの桁が、上限から示されていれば、示せなくなって止まるはず
            out.append(Mutant(i, "要素数の上限を広げる", line, f"{m.group(1)}{int(m.group(2)) * 10}{m.group(3)}"))
        if GUARD.match(line):
            out.append(Mutant(i, "確認を消す", line, None))
        elif TRANSITION.match(line):
            out.append(Mutant(i, "遷移の宣言を消す", line, None))
        elif REQUIRED.match(line) and not line.lstrip().startswith("input "):
            out.append(Mutant(i, "required を外す", line, REQUIRED.sub(r"\1\2", line, count=1)))
    return out


def apply(text: str, mu: Mutant) -> str:
    lines = text.splitlines()
    if mu.after is None:
        del lines[mu.line - 1]
    else:
        lines[mu.line - 1] = mu.after
    return "\n".join(lines) + "\n"


def changed_lines(old: str, new: str) -> set[int]:
    """new のうち、old から変わった行（1 始まり）"""
    out = set()
    for tag, _, _, j1, j2 in difflib.SequenceMatcher(None, old.splitlines(), new.splitlines(), autojunk=False).get_opcodes():
        if tag in ("replace", "insert"):
            out |= set(range(j1 + 1, j2 + 1))
    return out


def read_git(code: Path, rev: str) -> str:
    code = code.resolve()
    top = subprocess.run(["git", "-C", str(code.parent), "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True).stdout.strip()
    rel = code.relative_to(Path(top).resolve())
    old = subprocess.run(["git", "-C", top, "show", f"{rev}:{rel.as_posix()}"], capture_output=True, text=True)
    if old.returncode:
        raise ValueError(f"前の版を読めません（{rev}:{rel.as_posix()}）: {old.stderr.strip()}")
    return old.stdout


def stopped(text: str, path: str) -> tuple[bool, str]:
    """検査器が止めたか（エラーか問いが出た）と、その最初の指摘。構文エラーは例外のまま上へ"""
    ck = check(parse(text, path), strict_names=True)
    bad = [d for d in ck.diags if d.level in (ERROR, QUESTION)]
    return (bool(bad), str(bad[0]) if bad else "")


def run(path: str, base_text: str | None = None) -> tuple[list[Mutant], str | None]:
    """崩した版を順に検査する。戻り値は (結果つきの一覧, 元の版が OK でないときの理由)"""
    text = Path(path).read_text()
    broken, why = stopped(text, path)
    if broken:
        return [], f"元の版が検査を通らないので、変異検査は始めません: {why}"
    only = changed_lines(base_text, text) if base_text is not None else None
    ms = mutants_of(text, only)
    for mu in ms:
        try:
            killed, mu.why = stopped(apply(text, mu), path)
        except (ParseError, LexError, ModuleError) as e:
            mu.result, mu.why = INVALID, f"崩した版が構文として成り立たない: {e}"
            continue
        mu.result = KILLED if killed else SURVIVED
    return ms, None


def report(ms: list[Mutant], name: str, scope: str) -> str:
    n = {k: sum(1 for m in ms if m.result == k) for k in (KILLED, SURVIVED, INVALID)}
    out = [f"# 変異検査（{name}）", "",
           f"崩し方の範囲：{scope}。崩した版 {len(ms)} 件のうち、検査器が止めたもの（{KILLED}）{n[KILLED]}、"
           f"止めなかったもの（{SURVIVED}）{n[SURVIVED]}、構文として成り立たず試せなかったもの（{INVALID}）{n[INVALID]}。", ""]
    live = [m for m in ms if m.result == SURVIVED]
    if live:
        out += ["## 生き残り（検査器が何も言わなかった崩し方）", "",
                "異常とは限りません。その約束は、検査器ではなく、具体例のテストか人の目で守られています。"
                "元から冗長な確認なら、このままで構いません。守るべき約束なら、仕様（不変条件・遷移）か具体例に足してください。", "",
                "| 行 | 崩し方 | 元の行 |", "|---|---|---|"]
        out += [f"| {m.line} | {m.kind} | `{m.before.strip()}` |" for m in live]
    else:
        out += ["生き残りはありません（崩した版は、すべて検査器が止めました）。"]
    return "\n".join(out) + "\n"
