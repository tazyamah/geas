# SPDX-License-Identifier: GPL-3.0-or-later
"""コードから「検査可能な仕様書」（③）を生成する。突合では構造のまま使い、人向けには Markdown にする。"""
from .rest import screen_url
from .ast import *  # noqa: F401,F403
from .check import Checker, VERIFIED_STORES, phys, tok_of


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


def order_text(ck: Checker, q) -> str:
    """並び：日時は新しい順／古い順、数は大きい順／小さい順（コードの desc／asc）"""
    if not q.order:
        return ""
    f, d = q.order
    dt = ck.data.get(q.target)
    kind = next((x.type.kind for x in dt.fields if x.name == f), None) if dt else None
    words = ("新しい順", "古い順") if kind == "日時" else ("大きい順", "小さい順")
    return f"{f} {words[0] if d == '降順' else words[1]}"


def query_rows(ck: Checker, q) -> dict:
    conds = [" または ".join(f"{f} {op} {st}" for f, op, st in group) for group in q.conds]
    ins = dict(q.inputs)

    def absent(x, what):  # 無いかもしれない入力が実際に無いときの動きも、仕様として出す
        t = ins.get(x)
        return f"（{x}が無ければ{what}）" if t is not None and t.optional else ""

    conds += [f"{f} 直近 {fmt_secs(int(a))}" if op == "直近" else f"{f} {op} {a}{absent(a, '絞らない')}"
              for f, op, a in q.filters]
    cols = [c.name for c in q.select]
    return {"問い合わせ": q.name, "入力": input_names(q.inputs), "読む": sorted(q.reads), "条件": conds, "列": cols,
            "並び": order_text(ck, q),
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


def read_text(rd) -> str:
    """ページからの読み方を、人向けの一文に"""
    if rd is None:
        return ""
    parts = [f"{rd.subject}を、" + "・".join([f"URL（{u}）" for u in rd.urls] + [f"要素 {sel}" + (f"（形 {m}）" if m else "") for sel, m in rd.elements]) + "から読む"]
    if rd.value_rows:
        parts.append("値は、" + "・".join(rd.value_rows) + " の行から読む")
    if rd.lookup:
        parts.append(f"読めなければ対応表（{len(rd.lookup)}件：" + "、".join(f"{k}→{v}" for k, v in rd.lookup.items()) + "）から引く")
    return "。".join(parts)


def tell(e) -> str:
    """処理の中身に出す式。コードの書き方（round(…)・sum { … }・if then else・and/or/not）を、読める言葉にする。
    括弧と結びつきの強さは show と同じ。意味を変えるものは何も省かない"""
    t = tell
    if isinstance(e, Call) and e.n == "round" and len(e.args) == 3:
        mode, unit = (SHOW_JA.get(show(x), show(x)) for x in e.args[1:])
        return f"丸め（{t(e.args[0])}、{unit} の単位に{mode}）"
    if isinstance(e, Call) and e.n in ("min", "max") and e.args:
        return f"{'小さい' if e.n == 'min' else '大きい'}ほう（{'、'.join(t(x) for x in e.args)}）"
    if isinstance(e, Call) and e.n == "abs" and len(e.args) == 1:
        return f"絶対値（{t(e.args[0])}）"
    if isinstance(e, Call) and e.n == "len" and len(e.args) == 1:
        return f"長さ（{t(e.args[0])}）"
    if isinstance(e, Call):
        return f"{e.n}({', '.join(t(x) for x in e.args)})"
    if isinstance(e, Not):
        return f"{show_in(e.e, 9, t)} でない"
    if isinstance(e, Cond):
        return f"{t(e.c)} なら {t(e.a)}、そうでなければ {t(e.b)}"
    if isinstance(e, Tuple):
        return "(" + ", ".join(map(t, e.items)) + ")"
    if isinstance(e, Attr):
        return f"{t(e.e)}.{e.n}"
    if isinstance(e, Index):
        return f"{e.d}[{t(e.e)}]"
    if isinstance(e, Make):
        return f"{e.n} {{ {', '.join(f'{n}: {t(x)}' for n, x in e.fields)} }}"
    if isinstance(e, Bin):
        op = {"in": "が次に入っている：", "==": "=", "!=": "≠", "<=": "≦", ">=": "≧"}.get(e.op, e.op)
        return f"{show_in(e.l, prec(e), t)} {op} {show_in(e.r, prec(e) + 1, t)}" + (f"（割る数が 0 なら {t(e.dflt)}）" if e.dflt is not None else "")
    if isinstance(e, Pred):
        return f"{show_in(e.e, 5, t)} が {e.kind}"
    if isinstance(e, Implies):
        return f"{t(e.l)} なら {t(e.r)}"
    if isinstance(e, (SetB, SumE)):
        b = e.binder
        names = b.names[0] if len(b.names) == 1 else f"({', '.join(b.names)})"
        only = f" のうち {t(e.cond)} のもの" if e.cond is not None else ""
        head = "合計" if isinstance(e, SumE) else "集まり"
        return f"{head}〔{t(b.src)} の各 {names}{only}について： {t(e.elem)}〕"
    return show(e)


def step_lines(body, depth: int = 0) -> list[str]:
    """処理の中身を、上から順の手順にする（1 行 = 1 文。入れ子は行頭の「　」の数）。
    仕様書の「読む・書く・結果」だけでは、どの条件でどの結果になるか、何をいくつ書くかが出ないので、
    コードの文をそのまま（名前は表示名で）並べる。ここに出ない動きを、処理は持たない"""
    pad = "　" * depth
    out: list[str] = []

    def ret(r) -> str:
        return "結果 " + r.variant + (f"（{', '.join(tell(a) for a in r.args)}）" if r.args else "")

    for st in body:
        if isinstance(st, Return):
            out.append(pad + ret(st))
        elif isinstance(st, Assign):
            out.append(pad + f"{st.name} = {tell(st.e)}" + (f"。無ければ {ret(st.absent)}" if st.absent is not None else ""))
        elif isinstance(st, Create):
            out.append(pad + f"{st.name} = {st.data} を新しく作る（{', '.join(f'{n}: {tell(x)}' for n, x in st.fields)}）")
        elif isinstance(st, Delete):
            out.append(pad + f"{st.var} を消す")
        elif isinstance(st, If):
            if len(st.body) == 1 and isinstance(st.body[0], Return) and st.els is None:
                out.append(pad + f"{tell(st.cond)} なら {ret(st.body[0])}")
            else:
                out.append(pad + f"{tell(st.cond)} なら：")
                out += step_lines(st.body, depth + 1)
                if st.els is not None:
                    out.append(pad + "そうでなければ：")
                    out += step_lines(st.els, depth + 1)
        elif isinstance(st, SetField):
            if st.op in ("+=", "-=") and isinstance(st.e, Bin):
                text = f"{st.var}.{st.f} {'に足す' if st.op == '+=' else 'から引く'}： {tell(st.e.r)}"
            else:
                text = f"{st.var}.{st.f} = {tell(st.e)}"
            if st.guard:
                text += "（範囲の端で止める）" if st.guard[0] == "saturating" else "（範囲は確かめない）"
            out.append(pad + text)
        elif isinstance(st, Update):
            b = st.binder
            names = b.names[0] if len(b.names) == 1 else f"({', '.join(b.names)})"
            op = {"+=": "に足す", "-=": "から引く"}.get(st.op)
            head = f"{tell(b.src)} の各 {names} について" + (f"（{tell(st.cond)} のものだけ）" if st.cond is not None else "") + "、"
            out.append(pad + head + (f"{tell(st.target)} {op}： {tell(st.value)}" if op else f"{tell(st.target)} {st.op} {tell(st.value)}"))
        elif isinstance(st, Request):
            out.append(pad + f"外部作用 {st.effect}（{', '.join(tell(a) for a in st.args)}）を要求する" + (f"（冪等キー = {tell(st.idem)}）" if st.idem is not None else ""))
        elif isinstance(st, ForEach):
            b = st.binder
            names = b.names[0] if len(b.names) == 1 else f"({', '.join(b.names)})"
            out.append(pad + f"{tell(b.src)} の各 {names} について：")
            out += step_lines(st.body, depth + 1)
        else:
            raise ValueError(st)
    return out


def step_tree(body) -> list:
    """処理の中身を、図（フローチャート）にするための木にする。step_lines と同じ内容で、形だけが違う。
    ("する", 文)／("結果", 文)／("もし", 条件, 成り立つときの木, 成り立たないときの木 か None)／("無ければ", 文, 結果の文)／("各", 見出し, 木)"""
    out = []
    for st in body:
        line = step_lines([st])[0].strip()
        if isinstance(st, Return):
            out.append(("結果", line[3:]))
        elif isinstance(st, Assign) and st.absent is not None:
            head, _, r = line.partition("。無ければ 結果 ")
            out.append(("無ければ", head, r))
        elif isinstance(st, If):
            out.append(("もし", tell(st.cond), step_tree(st.body), step_tree(st.els) if st.els is not None else None))
        elif isinstance(st, ForEach):
            out.append(("各", line.rstrip("："), step_tree(st.body)))
        else:
            out.append(("する", line))
    return out


# 役割の節の前置き（仕様書とレビュー画面で同じ文）
ACTOR_NOTE = ("役割は、前に置くプロキシが本人を確かめ、ヘッダ X-Geas-Actor に載せて渡します。"
              "ヘッダの無い呼び出しは 401、役割が合わない呼び出しは 403 で断ります")


def actor_rows(p, route_rows) -> list[tuple]:
    """仕様書の「役割」の表：(役割, ヘッダに載せる名前, 呼べる URL)。ヘッダに載せる名前は、コードの名前そのまま（プロキシの設定に書く値）。
    誰でも呼べる口は、最後の「誰でも」の行にまとめる"""
    from .rest import ANYONE, internal_targets, route_actors, routes
    if not p.actors:
        return []
    who, inner = route_actors(p), internal_targets(p)
    urls = [(f"{m} {path}", who.get(tgt)) for m, path, _, tgt, _, _ in routes(p) if tgt not in inner]
    out = [(p.show_names(n), n, "、".join(u for u, w in urls if w and n in w) or "なし") for n, _ in p.actors]
    return out + [("誰でも", "（要らない）", "、".join(u for u, w in urls if w == [ANYONE]) or "なし")]


def build(ck: Checker) -> dict:
    from .rest import INTERNAL, OPEN, actors_text, internal_targets, routes
    spec = build_raw(ck)
    names = spec["名前"]
    spec = show_all(ck.p, spec)
    inner = internal_targets(ck.p)
    spec["ルート"] = [(m, path, kind, ck.p.show_names(tgt), ", ".join(f"{ck.p.show_names(v)}→{c}" for v, c in st), INTERNAL if tgt in inner else OPEN, actors_text(ck.p, tgt, inner))
                     for m, path, kind, tgt, _, st in routes(ck.p)]  # 経路は名前のまま
    spec["役割"] = actor_rows(ck.p, spec["ルート"])
    spec["画面"] = [x[:-1] + (screen_url(ck.p, sc),) for x, sc in zip(spec["画面"], ck.p.screens)]  # 画面の URL も名前のまま
    spec["名前"] = names
    return spec


def build_raw(ck: Checker) -> dict:
    p = ck.p
    spec: dict = {}
    spec["アプリ"] = [("アプリの名前", p.title)] if p.title else []
    spec["データ"] = [
        (d.name, f.name, show_type(f.type), "" if f.type.optional else "○", "○" if f.unique else "")
        for d in p.data
        for f in d.fields
    ]
    states, trans, rejects = [], [], []
    for d in p.data:
        for f in d.fields:
            if f.type.kind == "名前" and f.type.target in ck.states and not ck.states[f.type.target].plain:
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
    spec["流れ"] = [(fl.name, proc) for fl in p.flows for proc, _ in fl.entries]  # 書いた順。行き先は処理の「結果」の列に出る
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
            "手順": step_lines(proc.body),
            "図": step_tree(proc.body),
            "外部": sorted(f"{e}(冪等キー={k})" for e, k in info.effects),
            "参照": [(n, show_type(t)) for n, t in proc.refs],
            "採番": sorted(set(info.new_ids)),
            "物理数": [(n, phys(t)) for n, t in proc.inputs if t.kind == "整数"]
            + [(f"{n}.値", phys(t.value)) for n, t in proc.inputs if t.kind == "写像" and t.value.kind in ("整数", "金額")],
            "丸め": sorted(set(info.roundings)),
        })
    spec["処理"] = procs
    spec["問い合わせ"] = [query_rows(ck, q) for q in p.queries]
    spec["トリガ"] = [(tg.name, f"{ja_kind(k)} {fmt_cause(k, key)}", d) for tg in p.triggers for k, key, d in tg.causes]
    spec["応答"] = [(rp.name, ", ".join(input_names(rp.params))) for rp in p.replies]
    spec["合流"] = [(e.name, e.joins) for e in p.effects if e.joins]
    reps = {r.name: r for r in p.replies}
    spec["模擬"] = [(e.name, e.replies, " ／ ".join(on + (f"（{', '.join(g)}）" if g else "") for on, g in (reps[e.replies].outcomes or [("平常", [n for n, _ in reps[e.replies].params])])))
                  for e in p.effects if e.replies and e.replies in reps]
    spec["決め方"] = [(f.slot, " かつ ".join(f"{pa} = {v}" for pa, v in conds) or "上のどれでもない", str(val)) for f in p.rules for conds, val in f.rows]
    spec["外部作用"] = [(e.name, ", ".join(f"{n}: {show_type(t)}" for n, t in e.params)) for e in p.effects]
    spec["通信"] = [(e.name, f"{e.http.method} {e.http.url}", e.http.bearer or "なし", "、".join(k if k == n else f"{k}←{n}" for k, n in e.http.body) or "なし",
                  " ／ ".join(f"{lo if lo == hi else f'{lo}〜{hi}'} → {o}" + "".join(f"（{n}←{path}）" for n, path in tk) for lo, hi, o, tk in e.http.statuses)
                  + (f" ／ その他 → {e.http.other[0]}" + "".join(f"（{n}←{path}）" for n, path in e.http.other[1]) if e.http.other else ""))
                 for e in p.effects if e.http]
    spec["データ用フォルダ"] = ([("データ用フォルダ", f"パス {p.data_folder[0]}")] if p.data_folder else []) + [
        (f"パス {n}（{os_name or '相対：exe のあるフォルダが起点'}）", path) for n, _, es, _ in p.paths for os_name, path, _ in es]
    spec["ファイル"] = [(e.name, {"write": "新規に書く（すでにあれば書かない）", "overwrite": "書く（あれば置き換える）", "read": "読む", "delete": "消す"}[e.file.op],
                      f"引数 {e.file.dir} の場所の下の {e.file.rest}",
                      " ／ ".join(f"{k} → {o}" + "".join(f"（{n}←{s}）" for n, s in tk) for k, (o, tk) in e.file.rows.items()))
                     for e in p.effects if e.file]
    r = p.resident
    spec["常駐"] = [] if r is None else [
        ("アイコンの説明", r.tooltip or "アプリ名"), ("始めかた", "窓を開いて始める" if r.start == "shown" else "アイコンだけで始める（窓は隠れている）"),
        ("左クリック", "窓を開閉する" if r.click == "toggle" else "右クリックと同じメニューを出す"), ("右クリックのメニュー「開く」", "窓を出す（言語が常に足す）"),
        *[("　――――", "区切り線") if kind == "separator" else (f"右クリックのメニュー「{label}」", f"画面 {target} を開く" if kind == "screen" else f"出来事 {target} を起こす")
          for label, kind, target, _ in r.menu],
        ("右クリックのメニュー「終了」", "アプリを終える（言語が常に足す）")]
    on = {"online": "つながりが戻ったとき: ", "opened": "アプリを開いたとき: "}
    spec["出来事"] = [(ev.name, on.get(ev.on, "") + (f"各 {q}" if k == "各" else "外から"), d) for ev in p.events for k, q, d in ev.steps]
    spec["画面"] = [(sc.name, ", ".join(sc.shows), ", ".join(f + (f"←{sc.field_from[f]}" if f in sc.field_from else "") for f in sc.fields),
                   ", ".join(n for n, _ in sc.params), "○" if i == 0 or sc.entry else "", ", ".join(sc.slots), screen_url(p, sc))
                  for i, sc in enumerate(p.screens)]
    spec["画面遷移"] = [(sc.name, "行を選ぶ", "", t) for sc in p.screens for t in sc.opens] + \
                     [(sc.name, dn, v, {"stay": "そのまま", "back": "戻る"}.get(dest, dest)) for sc in p.screens for dn, rs in sc.actions for v, dest in rs]
    from .rest import routes
    deleted = {d for info in ck.infos.values() for d in info.deletes}
    spec["削除"] = [(d.name, "削除させない" if d.keep else (", ".join(d.removable) or ("消せる" if d.name in deleted else "")),
                   ", ".join(f"{s}.{f}→{ {'detach': '参照を空に', 'cascade': '参照元も消す', 'leave': 'そのまま残す'}[pol] }" for s, f, pol in d.on_delete))
                  for d in p.data if d.keep or d.removable or d.on_delete or d.name in deleted]
    spec["人の保証"] = [(x.name, n, show_type(t), {"verify": "検証ビルドだけ確かめる", "assume": "確かめない", "dangerous": "確かめない（危険：未検査の raw も受ける）"}[t.trust[0]], t.trust[1])
                    for x in [*p.processes, *p.queries] for n, t in x.inputs if t.trust]
    def migration_row(version, op, dn, fn, arg):
        if op == "rename":
            return (f"版 {version}", "名前を変える", f"{dn}.{fn}", f"値はそのまま、項目 {arg[1]} に移る")
        if op == "drop":
            return (f"版 {version}", "項目を消す", f"{dn}.{fn}", "値は捨てる")
        kind, val = arg
        return (f"版 {version}", "項目を足す", f"{dn}.{fn}", "前からある行へ入れる値： " + {"null": "空（無い）", "str": quote_str(str(val))}.get(kind, str(val).lower() if kind == "bool" else str(val)))
    spec["保存の形"] = [migration_row(v, op, dn, fn, arg) for v, op, dn, fn, arg, _ in ck.migration_steps()]
    spec["保存先"] = [(d.name, d.store, {"process": "このアプリのプロセスの中だけ", "store": "保存先が直列にする"}[p.isolations[d.store][0]]
                    + (f"（{p.isolations[d.store][1]}）" if p.isolations[d.store][0] == "store" else "")) for d in p.data if d.store and d.store in p.isolations]
    from . import envreq
    spec["動かす前提"] = envreq.rows(p)
    for where, (who, kind, why, _) in p.isolations.items():  # 直列の担い手。geas が契約の試験で確かめてある種類の保存先は、人の保証ではない
        if who == "process":
            spec["人の保証"].append((f"（保存先 {where}）", "リクエストの直列化", "このアプリのプロセスの中だけ", "geas のサーバーが直列にする（別プロセスとの関係は約束しない）", "プロセスの外は保証の対象外"))
        elif kind not in VERIFIED_STORES:
            spec["人の保証"].append((f"（保存先 {where}）", "リクエストの直列化", f"保存先 {kind} に任せる", "保存先が、1 リクエストを直列に並べた場合と同じ結果にする", why))
    for x in p.processes:  # 足し引きの範囲を人が引き受けた文
        for target, op, mode, why in ck.infos[x.name].guards:
            d, f = target.split(".")
            ft = next(y.type for y in ck.data[d].fields if y.name == f)
            spec["人の保証"].append((x.name, f"{target} {op}", show_type(ft),
                                  "範囲の端で止まる" if mode == "saturating" else "確かめない（届くと異常で止まる）", why))
    spec["拡張"] = [(sc.name, ", ".join(sc.pages), ", ".join(sc.shows), ", ".join(n for n, _ in ck.queries[sc.shows[0]].inputs), read_text(sc.read)) for sc in p.screens if sc.pages and sc.shows and sc.shows[0] in ck.queries]
    spec["画面デザイン"] = [(sc.name, k, v) for sc in p.screens for k, v in sc.design.items()]
    spec["不変条件"] = [text for text, _ in ck.invariant_status]
    spec["部品"] = [(pt.name, ", ".join(f"{n}: {show_type(t)}" for n, t in pt.params), show_type(pt.ret),
                   "= " + show(pt.body) if pt.body is not None else (show(pt.guarantee) if pt.guarantee is not None else "戻り値の範囲"))
                  for pt in p.parts if not pt.external or pt.name in ck.used_libs]
    from .partcheck import load_ledger, status_text
    ledger = load_ledger(getattr(ck, "code_path", None))  # 部品の境界値の検査（geas parts --record）の結果。宣言を実際に呼んで確かめたかを添える
    for pt in p.parts:  # 根の言語のライブラリを呼ぶ部品は、中身を検査器が見られない。呼ぶ先と、戻り値を確かめる段階を人の保証に出す
        if pt.external and pt.name in ck.used_libs:
            crate, version, expr, level, why = pt.lib
            where = crate if crate == "std" else f"{crate} {version}"
            spec["人の保証"].append((f"（ライブラリ {pt.name}）", f"Rust の {where} を呼ぶ: {expr}", show_type(pt.ret),
                                  {"check": "戻り値を毎回確かめる", "verify": "戻り値を検証ビルドだけ確かめる", "assume": "戻り値を確かめない"}[level]
                                  + "（panic は異常で止まる。無限ループ・メモリ枯渇は防げない）。" + status_text(pt, ledger), why))
    spec["保証の強さ"] = guarantee_levels(ck, spec)
    spec["指定"] = [(f"{x['処理']}.丸め", f"{SHOW_JA.get(m, m)}({SHOW_JA.get(u, u)})") for x in procs for m, u in x["丸め"]]
    spec["名前"] = [(n, " / ".join(f"{k or '既定'}: {v}" for k, v in ls.items())) for n, ls in p.labels.items()]
    spec["実現"] = [(f"{d.name}.{f.name}", phys(f.type)) for d in p.data for f in d.fields if f.type.kind in ("整数", "金額")]
    return spec


