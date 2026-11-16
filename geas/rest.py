# SPDX-License-Identifier: GPL-3.0-or-later
"""リソース（resource）の URL とルーティング。名前から URL を決める（Rails の resources に合わせる）。
動作と URL： index＝GET /複数形、show＝GET /複数形/:キー、create＝POST /複数形、update＝PUT /複数形/:id、destroy＝DELETE /複数形/:id。
入れ子は /親/:親_id/子。パスは「どのリソースか」を決め、絞り込みの条件はクエリ文字列の q[名前]=値 で渡す（ほかの形のクエリ文字列は受けない）。"""
import re

from .ast import AFTER, ORDER, Type, order_choices, paged, uses_caller

METHOD = {"index": "GET", "show": "GET", "create": "POST", "update": "PUT", "destroy": "DELETE"}


def snake(name: str) -> str:
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


def singular(word: str) -> str:
    if word.endswith("ies"):
        return word[:-3] + "y"
    return word[:-1] if word.endswith("s") else word


def first_by_name(p) -> dict:
    """名前 → リソース（同じ名前が 2 つあれば、先に書いたほう。重なりは検査器が止める）"""
    out: dict = {}
    for r in p.resources:
        out.setdefault(r.name, r)
    return out


def base_path(p, res, seen=(), by_name=None) -> str:
    """リソースの URL の土台（動作ごとの末尾を付ける前）。親があれば、親の URL に /:親_id を足す"""
    if res.under and res.under not in seen:
        parent = (by_name or first_by_name(p)).get(res.under)
        if parent is not None:
            ps = singular(snake(parent.name))
            own = snake(res.name)
            if own.startswith(ps + "_"):  # CustomerOrders under Customers → /customers/:customer_id/orders
                own = own[len(ps) + 1:]
            return f"{base_path(p, parent, (*seen, res.name), by_name)}/:{ps}_id/{own}"
    return "/" + snake(res.name)


# 仕様書の URL の表の「外から」の列：呼べる口と、流れの途中の処理の口（サーバーは --test のときだけ答える）
OPEN, INTERNAL = "呼べる", "内部（流れの途中）"


def internal_targets(p) -> set[str]:
    """流れの途中の処理：返事・期限（trigger の行き先）と、前の処理の結果（nexus の行き先。par の枝も）から進む処理。
    外から直接呼ぶと、決済の返事などの確かめを飛ばして先へ進められるので、サーバーは、この処理を呼ぶ URL を外に出さない"""
    out = {d for fl in p.flows for _, rs in fl.entries for _, d in rs if d != "reply" and not d.startswith("wait ")}
    return out | {d for t in p.triggers for _, _, d in t.causes}


ANYONE = "anyone"  # for anyone：誰でも呼べる口
ACTOR_ID_HEADER = "X-Geas-Actor-Id"  # 同じく、確かめた本人の ID を載せるヘッダ（範囲 scope の caller が比べる値）
ACTOR_HEADER = "X-Geas-Actor"  # 前に置くプロキシが、確かめた役割の名前を載せるヘッダ（複数はカンマ区切り）


def route_actors(p) -> dict:
    """呼ぶもの（処理・問い合わせ・出来事）→ 呼べる役割の名前の並び。誰でも呼べる口は ["anyone"]。for の無い動作は入らない"""
    return {a.target: list(a.actors) for res in p.resources for a in res.actions if a.actors is not None}


def actors_text(p, target, inner) -> str:
    """仕様書の URL の表の「呼べる人」の列。役割を宣言していないアプリでは空"""
    if not p.actors:
        return ""
    if target in inner:
        return "（外から呼べない）"
    who = route_actors(p).get(target)
    if who is None:
        return ""
    own = "（本人の分だけ）" if any(q.name == target and uses_caller(p, q) for q in p.queries) else ""
    return "誰でも" if who == [ANYONE] else "、".join(p.show_names(x) for x in who) + own


def path_inputs(p, res, action, q) -> list[str]:
    """問い合わせの動作（index・show）で、パスの「:名前」の部分が、左から順にどの入力になるか。
    親の ID の部分は、キーでない必須の入力に、宣言の順で当たる。show は、そのあとにキーが続く"""
    keys = list(res.keys) if action.kind == "show" else []
    parents = base_path(p, res).count("/:")
    rest = [n for n, t in q.inputs if not t.optional and n not in keys]
    have = {n for n, _ in q.inputs}  # 検査で止まる形（キーが入力に無い）でも、ある入力だけを返す
    return rest[:parents] + [k for k in keys if k in have]


