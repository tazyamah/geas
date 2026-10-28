# SPDX-License-Identifier: GPL-3.0-or-later
"""人が読む仕様書（HTML）を、コードから生成する。計画書（①）と同じ節の順で並べ、画面の遷移・デザイン・押して試せるモックを足す。
機械が比べる表（geas spec）とは別物で、人は直さない（直すときは会話で答えて、コードを変える）。"""
import json
import re
from html import escape

from .ast import *  # noqa: F401,F403
from .check import QUESTION, WARN
from .plan import Finding, UNSPEC, read_plan, usecases
from .spec import build, flow_diagram

MERMAID = "https://cdnjs.cloudflare.com/ajax/libs/mermaid/10.9.1/mermaid.min.js"


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
    return f'<div class="tw"><table class="{cls}"><thead><tr>{th}</tr></thead><tbody>{body}</tbody></table></div>'


def plan_sections(plan_md: str) -> tuple[str, str]:
    """計画書の「0. 概要」と「0.1 利用の形態」の本文（◆の表より前の文章）"""
    parts = re.split(r"^## ", plan_md, flags=re.M)
    got = {"概要": "", "利用": ""}
    for part in parts:
        head, _, body = part.partition("\n")
        if head.startswith("0.1"):
            got["利用"] = body
        elif head.startswith("0."):
            got["概要"] = body
    return got["概要"], got["利用"]


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


def screen_graph(ck) -> str:
    show = ck.p.show_names
    ids = {s.name: f"s{i}" for i, s in enumerate(ck.p.screens)}
    out = ["flowchart LR"]
    for i, s in enumerate(ck.p.screens):
        out.append(f'    {ids[s.name]}["{show(s.name)}"]')
        if i == 0 or s.entry:
            out.append(f'    in{i}(( )) --> {ids[s.name]}')
    for s in ck.p.screens:
        for t in s.opens:
            out.append(f'    {ids[s.name]} -- "行を選ぶ" --> {ids[t]}')
        for dn, rs in s.actions:
            for v, d in rs:
                if d in ids:
                    out.append(f'    {ids[s.name]} -- "{show(dn)}: {show(v)}" --> {ids[d]}')
    return "\n".join(out)


def state_graphs(spec) -> list[tuple[str, str]]:
    out = []
    for dname in dict.fromkeys(d for d, *_ in spec["遷移"]):
        lines = ["stateDiagram-v2"]
        names: dict[str, str] = {}

        def sid(s):
            return "[*]" if s == "(新規)" else names.setdefault(s, f"t{len(names)}")

        for d, a, b, proc in spec["遷移"]:
            if d == dname:
                lines.append(f"    {sid(a)} --> {sid(b)}: {proc}")
        lines += [f'    state "{s}" as {i}' for s, i in names.items()]
        for d, s, k in spec["状態"]:
            if d == dname and k == "終了" and s in names:
                lines.append(f"    {names[s]} --> [*]")
        out.append((dname, "\n".join(lines)))
    return out


