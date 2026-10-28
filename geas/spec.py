# SPDX-License-Identifier: GPL-3.0-or-later
"""コードから「検査可能な仕様書」（③）を生成する。突合では構造のまま使い、人向けには Markdown にする。"""
from .ast import *  # noqa: F401,F403
from .check import Checker, phys, tok_of


def ja_dest(d: str) -> str:
    """行き先の内部の書き方（reply / wait 名前）を、仕様書の日本語にする"""
    return "応答" if d == "reply" else "待つ " + d[5:] if d.startswith("wait ") else d


def ja_kind(k: str) -> str:
    return {"reply": "応答", "deadline": "期限", "all": "全部の応答"}.get(k, k)


def fmt_cause(kind: str, key: str) -> str:
    if kind == "all":
        return " ＋ ".join(key.split(","))  # 全部そろったら
    if kind != "deadline":
        return key
    s = int(key)
    for unit, sec in (("日", 86400), ("時間", 3600), ("分", 60)):
        if s % sec == 0 and not (unit == "日" and s < 2 * 86400):  # 24時間は「1日」でなく「24時間」と書く
            return f"現在+{s // sec}{unit}"
    return f"現在+{s}秒"


def fmt_secs(s: int) -> str:
    for unit, sec in (("日", 86400), ("時間", 3600), ("分", 60)):
        if s % sec == 0 and not (unit == "日" and s < 2 * 86400):
            return f"{s // sec}{unit}"
    return f"{s}秒"


def input_names(inputs) -> list[str]:
    """入力の名前。必ず要る入力（required）には「（必須）」を付ける（仕様書の関心は必須のほう）"""
    return [n if t.optional else f"{n}（必須）" for n, t in inputs]


def query_rows(q) -> dict:
    conds = [" または ".join(f"{f} {op} {st}" for f, op, st in group) for group in q.conds]
    ins = dict(q.inputs)

    def absent(x, what):  # 無いかもしれない入力が実際に無いときの動きも、仕様として出す
        t = ins.get(x)
        return f"（{x}が無ければ{what}）" if t is not None and t.optional else ""

    conds += [f"{f} 直近 {fmt_secs(int(a))}" if op == "直近" else f"{f} {op} {a}{absent(a, '絞らない')}"
              for f, op, a in q.filters]
    cols = [c.name for c in q.select]
    return {"問い合わせ": q.name, "入力": input_names(q.inputs), "読む": sorted(q.reads), "条件": conds, "列": cols,
            "並び": f"{q.order[0]} {q.order[1]}" if q.order else "",
            "ページ": f"{q.page_n}件" if q.page_n else f"{q.page}{absent(q.page, '全件')}" if q.page else ""}


def show_all(p, x):
    """仕様書の中の文字列の名前を、表示名（日本語）に置き換える（構造はそのまま）"""
    if isinstance(x, str):
        return p.show_names(x)
    if isinstance(x, dict):
        return {k: show_all(p, v) for k, v in x.items()}
    if isinstance(x, (list, tuple)):
        return type(x)(show_all(p, v) for v in x)
    return x


def build(ck: Checker) -> dict:
    spec = build_raw(ck)
    names = spec["名前"]
    spec = show_all(ck.p, spec)
    spec["名前"] = names
    return spec


