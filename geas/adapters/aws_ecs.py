# SPDX-License-Identifier: GPL-3.0-or-later
"""AWS の ECS（terraform plan の JSON）から、環境の事実を作るアダプタ。`terraform show -json PLAN` の出力を読む。

読むもの（root module の resource_changes と configuration）：aws_ecs_service、aws_appautoscaling_target、aws_ecs_task_definition、
aws_security_group（中に書いた ingress／egress）と、別に書いた規則（aws_security_group_rule、aws_vpc_security_group_ingress_rule／
egress_rule）、aws_lb／aws_alb とリスナー。関連づけは、値ではなく configuration の参照（references）からたどる（作成時は ID が未確定のため）。
ECS のサービスが複数あって、どれがアプリか決められないときは、台数に関する事実を null にする（`service` で指名できる）。
口に関わる値（規則の CIDR・ポート、LB の internal、リスナーのポート）が plan の時点で確定していなければ、公開の口は null にする
（見えない口を「無い」と言わない。照合では「確かめられなかった」になる）。子モジュールの中の参照は、まだたどらない。
"""
import math

NAME = "aws-ecs"
DEFAULT_MAX_PERCENT = 200  # aws_ecs_service の deployment_maximum_percent の既定


def _live(plan):
    """削除されるものを除いた resource_changes → {アドレス: (種類, after, after_unknown)}"""
    out = {}
    for rc in plan.get("resource_changes", []) or []:
        ch = rc.get("change", {}) or {}
        if ch.get("actions") == ["delete"] or ch.get("after") is None:
            continue
        out[rc["address"]] = (rc["type"], ch.get("after") or {}, ch.get("after_unknown") or {})
    return out


def _refs(node):
    """configuration の式の中の参照（aws_xxx.name の形）をすべて集める"""
    found = set()
    if isinstance(node, dict):
        for k, v in node.items():
            if k == "references" and isinstance(v, list):
                found.update(x for x in v if isinstance(x, str))
            else:
                found |= _refs(v)
    elif isinstance(node, list):
        for v in node:
            found |= _refs(v)
    return found


def _config_refs(plan):
    out = {}
    for r in (plan.get("configuration", {}).get("root_module", {}).get("resources", []) or []):
        out[r["address"]] = _refs(r.get("expressions", {}))
    return out


def _config_keys(plan):
    """configuration に書いてある属性の名前（root module のリソースごと）。書いていない属性は、プロバイダの既定になる"""
    return {r["address"]: set((r.get("expressions") or {}).keys())
            for r in (plan.get("configuration", {}).get("root_module", {}).get("resources", []) or [])}


def _known(after, unknown, key):
    """値が確定していて、null でもないとき、その値。それ以外は None"""
    if unknown.get(key) is True:
        return None
    return after.get(key)


def _ports(rule):
    proto, lo, hi = rule.get("protocol"), rule.get("from_port"), rule.get("to_port")
    if proto in ("-1", -1) or lo is None or hi is None:
        return "all"
    return str(lo) if lo == hi else f"{lo}-{hi}"


def _covers(rule, port):
    if rule.get("protocol") in ("-1", -1):
        return True
    lo, hi = rule.get("from_port"), rule.get("to_port")
    return isinstance(lo, int) and isinstance(hi, int) and lo <= port <= hi


def _open_cidr(rule):
    return "0.0.0.0/0" in (rule.get("cidr_blocks") or []) or "::/0" in (rule.get("ipv6_cidr_blocks") or [])


def _has_unknown(au):
    """after_unknown の値の中に、確定していない（true）ものがあるか"""
    if au is True:
        return True
    if isinstance(au, dict):
        return any(_has_unknown(v) for v in au.values())
    if isinstance(au, list):
        return any(_has_unknown(v) for v in au)
    return False


RULE_KEYS = ("from_port", "to_port", "protocol", "ip_protocol", "cidr_blocks", "ipv6_cidr_blocks", "cidr_ipv4", "cidr_ipv6")


def _sg_rules(live):
    """セキュリティグループの規則を、(向き, 規則, 確定していないか, どこの規則か) の列にする。
    中に書いた ingress／egress と、別のリソースに書いた規則の両方。別の書き方の規則は、中に書く形（cidr_blocks など）にそろえる"""
    out = []
    for a, (t, aft, au) in live.items():
        if t == "aws_security_group":
            for d in ("ingress", "egress"):
                unknown = _has_unknown(au.get(d))
                for i, r in enumerate(aft.get(d) or []):
                    out.append((d, r, unknown, f"{a} の {d}[{i}]"))
                if unknown and not aft.get(d):
                    out.append((d, {}, True, f"{a} の {d}"))
        elif t == "aws_security_group_rule":
            d = "ingress" if _has_unknown(au.get("type")) else aft.get("type")  # 向きが確定していなければ、入る向きとみなす（見落とさない側）
            out.append((d, aft, any(_has_unknown(au.get(k)) for k in RULE_KEYS + ("type",)), a))
        elif t in ("aws_vpc_security_group_ingress_rule", "aws_vpc_security_group_egress_rule"):
            r = {"from_port": aft.get("from_port"), "to_port": aft.get("to_port"), "protocol": aft.get("ip_protocol"),
                 "cidr_blocks": [aft["cidr_ipv4"]] if aft.get("cidr_ipv4") else [], "ipv6_cidr_blocks": [aft["cidr_ipv6"]] if aft.get("cidr_ipv6") else []}
            out.append(("ingress" if t.endswith("ingress_rule") else "egress", r, any(_has_unknown(au.get(k)) for k in RULE_KEYS), a))
    return out


