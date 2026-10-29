# SPDX-License-Identifier: GPL-3.0-or-later
"""人が読む仕様書（HTML）を、コードから生成する。見た目と構造は specgap のレビュー画面（問い→変更→ユースケース→画面→決まりごと→状態→つながり→流れ）に合わせ、
図は外部の読み込みのない自前の SVG で描く。geas 独自の画面デザインと、押して試せるモックを足す。
機械が比べる表（geas spec）とは別物で、人は直さない（直すときは会話で答えて、コードを変える）。"""
import json
import re
from html import escape

from .ast import *  # noqa: F401,F403
from .check import QUESTION, WARN
from . import svg
from .plan import COVERAGE_HEAD, Finding, UNSPEC, coverage, read_plan, split_top, tables, usecases
from .spec import build
from .mcp import tool_defs




def md_to_html(md: str) -> str:
    """計画書の文章の節（概要・利用の形態）を HTML にする。表・箇条書き・段落だけを扱う小さな変換"""
    out, rows, items = [], [], []

    def flush():
        nonlocal rows, items
        if rows:
            head, body = rows[0], [r for r in rows[2:]]
            out.append(table(head, body))
            rows = []
        if items:
            out.append("<ul>" + "".join(f"<li>{inline(i)}</li>" for i in items) + "</ul>")
            items = []

    for line in md.splitlines():
        s = line.strip()
        if s.startswith("|"):
            rows.append([c.strip() for c in s.strip("|").split("|")])
            continue
        if s.startswith("- "):
            items.append(s[2:])
            continue
        flush()
        if s.startswith("#"):
            out.append(f"<h3>{inline(s.lstrip('#').strip())}</h3>")
        elif s:
            out.append(f"<p>{inline(s)}</p>")
    flush()
    return "\n".join(out)


def inline(s: str) -> str:
    return re.sub(r"`([^`]+)`", r"<code>\1</code>", escape(s))


def table(head, rows, cls="") -> str:
    th = "".join(f"<th>{escape(h)}</th>" for h in head)
    body = "".join("<tr>" + "".join(f"<td>{inline(c) if isinstance(c, str) else escape(str(c))}</td>" for c in r) + "</tr>" for r in rows)
    return f'<div class="scroll"><table class="grid {cls}"><thead><tr>{th}</tr></thead><tbody>{body}</tbody></table></div>'


def plan_sections(plan_md: str) -> tuple[str, str]:
    """計画書の「0. 概要」と「0.1 利用の形態」の本文（◆の表より前の文章）"""
    parts = re.split(r"^## ", plan_md, flags=re.M)
    got = {"概要": "", "利用": ""}
    for part in parts:
        head, _, body = part.partition("\n")
        if head.startswith("0.1"):
            got["利用"] = body
        elif head.startswith("0.2"):  # その他の利用イメージは別のタブ（plan_others）
            continue
        elif head.startswith("0."):
            got["概要"] = body
    return got["概要"], got["利用"]


def plan_others(plan_md: str) -> tuple[str, list[tuple[str, list[dict]]]]:
    """計画書の「0.2 その他の利用イメージ」：前置きの文章と、場面ごとの台本（話し手・発言・呼ぶツールの表）。
    Geas で定義できないもの（AI との会話など）を、人と AI の認識合わせのために書く節で、突合しない"""
    for part in re.split(r"^## ", plan_md, flags=re.M):
        head, _, body = part.partition("\n")
        if not head.startswith("0.2"):
            continue
        intro, *scenes = re.split(r"^### ", body, flags=re.M)
        out = []
        for sc in scenes:
            title, _, rest = sc.partition("\n")
            rows = [r for hd, rs in tables(rest) for r in rs]
            out.append((title.strip(), rows))
        return intro, out
    return "", []


def mcp_summary(t: dict) -> str:
    sch = t["inputSchema"]
    ps = sch["properties"]
    return "、".join(f"{n}{'＊' if n in sch['required'] else ''}" for n in ps) or "なし"


def mcp_section(tools: list[dict]) -> str:
    rows = [(t["name"], t["title"], f'{t["geas"]["method"]} {t["geas"]["path"]}', mcp_summary(t),
             "読むだけ" if t["annotations"]["readOnlyHint"] else "消す" if t["annotations"]["destructiveHint"] else "書く") for t in tools]
    return (lead("AI（MCP クライアント）は、人や画面と同じ URL・同じ処理を、次のツールとして呼べます。ツールの定義は、URL の定義から機械的に作ったもので、手で足したものはありません。"
                 "押して動かすことはできません。入力の「＊」は必須です。")
            + table(["ツール名", "呼ぶもの", "URL", "入力", "種類"], rows))


def check_call(call: str, by_name: dict) -> str:
    """台本のツール呼び出し（名前と JSON の入力）が、ツール定義と合っているか。合っていれば空、違えばその理由"""
    name, _, arg = call.strip().partition(" ")
    name = name.strip("`")
    if name not in by_name:
        return "定義にないツール"
    sch = by_name[name]["inputSchema"]
    try:
        given = json.loads(arg.strip().strip("`") or "{}")
    except ValueError:
        return "入力が JSON ではありません"
    if set(given) - set(sch["properties"]):
        return "定義にない入力：" + "、".join(sorted(set(given) - set(sch["properties"])))
    if set(sch["required"]) - set(given):
        return "足りない入力：" + "、".join(sorted(set(sch["required"]) - set(given)))
    return ""


def others_view(plan_md: str, tools: list[dict]) -> str:
    intro, scenes = plan_others(plan_md)
    by_name = {t["name"]: t for t in tools}
    out = [f'<div class="intro">{md_to_html(intro)}</div>'] if intro.strip() else []
    for title, rows in scenes:
        bs = []
        for r in rows:
            who, say, call = r.get("話し手", ""), r.get("発言", ""), r.get("呼ぶツール", "")
            cls = {"人": "me", "AI": "ai"}.get(who, "tool")
            tool = ""
            if call:
                problem = check_call(call, by_name)
                tool = f'<div class="call{" bad" if problem else ""}">{f"⚠ {escape(problem)}：" if problem else "ツール："}{inline(call)}</div>'
            bs.append(f'<div class="bub {cls}"><span class="who">{escape(who)}</span><div class="say">{inline(say)}</div>{tool}</div>')
        out.append(f'<section><h2>{escape(title)}</h2><div class="chat">{"".join(bs)}</div></section>')
    return "".join(out)


def sample_value(ck, t: Type, i: int, label: str) -> str:
    k = t.kind
    if k == "ID":
        return f"{label[:2]}-{1000 + i}"
    if k == "文字列":
        return f"{label}の例{i}"
    if k in ("整数", "割合"):
        return str(i * 10)
    if k == "金額":
        return f"{i * 1200:,}"
    if k == "名前":
        st = ck.states.get(t.target)
        return ck.p.show_names(st.initial) if st and st.initial else t.target
    if k == "参照":
        return f"{t.target[:2]}-{1000 + i}"
    return "…"


def query_sample(ck, q, n=3) -> dict:
    """問い合わせの結果の見本。実際の値ではなく、列の型から作った例（モックで形を確かめるためのもの）"""
    show = ck.p.show_names
    dt = ck.data.get(q.target)
    cols = q.select or [Column(f.name, "項目", f.name) for f in (dt.fields[:1] if dt else [])]
    types = []
    for c in cols:
        ty = None
        if dt:
            if c.kind == "項目":
                f = next((x for x in dt.fields if x.name == c.field), None)
                ty = f.type if f else None
            elif c.kind == "参照先":
                via = next((x for x in dt.fields if x.name == c.via), None)
                tgt = ck.data.get(via.type.target) if via and via.type.kind == "参照" else None
                f = next((x for x in tgt.fields if x.name == c.field), None) if tgt else None
                ty = f.type if f else None
        types.append(ty or Type("整数"))
    return {"cols": [show(c.name) for c in cols],
            "rows": [[sample_value(ck, t, i + 1, show(c.name)) for c, t in zip(cols, types)] for i in range(n)]}


