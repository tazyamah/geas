# SPDX-License-Identifier: GPL-3.0-or-later
"""名前の書き方の検査。名前は半角英数にして、種類は大文字小文字で見分ける（記号は付けない）。
型・entity・状態・結果の種類・フローなど「もの」の名前は UpperCamel、
項目・変数・処理（deed）・入力・部品・変換（呼ぶと値が返る関数）など「動きと値」の名前は lower_snake。
小文字の名前は、組み込みや予約語と同じにできない（大文字で始まる名前とは、綴りが同じにならないので重ならない）。
日本語など仕様書に出す名前は、名前の前の /// で付ける（コンパイラは読まない）。"""
import re

from .ast import *  # noqa: F401,F403

UPPER = re.compile(r"[A-Z][A-Za-z0-9]*")
LOWER = re.compile(r"[a-z][a-z0-9_]*")

# 小文字の名前に使えない語：組み込みの型・関数と、宣言や文を始める語
RESERVED = {
    "id", "set", "text", "time", "bytes", "int", "ratio", "money", "ref", "vtime", "round", "floor", "ceil", "half_up", "sum", "now",
    "null", "absent", "nonempty", "empty", "true", "false",
    "let", "return", "if", "else", "for", "in", "create", "update", "request", "reply", "wait", "par",
    "entity", "ltype", "store", "state", "result", "effect", "part", "convert", "invariant", "deed", "nexus", "flow", "trigger", "query", "event",
    "input", "slot", "reads", "writes", "transition", "effects", "initial", "middle", "final", "reject", "required", "unique", "optional",
}


def name_problems(p: Program, ltype_names=()) -> list[tuple[str, str]]:
    """(場所, 指摘) の列"""
    out: list[tuple[str, str]] = []

    def upper(n: str, where: str):
        n = p.origin.get(n, (None, n))[1]  # モジュールの定義は、付け替える前の名前で見る
        if not UPPER.fullmatch(n):
            out.append((where, f"名前「{n}」は、英大文字で始まる英数字（UpperCamel）で書きます"))

    def lower(n: str, where: str):
        n = p.origin.get(n, (None, n))[1]
        if not LOWER.fullmatch(n):
            out.append((where, f"名前「{n}」は、英小文字で始まる英小文字・数字・_（lower_snake）で書きます"))
        elif n in RESERVED:
            out.append((where, f"名前「{n}」は組み込みか予約語なので使えません"))

    for n in ltype_names:
        upper(n, f"論理型 {n}")
    for d in p.data:
        upper(d.name, f"データ {d.name}")
        for f in d.fields:
            lower(f.name, f"データ {d.name}")
    for c in p.composites:
        for n, _ in c.parts or ():
            lower(n, f"合成型 {c.target}")
    for s in p.states:
        upper(s.name, f"状態 {s.name}")
        for m in [s.initial, *s.middle, *s.final]:
            upper(m, f"状態 {s.name}")
    for r in p.results:
        upper(r.name, f"結果 {r.name}")
        for v, fields in r.variants:
            upper(v, f"結果 {r.name}")
            for n, _ in fields:
                lower(n, f"結果 {r.name}")
    for e in p.effects:
        ns, _, op = e.name.partition(".")
        upper(ns, f"外部作用 {e.name}")
        if op:
            lower(op, f"外部作用 {e.name}")
        for n, _ in e.params:
            lower(n, f"外部作用 {e.name}")
    for x in p.parts:
        lower(x.name, f"部品 {x.name}")
        for n, _ in x.params:
            lower(n, f"部品 {x.name}")
    for t in p.triggers:
        upper(t.name, f"トリガ {t.name}")
        for n, _ in t.params:
            lower(n, f"トリガ {t.name}")
    for f in p.flows:
        upper(f.name, f"フロー {f.name}")
    for ev in p.events:
        upper(ev.name, f"出来事 {ev.name}")
    for q in p.queries:
        lower(q.name, f"問い合わせ {q.name}")
        for n, _ in [*q.inputs, *q.refs]:
            lower(n, f"問い合わせ {q.name}")
        for c in q.select:
            lower(c.name, f"問い合わせ {q.name}")
    for x in p.processes:
        lower(x.name, f"処理 {x.name}")
        for n, _ in [*x.inputs, *x.refs]:
            lower(n, f"処理 {x.name}")

        def walk(stmts):
            for st in stmts:
                if isinstance(st, Assign):
                    lower(st.name, f"処理 {x.name}")
                elif isinstance(st, Create):
                    lower(st.name, f"処理 {x.name}")
                elif isinstance(st, If):
                    walk(st.body)
                elif isinstance(st, ForEach):
                    walk(st.body)

        walk(x.body)
    return out