def build_raw(ck: Checker) -> dict:
    p = ck.p
    spec: dict = {}
    spec["データ"] = [
        (d.name, f.name, show_type(f.type), "" if f.type.optional else "○", "○" if f.unique else "")
        for d in p.data
        for f in d.fields
    ]
    states, trans, rejects = [], [], []
    for d in p.data:
        for f in d.fields:
            if f.type.kind == "名前" and not ck.states[f.type.target].plain:
                s = ck.states[f.type.target]
                states.append((d.name, s.initial, "初期"))
                states += [(d.name, x, "中間") for x in s.middle]
                states += [(d.name, x, "終了") for x in s.final]
                rejects += [(d.name, a, b, why) for a, b, why in s.rejects]
    for proc in p.processes:
        for dt, a, b in sorted(ck.infos[proc.name].transitions):
            trans.append((dt, a, b, proc.name))
    spec["状態"], spec["遷移"], spec["拒否"] = states, trans, rejects

    routes: dict[str, dict[str, str]] = {}
    for fl in p.flows:
        for proc, rs in fl.entries:
            routes.setdefault(proc, {}).update({v: ja_dest(ds[0]) if len(ds) == 1 else "並列［" + " ＋ ".join(ja_dest(x) for x in ds) + "］" for v, ds in route_map(rs).items()})
    procs = []
    for proc in p.processes:
        info = ck.infos[proc.name]
        rdef = ck.results.get(proc.result)
        rs = []
        for v, _ in rdef.variants if rdef else []:
            rs.append((v, routes.get(proc.name, {}).get(v, "（未定義）")))
        procs.append({
            "処理": proc.name,
            "入力": input_names(proc.inputs),
            "読む": sorted(info.reads),
            "書く": sorted(info.writes),
            "結果": rs,
            "外部": sorted(f"{e}(冪等キー={k})" for e, k in info.effects),
            "参照": [(n, show_type(t)) for n, t in proc.refs],
            "採番": sorted(set(info.new_ids)),
            "物理数": [(n, phys(t)) for n, t in proc.inputs if t.kind == "整数"]
            + [(f"{n}.値", phys(t.value)) for n, t in proc.inputs if t.kind == "写像" and t.value.kind in ("整数", "金額")],
            "丸め": sorted(set(info.roundings)),
        })
    spec["処理"] = procs
    spec["問い合わせ"] = [query_rows(q) for q in p.queries]
    spec["トリガ"] = [(tg.name, f"{ja_kind(k)} {fmt_cause(k, key)}", d) for tg in p.triggers for k, key, d in tg.causes]
    spec["応答"] = [(rp.name, ", ".join(input_names(rp.params))) for rp in p.replies]
    spec["合流"] = [(e.name, e.joins) for e in p.effects if e.joins]
    spec["外部作用"] = [(e.name, ", ".join(f"{n}: {show_type(t)}" for n, t in e.params)) for e in p.effects]
    on = {"online": "つながりが戻ったとき: ", "opened": "アプリを開いたとき: "}
    spec["出来事"] = [(ev.name, on.get(ev.on, "") + (f"各 {q}" if k == "各" else "外から"), d) for ev in p.events for k, q, d in ev.steps]
    spec["画面"] = [(sc.name, ", ".join(sc.shows), ", ".join(f + (f"←{sc.field_from[f]}" if f in sc.field_from else "") for f in sc.fields),
                   ", ".join(n for n, _ in sc.params), "○" if i == 0 or sc.entry else "", ", ".join(sc.slots))
                  for i, sc in enumerate(p.screens)]
    spec["画面遷移"] = [(sc.name, "行を選ぶ", "", t) for sc in p.screens for t in sc.opens] + \
                     [(sc.name, dn, v, {"stay": "そのまま", "back": "戻る"}.get(dest, dest)) for sc in p.screens for dn, rs in sc.actions for v, dest in rs]
    spec["画面デザイン"] = [(sc.name, k, v) for sc in p.screens for k, v in sc.design.items()]
    spec["不変条件"] = [text for text, _ in ck.invariant_status]
    spec["部品"] = [(pt.name, ", ".join(f"{n}: {show_type(t)}" for n, t in pt.params), show_type(pt.ret), show(pt.guarantee) if pt.body is None else "= " + show(pt.body))
                  for pt in p.parts]
    spec["指定"] = [(f"{x['処理']}.丸め", f"{SHOW_JA.get(m, m)}({SHOW_JA.get(u, u)})") for x in procs for m, u in x["丸め"]]
    spec["名前"] = [(n, " / ".join(f"{k or '既定'}: {v}" for k, v in ls.items())) for n, ls in p.labels.items()]
    spec["実現"] = [(f"{d.name}.{f.name}", phys(f.type)) for d in p.data for f in d.fields if f.type.kind in ("整数", "金額")]
    return spec


