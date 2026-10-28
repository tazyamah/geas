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


def base_path(p, res, seen=()) -> str:
    """リソースの URL の土台（動作ごとの末尾を付ける前）。親があれば、親の URL に /:親_id を足す"""
    if res.under and res.under not in seen:
        parent = next((r for r in p.resources if r.name == res.under), None)
        if parent is not None:
            ps = singular(snake(parent.name))
            own = snake(res.name)
            if own.startswith(ps + "_"):  # CustomerOrders under Customers → /customers/:customer_id/orders
                own = own[len(ps) + 1:]
            return f"{base_path(p, parent, (*seen, res.name))}/:{ps}_id/{own}"
    return "/" + snake(res.name)


def routes(p) -> list[tuple]:
    """(メソッド, 経路, 動作, 対象, リソース名, 結果→状態) の一覧。宣言の順"""
    out = []
    for res in p.resources:
        base = base_path(p, res)
        for a in res.actions:
            path = base
            if a.kind in ("show", "update", "destroy"):
                path += "/:" + (res.key or "id")
            out.append((METHOD[a.kind], path, a.kind, a.target, res.name, list(a.statuses)))
    return out


def screen_url(p, sc) -> str:
    """画面の URL（html の形式）。操作のある画面は、その処理を create するリソースの /new（入力欄を教える画面）、見せるだけの画面は、見せる問い合わせの URL。
    画面に渡す値（受け取る値）は、URL のフラグメント（#名前=値）に載せる。クエリ文字列は使わない。リソースが無ければ空"""
    by = {tgt: (path, kind) for _, path, kind, tgt, _, _ in routes(p)}
    for dn, _ in sc.actions:
        if dn in by and by[dn][1] == "create":
            return by[dn][0] + "/new"
        if dn in by and by[dn][1] == "update":
            return by[dn][0] + "/edit"
    for qn in sc.shows:
        if qn in by:
            return by[qn][0]
    return ""
