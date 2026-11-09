# SPDX-License-Identifier: GPL-3.0-or-later
"""リソース（resource）の URL とルーティング。名前から URL を決める（Rails の resources に合わせる）。
動作と URL： index＝GET /複数形、show＝GET /複数形/:キー、create＝POST /複数形、update＝PUT /複数形/:id、destroy＝DELETE /複数形/:id。
入れ子は /親/:親_id/子。クエリ文字列は使わない（条件はパスか、検索リソースの本文で渡す）。"""
import re

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
                    path += "/:" + (res.key or "id")
                out.append((METHOD[a.kind], path, a.kind, a.target, res.name, tuple(a.statuses)))
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
