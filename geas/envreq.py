# SPDX-License-Identifier: GPL-3.0-or-later
"""動かす前提：アプリが実行環境に要求すること。

環境の実際（いま何台か、どのポートが開いているか）は geas に書かない。geas に出るのは、アプリが成り立つ条件だけで、
検査器が宣言から導き出すものと、人が `needs` で書いたもの（理由つき）に分かれる。出力は、ツールに依存しない
「検査項目の一覧」で、`geas env-checks`（--json）と仕様書の「動かす前提」の節が同じものを使う。
照合は geas/envcheck.py（一覧の「条件」と、環境の事実を突き合わせる）。事実を plan から作る部分は geas/adapters/。
"""
import re

# 番号は、導出元から決まる名前（項目が増減しても、ほかの項目の番号はずれない）
ASPECTS = ("安全性", "到達性", "機密", "資源")
NAME_RE = re.compile(r"^[a-z][a-z0-9_]*$")
NS_RE = re.compile(r"^[A-Z][A-Za-z0-9]*$")

# 生成サーバーが持つ上限（docs/server.md）。前段の設定がこれより緩くてよいかは、環境の側の約束になる
BODY_LIMIT = "16 MiB"
JSON_DEPTH = 64


def effect_namespaces(p) -> list[str]:
    seen: list[str] = []
    for e in p.effects:
        ns = e.name.split(".")[0]
        if ns not in seen:
            seen.append(ns)
    return seen


def derived_instance_limit(p):
    """保存先の isolation から導く、同時に動けるサーバーの上限。process なら 1、そうでなければ導出できない（None）"""
    return 1 if any(who == "process" for who, _, _, _ in p.isolations.values()) else None


def stores(p) -> dict[str, list[str]]:
    """保存先の名前 → それを使う entity。`store` の行がない entity は標準の保存先（sqlite）で、名前は default"""
    out: dict[str, list[str]] = {}
    for d in p.data:
        out.setdefault(d.store or "default", []).append(d.name)
    return out


def store_kind(p, where: str) -> str:
    """保存先の種類。標準の保存先と、isolation process の保存先は、サーバーが持つ sqlite"""
    if where == "default" or where not in p.isolations:
        return "sqlite"
    who, kind, _, _ = p.isolations[where]
    return kind if who == "store" else "sqlite"


def _item(id_, aspect, text, origin, how, facts, trust=False, why="", cond=None):
    """cond は、照合器が読む条件（中立な事実の名前で書く。geas/envcheck.py）。要求の文と同じ内容を、機械が読める形にしたもの"""
    return {"id": id_, "観点": aspect, "要求": text, "由来": origin, "確かめ方": how, "環境の事実": facts, "人の保証": trust, "理由": why, "条件": cond or {}}


def items(p) -> list[dict]:
    out: list[dict] = []
    needs = p.needs
    # ---- 同時に動くサーバー ----
    parts, origins = [], []
    procs = [w for w, (who, _, _, _) in p.isolations.items() if who == "process"]
    if procs:
        parts.append("同時に動くサーバーは 1 台（入れ替えのときも、新旧が重ならない）")
        origins.append(f"導出（保存先 {'・'.join(procs)} が isolation process）")
    if any(store_kind(p, w) == "sqlite" and w not in procs for w in stores(p)):
        parts.append("同じ DB ファイルを使うサーバーは、すべて同じ機械の上で動く")
        origins.append("導出（保存先 sqlite は、同じ機械のファイルの書き込みロックで直列にする）")
    mine = [n for n in needs if n[0] == "instances"]
    for _, _, n, why, _ in mine:
        parts.append(f"同時に動くサーバーは {n} 台以下")
        origins.append(f"人が書いた（{why}）")
    if parts:
        limits = ([1] if procs else []) + [n[2] for n in mine]
        cond = {}
        if limits:
            cond["at_most"] = min(limits)  # 台数の上限と、更新のときの同時最大数が、どちらもこの数以下
        if any(store_kind(p, w) == "sqlite" and w not in procs for w in stores(p)):
            cond["same_host"] = True  # 台数が 2 以上なら、すべて同じ機械の上
        out.append(_item("instances", "安全性", "。".join(parts), "／".join(origins), ["plan", "実環境"],
                         ["台数", "更新のときの同時最大数", "保存先の実体"], why="／".join(n[3] for n in mine), cond=cond))
    # ---- 保存先の永続 ----
    for where, ents in stores(p).items():
        kind = store_kind(p, where)
        what = "DB ファイル" if kind == "sqlite" else f"保存先 {kind} のデータ"
        out.append(_item(f"durable.{where}", "安全性", f"{what}が、再起動や入れ替えで消えない領域にある",
                         f"導出（{'・'.join(ents)} が保存先 {where}（{kind}）に入る）", ["plan", "実環境"], ["保存先の実体", "領域の種類"],
                         cond={"persistent": True}))
    # ---- 外へ届くこと ----
    for ns in effect_namespaces(p):
        ops = [e.name for e in p.effects if e.name.split(".")[0] == ns]
        out.append(_item(f"reach.{ns}", "到達性", f"外部 {ns} の宛先に、サーバーから届く（宛先は環境から読む。書いていない宛先には出ない前提）",
                         f"導出（外部作用 {'・'.join(ops)}）", ["実環境"], ["通信の許可", "宛先の設定"], cond={"egress_https": True}))
    # ---- 秘密 ----
    for kind, target, _, why, _ in needs:
        if kind == "secret":
            out.append(_item(f"secret.{target}", "機密", f"シークレット {target} が環境にあり、plan に平文の値が書かれていない",
                             f"人が書いた（{why}）", ["plan", "実環境"], ["シークレットの有無", "plan の値"], why=why, cond={"secret": target}))
    # ---- 公開 ----
    for kind, _, value, why, _ in needs:
        if kind == "exposure":
            n = len(p.resources)
            text = (f"インターネットに公開する口は、リソース {n} 件の経路を出すものだけ" if value == "public" else "インターネットから届く口が無い")
            out.append(_item("exposure", "到達性", text, f"人が書いた（{why}）", ["plan", "実環境"], ["公開ポート", "公開の許可"], why=why, cond={"internet": value}))
    # ---- 前段の約束（外から確かめられない） ----
    if p.resources:
        out.append(_item("limits", "資源", "前段（LB・プロキシ）が、遅い読み込みを切る時間切れを持つ",
                         f"導出（生成サーバーは本文が {BODY_LIMIT} を超えると 413 で断り、JSON の入れ子は {JSON_DEPTH} 段まで。"
                         "読み込みの時間切れだけは部品に口がなく、前段に任せる。前段の本文上限は、低くても無くても、サーバーが守るので要求しない）",
                         ["人の保証"], [], trust=True, why="外から確かめられない"))
    return out


def rows(p) -> list[tuple]:
    """仕様書の表の行：(番号, 観点, 要求, 由来, 確かめ方)"""
    return [(x["id"], x["観点"], x["要求"], x["由来"], "・".join(x["確かめ方"])) for x in items(p)]


def to_json(p) -> str:
    import json
    return json.dumps({"items": items(p)}, ensure_ascii=False, indent=2) + "\n"


def to_text(p) -> str:
    lines = []
    for x in items(p):
        lines.append(f"{x['id']}（{x['観点']}）{x['要求']}")
        lines.append(f"    由来：{x['由来']}　確かめ方：{'・'.join(x['確かめ方'])}")
    return "\n".join(lines) + ("\n" if lines else "")