def facts_from_plan(plan: dict, service: str = "") -> tuple[dict, list[str]]:
    """(事実, 読めなかった理由などのメモ)。service は、アプリの aws_ecs_service のアドレス（1 つだけなら省ける）"""
    live, cfg, written = _live(plan), _config_refs(plan), _config_keys(plan)
    notes: list[str] = []
    facts = {k: None for k in ("instances_max", "rollout_max", "shared_host", "storage_persistent", "internet_ingress_ports", "egress_https")}
    services = [a for a, (t, _, _) in live.items() if t == "aws_ecs_service"]
    if service:
        services = [a for a in services if a == service]
        if not services:
            notes.append(f"指名された {service} が plan に無い")
    app = services[0] if len(services) == 1 else None
    if len(services) > 1:
        notes.append("aws_ecs_service が複数あり、アプリがどれか決められない（--service で指名する）")
    if not services and not service:
        notes.append("aws_ecs_service が plan に無い")

    if app:
        _, after, unk = live[app]
        facts["shared_host"] = False  # ECS のタスクは、別々の機械（Fargate は 1 タスク 1 機械）
        desired = _known(after, unk, "desired_count")
        cap = desired if isinstance(desired, int) else None
        if cap is None:
            notes.append(f"{app} の desired_count が確定していない")
        for a, (t, aft, au) in live.items():  # 自動拡張の最大が、台数の上限
            if t == "aws_appautoscaling_target" and app in cfg.get(a, set()):
                mx = _known(aft, au, "max_capacity")
                if isinstance(mx, int):
                    cap = mx if cap is None else max(cap, mx)
                else:
                    cap = None
                    notes.append(f"{a} の max_capacity が確定していない")
                    break
        facts["instances_max"] = cap
        if cap is not None:
            mp = _known(after, unk, "deployment_maximum_percent")
            mp = mp if isinstance(mp, int) else DEFAULT_MAX_PERCENT
            facts["rollout_max"] = math.ceil(cap * mp / 100)
        # タスク定義の領域
        tds = [a for a in cfg.get(app, set()) if a in live and live[a][0] == "aws_ecs_task_definition"]
        if len(tds) == 1:
            vols = live[tds[0]][1].get("volume")
            if isinstance(vols, list):
                facts["storage_persistent"] = any(
                    v.get("efs_volume_configuration") or v.get("configure_at_launch") or v.get("docker_volume_configuration") for v in vols)
            else:
                notes.append(f"{tds[0]} の volume が確定していない")
        else:
            notes.append("サービスから、タスク定義を 1 つにたどれなかった")

    # セキュリティグループ：インターネットから届く口と、外向きの 443（中に書いた規則と、別のリソースに書いた規則の両方）
    # セキュリティグループそのものが plan に無いと、plan の外の規則を見落としうるので、規則だけあっても言い切らない
    sgs = [a for a, (t, _, _) in live.items() if t == "aws_security_group"]
    rules = _sg_rules(live)
    if sgs:
        ports = []
        for d, r, unknown, where in rules:
            if d == "ingress" and unknown:  # CIDR やポートが確定していない規則は、インターネットに開いているかもしれない
                ports = None
                notes.append(f"{where} の CIDR かポートが確定していない（公開の口は言い切らない）")
                break
            if d == "ingress" and _open_cidr(r) and _ports(r) not in ports:
                ports.append(_ports(r))
        facts["internet_ingress_ports"] = ports
        out_rules = [r for d, r, unknown, _ in rules if d == "egress" and not unknown]
        facts["egress_https"] = any(_open_cidr(r) and _covers(r, 443) for r in out_rules) or (None if any(d == "egress" and u for d, _, u, _ in rules) else False)
    else:
        notes.append("aws_security_group が plan に無い")
    # 公開の LB（internal でない）は、リスナーのポートがインターネットに向く。aws_alb は aws_lb の別名
    listeners = [(a, aft, au) for a, (t, aft, au) in live.items() if t in ("aws_lb_listener", "aws_alb_listener")]
    for a, (t, aft, au) in live.items():
        if t not in ("aws_lb", "aws_alb"):
            continue
        internal = aft.get("internal")
        if internal is True:
            continue
        # internal は書かなければ false（インターネット向き）で、plan では「確定していない」と出ることがある。
        # 書いていないなら公開として数え、書いてあって確定していないなら、言い切らない
        if internal is not False and (a not in written or "internal" in written[a]):
            facts["internet_ingress_ports"] = None
            notes.append(f"{a} の internal が plan で確定していない。公開の口は言い切らない")
            break
        mine = [x for x in listeners if a in cfg.get(x[0], set())] or listeners  # 参照でたどれなければ、すべてのリスナー
        if not mine:
            facts["internet_ingress_ports"] = None
            notes.append(f"{a} は公開の LB だが、リスナーが plan に無く、公開のポートが分からない")
            break
        if facts["internet_ingress_ports"] is None:  # セキュリティグループが plan に無いと、ほかの口を見落としうるので、言い切らない
            break
        for la, aft_l, au_l in mine:
            port = aft_l.get("port")
            if not isinstance(port, int) or _has_unknown(au_l.get("port")):
                facts["internet_ingress_ports"] = None
                notes.append(f"{la} のポートが確定していない（公開の口は言い切らない）")
                break
            if str(port) not in facts["internet_ingress_ports"]:
                facts["internet_ingress_ports"].append(str(port))
        if facts["internet_ingress_ports"] is None:
            break
    return facts, notes