# 保証の強さ（強い順）。GitHub Next の PDD Kit が結果に「証明」か「テスト」かのラベルを付けるのに倣い、
# 約束ごとに「何が守っているか」を 1 つの表にする（人は、強さが弱い行だけを重点的に読めばよい）
LEVEL_PROVEN = "検査器が示した"
LEVEL_RUNTIME = "実行時に毎回確かめる"
LEVEL_VERIFY = "検証ビルドだけ確かめる"
LEVEL_STOP = "範囲の端で止まる（人の保証）"
LEVEL_TRUST = "確かめない（人の保証）"
LEVEL_NOT = "示せていない"
LEVELS = [LEVEL_PROVEN, LEVEL_RUNTIME, LEVEL_VERIFY, LEVEL_STOP, LEVEL_TRUST, LEVEL_NOT]


def level_of_check(text: str) -> str:
    """人の保証の表の「実行時の確認」の文言から、保証の強さを決める"""
    if "範囲の端で止まる" in text:
        return LEVEL_STOP
    if "毎回確かめる" in text:
        return LEVEL_RUNTIME
    if "検証ビルドだけ" in text:
        return LEVEL_VERIFY
    return LEVEL_TRUST


def guarantee_levels(ck: Checker, spec: dict) -> list[tuple[str, str, str]]:
    """(強さ, 対象, 内容)。不変条件は検査器が示せたか、人の保証の行は実行時にどこまで確かめるかで分ける"""
    rows = []
    for text, st in ck.invariant_status:
        ok = all(s.startswith("証明済み") or s == "対象外" for _, s in st)
        rows.append((LEVEL_PROVEN if ok else LEVEL_NOT, "不変条件", text))
    for who, target, typ, check, why in spec["人の保証"]:
        rows.append((level_of_check(check), f"{who} {target}", f"{typ}：{check}（理由：{why}）"))
    rank = {x: i for i, x in enumerate(LEVELS)}
    return sorted(rows, key=lambda r: rank[r[0]])  # 弱いものほど下に集まる。同じ強さの中は宣言の順