CSS = """
:root{--bg:#fafaf8;--fg:#1c1c1a;--mut:#62625c;--line:#d9d8d2;--card:#fff;--acc:#0b5cad;--warn:#8a5a00;--bad:#b3261e;--ok:#1b6e3c}
@media (prefers-color-scheme:dark){:root:not([data-theme=light]){--bg:#161615;--fg:#ececea;--mut:#a3a39b;--line:#3a3a36;--card:#20201e;--acc:#6aa8ee;--warn:#e0a63c;--bad:#f2867f;--ok:#6cc58d;color-scheme:dark}}
:root[data-theme=dark]{--bg:#161615;--fg:#ececea;--mut:#a3a39b;--line:#3a3a36;--card:#20201e;--acc:#6aa8ee;--warn:#e0a63c;--bad:#f2867f;--ok:#6cc58d;color-scheme:dark}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.65 system-ui,"Hiragino Sans","Noto Sans JP",sans-serif}
main{max-width:980px;margin:0 auto;padding-block:16px;padding-left:max(16px,env(safe-area-inset-left,0px));padding-right:max(16px,env(safe-area-inset-right,0px))}h1{font-size:1.5rem;margin:.6em 0 .2em}h2{font-size:1.2rem;margin:2em 0 .5em;padding-top:.6em;border-top:1px solid var(--line)}h3{font-size:1rem;margin:1.2em 0 .4em}
.sub{color:var(--mut);font-size:.9rem}nav{display:flex;flex-wrap:wrap;gap:6px 14px;margin:.8em 0;font-size:.9rem}nav a{color:var(--acc)}
.tw{overflow-x:auto;margin:.5em 0}table{border-collapse:collapse;font-size:.88rem;min-width:100%}th,td{border:1px solid var(--line);padding:5px 8px;text-align:left;vertical-align:top}th{background:var(--card);white-space:nowrap}
code{background:var(--card);border:1px solid var(--line);border-radius:4px;padding:0 4px;font-size:.85em}
.card{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:12px 14px;margin:.6em 0}.q{border-left:4px solid var(--warn)}.q.bad{border-color:var(--bad)}
.ok{color:var(--ok)}.mermaid{overflow-x:auto;text-align:center;background:var(--card);border:1px solid var(--line);border-radius:10px;padding:8px;margin:.5em 0}
.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(260px,1fr));gap:10px}
.wf{border:1px solid var(--line);border-radius:10px;overflow:hidden;background:var(--card)}.wf header{padding:6px 10px;color:#fff;font-size:.9rem}.wf .body{padding:10px;display:grid;gap:6px;min-height:90px}
.wf .bar{background:var(--line);border-radius:4px;height:14px}.wf .tile{background:var(--line);border-radius:6px;height:46px}.wf .g{display:grid;grid-template-columns:1fr 1fr;gap:6px}.wf .btn{border-radius:6px;height:22px}
.wf.dark{background:#1b1b1b}.wf.dark .bar,.wf.dark .tile{background:#3a3a3a}
#mock{border:2px solid var(--line);border-radius:14px;max-width:420px;margin:.6em auto;background:#fff;color:#1c1c1a;overflow:hidden}#mock.dark{background:#1b1b1b;color:#eee}
#mock header{padding:10px 12px;color:#fff;display:flex;gap:8px;align-items:center}#mock header b{flex:1}#mock .in{padding:12px;display:grid;gap:10px}
#mock button{font:inherit;border:0;border-radius:8px;padding:8px 12px;color:#fff;cursor:pointer}#mock button.ghost{background:transparent;color:inherit;border:1px solid #8886}
#mock input{font:inherit;padding:7px;border:1px solid #8886;border-radius:6px;width:100%;background:transparent;color:inherit}#mock label{font-size:.8rem;opacity:.8;display:block}
#mock table{min-width:0;width:100%}#mock th,#mock td{border-color:#8884}#mock tr.row{cursor:pointer}#mock tr.row:hover{background:#8882}
#mock .res{display:grid;gap:6px;border:1px dashed #8888;border-radius:8px;padding:8px}#mock .lay-grid .shows{display:grid;grid-template-columns:1fr 1fr;gap:8px}
#uc button{font:inherit;font-size:.85rem;color:inherit;background:transparent;border:1px solid var(--line);border-radius:8px;padding:5px 10px;cursor:pointer}#uc ol{margin:.6em 0 0;padding-left:1.4em}
.hint{font-size:.8rem;color:var(--mut);text-align:center}
"""