def screens_json(ck) -> dict:
    p, show = ck.p, ck.p.show_names
    out = {}
    for sc in p.screens:
        acts = []
        for dn, routes in sc.actions:
            proc = ck.procs[dn]
            acts.append({"title": show(dn), "inputs": [show(n) for n, _ in proc.inputs],
                         "routes": [{"result": show(v), "to": {"stay": "そのまま", "back": "戻る"}.get(d, show(d)), "kind": d if d in ("stay", "back") else "screen"} for v, d in routes]})
        out[show(sc.name)] = {
            "design": {"layout": sc.design.get("layout", "stack"), "accent": sc.design.get("accent", "#0b5cad"), "mode": sc.design.get("mode", "light")},
            "shows": [{"title": show(q), **query_sample(ck, ck.queries[q])} for q in sc.shows],
            "fields": [show(f) for f in sc.fields],
            "params": [show(n) for n, _ in sc.params],
            "opens": [show(o) for o in sc.opens],
            "actions": acts,
        }
    return out




# ---- 図（specgap と同じ自前 SVG。外部の読み込みがなく、オフラインでも出る） ----


def fig(diagram: str) -> str:
    return f'<div class="scroll figure">{diagram}</div>'


def lead(s: str) -> str:
    return f'<p class="lead">{s}</p>'


def effect_fields(content: str) -> str:
    """外部作用の「出す内容」（注文ID: ID, 通貨: Currency）から、項目名だけを取り出す"""
    return "・".join(x.split(":")[0].strip() for x in split_top(content, ","))


def usecase_externals(ck, spec: dict, ucs: dict) -> dict[str, dict[str, list[str]]]:
    """ユースケースの手順の処理から、フローと待ちをたどって、触れる外部システムと依頼の名前を集める"""
    show = ck.p.show_names
    procs = {x["処理"]: x for x in spec["処理"]}
    nxt: dict[str, list[str]] = {}
    for fl in ck.p.flows:
        for proc, routes in fl.entries:
            for ds in route_map(routes).values():
                for d in ds:
                    if d != "reply":
                        nxt.setdefault(show(proc), []).append("待つ " + show(d[5:]) if d.startswith("wait ") else show(d))
    for tname, _, to in spec["トリガ"]:
        nxt.setdefault("待つ " + tname, []).append(to)

    def reach(start: str) -> list[str]:
        seen, todo = [], [start]
        while todo:
            cur = todo.pop()
            if cur not in seen:
                seen.append(cur)
                todo += nxt.get(cur, [])
        return seen

    out: dict[str, dict[str, list[str]]] = {}
    for uc, rows in ucs.items():
        systems: dict[str, list[str]] = {}
        for r in rows:
            for q in reach(r["操作"]):
                for ext in procs.get(q, {}).get("外部", []):
                    system, _, op = ext.split("(")[0].partition(".")
                    if op not in systems.setdefault(system, []):
                        systems[system].append(op)
        out[uc] = systems
    return out


def usecase_diagram(ck, ucs: dict, spec: dict) -> str:
    """アクターは計画書の「アクター」列から。無いときは、ユースケースを始める画面ごとに分けて描く"""
    actors, cases = [], []
    given = any(rows[0].get("アクター") for rows in ucs.values())
    for name, rows in ucs.items():
        actor = rows[0].get("アクター") or rows[0]["画面"] + "から始める人"
        if actor not in actors:
            actors.append(actor)
        cases.append((actor, name, svg.SAME))
    ext = usecase_externals(ck, spec, ucs)
    sysmap: dict[str, tuple[list[str], list[str]]] = {}
    for uc, systems in ext.items():
        for system, ops in systems.items():
            ops_all, ucs_all = sysmap.setdefault(system, ([], []))
            ucs_all.append(uc)
            ops_all += [o for o in ops if o not in ops_all]
    externals = [(system, "・".join(ops), users) for system, (ops, users) in sysmap.items()]
    rows = [(a, c, str(len(ucs[c])), "、".join(dict.fromkeys(r["画面"] for r in ucs[c])), "、".join(ext[c]) or "なし") for a, c, _ in cases]
    return (lead("人と、その人が行うユースケースです。右の四角は、そのユースケースが処理を通じて触れる外部システムです。"
                 + ("" if given else "計画書にアクターの欄が無いので、ユースケースを始める画面ごとに分けています（ユースケースの表に「アクター」列を足すと、その名前で描きます）。")
                 + "実際の画面での動きは「動作」のタブで確かめます。")
            + fig(svg.usecases(actors, cases, "ユースケース図", externals))
            + table(["アクター" if given else "始める画面", "ユースケース", "手順数", "使う画面", "触れる外部システム"], rows))


def screen_flow(spec: dict) -> str:
    nodes = [x[0] for x in spec["画面"]]
    edges = []
    for scr, op, res, to in spec["画面遷移"]:
        if to in nodes and to != scr:
            edges.append(svg.Edge(scr, f"{op}：{res}" if res else op, to))
    entries = [x[0] for i, x in enumerate(spec["画面"]) if i and x[4]]
    outside = []
    if entries:
        outside = ["外から直接開く"]
        nodes.append(outside[0])
        edges += [svg.Edge(outside[0], "入口", e) for e in entries]
    diagram = svg.flow(nodes, edges, nodes[0], outside=outside, title="画面の流れ")
    return (lead("画面と、画面を移る操作です。左端の矢印が最初の画面です。同じ画面にとどまる操作は図に描かず、下の「遷移の表」に載せています。"
                 + ("点線の箱は、別の入口から画面を直接開くことを表します。" if outside else ""))
            + fig(diagram))


def state_machine(spec: dict, dname: str) -> str:
    states = [(s, k) for d, s, k in spec["状態"] if d == dname]
    names = [s for s, _ in states]
    trans = [(a, b, proc) for d, a, b, proc in spec["遷移"] if d == dname]
    edges = [svg.Edge(a, proc, b) for a, b, proc in trans if a in names]
    start = next(s for s, k in states if k == "初期")
    finals = [s for s, k in states if k == "終了"]
    diagram = svg.flow(names, edges, start, finals, title=f"状態の移り変わり {dname}")
    rows_of = list(dict.fromkeys(a for a, _, _ in trans))
    cols = [s for s in names if any(b == s for _, b, _ in trans)]
    cell = {}
    for a, b, proc in trans:
        cell.setdefault((a, b), []).append(proc)
    matrix = ('<div class="scroll"><table class="grid matrix"><thead><tr><th>元＼先</th>'
              + "".join(f"<th>{escape(c)}</th>" for c in cols) + "</tr></thead><tbody>"
              + "".join(f'<tr><th scope="row">{escape(r)}</th>' + "".join(f"<td>{escape(' ・ '.join(cell.get((r, c), [])))}</td>" for c in cols) + "</tr>" for r in rows_of)
              + "</tbody></table></div>")
    rejects = [(a, b, why) for d, a, b, why in spec["拒否"] if d == dname]
    return (lead(f"{escape(dname)}の状態と、状態を移す処理です。左端の矢印が最初の状態、二重枠が終わりの状態です。")
            + fig(diagram) + matrix
            + ("<h3>拒否</h3>" + table(["元", "先", "理由"], rejects) if rejects else ""))


def flow_edges(ck, fl, trig_rows) -> tuple[list[str], list[svg.Edge], str, list[tuple[str, str]]]:
    """応答で終わる結果は、矢印が一点に集まって読みにくいので図には描かず、別の表にする"""
    show = ck.p.show_names
    nodes: list[str] = []
    edges: list[svg.Edge] = []
    replies: list[tuple[str, str]] = []

    def node(n):
        if n not in nodes:
            nodes.append(n)
        return n

    def dest(d):
        return node("応答" if d == "reply" else "待つ " + show(d[5:]) if d.startswith("wait ") else show(d))

    for proc, routes in fl.entries:
        src = node(show(proc))
        for v, ds in route_map(routes).items():
            for d in ds:
                if d == "reply":
                    replies.append((show(proc), show(v)))
                else:
                    edges.append(svg.Edge(src, show(v) + ("（並列）" if len(ds) > 1 else ""), dest(d)))
    for name, why, to in trig_rows:
        if f"待つ {name}" in nodes and to in nodes:
            edges.append(svg.Edge(f"待つ {name}", why.split()[0] if why.split() else "トリガ", to))
    return nodes, edges, show(fl.entries[0][0]), replies