def flow_diagram(fl) -> list[str]:
    """フローの図（BPMN 風の Mermaid）。結果ごとの行き先は排他ゲートウェイ（×）、par は並列ゲートウェイ（＋）、応答は終了、待つは待ち"""
    ids: dict[str, str] = {}
    out = ["flowchart LR"]

    def node(key: str, label: str, shape: str = "[{}]") -> str:
        if key not in ids:
            ids[key] = f"n{len(ids)}"
            out.append(f'    {ids[key]}{shape.format(chr(34) + label + chr(34))}')
        return ids[key]

    def dest(d: str) -> str:
        if d == "reply":
            return node("reply", "応答（reply）", "([{}])")
        if d.startswith("wait "):
            return node(d, "待つ " + d[5:], "[[{}]]")
        return node("処理:" + d, d)

    for proc, routes in fl.entries:
        src = node("処理:" + proc, proc)
        rm = route_map(routes)
        if len(rm) > 1:
            gw = node("×:" + proc, "×", "(({}))")
            out.append(f"    {src} --> {gw}")
            src_of = lambda v: (gw, v)
        else:
            src_of = lambda v: (src, v)
        for v, ds in rm.items():
            a, label = src_of(v)
            if len(ds) > 1:
                par = node(f"＋:{proc}:{v}", "＋", "(({}))")
                out.append(f'    {a} -- "{label}" --> {par}')
                out += [f"    {par} --> {dest(d)}" for d in ds]
            else:
                out.append(f'    {a} -- "{label}" --> {dest(ds[0])}')
    return out


