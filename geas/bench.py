# SPDX-License-Identifier: GPL-3.0-or-later
"""AI が計画書から `.geas` を書く力の測定（評価）。

置き場所の決まり：
  ケース集/<名前>/plan.md                 ケース（計画書）。お手本のコードは要らない
  結果の置き場/<名前>/app.geas            そのケースで AI が書いた `.geas`
  結果の置き場/<名前>/rounds.txt          AI が検査して直した回数（任意。整数 1 つ）

`geas bench` は、AI の書いたものを採点するだけで、AI を呼ばない（呼ぶのは外の仕組み。geas に通信は無い）。
採点は 4 つ：検査を通ったか／計画書との食い違いの数／具体例が合ったか／直した回数。
`--baseline` に前回の JSON を渡すと、悪くなったケース・良くなったケースを示す。
走らせる頃合い：PR の直前、ビルドの直前、plan-interview や言語仕様を変えたあと。
"""
import json
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

from .check import check
from .lexer import LexError
from .modules import ModuleError
from .parser import ParseError, parse


@dataclass
class Score:
    case: str
    status: str = ""  # 通過／検査で止まる／構文エラー／書かれていない
    errors: int = 0
    questions: int = 0
    mismatches: int = 0  # 計画書との食い違い（未規定は数えない）
    examples: str = ""  # 合った／食い違い／実行しない／具体例なし
    rounds: int | None = None
    notes: list[str] = field(default_factory=list)

    @property
    def passed(self) -> bool:
        return self.status == "通過" and self.mismatches == 0 and self.examples in ("合った", "具体例なし", "実行しない")


def score_case(case: str, plan_text: str, app: Path, rounds_file: Path, run: bool = True) -> Score:
    from .examples import ExampleError, compare_examples, run_crate, runner_source, write_crate
    from .plan import EXAMPLE, UNSPEC, compare_static, read_plan
    from .rustgen import generate
    from .spec import build
    s = Score(case)
    if rounds_file.exists():
        try:
            s.rounds = int(rounds_file.read_text(encoding="utf-8").strip())
        except ValueError:
            s.notes.append("rounds.txt が整数ではありません")
    if not app.exists():
        s.status = "書かれていない"
        return s
    try:
        ck = check(parse(app.read_text(encoding="utf-8"), str(app)), strict_names=True)
    except (ParseError, LexError, ModuleError) as e:
        s.status, s.notes = "構文エラー", s.notes + [str(e)[:200]]
        return s
    s.errors = sum(1 for d in ck.diags if d.level == "エラー")
    s.questions = sum(1 for d in ck.diags if d.level == "問い")
    if not ck.ready:
        s.status = "検査で止まる"
        return s
    s.status = "通過"
    plan = read_plan(plan_text)
    findings = compare_static(build(ck), plan)
    rows = plan.get("具体例", [])
    code, problems = runner_source(ck, rows)
    findings += problems
    if not rows:
        s.examples = "具体例なし"
    elif not run:
        s.examples = "実行しない"
    else:
        with tempfile.TemporaryDirectory() as tmp:
            try:
                write_crate(ck, generate(ck), code, Path(tmp))
                bad = compare_examples(rows, run_crate(Path(tmp), ck.p.show_names))
                findings += bad
                s.examples = "食い違い" if bad else "合った"
            except ExampleError as e:
                s.examples, s.notes = "食い違い", s.notes + [str(e)[:200]]
                findings.append(_f(EXAMPLE))
    s.mismatches = sum(1 for f in findings if f.kind != UNSPEC)
    return s


def _f(kind):
    from .plan import Finding
    return Finding(kind, "具体例の実行", "動かせません")


def run_bench(cases_dir: Path, runs_dir: Path, run: bool = True) -> list[Score]:
    out = []
    for c in sorted(p for p in cases_dir.iterdir() if (p / "plan.md").exists()):
        out.append(score_case(c.name, (c / "plan.md").read_text(encoding="utf-8"), runs_dir / c.name / "app.geas", runs_dir / c.name / "rounds.txt", run))
    return out


def to_json(scores: list[Score]) -> str:
    return json.dumps({"cases": [{"case": s.case, "status": s.status, "errors": s.errors, "questions": s.questions, "mismatches": s.mismatches,
                                   "examples": s.examples, "rounds": s.rounds, "passed": s.passed} for s in scores]}, ensure_ascii=False, indent=2) + "\n"


def to_markdown(scores: list[Score], baseline: dict | None = None) -> str:
    n = len(scores)
    ok = sum(1 for s in scores if s.passed)
    rs = [s.rounds for s in scores if s.rounds is not None]
    out = ["# AI が書く力の測定", "", f"ケース {n} 件のうち、通ったもの {ok} 件（{(100 * ok // n) if n else 0}%）。"
           + (f" 直した回数の平均 {sum(rs) / len(rs):.1f} 回。" if rs else ""), "",
           "| ケース | 検査 | 食い違い | 具体例 | 直した回数 | 判定 |", "|---|---|---|---|---|---|"]
    for s in scores:
        chk = s.status + (f"（エラー {s.errors}・問い {s.questions}）" if s.status == "検査で止まる" else "")
        out.append(f"| {s.case} | {chk} | {s.mismatches if s.status == '通過' else '-'} | {s.examples or '-'} | {'' if s.rounds is None else s.rounds} | {'通った' if s.passed else '通らない'} |")
    notes = [(s.case, x) for s in scores for x in s.notes]
    if notes:
        out += ["", "## 補足", ""] + [f"- {c}：{x}" for c, x in notes]
    if baseline is not None:
        prev = {c["case"]: c for c in baseline.get("cases", [])}
        worse = [s.case for s in scores if s.case in prev and prev[s.case]["passed"] and not s.passed]
        better = [s.case for s in scores if s.case in prev and not prev[s.case]["passed"] and s.passed]
        new = [s.case for s in scores if s.case not in prev]
        out += ["", "## 前回との比較", "",
                f"- 悪くなった（通っていたのに通らない）：{'、'.join(worse) or 'なし'}",
                f"- 良くなった：{'、'.join(better) or 'なし'}",
                f"- 新しいケース：{'、'.join(new) or 'なし'}"]
    return "\n".join(out) + "\n"