def process_flow(ck, spec, fl) -> str:
    nodes, edges, start, replies = flow_edges(ck, fl, spec["トリガ"])
    diagram = svg.flow(nodes, edges, start, title=f"処理の流れ {fl.name}")
    grouped: dict[str, list[str]] = {}
    for proc, v in replies:
        grouped.setdefault(proc, []).append(v)
    return (lead("処理から処理へのつながりです。矢印の文字は処理の結果の種類で、「並列」は同じ結果から複数の処理を同時に起こします。「待つ」は、返事や期限を待つ点です。"
                 "応答（呼んだ側へ結果を返して終わる）になる結果は、図に描かず下の表にまとめています。")
            + fig(diagram) + "<h3>応答で終わる結果</h3>" + table(["処理", "結果の種類"], [(k, "・".join(v)) for k, v in grouped.items()])
            + sequence_of(ck, spec, fl))


def sequence_of(ck, spec, fl) -> str:
    """このフローの処理が外へ出す依頼と、返ってくる返事を、時間の順に並べる"""
    me = "このシステム"
    fields = {n: effect_fields(c) for n, c in spec["外部作用"]}
    reply_of = {}
    for eff, resp, _ in spec["模擬"]:
        reply_of.setdefault(eff, resp)
    join = dict(spec["合流"])
    procs = {x["処理"]: x for x in spec["処理"]}
    parts, msgs = [me], []
    for proc, _ in fl.entries:
        name = ck.p.show_names(proc)
        if name in {m.label for m in msgs} or not procs.get(name, {}).get("外部"):
            continue
        msgs.append(svg.Message(me, me, name))
        for ext in procs[name]["外部"]:
            eff = ext.split("(")[0]
            system, _, op = eff.partition(".")
            if system not in parts:
                parts.append(system)
            msgs.append(svg.Message(me, system, f"{op}（{fields.get(eff, '')}）" if fields.get(eff) else op))
            back = reply_of.get(eff, "結果") + (f" → {join[eff]}で待つ" if eff in join else "")
            msgs.append(svg.Message(system, me, back, "dashed"))
    if not msgs:
        return ""
    return ("<h3>外部とのやりとりの順序</h3>"
            + lead("上から順に進みます。実線は外への依頼、破線は返事です。")
            + fig(svg.sequence(parts, msgs, f"外部とのやりとり {fl.name}")))


def system_context(spec: dict) -> str:
    by_system: dict[str, list[str]] = {}
    for name, content in spec["外部作用"]:
        system, _, op = name.partition(".")
        by_system.setdefault(system, []).append(f"{op}（{effect_fields(content)}）" if effect_fields(content) else op)
    systems = [(s, "外部システム", lines, svg.SAME) for s, lines in by_system.items()]
    return (lead("このシステムが外へ依頼する相手と、依頼の中身です。")
            + fig(svg.context("このシステム", systems, "システムのつながり"))
            + "<h3>外へ出す内容</h3>" + table(["外部作用", "出す内容"], spec["外部作用"])
            + (("<h3>返事の合流先</h3>" + table(["外部作用", "合流先のトリガ"], spec["合流"])) if spec["合流"] else "")
            + (("<h3>外から返ってくる内容</h3>" + table(["応答", "中身"], spec["応答"])) if spec["応答"] else "")
            + (("<h3>開発用の模擬（返事の結果の種類。先頭が平常）</h3>" + table(["外部作用", "返ってくる応答", "結果の種類（返事に入る中身）"], spec["模擬"])) if spec["模擬"] else ""))


def examples(title_rows, head, rows) -> str:
    """具体例は、specgap と同じく折りたたみの表にする"""
    return f'<details open><summary>具体例 {len(rows)} 件</summary>{table(head, rows)}</details>' if rows else ""


def card(cls: str, label: str, where: str, msg: str) -> str:
    return (f'<article class="card {cls}"><div class="chips"><span class="chip {cls}">{label}</span></div>'
            f'<p class="ctx">{escape(where)}</p><p class="q">{escape(msg)}</p></article>')