def render(ck: Checker, spec: dict, name="") -> str:
    out = [f"# 生成した仕様書（③）{name}", "", "コードから生成。人が直さない。◆の欄は plan の仕様書（①）と機械で突合する。", ""]
    out += ["## データ", "", "| データ | 項目 | ◆型・範囲 | ◆必須 | ◆一意 |", "|---|---|---|---|---|"]
    out += [f"| {a} | {b} | {c} | {d} | {e} |" for a, b, c, d, e in spec["データ"]]
    out += ["", "## 状態と遷移", "", "| データ | ◆状態 | ◆区分 |", "|---|---|---|"]
    out += [f"| {a} | {b} | {c} |" for a, b, c in spec["状態"]]
    out += ["", "| データ | ◆元 | ◆先 | ◆起こす処理 |", "|---|---|---|---|"]
    out += [f"| {a} | {b} | {c} | {d} |" for a, b, c, d in spec["遷移"]]
    out += ["", "| データ | ◆元 | ◆先 | 拒否の理由 |", "|---|---|---|---|"]
    out += [f"| {a} | {b} | {c} | {d} |" for a, b, c, d in spec["拒否"]]
    out += ["", "## 処理", "", "| 処理 | ◆入力 | ◆読む | ◆書く | ◆結果 → 行き先 | ◆外部作用 |", "|---|---|---|---|---|---|"]
    for x in spec["処理"]:
        rs = " / ".join(f"{v}→{d}" for v, d in x["結果"])
        out.append(f"| {x['処理']} | {', '.join(x['入力'])} | {', '.join(x['読む'])} | {', '.join(x['書く'])} | {rs} | {', '.join(x['外部']) or 'なし'} |")
    for fl in ck.p.flows:
        out += ["", f"## フロー図 {fl.name}（×＝結果で行き先が分かれる、＋＝並列に起こす）", "", "```mermaid", *map(ck.p.show_names, flow_diagram(fl)), "```"]
    if spec["問い合わせ"]:
        out += ["", "## 問い合わせ（読むだけの処理）", "", "| 問い合わせ | ◆入力 | ◆読む | ◆条件 | ◆並び | ◆ページ |", "|---|---|---|---|---|---|"]
        for x in spec["問い合わせ"]:
            out.append(f"| {x['問い合わせ']} | {', '.join(x['入力'])} | {', '.join(x['読む'])} | {', '.join(x['条件'])} | {x['並び']} | {x['ページ']} |")
    out += ["", "## トリガ", "", "| トリガ | ◆要因 | ◆行き先 |", "|---|---|---|"]
    out += [f"| {a} | {b} | {c} |" for a, b, c in spec["トリガ"]]
    if spec["応答"]:
        out += ["", "## 応答の中身（外から返ってくる内容。入出力型）", "", "| 応答 | ◆中身 |", "|---|---|"]
        out += [f"| {a} | {b} |" for a, b in spec["応答"]]
    if spec["合流"]:
        out += ["", "## 返事の合流先（外部作用の返事が入るトリガ）", "", "| 外部作用 | ◆合流先のトリガ |", "|---|---|"]
        out += [f"| {a} | {b} |" for a, b in spec["合流"]]
    if spec["外部作用"]:
        out += ["", "## 外部作用（外へ出す内容。入出力型）", "", "| 外部作用 | ◆出す内容 |", "|---|---|"]
        out += [f"| {a} | {b} |" for a, b in spec["外部作用"]]
    if spec["出来事"]:
        out += ["", "## 出来事（外の出来事をきっかけに起こす処理）", "", "| 出来事 | ◆起こし方 | ◆処理 |", "|---|---|---|"]
        out += [f"| {a} | {b} | {c} |" for a, b, c in spec["出来事"]]
    if spec["画面"]:
        out += ["", "## 画面（入口は最初の画面と entry の画面。入力欄の「←」は選ぶ元の問い合わせ）", "", "| 画面 | ◆見せる | ◆入力欄 | ◆受け取る値 | ◆入口 | ◆選ばせる slot |", "|---|---|---|---|---|---|"]
        out += [f"| {a} | {b} | {c} | {d} | {e} | {f} |" for a, b, c, d, e, f in spec["画面"]]
        out += ["", "## 画面遷移", "", "| 画面 | ◆操作 | ◆結果 | ◆行き先 |", "|---|---|---|---|"]
        out += [f"| {a} | {b} | {c} | {d} |" for a, b, c, d in spec["画面遷移"]]
        out += ["", "## 画面デザイン（突合しない。配置と色）", "", "| 画面 | 指定 | 値 |", "|---|---|---|"]
        out += [f"| {a} | {b} | {c} |" for a, b, c in spec["画面デザイン"]]
    out += ["", "## 不変条件", "", "| ◆内容 | 検査の結果 |", "|---|---|"]
    for text, rows in ck.invariant_status:
        out.append(f"| {text} | " + "／".join(f"{p}: {s}" for p, s in rows) + " |")
    if spec["部品"]:
        out += ["", "## 部品（外から足す。呼ぶたびに、戻り値が保証に合うかを確かめる）", "", "| 部品 | ◆引数 | ◆戻り値 | ◆保証 |", "|---|---|---|---|"]
        out += [f"| {a} | {b} | {c} | {d} |" for a, b, c, d in spec["部品"]]
    out += ["", "## 指定事項（コードに書かれた、既定でない指定）", "", "| 項目 | ◆指定 |", "|---|---|"]
    out += [f"| {a} | {b} |" for a, b in spec["指定"]]
    if spec["名前"]:
        out += ["", "## 名前と表示名（説明コメント /// から）", "", "| 名前 | 表示名 |", "|---|---|"]
        out += [f"| {a} | {b} |" for a, b in spec["名前"]]
    out += ["", "## 実現の報告（振る舞いの仕様には含めない）", ""]
    out += [f"- 物理数 {a}: {b}" for a, b in spec["実現"]]
    for x in spec["処理"]:
        if x["参照"] or x["採番"]:
            out.append(f"- 処理 {x['処理']} の文脈: " + "、".join([f"参照 {n}: {t}（配備時に埋まる）" for n, t in x["参照"]]
                                                          + [f"採番 {d}（実行のたびに基盤が代入）" for d in x["採番"]]))
    out += ["", "## 検査の結果", ""]
    out += [f"- {d}" for d in ck.diags] or ["- 指摘なし"]
    return "\n".join(out) + "\n"