def q_inputs(p, res, action, q) -> list[str]:
    """問い合わせの動作（index・show）で、クエリ文字列の q[名前]=値 で渡る入力。パスに載らない入力のすべて（宣言の順）"""
    on_path = path_inputs(p, res, action, q)
    return [n for n, _ in q.inputs if n not in on_path] + ([ORDER] if order_choices(q) else []) + ([AFTER] if paged(q) else [])


def q_types(q) -> dict:
    """q[名前] で渡る値の型（名前 → 型）。問い合わせの入力と、ページのある問い合わせが言語から受ける q[after]（続きの印。省ける）"""
    return {**dict(q.inputs), **({ORDER: Type("文字列", optional=True)} if order_choices(q) else {}), **({AFTER: Type("ID", optional=True)} if paged(q) else {})}


def shown_path(p, path, kind, target) -> str:
    """仕様書に出す URL。絞り込みの条件があれば、パスの後ろに ?q[名前]=…&q[名前]=… を付ける（省ける条件は、名前の後ろに ?）"""
    if kind not in ("index", "show"):
        return path
    q = next((x for x in p.queries if x.name == target), None)
    res = next((r for r in p.resources if any(a.target == target and a.kind == kind for a in r.actions)), None)
    if q is None or res is None:
        return path
    act = next(a for a in res.actions if a.target == target and a.kind == kind)
    opt = {n: t.optional for n, t in q_types(q).items()}
    names = q_inputs(p, res, act, q)
    if act.post:  # POST で受ける問い合わせ：条件は、本文の 1 つのオブジェクト q に入れる
        return path + "（本文 {\"q\": {" + ", ".join(f"{n}{'?' if opt[n] else ''}" for n in names) + "}}）"
    return path + ("?" + "&".join(f"q[{n}{'?' if opt[n] else ''}]=…" for n in names) if names else "")


# パスと q[名前] に載せられる入力の型。数（整数・金額・日時）は 10 進の数字で書き、サーバーが数に直す。ほかは文字のまま渡す
PATH_TEXT, PATH_INT = ("ID", "参照", "文字列", "名前"), ("整数", "金額", "日時")


def post_queries(p) -> dict:
    """事情があって POST で受ける問い合わせ：問い合わせの名前 → 理由（index 問い合わせ post "理由"）"""
    return {a.target: a.post for res in p.resources for a in res.actions if a.post}


def _table(p) -> tuple[list, dict]:
    """(ルートの一覧, 対象 → (経路, 動作) の表)。画面ごと・生成物ごとに何度も引くので、Program ごとに 1 回だけ作る
    （リソースの宣言は構文解析の後に変わらない。差し替えられたら作り直す）"""
    c = p.__dict__.get("_rest_table")  # (作ったときの resources, 件数, 一覧, 表)
    if not (c and c[0] is p.resources and c[1] == len(p.resources)):
        by_name = first_by_name(p)
        out = []
        for res in p.resources:
            base = base_path(p, res, by_name=by_name)
            for a in res.actions:
                path = base
                if a.kind in ("show", "update", "destroy"):
                    path += "/:" + (res.keys[0] if res.keys else "id")
                out.append(("POST" if a.post else METHOD[a.kind], path, a.kind, a.target, res.name, tuple(a.statuses)))
        c = (p.resources, len(p.resources), out, {tgt: (path, kind) for _, path, kind, tgt, _, _ in out})
        p.__dict__["_rest_table"] = c
    return c[2], c[3]


def routes(p) -> list[tuple]:
    """(メソッド, 経路, 動作, 対象, リソース名, 結果→状態) の一覧。宣言の順"""
    return [(m, path, kind, tgt, rn, list(st)) for m, path, kind, tgt, rn, st in _table(p)[0]]


def screen_url(p, sc) -> str:
    """画面の URL（html の形式）。操作のある画面は、その処理を create するリソースの /new（入力欄を教える画面）、見せるだけの画面は、見せる問い合わせの URL。
    画面に渡す値（受け取る値）は、URL のフラグメント（#名前=値）に載せる。クエリ文字列は使わない。リソースが無ければ空"""
    by = _table(p)[1]
    for dn, _ in sc.actions:
        if dn in by and by[dn][1] == "create":
            return by[dn][0] + "/new"
        if dn in by and by[dn][1] == "update":
            return by[dn][0] + "/edit"
    for qn in sc.shows:
        if qn in by:
            return by[qn][0]
    return ""