CSS = """
/* レイアウト：一段組みの読み物。上から「問い → 変更 → 全体像（図）→ 詳細（表）」の順に降りていく */
:root {
  --bg: #f4f6f6; --surface: #ffffff; --ink: #18252a; --muted: #5a6b71; --line: #d3dcde;
  --accent: #0d6870; --accent-soft: #dff0f1;
  --add: #1d7a3c; --add-soft: #e2f3e6; --remove: #b42a20; --remove-soft: #fbe6e3; --ask: #8a5800; --ask-soft: #fbefd5;
  --font-display: "Zen Kaku Gothic New", "Hiragino Sans", "Noto Sans JP", sans-serif;
  --font-body: "IBM Plex Sans JP", "Hiragino Sans", "Noto Sans JP", "Yu Gothic", sans-serif;
  --font-mono: "IBM Plex Mono", ui-monospace, "SFMono-Regular", Menlo, monospace;
}
@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) {
  --bg: #101618; --surface: #172024; --ink: #e2eaec; --muted: #98a9ae; --line: #2b393d;
  --accent: #5cc0c8; --accent-soft: #15353a;
  --add: #6fcf8e; --add-soft: #163322; --remove: #ff8b80; --remove-soft: #3c1c1a; --ask: #efbe5c; --ask-soft: #382a10;
  color-scheme: dark; } }
:root[data-theme="dark"] {
  --bg: #101618; --surface: #172024; --ink: #e2eaec; --muted: #98a9ae; --line: #2b393d;
  --accent: #5cc0c8; --accent-soft: #15353a;
  --add: #6fcf8e; --add-soft: #163322; --remove: #ff8b80; --remove-soft: #3c1c1a; --ask: #efbe5c; --ask-soft: #382a10;
  color-scheme: dark; }
body { background: var(--bg); color: var(--ink); font-family: var(--font-body); font-size: 15px; line-height: 1.7; margin: 0; }
.page { max-width: 1080px; margin: 0 auto; padding-block: 32px 64px;
  padding-left: max(16px, env(safe-area-inset-left, 0px)); padding-right: max(16px, env(safe-area-inset-right, 0px));
  display: flex; flex-direction: column; gap: 40px; }
h1, h2, h3 { font-family: var(--font-display); text-wrap: balance; line-height: 1.35; margin: 0; }
h1 { font-size: 2rem; }
h2 { font-size: 1.35rem; padding-bottom: 8px; border-bottom: 2px solid var(--ink); }
h3 { font-size: 1.05rem; margin-top: 8px; }
h4 { font-size: .85rem; margin: 0 0 6px; color: var(--muted); letter-spacing: .06em; }
section { display: flex; flex-direction: column; gap: 14px; min-width: 0; }
.top { display: flex; flex-direction: column; gap: 10px; }
.eyebrow { margin: 0; font-size: .78rem; letter-spacing: .14em; color: var(--accent); font-weight: 600; }
.meta { display: flex; flex-wrap: wrap; gap: 8px 16px; align-items: center; color: var(--muted); font-variant-numeric: tabular-nums; }
nav { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 4px; }
nav a { font-size: .85rem; color: var(--accent); text-decoration: none; padding: 3px 10px; border: 1px solid var(--line); border-radius: 999px; background: var(--surface); }
nav a:hover, nav a:focus-visible { border-color: var(--accent); outline: none; }
.lead { margin: 0; color: var(--muted); max-width: 46em; }
.muted { color: var(--muted); }
.chip { display: inline-block; font-size: .75rem; font-weight: 600; letter-spacing: .06em; padding: 1px 10px; border-radius: 999px; }
.chip.ok { background: var(--add-soft); color: var(--add); }
.chip.reject { background: var(--remove-soft); color: var(--remove); }
.chip.ask { background: var(--ask-soft); color: var(--ask); }
.chip.tested { background: var(--accent-soft); color: var(--accent); }
.vs { font-size: .82rem; margin-top: 4px; display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
.cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 12px; }
.card { background: var(--surface); border: 1px solid var(--line); border-left: 4px solid var(--line); border-radius: 6px; padding: 14px 16px;
  display: flex; flex-direction: column; gap: 8px; align-items: flex-start; min-width: 0; }
.card.reject { border-left-color: var(--remove); }
.card.ask { border-left-color: var(--ask); }
.card .ctx { margin: 0; font-size: .82rem; color: var(--muted); }
.card .q { margin: 0; font-size: 1.05rem; font-weight: 600; }
.card .detail { margin: 0; font-size: .85rem; color: var(--muted); }
.chips { display: flex; gap: 8px; align-items: center; }
.qno { font-size: .75rem; color: var(--muted); font-variant-numeric: tabular-nums; }
.how { margin: 4px 0 0; display: grid; grid-template-columns: auto 1fr; gap: 2px 10px; font-size: .8rem; border-top: 1px dashed var(--line); padding-top: 8px; width: 100%; }
.how dt { color: var(--muted); white-space: nowrap; }
.how dd { margin: 0; }
.kvs { display: flex; flex-wrap: wrap; gap: 6px; }
.kv { display: inline-flex; font-size: .8rem; border: 1px solid var(--line); border-radius: 4px; overflow: hidden; }
.kv .k { padding: 1px 6px; background: var(--bg); color: var(--muted); }
.kv .v { padding: 1px 8px; font-family: var(--font-mono); font-variant-numeric: tabular-nums; }
.scroll { overflow-x: auto; max-width: 100%; }
.figure { background: var(--surface); border: 1px solid var(--line); border-radius: 6px; padding: 8px; }
table.grid { border-collapse: collapse; width: 100%; background: var(--surface); font-size: .88rem; font-variant-numeric: tabular-nums; }
table.grid th, table.grid td { border: 1px solid var(--line); padding: 6px 10px; text-align: left; vertical-align: top; }
table.grid thead th { background: var(--bg); font-weight: 600; font-size: .8rem; color: var(--muted); white-space: nowrap; }
table.grid td.num, table.grid th.num { width: 3em; text-align: center; color: var(--muted); }
table.grid tr.added td, table.grid td.added { background: var(--add-soft); }
table.grid tr.gap td { background: var(--ask-soft); }
table.grid td.ask { background: var(--ask-soft); color: var(--ask); font-weight: 600; }
table.grid td.reject { background: var(--remove-soft); color: var(--remove); font-weight: 600; }
table.grid td.go { color: var(--accent); }
table.grid td.out, table.grid th.out { border-left: 2px solid var(--ink); }
table.grid td.was { color: var(--remove); }
table.grid td.now { color: var(--add); font-weight: 600; }
table.matrix th[scope=row] { white-space: nowrap; background: var(--bg); }
details summary { cursor: pointer; color: var(--muted); font-size: .85rem; margin: 2px 0 6px; }
.note { margin: 0; font-size: .85rem; padding: 6px 10px; border-radius: 4px; }
.note.reject { background: var(--remove-soft); color: var(--remove); }
ul.plain { margin: 0; padding-left: 1.2em; }
ul.plain li { margin: 2px 0; }
.ensures .formula { font-family: var(--font-mono); margin-left: 12px; }
.cgroups { display: grid; grid-template-columns: repeat(auto-fill, minmax(320px, 1fr)); gap: 16px; }
.cgroup { min-width: 0; }
.clist { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 4px; font-size: .88rem; }
.clist li { display: flex; gap: 8px; padding: 4px 8px; border-radius: 4px; overflow-wrap: anywhere; }
.clist li.added { background: var(--add-soft); }
.clist li.removed { background: var(--remove-soft); text-decoration: line-through; text-decoration-color: var(--remove); }
.clist li.changed { background: var(--ask-soft); }
.clist .sign { font-family: var(--font-mono); font-weight: 600; width: 1em; flex: none; }
.clist li.added .sign { color: var(--add); }
.clist li.removed .sign { color: var(--remove); }
footer { border-top: 1px solid var(--line); padding-top: 16px; font-size: .8rem; color: var(--muted); }
footer p { margin: 0 0 6px; max-width: 60em; }
code { font-family: var(--font-mono); font-size: .85em; }
svg.diagram { display: block; font-family: var(--font-body); font-size: 13px; }
svg.diagram text { fill: var(--ink); }
svg.diagram .node rect { fill: var(--surface); stroke: var(--muted); stroke-width: 1.25; }
svg.diagram .node rect.inner { fill: none; stroke-width: 1; }
svg.diagram .node.start rect { stroke: var(--accent); stroke-width: 2; }
svg.diagram .node.outside rect { fill: none; stroke-dasharray: 3 3; }
svg.diagram .node.added rect { fill: var(--add-soft); stroke: var(--add); }
svg.diagram .node.removed rect { fill: var(--remove-soft); stroke: var(--remove); stroke-dasharray: 4 3; }
svg.diagram .edge { fill: none; stroke: var(--muted); stroke-width: 1.25; }
svg.diagram .edge.added { stroke: var(--add); stroke-width: 2; }
svg.diagram .edge.removed { stroke: var(--remove); stroke-dasharray: 5 4; }
svg.diagram .arrowhead { fill: var(--muted); }
svg.diagram .arrowhead.added { fill: var(--add); }
svg.diagram .arrowhead.removed { fill: var(--remove); }
svg.diagram .elabel rect { fill: var(--surface); }
svg.diagram .elabel text { font-size: 11.5px; fill: var(--muted); }
svg.diagram .elabel.added text { fill: var(--add); font-weight: 600; }
svg.diagram .elabel.removed text { fill: var(--remove); text-decoration: line-through; }
svg.diagram .usecase ellipse { fill: var(--accent-soft); stroke: var(--accent); stroke-width: 1.25; }
svg.diagram .usecase.added ellipse { fill: var(--add-soft); stroke: var(--add); stroke-width: 2; }
svg.diagram .usecase.removed ellipse { fill: var(--remove-soft); stroke: var(--remove); stroke-dasharray: 4 3; }
svg.diagram .actor circle, svg.diagram .actor path { fill: none; stroke: var(--ink); stroke-width: 1.5; }
svg.diagram .actor text { font-weight: 600; }
svg.diagram text.sub { font-size: 11px; fill: var(--muted); }
svg.diagram .lifeline { fill: none; stroke: var(--line); stroke-width: 1.25; stroke-dasharray: 3 4; }
svg.diagram .msg { fill: none; stroke: var(--muted); stroke-width: 1.25; }
svg.diagram .msg.reject { stroke: var(--remove); stroke-width: 2; }
svg.diagram .msg.ask { stroke: var(--ask); stroke-width: 2; }
svg.diagram .mlabel text { font-size: 11.5px; }
svg.diagram .mlabel.reject text { fill: var(--remove); font-weight: 600; }
svg.diagram .mlabel.ask text { fill: var(--ask); font-weight: 600; }
svg.diagram .note rect { fill: var(--accent-soft); stroke: var(--accent); stroke-width: 1; }
svg.diagram .note text { font-size: 11.5px; }
svg.diagram .note.reject rect { fill: var(--remove-soft); stroke: var(--remove); stroke-width: 2; }
svg.diagram .note.ask rect { fill: var(--ask-soft); stroke: var(--ask); stroke-width: 2; }
.bad { color: var(--remove); font-weight: 600; }
@media (max-width: 480px) { h1 { font-size: 1.55rem; } .cards { grid-template-columns: 1fr; } }
@media (prefers-reduced-motion: reduce) { * { scroll-behavior: auto !important; } }
/* タブ：ユースケース図／画面の動き／決まりごと…。画面の動きのタブでだけ、レビュー操作パネルが浮く */
.tabs { display: flex; gap: 8px; flex-wrap: wrap; margin-top: 4px; }
.tab { font: inherit; font-size: .9rem; padding: 8px 16px; border: 1px solid var(--line); background: var(--surface); color: var(--ink); border-radius: 8px; cursor: pointer; }
.tab[aria-selected="true"], .tab[aria-pressed="true"] { background: var(--accent); border-color: var(--accent); color: #fff; }
.tab:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
.view { display: flex; flex-direction: column; gap: 40px; min-width: 0; }
.view[hidden] { display: none; }
.devs { display: flex; gap: 8px; }
.top { flex-direction: row; flex-wrap: wrap; align-items: center; gap: 8px 16px; }
.top h1 { font-size: 1rem; font-weight: 600; color: var(--muted); }
.top nav { margin-top: 0; }
.page { padding-block: 12px 40px; gap: 16px; }
#view-act { padding-bottom: 130px; gap: 16px; }
#mock { display: flex; flex-direction: column; height: clamp(300px, calc(100vh - 260px), 820px); }
#mock .in { flex: 1; overflow-y: auto; align-content: start; }
#mock.pc { max-width: none; }
.pick { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; font-size: .85rem; }
.pick label { font-size: .85rem; color: var(--muted); }
.pick select { font: inherit; padding: 6px 10px; border-radius: 8px; border: 1px solid var(--line); background: var(--surface); color: var(--ink); max-width: 100%; }
.sub-view { display: flex; flex-direction: column; gap: 40px; min-width: 0; }
.env { display: grid; gap: 10px; }
.env fieldset { border: 1px solid var(--line); border-radius: 10px; padding: 10px 12px; margin: 0; }
.env legend { font-size: .85rem; color: var(--muted); padding: 0 6px; }
.chips { display: flex; flex-wrap: wrap; gap: 6px; }
.envchip { font-size: .82rem; border: 1px solid var(--line); border-radius: 999px; padding: 2px 10px; background: var(--surface); }
#mock.pc { max-width: none; border-radius: 8px; }
/* geas 独自：画面のモック・レビュー操作パネル・画面デザインの見取り図 */
.sub { color: var(--muted); font-size: .9rem; margin: 0; }
.wfs { display: grid; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); gap: 10px; }
.wf { border: 1px solid var(--line); border-radius: 10px; overflow: hidden; background: var(--surface); }
.wf header { padding: 6px 10px; color: #fff; font-size: .9rem; }
.wf .body { padding: 10px; display: grid; gap: 6px; min-height: 90px; }
.wf .bar { background: var(--line); border-radius: 4px; height: 14px; }
.wf .tile { background: var(--line); border-radius: 6px; height: 46px; }
.wf .g { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; }
.wf .btn { border-radius: 6px; height: 22px; }
.wf.dark { background: #1b1b1b; }
.wf.dark .bar, .wf.dark .tile { background: #3a3a3a; }
#mock { width: 100%; position: relative; border: 2px solid var(--line); border-radius: 18px; max-width: 400px; margin: .6em auto; background: #fff; color: #18252a; overflow: hidden; }
#mock.dark { background: #1b1b1b; color: #eee; }
#mock header { padding: 10px 12px; color: #fff; display: flex; gap: 8px; align-items: center; }
#mock header b { flex: 1; }
#mock .in { padding: 12px; display: grid; gap: 10px; }
#mock .ucid { position: absolute; left: 6px; top: 2px; font-size: .62rem; opacity: .45; color: #fff; z-index: 1; }
#mock button, #mock input { font: inherit; }
#mock button { border: 0; border-radius: 8px; padding: 8px 12px; color: #fff; }
#mock input { box-sizing: border-box; padding: 7px; border: 1px solid #8886; border-radius: 6px; width: 100%; background: transparent; color: inherit; }
#mock label { font-size: .8rem; opacity: .8; display: block; }
#mock table { border-collapse: collapse; font-size: .85rem; width: 100%; }
#mock th, #mock td { border: 1px solid #8884; padding: 4px 6px; text-align: left; vertical-align: top; }
#mock .lay-grid .shows { display: grid; grid-template-columns: 1fr; gap: 8px; }
#mock.pc .lay-grid .shows { grid-template-columns: 1fr 1fr; }
#mock .banner { border-radius: 8px; padding: 6px 10px; font-size: .82rem; background: #fbf0d4; color: #6b4700; }
#mock.dark .banner { background: #3a2f12; color: #f0c36a; }
.hl { outline: 5px solid #ffd400 !important; outline-offset: 2px; box-shadow: 0 0 0 9px rgba(255, 212, 0, .45) !important; animation: hl 1.2s ease-in-out infinite; }
@keyframes hl { 50% { box-shadow: 0 0 0 14px rgba(255, 212, 0, .15); } }
@media (prefers-reduced-motion: reduce) { .hl { animation: none; } }
.sc { margin: 0; }
#pad { position: fixed; left: 0; right: 0; bottom: 0; z-index: 20; background: var(--surface); border-top: 2px solid var(--line); box-shadow: 0 -4px 14px rgba(0, 0, 0, .18);
  padding: 8px max(12px, env(safe-area-inset-right, 0px)) calc(8px + env(safe-area-inset-bottom, 0px)) max(12px, env(safe-area-inset-left, 0px)); }
#pad .l { display: flex; justify-content: space-between; gap: 8px; font-size: .78rem; color: var(--muted); margin-bottom: 4px; }
#pad .l span:first-child { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
#pad .r { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
#pad select { flex: 1 1 150px; min-width: 0; font: inherit; font-size: .88rem; padding: 6px; border-radius: 8px; border: 1px solid var(--line); background: var(--bg); color: var(--ink); }
#pad button { font: inherit; font-size: .88rem; padding: 7px 12px; border-radius: 8px; border: 1px solid var(--line); background: var(--bg); color: var(--ink); cursor: pointer; }
#pad .go, #pad .okb { background: var(--accent); color: #fff; border-color: var(--accent); }
#pad .okb { background: var(--add); border-color: var(--add); }
#pad .ngb { color: var(--remove); border-color: var(--remove); }
#pad #bNet.off { background: var(--ask-soft); color: var(--ask); border-color: var(--ask); }
#pad .ng { display: flex; gap: 6px; margin-top: 6px; }
#pad .ng input { flex: 1; min-width: 0; font: inherit; padding: 6px; border-radius: 8px; border: 1px solid var(--line); background: var(--bg); color: var(--ink); }
#ngs li { margin: .3em 0; }

.intro p { margin: 0 0 .6em; }
.chat { display: flex; flex-direction: column; gap: 10px; max-width: 640px; }
.bub { max-width: 88%; padding: 8px 12px; border-radius: 14px; border: 1px solid var(--line); background: var(--surface); }
.bub.me { align-self: flex-end; background: var(--accent); border-color: var(--accent); color: #fff; }
.bub.tool { align-self: center; max-width: 100%; background: var(--bg); font-size: .85rem; color: var(--muted); }
.bub .who { display: block; font-size: .72rem; opacity: .7; margin-bottom: 2px; }
.bub .call { margin-top: 6px; padding: 4px 8px; border-radius: 8px; background: var(--bg); color: var(--ink); font-size: .82rem; }
.bub .call.bad { background: var(--ask-soft); color: var(--ask); font-weight: 600; }
[hidden] { display: none !important; }
"""

