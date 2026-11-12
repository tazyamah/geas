# SPDX-License-Identifier: GPL-3.0-or-later
"""他の言語の書き方（`&&`・`elif`・`string`・`True` など）で書いたときの、直し方の手がかり。

geas を書くのは AI で、AI は他の言語のノリで書きやすい。構文エラーと検査の指摘に、geas での書き方を添える。
ここは手がかりの表だけで、受け付ける書き方は増やさない（言語は変えない）。
"""

# ---- 型の名前 ----
FOREIGN_TYPES = {
    **dict.fromkeys(("string", "String", "str", "Str", "varchar", "char", "Text"),
                    "文字列は text です（長さと文字種は ltype で text 1..80 のように書きます）"),
    **dict.fromkeys(("boolean", "Boolean", "Bool"), "真偽は bool です"),
    **dict.fromkeys(("integer", "Integer", "Int", "number", "Number", "long", "short", "i8", "i16", "i32", "i64", "i128", "u8", "u16", "u32", "u64",
                     "u128", "usize", "isize"), "整数は int です（範囲は ltype で int 0..99 のように書き、Rust の型は範囲から選ばれます）"),
    **dict.fromkeys(("float", "double", "f32", "f64", "decimal", "Decimal", "real", "numeric"),
                    "小数の型はありません。金額は money(JPY)（最小単位の整数）、割合は ratio、それ以外は単位を決めて int で持ちます"),
    **dict.fromkeys(("uuid", "UUID", "Uuid", "ID", "Id"), "識別子は id です"),
    **dict.fromkeys(("datetime", "DateTime", "timestamp", "Timestamp", "date", "Date", "Instant", "Time"), "日時は time です（暦の日付は vtime）"),
    **dict.fromkeys(("blob", "binary", "Bytes", "bytea"), "バイト列は bytes です"),
    **dict.fromkeys(("list", "List", "array", "Array", "Vec", "vector", "Set"),
                    "並びは、集合なら ltype に set of id、数を持つなら写像 Product → Quantity を書きます"),
    **dict.fromkeys(("map", "Map", "dict", "Dict", "HashMap", "object"), "写像は ltype に Product → Quantity の形で書きます"),
    **dict.fromkeys(("enum", "Enum"), "選択肢の並びは、状態なら state、処理の結果なら result で書きます"),
    **dict.fromkeys(("Money", "Currency", "Ratio", "Raw", "Vtime"), "組み込みの型の名前は小文字です（money・currency・ratio・raw・vtime）"),
}


def type_hint(name: str) -> str:
    """未定義の型名への案内（他の言語の型名・? 付き・Option）。無ければ空"""
    if name.endswith("?") or name.split("<")[0] in ("Option", "Optional", "Maybe", "Nullable"):
        return "（? や Option は書きません。項目と入力は、書かなければ無いかもしれない（null になりうる）扱いで、必ずあるものに required を付けます）"
    base = name.split("<")[0]
    return f"（{FOREIGN_TYPES[base]}）" if base in FOREIGN_TYPES else ""


# ---- 式の中の名前 ----
FOREIGN_NAMES = {
    **dict.fromkeys(("True", "TRUE", "False", "FALSE"), "真偽は true／false と小文字で書きます"),
    **dict.fromkeys(("None", "nil", "NULL", "Null", "undefined", "nullptr"), "値が無いことは null です（確かめるのは x is null）"),
    **dict.fromkeys(("this", "self"), "this・self はありません。データは let x = Entity[キー] で引いて、x.項目 で読みます"),
    **dict.fromkeys(("Now", "today", "current_time", "current_timestamp"), "いまの時刻は now です（期間は 30 min のように書きます）"),
}


def name_hint(name: str) -> str:
    """定義されていない名前への案内。無ければ空"""
    if name.startswith("'") or name.endswith("'"):
        return "（文字列は \"…\" で囲みます。' は使えません）"
    return f"（{FOREIGN_NAMES[name]}）" if name in FOREIGN_NAMES else ""


