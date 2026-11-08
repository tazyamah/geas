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
    impact: dict = field(default_factory=dict)  # 動きが変わるかもしれないところ：{"処理": [...], "データ": [...], "ルート": [...], "画面": [...], "ユースケース": [...]}


def key_of(row: tuple[str, str, str]) -> str:
    """同じ約束を、前と今で結ぶ印。不変条件は文面、人の保証は「対象＋型」（強さの文言は含めない）"""
    _, target, content = row
    return content if target == "不変条件" else f"{target}｜{content.split('：', 1)[0]}"


def impact_of(new: dict, old: dict, plan_rows: list[dict] | None = None) -> dict:
    """前の版から変わった処理・問い合わせ・データを起点に、その影響が及ぶ URL・画面・ユースケースを、仕様書の表からたどる。
    たどるのは仕様書に出る関係だけ（処理が読む・書くデータ、URL の呼ぶ先、画面の操作と見せる問い合わせ、ユースケースの手順）"""
    from .review import _row_text

    def rows(spec, key):
        return {(x["処理"] if key == "処理" else x["問い合わせ"] if key == "問い合わせ" else x[0]): _row_text(x) for x in spec.get(key, [])}

    deeds, entities = set(), set()
    for key in ("処理", "問い合わせ"):
        a, b = rows(old, key), rows(new, key)
        deeds |= {k for k in {*a, *b} if a.get(k) != b.get(k)}
    for key in ("データ", "状態", "遷移", "拒否", "削除", "不変条件"):
        a = [_row_text(x) if not isinstance(x, str) else x for x in old.get(key, [])]
        b = [_row_text(x) if not isinstance(x, str) else x for x in new.get(key, [])]
        for x in b + a:
            if x not in a or x not in b:
                entities.add(x.split("　／　", 1)[0].split(":", 1)[0].strip())
    for d in [*new.get("処理", []), *new.get("問い合わせ", [])]:
        name = d.get("処理") or d.get("問い合わせ")
        if any(r.split(".", 1)[0] in entities for r in [*d.get("読む", []), *d.get("書く", [])]):
            deeds.add(name)
    routes = sorted({f"{m} {path}" for m, path, _, tgt, *_ in new.get("ルート", []) if tgt in deeds})
    screens = {sc for sc, op, _, _ in new.get("画面遷移", []) if op in deeds}
    screens |= {x[0] for x in new.get("画面", []) if any(q.strip() in deeds for q in x[1].split(","))}
    ucs = []
    for r in plan_rows or []:
        if r.get("操作", "").strip() in deeds or r.get("画面", "").strip() in screens:
            ucs.append(r["ユースケース"])
    return {"処理": sorted(deeds), "データ": sorted(e for e in entities if e), "ルート": routes, "画面": sorted(screens),
            "ユースケース": list(dict.fromkeys(ucs))}


def build_receipt(ck, base_ck, code_path: str, base_text: str | None, label: str, plan_rows: list[dict] | None = None) -> Receipt:
    from . import mutate
    from .rest import INTERNAL, OPEN
    new_spec, old_spec = build(ck), build(base_ck)
    new, old = new_spec["保証の強さ"], old_spec["保証の強さ"]
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
    # 流れの途中から外れて、外から直接呼べるようになった URL は、確かめを飛ばせる口が増えたので「弱くなった」に出す
    was_out = {(x[0], x[1]): x[5] for x in old_spec["ルート"]}
    for m, path, *_, now in new_spec["ルート"]:
        was = was_out.get((m, path))
        if was == INTERNAL and now == OPEN:
            r.weakened.append((f"URL {m} {path}", was, now, "流れの途中から外れ、外から直接呼べるようになりました"))
        elif was == OPEN and now == INTERNAL:
            r.strengthened.append((f"URL {m} {path}", was, now, "流れの途中の処理になり、サーバーは外に出さなくなりました"))
    r.impact = impact_of(new_spec, old_spec, plan_rows)
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


IMPACT_LABELS = (("処理", "処理・問い合わせ"), ("データ", "データ（項目・状態・遷移・削除・不変条件）"), ("ルート", "URL"), ("画面", "画面"), ("ユースケース", "ユースケース（計画書を渡したときだけ）"))


def impact_lines(imp: dict) -> list[str]:
    if not any(imp.values()):
        return ["処理・データの変更はありません（または、仕様書に出る関係では影響が見つかりません）。"]
    out = ["変わった処理・データと、それにつながる URL・画面・ユースケースです。人が動きを確かめる範囲の目安で、漏れがないことまでは保証しません。", ""]
    return out + [f"- {label}：{'、'.join(imp[k])}" for k, label in IMPACT_LABELS if imp.get(k)]


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
    out += ["", "### 動きが変わるかもしれないところ", ""] + impact_lines(r.impact)
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
    parts.append("<h3>動きが変わるかもしれないところ</h3>" + "".join(lead(escape(x)) for x in impact_lines(r.impact) if x))
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