TAB_JS = """
document.querySelectorAll('.tab[data-view]').forEach(function(t){t.addEventListener('click',function(){
 document.querySelectorAll('.tab[data-view]').forEach(function(x){x.setAttribute('aria-selected',x===t?'true':'false')});
 document.querySelectorAll('.view').forEach(function(v){v.hidden=v.id!=='view-'+t.dataset.view});window.scrollTo(0,0)})});
var org=document.getElementById('origin');
if(org){var fo=function(){['walk','api','mcp'].forEach(function(id){var e=document.getElementById(id);if(e)e.hidden=org.value!==id})};org.onchange=fo;fo()}
var sp=document.getElementById('specsel');
if(sp){var fs=function(){document.querySelectorAll('.sub-view').forEach(function(v){v.hidden=v.dataset.sub!==sp.value})};sp.onchange=fs;fs()}
document.querySelectorAll('[data-dev]').forEach(function(b){b.addEventListener('click',function(){
 document.querySelectorAll('[data-dev]').forEach(function(x){x.setAttribute('aria-pressed',x===b?'true':'false')});
 var m=document.getElementById('mock');if(m)m.classList.toggle('pc',b.dataset.dev==='pc')})});
"""

JS = """
const S=JSON.parse(document.getElementById('screens-data').textContent),U=JSON.parse(document.getElementById('ucs').textContent);
const mock=document.getElementById('mock'),pad=document.getElementById('pad'),sel=document.getElementById('opsel'),$=id=>document.getElementById(id);
let op=0,k=0,res=U.map(()=>null),editing=false,moved=false,net=true,netmsg='';
const EV=document.getElementById('events')?JSON.parse(document.getElementById('events').textContent):[];
function el(t,c,h){const e=document.createElement(t);if(c)e.className=c;if(h!==undefined)e.textContent=h;return e}
function nsteps(){return U[op].steps.length}
function curScreen(){const u=U[op];return k<u.steps.length?u.steps[k].screen:u.end}
function banner(){if(!net)return'オフライン：つながりが切れています';if(netmsg)return netmsg;const u=U[op];if(k>0){const p=u.steps[k-1];if(p.result&&p.to==='そのまま')return p.op+'の結果：'+p.result}return''}
function draw(){
 const u=U[op],n=curScreen(),s=S[n],d=s.design;mock.className=(d.mode==='dark'?'dark':'')+(document.querySelector('[data-dev=pc]').getAttribute('aria-pressed')==='true'?' pc':'');mock.innerHTML='';
 mock.append(el('span','ucid',u.id));
 const h=el('header');h.style.background=d.accent;h.append(el('b','',n));mock.append(h);
 const body=el('div','in lay-'+d.layout);const bn=banner();if(bn)body.append(el('div','banner',bn));
 const st=k<u.steps.length?u.steps[k]:null;
 s.fields.forEach(f=>{const w=el('div');w.append(el('label','',f));const i=document.createElement('input');i.placeholder=f;i.disabled=true;w.append(i);body.append(w)});
 const sh=el('div','shows');
 s.shows.forEach(q=>{const w=el('div');w.append(el('label','',q.title));const t=el('table');
  const hr=document.createElement('tr');q.cols.forEach(c=>hr.append(el('th','',c)));t.append(hr);
  q.rows.forEach((r,i)=>{const tr=document.createElement('tr');r.forEach(c=>tr.append(el('td','',c)));if(st&&st.op==='行を選ぶ'&&i===0)tr.className='hl';t.append(tr)});w.append(t);sh.append(w)});
 body.append(sh);
 s.actions.forEach(a=>{const b=el('button','',a.title);b.style.background=d.accent;if(st&&st.op===a.title)b.className='hl';body.append(b)});
 if(st&&st.op==='見る')h.className='hl';
 mock.append(body);
 const last=k>=nsteps();
 $('bNext').hidden=last;$('bOk').hidden=!last;$('bPrev').hidden=(op===0&&k===0);
 $('line').textContent=u.id+' '+u.name+'　'+(last?'手順は以上':(k+1)+'/'+nsteps()+' '+st.screen+(st.url?' '+st.url:'')+'：'+st.op+(st.result?'（'+st.result+'）':''));
 const done=res.filter(x=>x).length;$('prog').textContent=done===U.length?'すべて完了（'+done+'件）':'済 '+done+' / '+U.length;
 const ng=res[op]&&res[op].v==='NG';$('bNg').textContent=ng?'NG✎':'NG';$('bNg').title=ng?res[op].why:'';
 sel.innerHTML='';U.forEach((x,i)=>{const o=el('option','',(res[i]?(res[i].v==='OK'?'✓ ':'✗ '):'')+x.id+' '+x.name);o.value=i;if(i===op)o.selected=true;sel.append(o)});
 if($('bNet')){$('bNet').textContent=net?'オンライン':'オフライン';$('bNet').className=net?'':'off'}
 $('ngrow').hidden=!editing;
 list();if(moved){const h=mock.querySelector('.hl');if(h)h.scrollIntoView({block:'center',behavior:'smooth'})}}
function list(){const ul=$('ngs');ul.innerHTML='';res.forEach((r,i)=>{if(r&&r.v==='NG'){const li=el('li','',U[i].id+' '+U[i].name+'：'+(r.why||'（理由なし）')+'　［'+r.at+'］');ul.append(li)}});$('ngwrap').hidden=!ul.children.length}
function pick(i){op=i;k=0;editing=false;netmsg='';draw()}
function next(){if(k<nsteps()){k++;moved=true;netmsg='';draw()}}
function prev(){if(k>0){k--}else if(op>0){op--;k=nsteps()}editing=false;moved=true;netmsg='';draw()}
function advance(){if(op<U.length-1){pick(op+1)}else draw()}
if($('bNet'))$('bNet').onclick=()=>{net=!net;netmsg=net&&EV.length?EV[0].name+'：'+EV[0].proc+'を行います':'';draw()};
$('bNext').onclick=next;$('bPrev').onclick=prev;$('bReset').onclick=()=>pick(op);sel.onchange=()=>pick(+sel.value);
$('bOk').onclick=()=>{res[op]={v:'OK'};advance()};
$('bNg').onclick=()=>{editing=true;$('why').value=res[op]&&res[op].v==='NG'?res[op].why:'';draw();$('why').focus()};
$('ngsave').onclick=()=>{res[op]={v:'NG',why:$('why').value,at:curScreen()+'・手順'+Math.min(k+1,nsteps())};editing=false;advance()};
$('copy').onclick=()=>{const t=Array.from($('ngs').children).map(x=>'- '+x.textContent).join('\\n');try{navigator.clipboard.writeText(t).catch(()=>{})}catch(e){}};
const sec=$('walk');
if('IntersectionObserver' in window){new IntersectionObserver(es=>{pad.hidden=!es[0].isIntersecting},{threshold:0.02}).observe(sec)}
draw();
"""