def level_counts(rows) -> list[tuple[str, int]]:
    return [(lv, sum(1 for r in rows if r[0] == lv)) for lv in LEVELS if any(r[0] == lv for r in rows)]


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
    if spec.get("アプリ"):
        out += [f"アプリの名前：{spec['アプリ'][0][1]}", ""]
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
    out += ["", "## 処理の中身（上から順に進む。「結果」に着いたら、そこで終わる）"]
    for x in spec["処理"]:
        out += ["", f"### {x['処理']}", ""]
        if x["参照"]:
            out += ["外から決まる値（slot）：" + "、".join(f"{n}: {ty}" for n, ty in x["参照"]), ""]
        out += [f"{i}. {s}" for i, s in enumerate(x["手順"], 1)]
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
    if spec["決め方"]:
        out += ["", "## slot の決め方（サーバーが決める値。上の行から順に、最初に当てはまった値になる）", "", "| slot | 条件 | 値 |", "|---|---|---|"]
        out += [f"| {a} | {b} | {c} |" for a, b, c in spec["決め方"]]
    if spec["模擬"]:
        out += ["", "## 開発用の模擬（外部作用の返事。先頭が平常。開発用の操作パネルで切り替える）", "", "| 外部作用 | 返ってくる応答 | 結果の種類（返事に入る中身） |", "|---|---|---|"]
        out += [f"| {a} | {b} | {c} |" for a, b, c in spec["模擬"]]
    if spec["合流"]:
        out += ["", "## 返事の合流先（外部作用の返事が入るトリガ）", "", "| 外部作用 | ◆合流先のトリガ |", "|---|---|"]
        out += [f"| {a} | {b} |" for a, b in spec["合流"]]
    if spec["データ用フォルダ"]:
        out += ["", "## データ用フォルダ（DB・ログ・シークレットの置き場。OS ごとの書き方。実際の環境で書き込めるかは、起動時に確かめる）", "", "| 場合 | ◆パス |", "|---|---|"]
        out += [f"| {a} | {b} |" for a, b in spec["データ用フォルダ"]]
    if spec["ファイル"]:
        out += ["", "## ファイルを触る口（実際にファイルに触る。ここに書いた操作・場所・ファイルにしか触りません）", "", "| 外部作用 | ◆操作 | ◆場所 | ◆結果 → 結果の種類 |", "|---|---|---|---|"]
        out += [f"| {a} | {b} | {c} | {d} |" for a, b, c, d in spec["ファイル"]]
    if spec["常駐"]:
        out += ["", "## 常駐（窓を閉じても終了せず、タスクトレイ・メニューバーのアイコンに居続ける。左クリックの動きと右クリックのメニューを宣言できる）", "", "| 項目 | ◆内容 |", "|---|---|"]
        out += [f"| {a} | {b} |" for a, b in spec["常駐"]]
    if spec["通信"]:
        out += ["", "## 外へ出る通信（実通信。ここに書いた先にしか出ません。リダイレクトにも従いません）", "", "| 外部作用 | ◆通信先 | ◆認証（シークレット） | ◆本文の項目 | ◆返事の状態 → 結果の種類 |", "|---|---|---|---|---|"]
        out += [f"| {a} | {b} | {c} | {d} | {e} |" for a, b, c, d, e in spec["通信"]]
    if spec["外部作用"]:
        out += ["", "## 外部作用（外へ出す内容。入出力型）", "", "| 外部作用 | ◆出す内容 |", "|---|---|"]
        out += [f"| {a} | {b} |" for a, b in spec["外部作用"]]
    if spec["出来事"]:
        out += ["", "## 出来事（外の出来事をきっかけに起こす処理）", "", "| 出来事 | ◆起こし方 | ◆処理 |", "|---|---|---|"]
        out += [f"| {a} | {b} | {c} |" for a, b, c in spec["出来事"]]
    if spec["画面"]:
        out += ["", "## 画面（入口は最初の画面と entry の画面。入力欄の「←」は選ぶ元の問い合わせ）", "", "| 画面 | ◆見せる | ◆入力欄 | ◆受け取る値 | ◆入口 | ◆選ばせる slot | ◆URL |", "|---|---|---|---|---|---|---|"]
        out += [f"| {a} | {b} | {c} | {d} | {e} | {f} | {g} |" for a, b, c, d, e, f, g in spec["画面"]]
        out += ["", "## 画面遷移", "", "| 画面 | ◆操作 | ◆結果 | ◆行き先 |", "|---|---|---|---|"]
        out += [f"| {a} | {b} | {c} | {d} |" for a, b, c, d in spec["画面遷移"]]
        if spec["拡張"]:
            out += ["", "## ブラウザ拡張（ページに出す画面。ページからの読み方も宣言から生成）", "", "| 画面 | ページ | 見せる問い合わせ | ページから読む値 | 読み方 |", "|---|---|---|---|---|"]
            out += [f"| {a} | {b} | {c} | {d} | {e} |" for a, b, c, d, e in spec["拡張"]]
        out += ["", "## 画面デザイン（突合しない。配置と色）", "", "| 画面 | 指定 | 値 |", "|---|---|---|"]
        out += [f"| {a} | {b} | {c} |" for a, b, c in spec["画面デザイン"]]
    if spec["人の保証"]:
        out += ["", "## 人の保証（範囲の確認を、人が危険を承知で省いたもの。検査器は宣言した範囲を事実として使う）", "", "| 処理 | 入力 | 型・範囲 | 実行時の確認 | 理由 |", "|---|---|---|---|---|"]
        out += [f"| {a} | {b} | {c} | {d} | {e} |" for a, b, c, d, e in spec["人の保証"]]
    if spec["保存の形"]:
        out += ["", f"## 保存の形の変更（今は版 {ck.schema_version}。前の版の DB は、起動時に 1 回でまとめて今の形にする）", "", "| 版 | すること | 項目 | 内容 |", "|---|---|---|---|"]
        out += [f"| {a} | {b} | {c} | {d} |" for a, b, c, d in spec["保存の形"]]
    if spec["保存先"]:
        out += ["", "## 保存先（entity を置く場所と、リクエストの直列を誰が保証するか）", "", "| データ | 保存先 | 直列の担い手 |", "|---|---|---|"]
        out += [f"| {a} | {b} | {c} |" for a, b, c in spec["保存先"]]
    if spec["動かす前提"]:
        out += ["", "## 動かす前提（アプリが実行環境に要求すること。環境の実際は書かず、plan や実環境から読んで照合する。照合は geas env-verify（plan の JSON から。読めなかった事実は「確かめられなかった」で、合格にはしない））", "",
                "| 番号 | 観点 | 要求 | 由来 | 確かめ方 |", "|---|---|---|---|---|"]
        out += [f"| {a} | {b} | {c} | {d} | {e} |" for a, b, c, d, e in spec["動かす前提"]]
    if spec["削除"]:
        out += ["", "## 削除（消してよい状態と、消したときの参照元の扱い）", "", "| データ | 消してよい状態 | 参照元の扱い |", "|---|---|---|"]
        out += [f"| {a} | {b} | {c} |" for a, b, c in spec["削除"]]
    if spec["ルート"]:
        out += ["", "## ルート（URL。結果の種類ごとの HTTP の状態つき。「内部」は流れの途中の処理で、サーバーは外に出さない）", "",
                "| メソッド | 経路 | 動作 | 呼ぶもの | 結果→状態 | 外から |" + (" 呼べる人 |" if spec.get("役割") else ""), "|---|---|---|---|---|---|" + ("---|" if spec.get("役割") else "")]
        out += [f"| {a} | {b} | {c} | {d} | {e} | {f} |" + (f" {g} |" if spec.get("役割") else "") for a, b, c, d, e, f, g in spec["ルート"]]
    if spec.get("役割"):
        out += ["", f"## 役割（呼べる人の区別。{ACTOR_NOTE}）", "", "| 役割 | ヘッダに載せる名前 | 呼べる URL |", "|---|---|---|"]
        out += [f"| {a} | {b} | {c} |" for a, b, c in spec["役割"]]
    out += ["", "## 不変条件", "", "| ◆内容 | 検査の結果 |", "|---|---|"]
    for text, rows in ck.invariant_status:
        out.append(f"| {text} | " + "／".join(f"{p}: {s}" for p, s in rows) + " |")
    if spec["保証の強さ"]:
        out += ["", "## 保証の強さ（約束ごとに、何が守っているか。上ほど強く、下の行ほど人が重点的に読む）", "",
                "件数：" + "、".join(f"{lv} {n}" for lv, n in level_counts(spec["保証の強さ"])), "",
                "| 強さ | 対象 | 内容 |", "|---|---|---|"]
        out += [f"| {a} | {b} | {c} |" for a, b, c in spec["保証の強さ"]]
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
    # 表の1行は1行に収める（理由などの文字列に \n で書いた改行は、表の中の改行 <br> にする）
    return "\n".join(x.replace("\n", "<br>") if x.startswith("|") else x for x in out) + "\n"
