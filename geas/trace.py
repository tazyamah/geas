# SPDX-License-Identifier: GPL-3.0-or-later
"""運用のログから、仕様書へ戻る。

生成したサーバーは、REST の経路ごとに 1 行の JSON ログ（`"log": "geas.route"`）を標準エラーに出す。
`geas trace` は、そのログをまとめ、仕様書のルート表の行（呼ぶ処理と結果の種類）に結び付ける。
計画書を渡すと、その処理を使うユースケースと画面も出す。
ログには、本文と入力の値は載らない（経路の型、処理の名前、結果の種類、HTTP の状態、かかった時間だけ）。
"""
import json
from collections import defaultdict

from .rest import routes


def read_log(text: str) -> list[dict]:
    """ログの行から、geas のルートのログだけを取り出す（他の行・壊れた行は読み飛ばす）"""
    out = []
    for line in text.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            d = json.loads(line)
        except ValueError:
            continue
        if d.get("log") == "geas.route":
            out.append(d)
    return out


def trace(ck, entries: list[dict], plan_rows: list[dict] | None = None) -> dict:
    """ログを (経路, 処理, 結果) ごとにまとめ、仕様書の行に結ぶ。戻り値は {"rows": [...], "unknown": [...]}"""
    show = ck.p.show_names
    spec = {(m, path): (kind, tgt, dict(st)) for m, path, kind, tgt, _, st in routes(ck.p)}
    by_op: dict[str, list[dict]] = defaultdict(list)
    for r in plan_rows or []:
        by_op[r.get("操作", "").strip()].append(r)
    groups: dict[tuple, list[dict]] = defaultdict(list)
    for e in entries:
        groups[(e.get("method", ""), e.get("route", ""), e.get("target", ""), e.get("result") or "")].append(e)
    rows, unknown = [], []
    for (m, route, target, result), es in sorted(groups.items()):
        ms = sorted(int(e.get("ms", 0)) for e in es)
        statuses = sorted({int(e.get("status", 0)) for e in es})
        row = {"method": m, "route": route, "target": show(target), "result": show(result), "count": len(es),
               "status": statuses, "p50_ms": ms[len(ms) // 2], "max_ms": ms[-1]}
        hit = spec.get((m, route))
        if hit is None or hit[1] != target:
            unknown.append(row | {"why": "仕様書のルート表にない経路です"})
            continue
        want = hit[2].get(result) if result else None
        if result and want is None:
            unknown.append(row | {"why": "仕様書にない結果の種類です"})
            continue
        if result and want is not None and statuses != [want]:
            unknown.append(row | {"why": f"HTTP の状態が仕様書（{want}）と違います"})
            continue
        steps = by_op.get(show(target), [])
        row["usecases"] = sorted({s["ユースケース"] for s in steps if not result or not s.get("結果") or s["結果"].strip() == show(result)})
        row["screens"] = sorted({s["画面"] for s in steps})
        rows.append(row)
    return {"rows": rows, "unknown": unknown}


def to_markdown(t: dict, name: str) -> str:
    out = [f"# ログと仕様書の対応（{name}）", ""]
    rows, unknown = t["rows"], t["unknown"]
    out += [f"ログ {sum(r['count'] for r in rows + unknown)} 件。仕様書に結べたもの {sum(r['count'] for r in rows)} 件、結べなかったもの {sum(r['count'] for r in unknown)} 件。", ""]
    if unknown:
        out += ["## 仕様書に結べなかったもの（最初に見てください）", "", "コードが仕様書と違う動きをした記録か、古い版のログです。", "",
                "| 動作 | 経路 | 処理 | 結果 | 状態 | 件数 | 理由 |", "|---|---|---|---|---|---|---|"]
        out += [f"| {r['method']} | {r['route']} | {r['target']} | {r['result'] or '（なし）'} | {'/'.join(map(str, r['status']))} | {r['count']} | {r['why']} |" for r in unknown] + [""]
    if rows:
        out += ["## 仕様書に結べたもの", "", "| 動作 | 経路 | 処理 | 結果 | 状態 | 件数 | 中央値 ms | 最大 ms | ユースケース | 画面 |", "|---|---|---|---|---|---|---|---|---|---|"]
        out += [f"| {r['method']} | {r['route']} | {r['target']} | {r['result'] or '（なし）'} | {'/'.join(map(str, r['status']))} | {r['count']} | {r['p50_ms']} | {r['max_ms']} | "
                f"{'、'.join(r['usecases'])} | {'、'.join(r['screens'])} |" for r in rows]
    return "\n".join(out) + "\n"