CHANGE_KEYS = [("データ", "データ"), ("状態", "状態"), ("遷移", "遷移"), ("拒否", "拒否"), ("トリガ", "トリガ"), ("外部作用", "外部作用"), ("合流", "合流"), ("応答", "応答"), ("模擬", "模擬"),
               ("決め方", "slotの決め方"), ("出来事", "出来事"), ("削除", "削除"), ("ルート", "ルート"), ("画面", "画面"), ("画面遷移", "画面遷移"), ("画面デザイン", "画面デザイン")]


def _row_text(x) -> str:
    if isinstance(x, dict):
        return "　".join(f"{k}：{v if not isinstance(v, list) else '・'.join(map(str, v))}" for k, v in x.items() if k in ("処理", "問い合わせ", "入力", "読む", "書く", "結果", "外部", "条件", "列"))
    return "　／　".join(str(c) for c in x) if isinstance(x, (tuple, list)) else str(x)


def changes_section(new: dict, old: dict, label: str) -> str:
    """前の版との差。行の追加（＋）と削除（−）を、表の種類ごとに並べる（行が変わったものは、削除と追加の組になる）"""
    groups = []
    for key, title in CHANGE_KEYS + [("処理", "処理"), ("問い合わせ", "問い合わせ")]:
        a = [_row_text(x) for x in old.get(key, [])]
        b = [_row_text(x) for x in new.get(key, [])]
        added = [x for x in b if x not in a]
        removed = [x for x in a if x not in b]
        if not (added or removed):
            continue
        items = "".join(f'<li class="removed"><span class="sign">−</span>{escape(x)}</li>' for x in removed) + "".join(f'<li class="added"><span class="sign">＋</span>{escape(x)}</li>' for x in added)
        groups.append(f'<div class="cgroup"><h4>{escape(title)}</h4><ul class="clist">{items}</ul></div>')
    if not groups:
        return lead(f"{escape(label or '前の版')}から、仕様書に出る内容の変更はありません。")
    return lead(f"{escape(label or '前の版')}と比べて、書き換わった箇所です。意図した変更かどうかを確かめてください。") + f'<div class="cgroups">{"".join(groups)}</div>'



def collect_questions(ck, findings):
    """食い違い・検査のエラーは「拒否」、検査の問い・警告は「確認」、未規定は「記録」"""
    rejects, asks, records = [], [], []
    for f in findings or []:
        (records if f.kind == UNSPEC else rejects).append(card("reject" if f.kind != UNSPEC else "ask", escape(f.kind), f.where, f.msg))
    for dg in ck.diags:
        if dg.level == "エラー":
            rejects.append(card("reject", "拒否", dg.where, dg.msg))
        elif dg.level in (QUESTION, WARN):
            asks.append(card("ask", "確認", dg.where, dg.msg))
    return rejects, asks, records


def page(title: str, h1: str, head_extra: str, body: str, scripts: str, fragment: bool) -> str:
    head = (f"<title>{escape(title)}</title>\n"
            '<meta name="claude:viewport-fit-sides" content="cover">\n'
            '<link rel="preconnect" href="https://fonts.googleapis.com">\n'
            '<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500&family=IBM+Plex+Sans+JP:wght@400;500;600&family=Zen+Kaku+Gothic+New:wght@700&display=swap">\n'
            f"<style>{CSS}</style>\n")
    content = f'<div class="page"><header class="top"><h1>{escape(h1)}</h1>{head_extra}</header>{body}</div>{scripts}'
    if fragment:
        return head + content
    return ('<!doctype html>\n<html lang="ja"><head><meta charset="utf-8">'
            '<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">\n'
            f"{head}</head><body>{content}</body></html>\n")


