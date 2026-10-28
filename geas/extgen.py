# SPDX-License-Identifier: GPL-3.0-or-later
"""page を持つ screen（ブラウザ拡張の画面）から、Chrome 拡張を作る。
拡張のファイル（background.js・content.js・popup）は、お店を知らない汎用のもの（geas/ext）で、ここで作る defs.js だけで動く。
ページから値を読む reader.js も汎用で、読み方は screen の read（defs.js の read）で決まる。手で書く部分は無い。"""
import json
import shutil
from pathlib import Path

from .rest import routes
from .screengen import CURRENCY_MINOR, query_def

DEFAULT_SERVER = "http://localhost:8787"


def ext_screens(ck):
    return [sc for sc in ck.p.screens if sc.pages]


def defs_of(ck, sc, server: str) -> dict:
    p, show = ck.p, ck.p.show_names
    qn = sc.shows[0]
    qd = query_def(ck, ck.queries[qn], show)
    route = next((m, path) for m, path, kind, tgt, _, _ in routes(p) if tgt == qn)
    title = p.title or show(sc.name)
    return {
        "title": title, "label": show(sc.name), "about": f"ページを開くと、{title}の「{show(qn)}」を表示します。",
        "server": server, "route": {"method": route[0], "path": route[1]},
        "input": {**qd["inputs"][0], "label": show(qd["inputs"][0]["name"]), "check": ck.queries[qn].inputs[0][1].check or ""},
        "read": {"subject": sc.read.subject, "urls": sc.read.urls, "elements": [list(e) for e in sc.read.elements], "rows": sc.read.value_rows, "lookup": sc.read.lookup}, "columns": qd["columns"], "minor": CURRENCY_MINOR,
        "texts": {"loading": "調べています…", "unknown": "この商品の{input}が分かりません（{subject}）。",
                  "none": f"この商品は、{title}では扱っていません（{{input}} {{value}}）。", "open": f"{title}を開く",
                  "failed": "調べられません", "offline": f"{title}のサーバーにつながりません"},
    }


def write_ext(ck, dest, server: str = DEFAULT_SERVER) -> None:
    sc = ext_screens(ck)[0]
    out = Path(dest) / "ext"
    out.mkdir(parents=True, exist_ok=True)
    for f in (Path(__file__).parent / "ext").iterdir():
        shutil.copy(f, out / f.name)
    d = defs_of(ck, sc, server)
    (out / "defs.js").write_text("self.GEAS_EXT = " + json.dumps(d, ensure_ascii=False, indent=1) + ";\n")
    origin = server.rstrip("/") + "/*"
    other = origin.replace("localhost", "127.0.0.1") if "localhost" in origin else None
    (out / "manifest.json").write_text(json.dumps({
        "manifest_version": 3, "name": d["label"], "version": "0.1.0",
        "description": f"{d['title']}：{d['label']}",
        "permissions": ["storage"],
        "host_permissions": [origin] + ([other] if other else []),
        "background": {"service_worker": "background.js"},
        "content_scripts": [{"matches": sc.pages, "js": ["defs.js", "reader.js", "content.js"], "css": ["content.css"], "run_at": "document_idle"}],
        "action": {"default_title": d["label"], "default_popup": "popup.html"},
    }, ensure_ascii=False, indent=2) + "\n")