JS = """
const S=JSON.parse(document.getElementById('screens-data').textContent),mock=document.getElementById('mock'),start=document.getElementById('mock-start').dataset.s.split(',');
const U=JSON.parse(document.getElementById('ucs').textContent),ucbox=document.getElementById('uc');
let stack=[start[0]],cur=-1,step=0;
function ev(scr,op,res){if(cur<0)return;const st=U[cur].steps[step];if(st&&st.screen===scr&&st.op===op&&st.result===res){step++;adv()}}
function adv(){const t=U[cur].steps;while(step<t.length&&t[step].op==='見る'&&t[step].screen===stack[stack.length-1])step++;drawUc()}
function pick(i){cur=i;step=0;stack=[U[i].steps[0].screen];draw();adv()}
function drawUc(){ucbox.innerHTML='';const sel=el('div');U.forEach((u,i)=>{const b=el('button','ghost',u.name);b.style.margin='2px';if(i===cur)b.style.background='#8883';b.onclick=()=>pick(i);sel.append(b)});ucbox.append(sel);
 if(cur>=0){const ol=el('ol');U[cur].steps.forEach((t,i)=>{ol.append(el('li','',(i<step?'✓ ':i===step?'▶ ':'')+t.screen+'：'+t.op+(t.result?'（'+t.result+'）':'')))});ucbox.append(ol);if(step>=U[cur].steps.length)ucbox.append(el('div','ok','このユースケースは、画面でたどれました。'))}}
function go(n){stack.push(n);draw();adv()}function back(){if(stack.length>1){stack.pop();draw();adv()}}
function el(t,c,h){const e=document.createElement(t);if(c)e.className=c;if(h!==undefined)e.innerHTML=h;return e}
function draw(){
 const n=stack[stack.length-1],s=S[n],d=s.design;mock.className=d.mode==='dark'?'dark':'';mock.innerHTML='';
 const h=el('header');h.style.background=d.accent;
 if(stack.length>1){const b=el('button','ghost','‹');b.style.color='#fff';b.onclick=back;h.append(b)}
 h.append(el('b','',n));mock.append(h);
 const body=el('div','in lay-'+d.layout);
 s.fields.forEach(f=>{const w=el('div');w.append(el('label','',f));const i=document.createElement('input');i.placeholder=f+' を入力';w.append(i);body.append(w)});
 const sh=el('div','shows');
 s.shows.forEach(q=>{const w=el('div');w.append(el('label','',q.title+'（見本）'));
  const t=el('table');t.innerHTML='<tr>'+q.cols.map(c=>'<th>'+c+'</th>').join('')+'</tr>';
  q.rows.forEach(r=>{const tr=el('tr',s.opens.length?'row':'');tr.innerHTML=r.map(c=>'<td>'+c+'</td>').join('');if(s.opens.length)tr.onclick=()=>{ev(n,'行を選ぶ','');go(s.opens[0])};t.append(tr)});
  w.append(t);if(s.opens.length)w.append(el('div','hint','行を押すと「'+s.opens[0]+'」へ'));sh.append(w)});
 body.append(sh);
 s.actions.forEach(a=>{const w=el('div');const b=el('button','',a.title);b.style.background=d.accent;
  const box=el('div','res');box.style.display='none';box.append(el('div','hint','この動作の結果はどうなりましたか（押して、画面の動きを確かめる）'));
  a.routes.forEach(r=>{const x=el('button','ghost',r.result+' → '+r.to);x.onclick=()=>{ev(n,a.title,r.result);if(r.kind==='screen')go(r.to);else if(r.kind==='back')back();else{box.style.display='none';draw();adv()}};box.append(x)});
  b.onclick=()=>{box.style.display=box.style.display==='none'?'grid':'none'};w.append(b);w.append(box);body.append(w)});
 mock.append(body)}
draw();drawUc();
"""


def tbl(spec, key, head):
    return table(head, spec[key])


