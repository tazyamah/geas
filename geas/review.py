# SPDX-License-Identifier: GPL-3.0-or-later
"""人が読む仕様書（HTML）を、コードから生成する。計画書（①）と同じ節の順で並べ、画面の遷移・デザイン・押して試せるモックを足す。
機械が比べる表（geas spec）とは別物で、人は直さない（直すときは会話で答えて、コードを変える）。"""
import json
import re
from html import escape

from .ast import *  # noqa: F401,F403
from .check import QUESTION, WARN
from .plan import COVERAGE_HEAD, Finding, UNSPEC, coverage, read_plan, usecases
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
#mock{position:relative;border:2px solid var(--line);border-radius:18px;max-width:400px;margin:.6em auto;background:#fff;color:#1c1c1a;overflow:hidden}#mock.dark{background:#1b1b1b;color:#eee}
#mock header{padding:10px 12px;color:#fff;display:flex;gap:8px;align-items:center}#mock header b{flex:1}#mock .in{padding:12px;display:grid;gap:10px}
#mock .ucid{position:absolute;left:6px;top:2px;font-size:.62rem;opacity:.45;color:#fff;z-index:1}
#mock button,#mock input{font:inherit}#mock button{border:0;border-radius:8px;padding:8px 12px;color:#fff}#mock input{padding:7px;border:1px solid #8886;border-radius:6px;width:100%;background:transparent;color:inherit}#mock label{font-size:.8rem;opacity:.8;display:block}
#mock table{min-width:0;width:100%}#mock th,#mock td{border-color:#8884}#mock .lay-grid .shows{display:grid;grid-template-columns:1fr 1fr;gap:8px}
#mock .banner{border-radius:8px;padding:6px 10px;font-size:.82rem;background:#fbf0d4;color:#6b4700}#mock.dark .banner{background:#3a2f12;color:#f0c36a}
.hl{outline:5px solid #ffd400!important;outline-offset:2px;box-shadow:0 0 0 9px rgba(255,212,0,.45)!important;animation:hl 1.2s ease-in-out infinite}
@keyframes hl{50%{box-shadow:0 0 0 14px rgba(255,212,0,.15)}}@media (prefers-reduced-motion:reduce){.hl{animation:none}}
.sc{margin-bottom:240px}
#pad{position:fixed;left:0;right:0;bottom:0;z-index:20;background:var(--card);border-top:2px solid var(--line);box-shadow:0 -4px 14px rgba(0,0,0,.18);padding:8px max(12px,env(safe-area-inset-right,0px)) calc(8px + env(safe-area-inset-bottom,0px)) max(12px,env(safe-area-inset-left,0px))}
#pad .l{display:flex;justify-content:space-between;gap:8px;font-size:.78rem;color:var(--mut);margin-bottom:4px}#pad .l span:first-child{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
#pad .r{display:flex;gap:6px;align-items:center;flex-wrap:wrap}#pad select{flex:1 1 150px;min-width:0;font:inherit;font-size:.88rem;padding:6px;border-radius:8px;border:1px solid var(--line);background:var(--bg);color:var(--fg)}
#pad button{font:inherit;font-size:.88rem;padding:7px 12px;border-radius:8px;border:1px solid var(--line);background:var(--bg);color:var(--fg);cursor:pointer}
#pad .go,#pad .okb{background:var(--acc);color:#fff;border-color:var(--acc)}#pad .okb{background:var(--ok);border-color:var(--ok)}#pad .ngb{color:var(--bad);border-color:var(--bad)}
#pad .ng{display:flex;gap:6px;margin-top:6px}#pad .ng input{flex:1;min-width:0;font:inherit;padding:6px;border-radius:8px;border:1px solid var(--line);background:var(--bg);color:var(--fg)}
#ngs li{margin:.3em 0}[hidden]{display:none!important}
.hint{font-size:.8rem;color:var(--mut);text-align:center}
"""

JS = """
const S=JSON.parse(document.getElementById('screens-data').textContent),U=JSON.parse(document.getElementById('ucs').textContent);
const mock=document.getElementById('mock'),pad=document.getElementById('pad'),sel=document.getElementById('opsel'),$=id=>document.getElementById(id);
let op=0,k=0,res=U.map(()=>null),editing=false,moved=false;
function el(t,c,h){const e=document.createElement(t);if(c)e.className=c;if(h!==undefined)e.textContent=h;return e}
function nsteps(){return U[op].steps.length}
function curScreen(){const u=U[op];return k<u.steps.length?u.steps[k].screen:u.end}
function banner(){const u=U[op];if(k>0){const p=u.steps[k-1];if(p.result&&p.to==='そのまま')return p.op+'の結果：'+p.result}return''}
function draw(){
 const u=U[op],n=curScreen(),s=S[n],d=s.design;mock.className=d.mode==='dark'?'dark':'';mock.innerHTML='';
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
 $('ngrow').hidden=!editing;
 list();if(moved){const h=mock.querySelector('.hl');if(h)h.scrollIntoView({block:'center',behavior:'smooth'})}}
function list(){const ul=$('ngs');ul.innerHTML='';res.forEach((r,i)=>{if(r&&r.v==='NG'){const li=el('li','',U[i].id+' '+U[i].name+'：'+(r.why||'（理由なし）')+'　［'+r.at+'］');ul.append(li)}});$('ngwrap').hidden=!ul.children.length}
function pick(i){op=i;k=0;editing=false;draw()}
function next(){if(k<nsteps()){k++;moved=true;draw()}}
function prev(){if(k>0){k--}else if(op>0){op--;k=nsteps()}editing=false;moved=true;draw()}
function advance(){if(op<U.length-1){pick(op+1)}else draw()}
$('bNext').onclick=next;$('bPrev').onclick=prev;$('bReset').onclick=()=>pick(op);sel.onchange=()=>pick(+sel.value);
$('bOk').onclick=()=>{res[op]={v:'OK'};advance()};
$('bNg').onclick=()=>{editing=true;$('why').value=res[op]&&res[op].v==='NG'?res[op].why:'';draw();$('why').focus()};
$('ngsave').onclick=()=>{res[op]={v:'NG',why:$('why').value,at:curScreen()+'・手順'+Math.min(k+1,nsteps())};editing=false;advance()};
$('copy').onclick=()=>{const t=Array.from($('ngs').children).map(x=>'- '+x.textContent).join('\\n');try{navigator.clipboard.writeText(t).catch(()=>{})}catch(e){}};
const sec=$('walk');
if('IntersectionObserver' in window){new IntersectionObserver(es=>{pad.hidden=!es[0].isIntersecting},{threshold:0.02}).observe(sec)}
draw();
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
            + table(["画面", "見せる", "入力欄（←選ぶ元）", "受け取る値", "入口", "選ばせる slot", "URL"], [tuple(show(c) for c in x) for x in spec["画面"]]
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
        add("design", "画面デザイン（①との突合には含めない）", f'<p class="sub">この指定は、実際の画面（geas が生成する汎用の画面）の配置・色・明暗にそのまま使われ、最初の画面の色はインストール時のアイコンとテーマ色にもなります。下の絵は、その配置の見取り図です。</p><div class="grid">{"".join(wfs)}</div>' + table(["画面", "指定", "値"], spec["画面デザイン"]))
        if plan_md and spec["ルート"]:
            cov, got, total = coverage(spec, read_plan(plan_md))
            add("coverage", f"内部 URL ごとの確認の網羅（{got} / {total}）", '<p class="sub">具体例とユースケースが、どの URL のどの結果（HTTP の状態）に当たっているか。「未」は、どちらでも確認していません。</p>' + table(COVERAGE_HEAD, cov))
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
        add("walk", "ユースケースを画面でたどる", '<p class="sub">画面の下に浮いているレビュー操作パネルで、ユースケースを選び、[次へ]を押していくと、押す場所が黄色く光って画面が進みます。最後に OK か NG を付けてください（NG は理由を書けます）。表の中身は型から作った見本で、処理は動きません。</p>'
            '<div class="sc"><div id="mock"></div></div>'
            '<div id="ngwrap" hidden><h3>NG にしたもの</h3><ul id="ngs" class="plain"></ul><button id="copy" type="button">コピー</button></div>'
            f'<script type="application/json" id="ucs">{json.dumps(uj, ensure_ascii=False).replace("</", "<\\/")}</script>'
            f'<script type="application/json" id="screens-data">{json.dumps(sj, ensure_ascii=False).replace("</", "<\\/")}</script>'
            '<div id="pad" role="group" aria-label="レビュー操作パネル"><div class="l"><span id="line"></span><span id="prog"></span></div>'
            '<div class="r"><select id="opsel" aria-label="確認するユースケース"></select><button id="bReset" type="button">最初から</button><button id="bPrev" type="button">前へ</button>'
            '<button id="bNext" class="go" type="button">次へ</button><button id="bOk" class="okb" type="button">OK</button><button id="bNg" class="ngb" type="button">NG</button></div>'
            '<div class="ng" id="ngrow" hidden><input id="why" placeholder="NGの理由（空でもよい）" aria-label="NGの理由"><button id="ngsave" type="button">保存して次へ</button></div></div>')

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
            + (("<h3>外から返ってくる内容</h3>" + table(["応答", "中身"], spec["応答"])) if spec["応答"] else "")
            + (("<h3>開発用の模擬（返事の結果の種類。先頭が平常）</h3>" + table(["外部作用", "返ってくる応答", "結果の種類（返事に入る中身）"], spec["模擬"])) if spec["模擬"] else ""))
    if spec["決め方"]:
        add("rules", "slot の決め方（サーバーが決める値。上の行から順に、最初に当てはまった値）", table(["slot", "条件", "値"], spec["決め方"]))
    if spec["出来事"]:
        add("events", "外の出来事", table(["出来事", "起こし方", "処理"], spec["出来事"]))
    if spec["削除"]:
        add("deletes", "削除", '<p class="sub">消してよい状態と、消したときの参照元の扱いです。「削除させない」と書いていないデータは、消されうるものとして扱います（決まっていないことは、確認してほしいことに出ます）。</p>'
            + table(["データ", "消してよい状態", "参照元の扱い"], spec["削除"]))
    if spec["ルート"]:
        add("routes", "URL（ルート）", '<p class="sub">外から触れるものはすべて URL を持ちます。リソースは名詞で、動作は index／show／create／update／destroy。クエリ文字列は使わず、条件はパスで渡します。結果の種類ごとの HTTP の状態も決まっています。</p>'
            + table(["メソッド", "経路", "動作", "呼ぶもの", "結果→状態"], spec["ルート"]))
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
