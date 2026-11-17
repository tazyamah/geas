# SPDX-License-Identifier: GPL-3.0-or-later
"""raw から変換する convert（型の read に結んだ変換）の「拒否の条件」（require）を、生成物の外でも評価する。
使う所は 2 つ：画面の read に直に書いた値（lookup）の検査と、ブラウザ拡張がページから読んだ値を送る前の確かめ（geas/ext/reader.js が同じ木を評価する）。
条件の式を、JSON にできる木（入れ子のリスト）にする。木にできない式（OS のパスの展開など）があれば None で、そのときは事前には確かめない（境界で確かめる）。"""
from .ast import Bin, BoolLit, Call, Cond, Name, Not, Num, SumE
from .parser import parse_chars

LIMIT = 2**127  # 生成物は i128 で計算し、あふれは入力不正にする


def tree(e, raw: str):
    """式 → 木。raw は、変換の引数（raw の値）の名前"""
    if isinstance(e, Num):
        return ["num", e.v]
    if isinstance(e, BoolLit):
        return ["bool", bool(e.v)]
    if isinstance(e, Name):
        return ["raw"] if e.n == raw else ["var", e.n]
    if isinstance(e, Not):
        return ["not", tree(e.e, raw)]
    if isinstance(e, Cond):
        return ["if", tree(e.c, raw), tree(e.a, raw), tree(e.b, raw)]
    if isinstance(e, SumE):  # sum { 式 | i in positions(raw) }
        return ["sum", e.binder.names[0], tree(e.elem, raw)]
    if isinstance(e, Call) and e.n == "len":
        return ["len"]
    if isinstance(e, Call) and e.n == "only":
        return ["only", [list(p) for p in parse_chars(e.args[1].s)]]
    if isinstance(e, Call) and e.n == "digit":
        return ["digit", tree(e.args[1], raw)]
    if isinstance(e, Call) and e.n in ("min", "max", "abs"):
        return [e.n, *[tree(a, raw) for a in e.args]]
    if isinstance(e, Bin) and e.op in ("+", "-", "*", "<", ">", "<=", ">=", "==", "!=", "かつ", "または"):
        return [{"かつ": "and", "または": "or"}.get(e.op, e.op), tree(e.l, raw), tree(e.r, raw)]
    if isinstance(e, Bin) and e.op in ("/", "%") and isinstance(e.r, Num):  # 正の定数で割る（負の値も、切り捨てと 0 以上の余り）
        return [e.op, tree(e.l, raw), tree(e.r, raw)]
    raise ValueError(e)


def requires_tree(part):
    """変換の拒否の条件 → [[木, 理由], …]。木にできない条件があれば None"""
    if part is None or not part.requires or not part.params:
        return None
    try:
        return [[tree(cond, part.params[0][0]), why] for cond, why in part.requires]
    except (ValueError, AttributeError, IndexError):
        return None


class Overflow(Exception):
    pass


def value(t, s: str, env: dict):
    k = t[0]
    if k in ("num", "bool"):
        return t[1]
    if k == "var":
        return env[t[1]]
    if k == "len":
        return len(s)
    if k == "only":
        return all(any(a <= ord(c) <= b for a, b in t[1]) for c in s)
    if k == "digit":  # 数字でない文字と、文字数の外の位置は 0
        i = value(t[1], s, env)
        return int(s[i]) if 0 <= i < len(s) and s[i] in "0123456789" else 0
    if k == "not":
        return not value(t[1], s, env)
    if k == "if":
        return value(t[2] if value(t[1], s, env) else t[3], s, env)
    if k == "and":
        return value(t[1], s, env) and value(t[2], s, env)
    if k == "or":
        return value(t[1], s, env) or value(t[2], s, env)
    if k == "sum":
        out = sum(value(t[2], s, {**env, t[1]: i}) for i in range(len(s)))
    else:
        xs = [value(a, s, env) for a in t[1:]]
        if k in ("<", ">", "<=", ">=", "==", "!="):
            a, b = xs
            return {"<": a < b, ">": a > b, "<=": a <= b, ">=": a >= b, "==": a == b, "!=": a != b}[k]
        out = {"+": lambda: xs[0] + xs[1], "-": lambda: xs[0] - xs[1], "*": lambda: xs[0] * xs[1], "/": lambda: xs[0] // xs[1], "%": lambda: xs[0] % xs[1],
               "min": lambda: min(xs), "max": lambda: max(xs), "abs": lambda: abs(xs[0])}[k]()
    if not -LIMIT <= out < LIMIT:
        raise Overflow()
    return out


def refusal(requires: list, s: str):
    """値 s を変換が断る理由。通るなら None"""
    for t, why in requires:
        try:
            if not value(t, s, {}):
                return why
        except Overflow:
            return "計算があふれました"
    return None
