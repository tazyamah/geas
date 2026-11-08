# SPDX-License-Identifier: GPL-3.0-or-later
"""動かす前提の照合：検査項目の一覧（geas/envreq.py）と、環境の事実を突き合わせる。

環境の事実は、クラウドの種類に依存しない形（下の FACTS）で受け取る。plan や実環境から事実を作る部分は、プラットフォームごとの
アダプタ（geas/adapters/）が担い、ここは事実の JSON だけを読む。事実が無い（null）項目は「確かめられなかった」で、合格にはしない。

判定は 4 つ：合格・不合格・確かめられなかった・人の保証（外から確かめられない項目）。
"""
PASS, FAIL, UNKNOWN, TRUST = "合格", "不合格", "確かめられなかった", "人の保証"

# 事実の名前と意味。値が null なら、読めなかった（アダプタが言い切れなかった）こと
FACTS = {
    "instances_max": "同時に動くサーバーの台数の上限（自動拡張の最大を含む）。整数",
    "rollout_max": "入れ替えのときに、新旧が重なって同時に動く最大数。整数",
    "shared_host": "すべてのサーバーが同じ機械の上で動くことを、プラットフォームが保証するか。true/false",
    "storage_persistent": "データの領域が、再起動や入れ替えで消えない種類か。true/false",
    "internet_ingress_ports": "インターネット（0.0.0.0/0）から届く口。ポート番号の文字列の列（範囲は \"8000-8100\"、全部は \"all\"）",
    "egress_https": "外向きの 443 が許されているか。true/false（宛先ごとの到達は表せない）",
}

# 公開してよい口（public と書いたとき、これ以外の口が開いていたら不合格）
WEB_PORTS = {"80", "443"}


def result(item, status, detail):
    return {"id": item["id"], "観点": item["観点"], "要求": item["要求"], "判定": status, "詳細": detail}


def _int(x):
    return x if isinstance(x, int) and not isinstance(x, bool) else None


def check_instances(item, f):
    c = item["条件"]
    imax, roll = _int(f.get("instances_max")), _int(f.get("rollout_max"))
    bad, unknown = [], []
    if "at_most" in c:
        n = c["at_most"]
        for label, v in (("台数の上限", imax), ("入れ替えのときの同時最大数", roll)):
            if v is None:
                unknown.append(label)
            elif v > n:
                bad.append(f"{label}が {v}（{n} 以下のはず）")
    if c.get("same_host"):
        shared = f.get("shared_host")
        if imax is not None and imax <= 1:
            pass  # 1 台なら、同じ機械
        elif shared is True:
            pass
        elif shared is False and imax is not None:
            bad.append(f"台数の上限が {imax} で、同じ機械の上で動くことが保証されていない")
        else:
            unknown.append("台数の上限" if imax is None else "同じ機械の上で動くか")
    if bad:
        return result(item, FAIL, "。".join(bad))
    if unknown:
        return result(item, UNKNOWN, "読めなかった事実：" + "・".join(dict.fromkeys(unknown)))
    return result(item, PASS, f"台数の上限 {imax}、入れ替えのときの同時最大数 {roll}")


def check_durable(item, f):
    v = f.get("storage_persistent")
    if v is None:
        return result(item, UNKNOWN, "読めなかった事実：データの領域の種類")
    if v:
        return result(item, PASS, "消えない種類の領域が付いている（保存先のパスが、その領域の上にあるかまでは見ていない）")
    return result(item, FAIL, "データの領域が、再起動や入れ替えで消える種類")


def check_exposure(item, f):
    ports = f.get("internet_ingress_ports")
    if ports is None:
        return result(item, UNKNOWN, "読めなかった事実：インターネットから届く口")
    ports = [str(x) for x in ports]
    if item["条件"].get("internet") == "internal":
        return result(item, PASS, "インターネットから届く口は無い") if not ports else result(item, FAIL, f"インターネットから届く口がある：{'・'.join(ports)}")
    extra = [x for x in ports if x not in WEB_PORTS]
    if extra:
        return result(item, FAIL, f"公開してよい口（{'・'.join(sorted(WEB_PORTS))}）以外が開いている：{'・'.join(extra)}")
    return result(item, PASS, f"インターネットから届く口：{'・'.join(ports) or 'なし'}")


def check_reach(item, f):
    v = f.get("egress_https")
    if v is False:
        return result(item, FAIL, "外向きの 443 が許されていない")
    if v is None:
        return result(item, UNKNOWN, "読めなかった事実：外向きの通信の許可")
    return result(item, UNKNOWN, "外向きの 443 は許されている。宛先ごとに届くかは、plan からは言えない（実環境で確かめる）")


def check_secret(item, f):
    return result(item, UNKNOWN, "シークレットの名前を環境の変数名に結ぶ取り決めがまだ無く、plan からは確かめられない")


def verify(items, facts: dict) -> list[dict]:
    out = []
    for it in items:
        key = it["id"].split(".")[0]
        if it["人の保証"]:
            out.append(result(it, TRUST, it["理由"] or "外から確かめられない"))
        else:
            fn = {"instances": check_instances, "durable": check_durable, "exposure": check_exposure, "reach": check_reach, "secret": check_secret}.get(key)
            out.append(fn(it, facts) if fn else result(it, UNKNOWN, "この項目の照合は、まだ実装されていない"))
    return out


def counts(results) -> dict:
    out = {PASS: 0, FAIL: 0, UNKNOWN: 0, TRUST: 0}
    for r in results:
        out[r["判定"]] += 1
    return out


def to_text(results) -> str:
    lines = []
    for r in results:
        lines.append(f"[{r['判定']}] {r['id']}（{r['観点']}）{r['要求']}")
        lines.append(f"    {r['詳細']}")
    c = counts(results)
    lines.append("結果：" + "、".join(f"{k} {v}" for k, v in c.items()))
    return "\n".join(lines) + "\n"
