"""geas の読み書きは、OS の既定の文字コードによらず UTF-8（Windows の既定は cp932 などで、.geas・計画書・生成物の日本語が壊れる）"""
import ast
import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).parent.parent
FIX = ROOT / "tests" / "fixtures" / "overseas_shop"


def unencoded(tree) -> list[int]:
    """文字コードを書かずにテキストを読み書きしている呼び出しの行"""
    out = []
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        f = node.func
        name = f.attr if isinstance(f, ast.Attribute) else f.id if isinstance(f, ast.Name) else ""
        kws = {k.arg for k in node.keywords}
        if "encoding" in kws:
            continue
        if name in ("read_text", "write_text") and isinstance(f, ast.Attribute):  # Path の読み書き（同じ名前の関数は除く）
            out.append(node.lineno)
        elif name == "open" and isinstance(f, ast.Name):
            mode = node.args[1] if len(node.args) > 1 else next((k.value for k in node.keywords if k.arg == "mode"), None)
            if not (isinstance(mode, ast.Constant) and "b" in str(mode.value)):
                out.append(node.lineno)
        elif name in ("run", "Popen", "check_output") and kws & {"text", "universal_newlines"}:
            out.append(node.lineno)
    return out


def test_geasの読み書きは_文字コードを書く():
    bad = [f"{f.relative_to(ROOT)}:{line}" for f in sorted((ROOT / "geas").rglob("*.py"))
           for line in unencoded(ast.parse(f.read_text(encoding="utf-8")))]
    assert not bad, "encoding=\"utf-8\" を付けてください: " + "、".join(bad)


def geas(*args, env=None):
    """既定の文字コードを使った所があれば例外にして（-X warn_default_encoding）、geas を動かす"""
    return subprocess.run([sys.executable, "-X", "warn_default_encoding", "-W", "error::EncodingWarning", "-m", "geas", *args],
                          capture_output=True, cwd=ROOT, env=env)


def test_読む書くは_既定の文字コードに頼らない(tmp_path):
    r = geas("spec", str(FIX / "overseas_shop.geas"), "--plan", str(FIX / "overseas_shop_plan.md"), "-o", str(tmp_path / "spec.md"))
    assert r.returncode == 0, r.stderr.decode("utf-8", "replace")
    # 標準出力に出したもの（print が末尾に改行を足す）と同じ
    assert (tmp_path / "spec.md").read_text(encoding="utf-8") + "\n" == (FIX / "overseas_shop_spec.md").read_text(encoding="utf-8")


def test_出力の文字コードが_UTF_8でなくても_UTF_8で出す():
    """Windows でリダイレクトしたとき（標準出力が cp932 など）の代わりに、latin-1 にして確かめる"""
    env = {**os.environ, "PYTHONIOENCODING": "latin-1"}
    r = geas("check", str(FIX / "overseas_shop.geas"), env=env)
    assert r.returncode == 0, r.stderr.decode("utf-8", "replace")
    assert "検査: 通りました" in r.stdout.decode("utf-8")
    r = geas("check", str(FIX / "nothing.geas"), env=env)
    assert r.returncode == 2 and "ファイルがありません" in r.stderr.decode("utf-8")


def test_MCPの橋渡しは_標準入出力をUTF_8で読み書きする(tmp_path):
    tools = tmp_path / "tools.json"
    tools.write_text(json.dumps([{"name": "place_order", "description": "注文する", "inputSchema": {"type": "object", "properties": {}},
                                  "geas": {"method": "POST", "path": "/orders", "body": []}}], ensure_ascii=False), encoding="utf-8")
    lines = [{"jsonrpc": "2.0", "id": 1, "method": "tools/list"},
             {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "存在しない", "arguments": {}}}]
    r = subprocess.run([sys.executable, str(ROOT / "geas" / "mcp_bridge.py"), str(tools), "http://127.0.0.1:9"],
                       input="".join(json.dumps(x, ensure_ascii=False) + "\n" for x in lines).encode("utf-8"),
                       capture_output=True, env={**os.environ, "PYTHONIOENCODING": "latin-1"}, timeout=30)
    assert r.returncode == 0, r.stderr.decode("utf-8", "replace")
    out = [json.loads(x) for x in r.stdout.decode("utf-8").splitlines()]
    assert out[0]["result"]["tools"][0]["description"] == "注文する"
    assert "存在しない" in out[1]["error"]["message"]


def test_MCPの橋渡しは_読めない行で止まらず_誤りを返して続ける(tmp_path):
    tools = tmp_path / "tools.json"
    tools.write_text("[]", encoding="utf-8")
    r = subprocess.run([sys.executable, str(ROOT / "geas" / "mcp_bridge.py"), str(tools), "http://127.0.0.1:9"],
                       input='not json\n[1, 2]\n{"jsonrpc": "2.0", "id": 7, "method": "ping"}\n', capture_output=True, text=True, encoding="utf-8", timeout=30)
    out = [json.loads(x) for x in r.stdout.splitlines()]
    assert r.returncode == 0, r.stderr
    assert [o.get("error", {}).get("code") for o in out] == [-32700, -32600, None] and out[2]["id"] == 7
