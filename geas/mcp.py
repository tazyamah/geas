# SPDX-License-Identifier: GPL-3.0-or-later
"""MCP（Model Context Protocol）のツール定義。REST の定義（routes）から機械的に作る。AI が使う口も、人や画面と同じ URL・同じ処理で、手で書く部分は無い。
1 ルート＝1 ツール。ツール名は「動作_経路の名詞」（create_orders、index_customers_orders）。入力の JSON スキーマは、処理・問い合わせの入力の型から作る。
GET は読むだけ、DELETE は壊す動作の注釈を付ける。サーバーを呼ぶ橋渡しは geas/mcp_bridge.py（定義を読むだけの汎用のもの）。"""
import re
from dataclasses import replace

from .ast import *  # noqa: F401,F403
from .rest import routes as rest_routes


def class_of(chars: tuple) -> str:
    """文字の範囲の組（コードポイント）から、正規表現の文字クラス"""
    esc = lambda c: ("\\" if chr(c) in "\\]^-[" else "") + chr(c)
    return "[" + "".join(esc(lo) if lo == hi else f"{esc(lo)}-{esc(hi)}" for lo, hi in chars) + "]"


def type_schema(ck, t: Type) -> dict:
    """型 → JSON スキーマ。JSON の形は、生成した host の読み方（hostgen.from_j）と同じ"""
    k = t.kind
    r = t.range
    if k in ("ID", "参照"):
        return {"type": "string"}
    if k == "文字列":
        s: dict = {"type": "string"}
        if r and r.lo is not None:
            s["minLength"] = r.lo
        if r and r.hi is not None:
            s["maxLength"] = r.hi
        if t.chars:
            s["pattern"] = f"^{class_of(t.chars)}*$"
        return s
    if k in ("整数", "金額", "日時"):
        s = {"type": "integer"}
        if r and r.lo is not None:
            s["minimum"] = r.lo
        if r and r.hi is not None:
            s["maximum"] = r.hi
        if k == "金額":
            s["description"] = "金額（通貨の最小単位の整数）"
        return s
    if k == "割合":
        return {"type": "string", "description": "割合（小数の文字列。例 0.1）"}
    if k == "バイト列":
        return {"type": "string", "contentEncoding": "base64"}
    if k == "集合":
        return {"type": "array", "items": {"type": "string"}, "uniqueItems": True}
    if k == "名前":
        st = ck.states[t.target]
        return {"type": "string", "enum": [st.initial, *st.middle, *st.final]}
    if k == "合成":
        return {"type": "object", "properties": {n: type_schema(ck, pt) for n, pt in t.parts},
                "required": [n for n, pt in t.parts if not pt.optional]}
    if k == "写像":
        return {"type": "object", "additionalProperties": type_schema(ck, t.value)}
    return {}


def tool_name(kind: str, path: str) -> str:
    words = [w for w in path.strip("/").split("/") if not w.startswith(":")]
    return re.sub(r"\W", "_", kind + "_" + "_".join(words))


def tool_defs(ck) -> list[dict]:
    p, show = ck.p, ck.p.show_names
    server_slots = {f.slot for f in p.rules}  # サーバーが決める slot（客は渡さない）
    out = []
    for method, path, kind, target, res, statuses in rest_routes(p):
        props: dict = {}
        required: list[str] = []
        caps = [s[1:] for s in path.split("/") if s.startswith(":")]
        proc = ck.procs.get(target)
        query = next((q for q in p.queries if q.name == target), None)
        inputs = (proc.inputs if proc else query.inputs if query else [])
        if proc is not None:
            id_in = next((n for n, t in proc.inputs if t.kind == "ID"), "") if kind in ("update", "destroy") else ""
            for c in caps:
                props[c] = {"type": "string", "description": f"{show(res)}のキー（URL の /:{c}）"}
                required.append(c)
            for n, t in inputs:
                if n == id_in:
                    continue
                props[n] = {**type_schema(ck, replace_opt(t)), "description": show(n)}
                if not t.optional:
                    required.append(n)
            slots = {n: type_schema(ck, t) | {"description": show(n)} for n, t in proc.refs if n not in server_slots}
            if slots:
                props["slots"] = {"type": "object", "description": "外から決めて渡す値（サーバーが決めない slot）", "properties": slots, "required": list(slots)}
                required.append("slots")
        else:  # 問い合わせ・出来事：パスの値と、入力
            for c, (n, t) in zip(caps, inputs):
                props[c] = {**type_schema(ck, t), "description": show(n)}
                required.append(c)
            if kind == "create":
                for n, t in inputs:
                    props[n] = {**type_schema(ck, replace_opt(t)), "description": show(n)}
                    if not t.optional:
                        required.append(n)
        desc = f"{show(target)}（{method} {path}）"
        if statuses:
            desc += "。結果：" + "、".join(f"{show(k)}→{v}" for k, v in dict(statuses).items())
        out.append({
            "name": tool_name(kind, path),
            "title": show(target),
            "description": desc,
            "inputSchema": {"type": "object", "properties": props, "required": required},
            "annotations": {"readOnlyHint": method == "GET", "destructiveHint": method == "DELETE", "idempotentHint": method in ("GET", "PUT", "DELETE")},
            "geas": {"method": method, "path": path, "body": [n for n in props if n not in caps] if method != "GET" else []},
        })
    names = [t["name"] for t in out]
    if len(set(names)) != len(names):
        raise ValueError(f"MCP のツール名が重なっています: {sorted(n for n in set(names) if names.count(n) > 1)}")
    return out


def replace_opt(t: Type) -> Type:
    return replace(t, optional=False)


def write_mcp(ck, dest, server: str = "http://localhost:8787") -> None:
    """dest/mcp/ に、ツール定義（tools.json）と橋渡し（bridge.py）と、AI への登録例（mcp.json）を置く"""
    import json
    import shutil
    from pathlib import Path
    out = Path(dest) / "mcp"
    out.mkdir(parents=True, exist_ok=True)
    (out / "tools.json").write_text(json.dumps(tool_defs(ck), ensure_ascii=False, indent=1) + "\n")
    shutil.copy(Path(__file__).parent / "mcp_bridge.py", out / "bridge.py")
    (out / "mcp.json").write_text(json.dumps({"mcpServers": {"geas": {"command": "python3", "args": ["bridge.py", "tools.json", server]}}}, indent=2) + "\n")
