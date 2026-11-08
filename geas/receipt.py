# SPDX-License-Identifier: GPL-3.0-or-later
"""変更ごとのレシート：前の版と比べて、「信頼に関わる変化」だけを抜き出す（GitHub Next の PDD Kit の「レシート」に倣う）。

仕様書の差分は全部を見せるが、レシートは、人が最初に見るべき所に絞る。
1. 保証が弱くなったもの、増えた人の保証
2. 保証の強さの件数の変化
3. 変異検査の生き残り（変わった行を崩して、検査器が止めなかったもの）
4. 検査の結果（エラーと問いの件数）
Markdown（PR の本文に貼る）と、差分 HTML の節の両方に、同じ内容を出す。
"""
from dataclasses import dataclass, field
from html import escape

from .check import ERROR, QUESTION
from .spec import LEVELS, build, level_counts

WEAK = LEVELS[2:]  # 検証ビルドだけ／範囲の端で止まる／確かめない／示せていない


@dataclass
class Receipt:
    label: str
    weakened: list[tuple[str, str, str, str]] = field(default_factory=list)  # (対象, 前の強さ, 今の強さ, 内容)
    added_weak: list[tuple[str, str, str]] = field(default_factory=list)  # (強さ, 対象, 内容)：新しく増えた、弱い約束
    strengthened: list[tuple[str, str, str, str]] = field(default_factory=list)  # (対象, 前の強さ, 今の強さ, 内容)
    before: list[tuple[str, int]] = field(default_factory=list)
    after: list[tuple[str, int]] = field(default_factory=list)
    survivors: list[tuple[int, str, str]] = field(default_factory=list)  # (行, 崩し方, 元の行)
    mutation_note: str = ""  # 変異検査を回せなかった理由（元の版が検査を通らないなど）
    errors: int = 0
    questions: int = 0


def key_of(row: tuple[str, str, str]) -> str:
    """同じ約束を、前と今で結ぶ印。不変条件は文面、人の保証は「対象＋型」（強さの文言は含めない）"""
    _, target, content = row
    return content if target == "不変条件" else f"{target}｜{content.split('：', 1)[0]}"


def build_receipt(ck, base_ck, code_path: str, base_text: str | None, label: str) -> Receipt:
    from . import mutate
    new, old = build(ck)["保証の強さ"], build(base_ck)["保証の強さ"]
    old_by = {key_of(r): r for r in old}
    new_keys = {key_of(r) for r in new}
    r = Receipt(label, before=level_counts(old), after=level_counts(new))
    rank = {x: i for i, x in enumerate(LEVELS)}
    for row in new:
        prev = old_by.get(key_of(row))
        if prev is None:
            if row[0] in WEAK:
                r.added_weak.append(row)
        elif rank[row[0]] > rank[prev[0]]:
            r.weakened.append((row[1], prev[0], row[0], row[2]))
        elif rank[row[0]] < rank[prev[0]]:
            r.strengthened.append((row[1], prev[0], row[0], row[2]))
    r.strengthened += [(p[1], p[0], "（無くなった）", p[2]) for p in old if key_of(p) not in new_keys and p[0] in WEAK]
    r.errors = sum(1 for d in ck.diags if d.level == ERROR)
    r.questions = sum(1 for d in ck.diags if d.level == QUESTION)
    try:
        ms, why = mutate.run(code_path, base_text)
        r.mutation_note = why or ""
        r.survivors = [(m.line, m.kind, m.before.strip()) for m in ms if m.result == mutate.SURVIVED]
    except Exception as e:  # 構文エラーなどは、変異検査だけ飛ばす（レシートは出す）
        r.mutation_note = f"変異検査を回せませんでした: {e}"
    return r


def counts_text(cs: list[tuple[str, int]]) -> str:
    return "、".join(f"{lv} {n}" for lv, n in cs) or "なし"


def to_markdown(r: Receipt) -> str:
    out = [f"## 信頼のレシート（比較元：{r.label}）", ""]
    bad = r.weakened or r.added_weak
    if bad:
        out += ["### 保証が弱くなったもの・増えた人の保証（最初に見てください）", "", "| 対象 | 変化 | 内容 |", "|---|---|---|"]
        out += [f"| {t} | {a} → {b} | {c} |" for t, a, b, c in r.weakened]
        out += [f"| {t} | （新しい）→ {lv} | {c} |" for lv, t, c in r.added_weak]
    else:
        out += ["保証が弱くなったもの、増えた人の保証はありません。"]
    if r.strengthened:
        out += ["", "### 強くなった・無くなったもの", "", "| 対象 | 変化 | 内容 |", "|---|---|---|"]
        out += [f"| {t} | {a} → {b} | {c} |" for t, a, b, c in r.strengthened]
    out += ["", f"### 保証の強さの件数", "", f"- 前：{counts_text(r.before)}", f"- 今：{counts_text(r.after)}"]
    out += ["", "### 変異検査の生き残り（変わった行を崩しても、検査器が何も言わなかったもの）", ""]
    if r.mutation_note:
        out += [r.mutation_note]
    elif r.survivors:
        out += ["守るべき約束なら、不変条件・遷移・具体例に足してください。元から冗長な確認なら、そのままで構いません。", "", "| 行 | 崩し方 | 元の行 |", "|---|---|---|"]
        out += [f"| {n} | {k} | `{b}` |" for n, k, b in r.survivors]
    else:
        out += ["ありません。"]
    out += ["", "### 検査の結果", "", f"- エラー {r.errors} 件、問い {r.questions} 件" + ("" if not (r.errors or r.questions) else "（先へ進めません）")]
    return "\n".join(out) + "\n"


def to_html(r: Receipt) -> str:
    """差分 HTML の節（review.review_changes から呼ぶ）"""
    from .review import lead, table
    parts = []
    bad = r.weakened or r.added_weak
    if bad:
        rows = [(t, f"{a} → {b}", c) for t, a, b, c in r.weakened] + [(t, f"（新しい）→ {lv}", c) for lv, t, c in r.added_weak]
        parts.append("<h3>保証が弱くなったもの・増えた人の保証（最初に見てください）</h3>" + table(["対象", "変化", "内容"], rows))
    else:
        parts.append(lead("保証が弱くなったもの、増えた人の保証はありません。"))
    if r.strengthened:
        parts.append("<h3>強くなった・無くなったもの</h3>" + table(["対象", "変化", "内容"], [(t, f"{a} → {b}", c) for t, a, b, c in r.strengthened]))
    parts.append("<h3>保証の強さの件数</h3>" + lead(f"前：{escape(counts_text(r.before))}") + lead(f"今：{escape(counts_text(r.after))}"))
    parts.append("<h3>変異検査の生き残り</h3>")
    if r.mutation_note:
        parts.append(lead(escape(r.mutation_note)))
    elif r.survivors:
        parts.append(lead("変わった行を崩しても、検査器が何も言わなかったものです。守るべき約束なら、不変条件・遷移・具体例に足してください。元から冗長な確認なら、そのままで構いません。")
                     + table(["行", "崩し方", "元の行"], r.survivors))
    else:
        parts.append(lead("ありません。"))
    parts.append("<h3>検査の結果</h3>" + lead(f"エラー {r.errors} 件、問い {r.questions} 件"))
    return "".join(parts)
