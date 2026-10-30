# SPDX-License-Identifier: GPL-3.0-or-later
"""画面（screen）から、実行時の画面が使う定義と、Rust の定数を作る。
画面のファイル（HTML・JS）は shop を知らない汎用のもの（geas/ui）で、ここで作る定義だけで動く。
手で書く画面は無い。見た目（design）は、定義と Rust の定数 SCREENS に載り、サーバーの manifest（テーマ色）にも使われる。"""
from .check import column_type
from .rest import screen_url

# 通貨ごとの最小単位の桁数（金額は最小単位の整数で持つ。check.CURRENCIES と同じ並び）
CURRENCY_MINOR = {"JPY": 0, "USD": 2, "EUR": 2}
DEFAULT_ACCENT = "#0b5cad"


def design_of(sc) -> dict:
    d = sc.design
    return {"layout": d.get("layout", "stack"), "accent": d.get("accent", DEFAULT_ACCENT), "mode": d.get("mode", "light")}


def type_def(ck, t, show) -> dict:
    """入力・列の型を、画面が欄を作るのに足りる形にする"""
    k = t.kind
    if k in ("ID", "参照"):
        return {"kind": "id"}
    if k == "整数":
        return {"kind": "int", "lo": t.range.lo if t.range else None, "hi": t.range.hi if t.range else None}
    if k == "金額":
        u = t.unit or ""
        return {"kind": "money", "unit": u[1:]} if u.startswith("$") else {"kind": "money", "currency": u}  # 通貨を持つ列の名前か、固定の通貨
    if k == "名前":
        st = ck.states.get(t.target)
        values = ([st.initial] if st and st.initial else []) + (st.middle if st else []) + (st.final if st else [])
        return {"kind": "enum", "values": [{"value": v, "label": show(v)} for v in values]}
    if k == "真偽":
        return {"kind": "bool"}
    if k == "文字列":
        return {"kind": "text", "min": t.range.lo if t.range else None, "max": t.range.hi if t.range else None}
    if k == "合成":
        return {"kind": "group", "parts": [{"name": n, "label": show(n), **type_def(ck, pt, show)} for n, pt in t.parts]}
    if k == "写像":
        return {"kind": "map", "key": t.key, "value": type_def(ck, t.value, show)}
    return {"kind": "text"}


def query_def(ck, q, show) -> dict:
    cols = []
    for c in q.select:
        cols.append({"name": c.name, "label": show(c.name), **type_def(ck, column_type(ck, q, c), show)})
    if not cols:  # 列の指定が無い問い合わせは、IDの列だけ
        cols = [{"name": "id", "label": "ID", "kind": "id"}]
    return {"inputs": [{"name": n, **type_def(ck, t, show)} for n, t in q.inputs], "columns": cols}


def field_type(ck, sc, f, show) -> dict:
    """入力欄の型。呼ぶ処理の入力から、無ければ見せる問い合わせの入力から取る"""
    for dn, _ in sc.actions:
        for n, t in ck.procs[dn].inputs:
            if n == f:
                return type_def(ck, t, show)
    for qn in sc.shows:
        for n, t in ck.queries[qn].inputs:
            if n == f:
                return type_def(ck, t, show)
    return {"kind": "text"}