def review(ck, plan_md: str | None = None, findings: list[Finding] | None = None, name: str = "", fragment: bool = False) -> str:
    """fragment=True は、<head> などの外枠と Mermaid の読み込みを持たない本文だけ（Artifact として公開する用）"""
    p, show = ck.p, ck.p.show_names
    ucs = usecases(read_plan(plan_md).get("ユースケース", [])) if plan_md else {}
    spec = build(ck)
    sec = []

    def add(aid, title, body):
        sec.append((aid, title, f'<section id="{aid}"><h2>{escape(title)}</h2>{body}</section>'))

    overview, usage = plan_sections(plan_md) if plan_md else ("", "")
    if overview:
        add("overview", "概要", md_to_html(overview))
    if usage:
        add("usage", "利用の形態", md_to_html(usage))

    # 確認すること
    qs = []
    for f in findings or []:
        qs.append(("bad" if f.kind != UNSPEC else "", f"<b>{escape(f.kind)}</b>　{escape(f.where)}<br>{escape(f.msg)}"))
    for dg in ck.diags:
        if dg.level in (QUESTION, WARN) or dg.level == "エラー":
            qs.append(("bad" if dg.level == "エラー" else "", f"<b>{escape(dg.level)}</b>　{escape(dg.where)}<br>{escape(dg.msg)}"))
    head = "計画書との食い違いはなく、検査の指摘もありません。" if not qs and plan_md else "指摘はありません。" if not qs else f"{len(qs)}件あります。"
    add("check", "確認してほしいこと", f'<p class="{"ok" if not qs else ""}">{head}</p>' + "".join(f'<div class="card q {c}">{b}</div>' for c, b in qs))

    # 画面
    if p.screens:
        sj = screens_json(ck)
        entries = [show(s.name) for i, s in enumerate(p.screens) if i == 0 or s.entry]
        add("screens", "画面の遷移", f'<pre class="mermaid">{escape(screen_graph(ck))}</pre>'
            + table(["画面", "見せる", "入力欄", "受け取る値", "入口"], [(show(a), show(b), show(c), show(d), e) for a, b, c, d, e in [(x[0], x[1], x[2], x[3], x[4]) for x in build(ck)["画面"]]]
                    ) + "<h3>遷移の表</h3>" + table(["画面", "操作", "結果", "行き先"], spec["画面遷移"]))
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
        add("design", "画面デザイン（①との突合には含めない）", f'<div class="grid">{"".join(wfs)}</div>' + table(["画面", "指定", "値"], spec["画面デザイン"]))
        uj = [{"name": k, "steps": [{"screen": r["画面"], "op": r["操作"], "result": r.get("結果", "")} for r in v]} for k, v in ucs.items()]
        add("walk", "ユースケースを画面でたどる", '<p class="sub">ユースケースを選び、手順どおりに押すと、画面でその動きが実現できることを確かめられます。表の中身は型から作った見本で、処理は動きません。結果は「どうなったか」を選びます。</p>'
            f'<div id="mock-start" data-s="{escape(",".join(entries))}"></div><div class="card" id="uc"></div><div id="mock"></div>'
            f'<script type="application/json" id="ucs">{json.dumps(uj, ensure_ascii=False).replace("</", "<\\/")}</script>'
            f'<script type="application/json" id="screens-data">{json.dumps(sj, ensure_ascii=False).replace("</", "<\\/")}</script>')

    # 仕様
    add("data", "データ", table(["データ", "項目", "型・範囲", "必須", "一意"], spec["データ"]))
    gs = "".join(f'<h3>{escape(d)}</h3><pre class="mermaid">{escape(g)}</pre>' for d, g in state_graphs(spec))
    add("states", "状態と遷移", gs + table(["データ", "状態", "区分"], spec["状態"]) + "<h3>遷移</h3>" + table(["データ", "元", "先", "起こす処理"], spec["遷移"])
        + ("<h3>拒否</h3>" + table(["データ", "元", "先", "理由"], spec["拒否"]) if spec["拒否"] else ""))
    procs = [(x["処理"], ", ".join(x["入力"]), ", ".join(x["読む"]), ", ".join(x["書く"]), " / ".join(f"{v}→{d}" for v, d in x["結果"]), ", ".join(x["外部"]) or "なし") for x in spec["処理"]]
    add("deeds", "処理", table(["処理", "入力", "読む", "書く", "結果 → 行き先", "外部作用"], procs))
    add("flows", "処理のつながり", "".join(f'<h3>{escape(show(f.name))}（×＝結果で行き先が分かれる、＋＝並列に起こす）</h3><pre class="mermaid">{escape(show(chr(10).join(flow_diagram(f))))}</pre>' for f in p.flows))
    if spec["問い合わせ"]:
        add("queries", "問い合わせ", table(["問い合わせ", "入力", "読む", "条件", "並び", "ページ"],
                                     [(x["問い合わせ"], ", ".join(x["入力"]), ", ".join(x["読む"]), ", ".join(x["条件"]), x["並び"], x["ページ"]) for x in spec["問い合わせ"]]))
    add("triggers", "待ちと期限（トリガ）", table(["トリガ", "要因", "行き先"], spec["トリガ"]))
    if spec["外部作用"]:
        add("effects", "外部とのやりとり", table(["外部作用", "出す内容"], spec["外部作用"]) + (("<h3>返事の合流先</h3>" + table(["外部作用", "合流先のトリガ"], spec["合流"])) if spec["合流"] else "")
            + (("<h3>外から返ってくる内容</h3>" + table(["応答", "中身"], spec["応答"])) if spec["応答"] else ""))
    if spec["出来事"]:
        add("events", "外の出来事", table(["出来事", "起こし方", "処理"], spec["出来事"]))
    add("invariants", "いつでも成り立つこと（不変条件）", table(["内容", "検査の結果"], [(t, "／".join(f"{a}: {b}" for a, b in rows)) for t, rows in ck.invariant_status]))

    nav = "".join(f'<a href="#{a}">{escape(t)}</a>' for a, t, _ in sec)
    title = f"仕様書 {name}".strip()
    body = (f'<main><h1>{escape(title)}</h1><p class="sub">コードから生成した人向けの仕様書です。直さずに、会話で答えてコードを変えます。</p><nav>{nav}</nav>'
            + "".join(h for _, _, h in sec) + f'</main><script>{JS if p.screens else ""}</script>')
    if fragment:
        return f'<meta name="claude:viewport-fit-sides" content="cover"><title>{escape(title)}</title><style>{CSS}body{{background:var(--bg);color:var(--fg)}}</style>{body}'
    return (f'<!doctype html><html lang="ja"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{escape(title)}</title>'
            f'<style>{CSS}body{{background:var(--bg);color:var(--fg)}}</style></head><body>{body}<script src="{MERMAID}"></script><script>'
            'if(window.mermaid)mermaid.initialize({startOnLoad:true,theme:matchMedia("(prefers-color-scheme: dark)").matches?"dark":"default",flowchart:{useMaxWidth:true,htmlLabels:false}});'
            "</script></body></html>")