# ---- 構文：字句 ----
ELSE = ("else は、if 条件 { … } の閉じる } と同じ行に続けて書きます（} else { … }、} else if 条件 { … }）。"
        "let の else も、let x = … else { return … } と同じ行に書きます")
ELIF = "else if 条件 { … } と書きます（elif・elsif はありません）"


def shown(t) -> str:
    """エラーに出す字句の見せ方"""
    if t.kind == "NL":
        return "行末"
    if t.kind == "EOF":
        return "ファイルの終わり"
    if t.kind == "STR":
        from .ast import quote_str
        return quote_str(t.val)
    return t.val


def token_hint(toks, i: int) -> str:
    """位置 i の字句が、他の言語の書き方らしいときの手がかり。無ければ空"""
    t = toks[min(i, len(toks) - 1)]
    nxt = toks[i + 1] if i + 1 < len(toks) else t
    v = t.val if t.kind in ("NAME", "OP") else ""
    if v == "&&" or v == "&":
        return "「かつ」は and と書きます（&& は使えません）"
    if v in ("||", "|") and (v == "||" or (nxt.kind == "OP" and nxt.val == "|")):
        return "「または」は or と書きます（|| は使えません）"
    if v == "!" or (v.startswith("!") and len(v) > 1):
        return "否定は not と書きます（! は使えません。等しくないは !=）"
    if v == ";":
        return "文の区切りは改行です（; は書きません）"
    if v == "=" and nxt.kind == "OP" and nxt.val == ">":
        return "矢印は → か -> と書きます（=> ではありません）"
    if v == "==" and nxt.kind == "OP" and nxt.val == "=":
        return "等しいは == です（=== はありません）"
    if v in ("+", "-") and nxt.kind == "OP" and nxt.val == v:
        return f"1 だけ{'足す' if v == '+' else '引く'}なら 名前.項目 {v}= 1 と書きます（{v}{v} はありません）"
    if v == "/" and nxt.kind == "OP" and nxt.val == "*":
        return "コメントは // から行末です（/* */ はありません）"
    if v == "mut":
        return "let に mut は付けません（let の変数は作り直さず、項目の更新は 名前.項目 = 値 と書きます）"
    if v == "else":
        return ELSE
    if v == "?" or v.endswith("?"):
        return "? はありません。値を選ぶ式は if 条件 then 式 else 式、無いかもしれない項目は書かなければそう扱われます"
    if v in ("elif", "elsif", "elseif"):
        return ELIF
    if t.kind == "NAME" and (v.startswith("'") or v.endswith("'")):
        return "文字列は \"…\" で囲みます（' は使えません）"
    if v in FOREIGN_NAMES:
        return FOREIGN_NAMES[v]
    return ""


# ---- 構文：定義と文の始まり ----
TOP_KEYWORDS = ("app", "timezone", "use", "module", "ltype", "entity", "store", "storage", "state", "result", "effect", "reply", "library", "convert",
                "invariant", "deed", "nexus", "flow", "trigger", "rule", "migration", "query", "event", "screen", "resource", "needs", "resident", "path", "data_folder")

DEF_HINTS = {
    **dict.fromkeys(("type", "typedef", "alias", "newtype"), "型に名前を付けるのは ltype です（ltype Quantity = int 1..99）"),
    **dict.fromkeys(("enum",), "選択肢の並びは、状態なら state、処理の結果なら result で書きます"),
    **dict.fromkeys(("struct", "class", "model", "table", "record", "schema", "interface", "object", "data"),
                    "保存するデータは entity、値の組は ltype 名前 { 成分: 型 } で書きます"),
    **dict.fromkeys(("fn", "func", "function", "def", "method", "proc", "procedure", "action", "handler", "command", "service"),
                    "データを読み書きする処理は deed、式で書く値の変換は convert です"),
    **dict.fromkeys(("import", "include", "require", "using", "from", "mod", "package"), 'ほかのファイルの取り込みは use "ファイル.geas" です'),
    **dict.fromkeys(("const", "let", "var", "val", "static", "global"),
                    "定義の外に定数や変数は置けません。範囲は ltype、サーバーが決める値は slot と rule で書きます"),
    **dict.fromkeys(("route", "routes", "endpoint", "api", "controller", "router", "get", "post", "put", "patch", "delete"),
                    "URL は resource で書きます（index・show・create・update・destroy）"),
    **dict.fromkeys(("view", "page", "component", "form", "layout"), "画面は screen で書きます"),
    **dict.fromkeys(("test", "tests", "example", "examples", "spec", "describe"), "テストは書きません。計画書の具体例の表から生成します"),
    **dict.fromkeys(("on", "listener", "subscribe", "cron", "schedule", "job", "timer"), "出来事は event、返事の待ちと期限は trigger で書きます"),
    **dict.fromkeys(("constraint", "assert", "check", "validate"), "いつでも成り立つ約束は invariant、入力の確かめは ltype の範囲か convert の require で書きます"),
}


