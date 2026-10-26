"""コードから「検査可能な仕様書」（③）を生成する。突合では構造のまま使い、人向けには Markdown にする。"""
from .ast import *  # noqa: F401,F403
from .check import Checker, phys, tok_of


def fmt_cause(kind: str, key: str) -> str:
    if kind != "期限":
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


def query_rows(q) -> dict:
    conds = [f"{f} {op} {st}" for f, op, st in q.conds]
    conds += [f"{f} 直近 {fmt_secs(int(a))}" if op == "直近" else f"{f} {op} {a}" for f, op, a in q.filters]
    return {"問い合わせ": q.name, "入力": [n for n, _ in q.inputs], "読む": sorted(q.reads), "条件": conds,
            "並び": f"{q.order[0]} {q.order[1]}" if q.order else "", "先頭": q.limit or ""}


def build(ck: Checker) -> dict:
    p = ck.p
    spec: dict = {}
    spec["データ"] = [
        (d.name, f.name, show_type(f.type), "" if f.optional else "○", "○" if f.unique else "")
        for d in p.data
        for f in d.fields
    ]
    states, trans, rejects = [], [], []
    for d in p.data:
        for f in d.fields:
            if f.type.kind == "名前":
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
            routes.setdefault(proc, {}).update(dict(rs))
    procs = []
    for proc in p.processes:
        info = ck.infos[proc.name]
        rdef = ck.results.get(proc.result)
        rs = []
        for v, _ in rdef.variants if rdef else []:
            rs.append((v, routes.get(proc.name, {}).get(v, "（未定義）")))
        procs.append({
            "処理": proc.name,
            "入力": [n for n, _ in proc.inputs],
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
    spec["トリガ"] = [(tg.name, f"{k} {fmt_cause(k, key)}", d) for tg in p.triggers for k, key, d in tg.causes]
    spec["外部作用"] = [(e.name, ", ".join(f"{n}: {show_type(t)}" for n, t in e.params)) for e in p.effects]
    spec["出来事"] = [(ev.name, (f"各 {q}" if k == "各" else "外から"), d) for ev in p.events for k, q, d in ev.steps]
    spec["不変条件"] = [text for text, _ in ck.invariant_status]
    spec["指定"] = [(f"{x['処理']}.丸め", f"{m}({u})") for x in procs for m, u in x["丸め"]]
    spec["実現"] = [(f"{d.name}.{f.name}", phys(f.type)) for d in p.data for f in d.fields if f.type.kind in ("整数", "金額")]
    return spec


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
    if spec["問い合わせ"]:
        out += ["", "## 問い合わせ（読むだけの処理）", "", "| 問い合わせ | ◆入力 | ◆読む | ◆条件 | ◆並び | ◆先頭 |", "|---|---|---|---|---|---|"]
        for x in spec["問い合わせ"]:
            out.append(f"| {x['問い合わせ']} | {', '.join(x['入力'])} | {', '.join(x['読む'])} | {', '.join(x['条件'])} | {x['並び']} | {x['先頭']} |")
    out += ["", "## トリガ", "", "| トリガ | ◆要因 | ◆行き先 |", "|---|---|---|"]
    out += [f"| {a} | {b} | {c} |" for a, b, c in spec["トリガ"]]
    if spec["外部作用"]:
        out += ["", "## 外部作用（外へ出す内容。入出力型）", "", "| 外部作用 | ◆出す内容 |", "|---|---|"]
        out += [f"| {a} | {b} |" for a, b in spec["外部作用"]]
    if spec["出来事"]:
        out += ["", "## 出来事（外の出来事をきっかけに起こす処理）", "", "| 出来事 | ◆起こし方 | ◆処理 |", "|---|---|---|"]
        out += [f"| {a} | {b} | {c} |" for a, b, c in spec["出来事"]]
    out += ["", "## 不変条件", "", "| ◆内容 | 検査の結果 |", "|---|---|"]
    for text, rows in ck.invariant_status:
        out.append(f"| {text} | " + "／".join(f"{p}: {s}" for p, s in rows) + " |")
    out += ["", "## 指定事項（コードに書かれた、既定でない指定）", "", "| 項目 | ◆指定 |", "|---|---|"]
    out += [f"| {a} | {b} |" for a, b in spec["指定"]]
    out += ["", "## 実現の報告（振る舞いの仕様には含めない）", ""]
    out += [f"- 物理数 {a}: {b}" for a, b in spec["実現"]]
    for x in spec["処理"]:
        if x["参照"] or x["採番"]:
            out.append(f"- 処理 {x['処理']} の文脈: " + "、".join([f"参照 {n}: {t}（配備時に埋まる）" for n, t in x["参照"]]
                                                          + [f"採番 {d}（実行のたびに基盤が代入）" for d in x["採番"]]))
    out += ["", "## 検査の結果", ""]
    out += [f"- {d}" for d in ck.diags] or ["- 指摘なし"]
    return "\n".join(out) + "\n"
