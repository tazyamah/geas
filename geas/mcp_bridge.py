# SPDX-License-Identifier: GPL-3.0-or-later
"""MCP サーバー（標準入出力・JSON-RPC）。ツール定義（tools.json、geas が生成）を読み、ツールの呼び出しを、動いているサーバーの REST に中継するだけの汎用の橋渡し。
アプリを知らない。使い方：python mcp_bridge.py tools.json http://127.0.0.1:8080"""
import json
import sys
import urllib.error
import urllib.parse
import urllib.request

PROTOCOL = "2025-06-18"


def call(tool: dict, args: dict, base: str) -> tuple[int, str]:
    g = tool["geas"]
    path = "/".join(urllib.parse.quote(str(args[s[1:]]), safe="") if s.startswith(":") else s for s in g["path"].split("/"))
    body = json.dumps({k: args[k] for k in g["body"] if k in args}).encode() if g["method"] != "GET" else None
    req = urllib.request.Request(base.rstrip("/") + path, data=body, method=g["method"], headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.status, r.read().decode()
    except urllib.error.HTTPError as e:  # 結果の種類（404・409・422 など）も、AI が読む返事
        return e.code, e.read().decode()


def handle(msg: dict, tools: list[dict], base: str) -> dict | None:
    m, i = msg.get("method"), msg.get("id")
    if i is None:  # 通知には返事しない
        return None
    if m == "initialize":
        res = {"protocolVersion": PROTOCOL, "capabilities": {"tools": {}}, "serverInfo": {"name": "geas", "version": "0.1.0"}}
    elif m == "tools/list":
        res = {"tools": [{k: v for k, v in t.items() if k != "geas"} for t in tools]}
    elif m == "tools/call":
        prm = msg.get("params", {})
        tool = next((t for t in tools if t["name"] == prm.get("name")), None)
        if tool is None:
            return {"jsonrpc": "2.0", "id": i, "error": {"code": -32602, "message": f"ツール「{prm.get('name')}」はありません"}}
        try:
            code, text = call(tool, prm.get("arguments") or {}, base)
        except (KeyError, OSError) as e:
            return {"jsonrpc": "2.0", "id": i, "result": {"isError": True, "content": [{"type": "text", "text": f"呼べませんでした: {e!r}"}]}}
        res = {"isError": code >= 400, "content": [{"type": "text", "text": f"HTTP {code}\n{text}"}]}
    elif m == "ping":
        res = {}
    else:
        return {"jsonrpc": "2.0", "id": i, "error": {"code": -32601, "message": f"未対応: {m}"}}
    return {"jsonrpc": "2.0", "id": i, "result": res}


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print("使い方: python mcp_bridge.py tools.json サーバーのURL", file=sys.stderr)
        return 2
    tools = json.load(open(argv[1], encoding="utf-8"))
    for line in sys.stdin:
        if line.strip():
            out = handle(json.loads(line), tools, argv[2])
            if out is not None:
                print(json.dumps(out, ensure_ascii=False), flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