def def_hint(kw: str) -> str:
    return DEF_HINTS.get(kw, "") or DEF_HINTS.get(kw.lower(), "")


STMT_HINTS = {
    **dict.fromkeys(("var", "const", "val", "mut", "auto"), "変数は let で作ります（let x = 式）"),
    **dict.fromkeys(("while", "loop", "do", "repeat", "until"), "繰り返しは for 要素 in 集まり { … } だけです（while はありません）"),
    **dict.fromkeys(("match", "switch", "case", "when"), "分岐は if 条件 { … } else if 条件 { … } else { … } で書きます（match・switch はありません）"),
    **dict.fromkeys(("throw", "raise", "panic", "error", "fail", "abort", "try", "catch"),
                    "例外はありません。失敗も結果の種類にして return します（return OutOfStock など）"),
    **dict.fromkeys(("print", "println", "log", "console", "debug", "echo"), "画面や記録への出力は書けません（処理は結果を返すだけです）"),
    "else": ELSE,
    **dict.fromkeys(("elif", "elsif", "elseif"), ELIF),
    **dict.fromkeys(("assert", "require", "ensure", "guard"), "処理の中の確かめは if 条件 { return 結果の種類 } と書きます（require は raw から変換する convert の中だけ）"),
    **dict.fromkeys(("await", "async", "emit", "publish", "send", "call", "fetch"), "外への要求は request 名前空間.操作(…, idempotency_key = …) で書き、返事は trigger で待ちます"),
    **dict.fromkeys(("set",), "項目の更新は 名前.項目 = 値 と書きます"),
    **dict.fromkeys(("insert", "new", "create", "add"), "新規作成は let x = create Entity { 項目: 値 } と書きます"),
    **dict.fromkeys(("remove", "del", "drop", "destroy"), "削除は delete 変数（let で引いた1件）です"),
    **dict.fromkeys(("break", "continue"), "for の途中で抜ける書き方はありません。条件で絞った集まりを回します"),
}


def stmt_hint(kw: str, open_def: str = "") -> str:
    """文の始まりが分からないときの手がかり。open_def は、閉じていないかもしれない定義（「12行目の deed place_order」）"""
    if kw in TOP_KEYWORDS:
        return f"「{kw}」は定義の始まりです。{open_def or '直前の定義'}の }} が足りないかもしれません"
    return STMT_HINTS.get(kw, "")


def place(t) -> str:
    """字句の場所（use で取り込んだファイルの字句なら、ファイルの名前も）"""
    from pathlib import Path
    return (f"{Path(t.file).name} の " if t.file else "") + f"{t.line}行目"


def brace_hint(toks) -> str:
    """{ と } の対応が崩れていれば、その場所。合っていれば空（ファイルごとに見る）"""
    stacks: dict[str, list] = {}
    for t in toks:
        if t.kind != "OP" or t.val not in "{}":
            continue
        st = stacks.setdefault(t.file, [])
        if t.val == "{":
            st.append(t)
        elif st:
            st.pop()
        else:
            return f"{place(t)}の }} に対応する {{ がありません（}} が多いか、{{ を書き忘れています）"
    for st in stacks.values():
        if st:
            return f"{place(st[-1])}の {{ が閉じていません（}} が足りません）"
    return ""