def review_changes(ck, findings: list[Finding] | None = None, name: str = "", base_ck=None, base_label: str = "", fragment: bool = False) -> str:
    """仕様書とは別の、答えてほしいことと前の版からの変更だけの HTML。更新のたびに人が見るもの"""
    rejects, asks, records = collect_questions(ck, findings)
    verdict, vcls = ("受け入れ可", "ok") if not rejects else ("拒否", "reject")
    meta = [f'<span class="chip {vcls}">{verdict}</span>', f"<span>拒否 {len(rejects)} 件 ・ 確認 {len(asks)} 件 ・ 記録 {len(records)} 件</span>"]
    if base_ck is not None:
        meta.append(f"<span>比較元：{escape(base_label or '前の版')}</span>")
    parts = []
    if rejects or asks:
        parts.append('<section id="questions"><h2>答えてほしいこと</h2>'
                     + lead(f"拒否 {len(rejects)} 件、確認 {len(asks)} 件です。答えるとコードか計画書が直り、もう一度検査されます。")
                     + f'<div class="cards">{"".join(rejects + asks)}</div></section>')
    if base_ck is not None:
        parts.append(f'<section id="changes"><h2>前の版からの変更</h2>{changes_section(build(ck), build(base_ck), base_label)}</section>')
    if records:
        parts.append('<section id="open"><h2>意図的に決めていないこと</h2>'
                     + lead("計画書で「未規定」としたところです。実装が自由に選んでよい箇所で、書き忘れとは区別して記録しています。")
                     + f'<div class="cards">{"".join(records)}</div></section>')
    if not parts:
        parts.append(lead("答えてほしいことはありません。前の版も渡されていないので、変更点もありません。"))
    return page(f"確認事項と変更点 {name}".strip(), f"確認事項と変更点 {name.strip('（）')}".strip(), f'<div class="meta">{"".join(meta)}</div>', "".join(parts), "", fragment)