def screens_def(ck) -> dict:
    p = ck.p
    show = p.show_names
    first = p.screens[0].name if p.screens else ""
    screens = {}
    for sc in p.screens:
        actions = []
        for dn, routes in sc.actions:
            proc = ck.procs[dn]
            rdef = ck.results[proc.result]
            variants = {v: [{"name": n, "label": show(n)} for n, _ in ps] for v, ps in rdef.variants}
            actions.append({
                "deed": dn, "label": show(dn),
                "inputs": [{"name": n, "label": show(n), **type_def(ck, t, show)} for n, t in proc.inputs],
                "routes": [{"result": v, "label": show(v), "to": d, "body": variants.get(v, [])} for v, d in routes],
            })
        slots = []
        for dn, _ in sc.actions:
            for n, t in ck.procs[dn].refs:
                if n in sc.slots and all(s["name"] != n for s in slots):
                    slots.append({"name": n, "label": show(n), **type_def(ck, t, show)})
        screens[sc.name] = {
            "label": show(sc.name), "url": screen_url(p, sc), "entry": sc.entry or sc.name == first,
            "params": [{"name": n, "label": show(n)} for n, _ in sc.params],
            "shows": list(sc.shows), "opens": list(sc.opens),
            "fields": [{"name": f, "label": show(f), "from": sc.field_from.get(f), "type": field_type(ck, sc, f, show)} for f in sc.fields],
            "slots": slots, "actions": actions, "design": design_of(sc),
        }
    return {
        "title": p.title or (show(first) if first else ""),
        "first": first,
        "order": [sc.name for sc in p.screens],
        "minor": CURRENCY_MINOR,
        "screens": screens,
        "queries": {q.name: {"label": show(q.name), **query_def(ck, q, show)} for q in p.queries},
        "events_on": {ev.name: ev.on for ev in p.events if ev.on},
    }


def rust_screens(ck) -> str:
    """lib.rs に足す定数。サーバーは、ここから manifest のテーマ色や名前を作る"""
    p = ck.p
    if not p.screens:
        return ""
    show = p.show_names
    first = p.screens[0].name
    title = (p.title or show(first)).replace("\\", "\\\\").replace('"', '\\"')
    out = ["// 【画面】：screen の宣言。見た目（design）はここに載る。画面のファイルは汎用で、定義（DEFS_JSON の screens）で動く。",
           "pub struct ScreenDef { pub name: &'static str, pub label: &'static str, pub url: &'static str, pub entry: bool, pub layout: &'static str, pub accent: &'static str, pub mode: &'static str }",
           f'pub const APP_TITLE: &str = "{title}";',
           "pub const screens: &[ScreenDef] = &["]
    for sc in p.screens:
        d = design_of(sc)
        label = show(sc.name).replace("\\", "\\\\").replace('"', '\\"')
        entry = "true" if sc.entry or sc.name == first else "false"
        out.append(f'    ScreenDef {{ name: "{sc.name}", label: "{label}", url: "{screen_url(p, sc)}", entry: {entry}, layout: "{d["layout"]}", accent: "{d["accent"]}", mode: "{d["mode"]}" }},')
    out.append("];\n")
    return "\n".join(out)


def write_ui(ck, dest) -> None:
    """汎用の画面一式（geas/ui）を dest/ui に置く。アイコンは、最初の画面の accent から作る（手で描いたものは無い）"""
    import shutil
    import struct
    import zlib
    from pathlib import Path
    src, out = Path(__file__).parent / "ui", Path(dest) / "ui"
    out.mkdir(parents=True, exist_ok=True)
    for f in src.iterdir():
        shutil.copy(f, out / f.name)
    p = ck.p
    accent = design_of(p.screens[0])["accent"] if p.screens else DEFAULT_ACCENT
    title = p.title or (p.show_names(p.screens[0].name) if p.screens else "")
    from html import escape
    (out / "icon.svg").write_text(
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512"><rect width="512" height="512" rx="96" fill="{accent}"/>'
        f'<text x="256" y="350" font-size="300" text-anchor="middle" fill="#fff" font-family="sans-serif">{escape(title[:1])}</text></svg>\n')
    rgb = bytes(int(accent[i:i + 2], 16) for i in (1, 3, 5))

    def png(n):
        row = b"\x00" + rgb * n
        raw = zlib.compress(row * n)
        chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
        return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", n, n, 8, 2, 0, 0, 0)) + chunk(b"IDAT", raw) + chunk(b"IEND", b"")
    for n in (192, 512):
        (out / f"icon-{n}.png").write_bytes(png(n))