def review(ck, plan_md: str | None = None, findings: list[Finding] | None = None, name: str = "", fragment: bool = False) -> str:
    """人が読む仕様書。fragment=True は、<head> などの外枠を持たない本文だけ（Artifact として公開する用）。
    答えてほしいことと前の版からの変更は、別の HTML（review_changes）に出す"""
    p, show = ck.p, ck.p.show_names
    ucs = usecases(read_plan(plan_md).get("ユースケース", [])) if plan_md else {}
    plan = read_plan(plan_md) if plan_md else {}
    spec = build(ck)
    sec = []

    def add(aid, title, body):
        h2 = f"<h2>{escape(title)}</h2>" if title else ""
        sec.append((aid, title or aid, f'<section id="{aid}">{h2}{body}</section>'))

    _, _, records = collect_questions(ck, findings)

    overview, usage = plan_sections(plan_md) if plan_md else ("", "")
    if overview:
        add("overview", "概要", md_to_html(overview))
    if usage:
        add("usage", "利用の形態", md_to_html(usage))

    if ucs:
        add("usecases", "誰が何をするか", usecase_diagram(ck, ucs, spec))

    # 画面
    if p.screens:
        sj = screens_json(ck)
        add("screens", "画面の流れ", screen_flow(spec) + "<h3>画面の一覧</h3>"
            + table(["画面", "見せる", "入力欄（←選ぶ元）", "受け取る値", "入口", "選ばせる slot", "URL"], [tuple(show(c) for c in x) for x in spec["画面"]])
            + "<h3>遷移の表</h3>" + table(["画面", "操作", "結果", "行き先"], spec["画面遷移"]))
        wfs = []
        for n, s in sj.items():
            d = s["design"]
            tiles = {"grid": '<div class="g"><div class="tile"></div><div class="tile"></div><div class="tile"></div><div class="tile"></div></div>',
                     "list": '<div class="bar"></div><div class="bar"></div><div class="bar"></div>',
                     "form": '<div class="bar"></div><div class="bar"></div><div class="bar"></div>',
                     "stack": '<div class="tile"></div><div class="bar"></div>'}[d["layout"]]
            btn = f'<div class="btn" style="background:{d["accent"]}"></div>' if s["actions"] else ""
            wfs.append(f'<div class="wf {"dark" if d["mode"] == "dark" else ""}"><header style="background:{escape(d["accent"])}">{escape(n)}</header><div class="body">{tiles}{btn}</div>'
                       f'<div class="sub" style="padding:0 10px 8px">配置 {d["layout"]}／色 {escape(d["accent"])}／{"ダーク" if d["mode"] == "dark" else "ライト"}</div></div>')
        add("design", "画面デザイン（①との突合には含めない）", lead("この指定は、実際の画面（geas が生成する汎用の画面）の配置・色・明暗にそのまま使われ、最初の画面の色はインストール時のアイコンとテーマ色にもなります。下の絵は、その配置の見取り図です。")
            + f'<div class="wfs">{"".join(wfs)}</div>' + table(["画面", "指定", "値"], spec["画面デザイン"]))
        if plan_md and spec["ルート"]:
            cov, got, total = coverage(spec, plan)
            add("coverage", f"内部 URL ごとの確認の網羅（{got} / {total}）", lead("具体例とユースケースが、どの URL のどの結果（HTTP の状態）に当たっているか。「未」は、どちらでも確認していません。") + table(COVERAGE_HEAD, cov))
        dest_of = {(x[0], x[1], x[2]): x[3] for x in spec["画面遷移"]}
        url_of = {x[0]: x[-1] for x in spec["画面"]}
        uj = []
        for i, (k, v) in enumerate(ucs.items(), 1):
            steps, here = [], None
            for r in v:
                scr, o, rs = r["画面"], r["操作"], r.get("結果", "")
                to = dest_of.get((scr, o, rs), "")
                steps.append({"screen": scr, "op": o, "result": rs, "to": to, "url": url_of.get(scr, "")})
                here = scr if o == "見る" or to == "そのまま" else to
            uj.append({"id": f"UC{i}", "name": k, "steps": steps, "end": here or v[-1]["画面"]})
        net_events = [x for x in spec["出来事"] if "つながり" in x[0]]
        ex = {(r["ユースケース"], r["手順"]): r for r in plan.get("画面例", [])}
        per_uc = []
        for i, (k, v) in enumerate(ucs.items(), 1):
            rows_ = [(r["手順"], r["画面"], r["操作"] + (f"（{r['結果']}）" if r.get("結果") else ""), ex.get((k, r["手順"]), {}).get("入れる・選ぶ", ""), ex.get((k, r["手順"]), {}).get("見えること", "")) for r in v]
            per_uc.append(f'<details><summary><b>UC{i}</b> {escape(k)}（{len(v)} 手順）</summary>{table(["手順", "画面", "操作（結果）", "入れる・選ぶ", "見えること"], rows_)}</details>')
        env = ""
        if spec["模擬"]:
            env += '<fieldset><legend>外部の返事の種類（開発用の模擬）</legend><div class="chips">' + "".join(f'<span class="envchip">{escape(e)} → {escape(r)}</span>' for e, _, r in spec["模擬"]) + "</div></fieldset>"
        if spec["出来事"]:
            env += '<fieldset><legend>外の出来事</legend><div class="chips">' + "".join(f'<span class="envchip">{escape(e)}：{escape(w)}</span>' for e, w, _ in spec["出来事"]) + "</div></fieldset>"
        timers = [(n_, w) for n_, w, _ in spec["トリガ"] if "期限" in w]
        if timers:
            env += '<fieldset><legend>時間（期限）</legend><div class="chips">' + "".join(f'<span class="envchip">{escape(n_)}：{escape(w[w.index("期限"):])}</span>' for n_, w in timers) + "</div></fieldset>"
        env_html = ("<h3>この画面の外にある設定</h3>" + lead("画面の操作では動かせない、環境の側の設定です（オンライン／オフラインは、操作パネルで切り替えられます）。返事の種類と期限は、動作の一覧の「返事を落とす」「時計を進める」の手順で確かめます。")
                    + f'<div class="env">{env}</div>') if env else ""
        add("walk", None, '<div class="sc"><div id="mock"></div></div>'
            + env_html
            + "<h3>ユースケースの動作の一覧</h3>" + "".join(per_uc)
            + '<div id="ngwrap" hidden><h3>NG にしたもの</h3><ul id="ngs" class="plain"></ul><button id="copy" type="button">コピー</button></div>'
            + (f'<script type="application/json" id="events">{json.dumps([{"name": e, "proc": pr} for e, _, pr in net_events], ensure_ascii=False).replace("</", "<\\/")}</script>' if net_events else "")
            + f'<script type="application/json" id="ucs">{json.dumps(uj, ensure_ascii=False).replace("</", "<\\/")}</script>'
            f'<script type="application/json" id="screens-data">{json.dumps(sj, ensure_ascii=False).replace("</", "<\\/")}</script>'
            '<div id="pad" role="group" aria-label="レビュー操作パネル"><div class="l"><span id="line"></span><span id="prog"></span></div>'
            '<div class="r">' + ('<button id="bNet" type="button" aria-label="つながりの切り替え">オンライン</button>' if net_events else '') + '<select id="opsel" aria-label="確認するユースケース"></select><button id="bReset" type="button">最初から</button><button id="bPrev" type="button">前へ</button>'
            '<button id="bNext" class="go" type="button">次へ</button><button id="bOk" class="okb" type="button">OK</button><button id="bNg" class="ngb" type="button">NG</button></div>'
            '<div class="ng" id="ngrow" hidden><input id="why" placeholder="NGの理由（空でもよい）" aria-label="NGの理由"><button id="ngsave" type="button">保存して次へ</button></div></div>')

    # 動作：MCP（AI から）。ツールは URL の定義から機械的に作る
    tools = tool_defs(ck) if spec["ルート"] else []
    if tools:
        add("mcp", None, mcp_section(tools))

    # 動作：API
    if spec["ルート"]:
        url_ex = plan.get("URL例", [])
        add("api", None, lead("API は押して動かせません。計画書の URL の具体例を、リクエストと期待する応答の表にしています。")
            + (table(["ID", "メソッド", "経路", "入力", "期待する結果", "期待する状態"], [(r["ID"], r["メソッド"], r["経路"], r["入力"], r["期待する結果"], r["期待する状態"]) for r in url_ex])
               if url_ex else table(["メソッド", "経路", "動作", "呼ぶもの", "結果→状態"], spec["ルート"])))

    # 決まりごと
    add("data", "データ", table(["データ", "項目", "型・範囲", "必須", "一意"], spec["データ"]))
    procs = [(x["処理"], ", ".join(x["入力"]), ", ".join(x["読む"]), ", ".join(x["書く"]), " / ".join(f"{v}→{d}" for v, d in x["結果"]), ", ".join(x["外部"]) or "なし") for x in spec["処理"]]
    rules = "<h3>処理</h3>" + table(["処理", "入力", "読む", "書く", "結果 → 行き先", "外部作用"], procs)
    if spec["問い合わせ"]:
        rules += "<h3>問い合わせ</h3>" + table(["問い合わせ", "入力", "読む", "条件", "並び", "ページ"],
                                           [(x["問い合わせ"], ", ".join(x["入力"]), ", ".join(x["読む"]), ", ".join(x["条件"]), x["並び"], x["ページ"]) for x in spec["問い合わせ"]])
    if spec["決め方"]:
        rules += "<h3>slot の決め方（サーバーが決める値。上の行から順に、最初に当てはまった値）</h3>" + table(["slot", "条件", "値"], spec["決め方"])
    rules += "<h3>いつでも成り立つこと（不変条件）</h3>" + table(["内容", "検査の結果"], [(t, "／".join(f"{a}: {b}" for a, b in rows)) for t, rows in ck.invariant_status])
    add("rules", "決まりごと", rules)

    for dname in dict.fromkeys(d for d, *_ in spec["遷移"]):
        add(f"states-{dname}", f"状態の移り変わり：{dname}", state_machine(spec, dname))

    if spec["外部作用"]:
        add("systems", "システムのつながり", system_context(spec))
    for fl in p.flows:
        add(f"flow-{fl.name}", f"処理の流れ：{show(fl.name)}", process_flow(ck, spec, fl))

    add("triggers", "待ちと期限（トリガ）", table(["トリガ", "要因", "行き先"], spec["トリガ"])
        + (("<h3>外の出来事</h3>" + table(["出来事", "起こし方", "処理"], spec["出来事"])) if spec["出来事"] else ""))
    if spec["削除"]:
        add("deletes", "削除", lead("消してよい状態と、消したときの参照元の扱いです。「削除させない」と書いていないデータは、消されうるものとして扱います。")
            + table(["データ", "消してよい状態", "参照元の扱い"], spec["削除"]))
    if spec["ルート"]:
        add("routes", "URL（ルート）", lead("外から触れるものはすべて URL を持ちます。リソースは名詞で、動作は index／show／create／update／destroy。クエリ文字列は使わず、パスで表します。")
            + table(["メソッド", "経路", "動作", "呼ぶもの", "結果→状態"], spec["ルート"]))

    if records:
        add("open", "意図的に決めていないこと", lead("計画書で「未規定」としたところです。実装が自由に選んでよい箇所で、書き忘れとは区別して記録しています。") + f'<div class="cards">{"".join(records)}</div>')

    by_id = {a: h for a, _, h in sec}
    titles = {a: t for a, t, _ in sec}
    views, tabs = [], []

    def add_view(gid, label, html):
        first = not tabs
        tabs.append(f'<button class="tab" role="tab" id="t-{gid}" aria-selected="{"true" if first else "false"}" data-view="{gid}">{escape(label)}</button>')
        views.append(f'<div class="view" id="view-{gid}"{"" if first else " hidden"}>{html}</div>')

    # [ユースケース]：全体を把握するための図
    if "usecases" in by_id:
        add_view("uc", "ユースケース", by_id["usecases"])
    # [動作]：起点を選んで、動く画面とコントローラーで確かめる
    origins = [(a, l) for a, l in (("walk", "画面（スマホ・PC）"), ("api", "API"), ("mcp", "MCP（AI から）")) if a in by_id]
    if origins:
        devs = ('<span id="devs" class="devs" role="group" aria-label="端末"><button class="tab" data-dev="phone" aria-pressed="true">スマホ</button>'
                '<button class="tab" data-dev="pc" aria-pressed="false">PC</button></span>') if "walk" in by_id else ""
        sel = ('<label for="origin">動作の起点</label><select id="origin">'
               + "".join(f'<option value="{a}">{escape(l)}</option>' for a, l in origins) + "</select>") if len(origins) > 1 else ""
        hint = '<span class="muted">画面下部の操作パネルで動作を確認できます</span>' if "walk" in by_id else ""
        add_view("act", "動作", f'<div class="pick">{sel}{devs}{hint}</div>' + "".join(by_id[a] for a, _ in origins))
    # [仕様]：上の二つに入らなかったすべて。種類ごとに分けて、一つずつ見る
    spec_groups = [
        ("overview", "概要", ["overview", "usage"]),
        ("screens", "画面（流れ・一覧・デザイン・確認の網羅）", ["screens", "design", "coverage"]),
        ("data", "データと状態", ["data"] + [a for a in by_id if a.startswith("states-")]),
        ("rules", "決まりごと（処理・問い合わせ・不変条件）", ["rules"]),
        ("systems", "外部とのつながり", ["systems"]),
        ("flows", "処理の流れ", [a for a in by_id if a.startswith("flow-")]),
        ("triggers", "待ち・期限・出来事・削除", ["triggers", "deletes"]),
        ("routes", "URL", ["routes"]),
        ("open", "意図的に決めていないこと", ["open"]),
    ]
    spec_groups = [(g, l, [a for a in ids if a in by_id]) for g, l, ids in spec_groups]
    spec_groups = [x for x in spec_groups if x[2]]
    sub = "".join(f'<div class="sub-view" data-sub="{g}"{"" if i == 0 else " hidden"}>{"".join(by_id[a] for a in ids)}</div>' for i, (g, _, ids) in enumerate(spec_groups))
    pick = ('<div class="pick"><label for="specsel">分類</label><select id="specsel">'
            + "".join(f'<option value="{g}">{escape(l)}</option>' for g, l, _ in spec_groups) + "</select></div>")
    add_view("spec", "仕様", pick + sub)
    # [その他]：Geas で定義できないものの利用イメージ（AI との会話など）。計画書に書いたときだけ
    if plan_md and plan_others(plan_md)[1]:
        add_view("etc", "その他", others_view(plan_md, tools))
    nav = f'<div class="tabs" role="tablist">{"".join(tabs)}</div>'
    body = "".join(views)
    return page(f"仕様書 {name}".strip(), name.strip("（）") or "仕様書", nav, body, f"<script>{TAB_JS}</script>" + (f"<script>{JS}</script>" if p.screens else ""), fragment)
