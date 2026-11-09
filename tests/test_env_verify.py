"""動かす前提の照合：事実（プラットフォームに依存しない形）と、ECS（terraform plan の JSON）のアダプタ。
見本の plan（tests/fixtures/env/aws_ecs_plan_sample.json）は手書きで、実物の plan とは未照合。"""
import copy
import json
import subprocess
import sys
from pathlib import Path

from geas.adapters.aws_ecs import facts_from_plan
from geas.check import check
from geas.envcheck import FAIL, PASS, TRUST, UNKNOWN, counts, verify
from geas.envreq import items
from geas.parser import parse

ROOT = Path(__file__).parent.parent
SHOP = (ROOT / "tests" / "fixtures" / "shop" / "shop.geas").read_text()
DEMO = (ROOT / "tests" / "fixtures" / "runtime_demo" / "runtime_demo.geas").read_text()
PLAN = json.loads((ROOT / "tests" / "fixtures" / "env" / "aws_ecs_plan_sample.json").read_text())

GOOD = {"instances_max": 1, "rollout_max": 1, "shared_host": False, "storage_persistent": True, "internet_ingress_ports": ["443"], "egress_https": True}


def judged(src, facts):
    return {r["id"]: r for r in verify(items(parse(src)), facts)}


def rc(plan, address):
    return next(r for r in plan["resource_changes"] if r["address"] == address)


# ---- 照合器 ----
def test_1台で消えない領域があれば_instances_と_durable_は合格する():
    got = judged(SHOP, GOOD)
    assert got["instances"]["判定"] == PASS and got["durable.default"]["判定"] == PASS


def test_台数が2以上でプラットフォームが同じ機械を保証しなければ_不合格():
    got = judged(SHOP, {**GOOD, "instances_max": 6, "rollout_max": 12})
    assert got["instances"]["判定"] == FAIL and "同じ機械" in got["instances"]["詳細"]
    assert judged(SHOP, {**GOOD, "instances_max": 6, "rollout_max": 12, "shared_host": True})["instances"]["判定"] == PASS


def test_isolation_process_は_入れ替えのときも1台でなければ不合格():
    src = SHOP + "\nstorage Orders isolation process\nstore Order in Orders\n"
    assert judged(src, GOOD)["instances"]["判定"] == PASS
    got = judged(src, {**GOOD, "rollout_max": 2})  # 台数は 1 でも、入れ替えで新旧が重なる
    assert got["instances"]["判定"] == FAIL and "入れ替え" in got["instances"]["詳細"]


def test_人が書いた上限は_台数と入れ替えの両方に効く():
    src = SHOP + 'needs instances at_most 3 "決済事業者の上限"\n'
    assert judged(src, {**GOOD, "instances_max": 2, "rollout_max": 3, "shared_host": True})["instances"]["判定"] == PASS
    assert judged(src, {**GOOD, "instances_max": 2, "rollout_max": 4, "shared_host": True})["instances"]["判定"] == FAIL


def test_読めなかった事実は_合格にせず_確かめられなかったになる():
    none = {k: None for k in GOOD}
    got = judged(SHOP + 'needs exposure internal "社内向け"\nneeds secret Payment.api_key "鍵"\n', none)
    for i in ("instances", "durable.default", "exposure", "reach.Payment", "secret.Payment.api_key"):
        assert got[i]["判定"] == UNKNOWN, (i, got[i])
    assert judged(SHOP, {**GOOD, "instances_max": None})["instances"]["判定"] == UNKNOWN  # 台数が読めなければ、同じ機械かも言えない


def test_消える領域は不合格_外向きの443が無ければ_reach_は不合格_あっても宛先までは言えない():
    assert judged(SHOP, {**GOOD, "storage_persistent": False})["durable.default"]["判定"] == FAIL
    assert judged(SHOP, {**GOOD, "egress_https": False})["reach.Payment"]["判定"] == FAIL
    assert judged(SHOP, GOOD)["reach.Payment"]["判定"] == UNKNOWN  # 合格にはしない


def test_公開範囲_internal_は口が無ければ合格_public_は_80_443_以外が開いていれば不合格():
    internal = SHOP + 'needs exposure internal "社内向け"\n'
    assert judged(internal, {**GOOD, "internet_ingress_ports": []})["exposure"]["判定"] == PASS
    assert judged(internal, GOOD)["exposure"]["判定"] == FAIL
    public = SHOP + 'needs exposure public "一般向け"\n'
    assert judged(public, GOOD)["exposure"]["判定"] == PASS
    got = judged(public, {**GOOD, "internet_ingress_ports": ["443", "22"]})["exposure"]
    assert got["判定"] == FAIL and "22" in got["詳細"]


def test_外から確かめられない項目は_人の保証として数える():
    got = judged(DEMO + 'needs exposure internal "社内"\n', GOOD)
    assert got["limits"]["判定"] == TRUST
    assert sum(counts(list(got.values())).values()) == len(got)


# ---- ECS のアダプタ ----
def test_ECS_の見本から_台数の上限は自動拡張の最大_入れ替えは最大割合から出る():
    facts, notes = facts_from_plan(PLAN)
    assert facts["instances_max"] == 6 and facts["rollout_max"] == 12 and facts["shared_host"] is False  # desired 2、最大 6、200%
    assert facts["storage_persistent"] is False  # volume が空
    assert facts["internet_ingress_ports"] == ["443"]  # アプリの SG は 10.0.0.0/16 だけ。公開 LB のリスナー 443
    assert facts["egress_https"] is True and notes == []


def test_ECS_自動拡張が無ければ_desired_count_が上限で_永続領域があれば_persistent():
    plan = copy.deepcopy(PLAN)
    plan["resource_changes"] = [r for r in plan["resource_changes"] if r["type"] != "aws_appautoscaling_target"]
    rc(plan, "aws_ecs_service.app")["change"]["after"].update(desired_count=1, deployment_maximum_percent=100)
    rc(plan, "aws_ecs_task_definition.app")["change"]["after"]["volume"] = [{"name": "data", "efs_volume_configuration": [{"file_system_id": "fs-1"}]}]
    facts, _ = facts_from_plan(plan)
    assert (facts["instances_max"], facts["rollout_max"], facts["storage_persistent"]) == (1, 1, True)


def test_ECS_確定していない値や複数のサービスは_言い切らず_null_にしてメモを残す():
    plan = copy.deepcopy(PLAN)
    rc(plan, "aws_ecs_service.app")["change"]["after_unknown"]["desired_count"] = True
    rc(plan, "aws_appautoscaling_target.app")["change"]["after_unknown"] = {"max_capacity": True}
    facts, notes = facts_from_plan(plan)
    assert facts["instances_max"] is None and facts["rollout_max"] is None and any("確定していない" in n for n in notes)
    two = copy.deepcopy(PLAN)
    other = copy.deepcopy(rc(two, "aws_ecs_service.app"))
    other["address"], other["name"] = "aws_ecs_service.worker", "worker"
    two["resource_changes"].append(other)
    facts, notes = facts_from_plan(two)
    assert facts["instances_max"] is None and any("--service" in n for n in notes)
    facts, _ = facts_from_plan(two, service="aws_ecs_service.app")
    assert facts["instances_max"] == 6


def test_ECS_公開の_LB_にリスナーが無ければ_公開の口は分からない():
    plan = copy.deepcopy(PLAN)
    plan["resource_changes"] = [r for r in plan["resource_changes"] if r["type"] != "aws_lb_listener"]
    facts, notes = facts_from_plan(plan)
    assert facts["internet_ingress_ports"] is None and any("リスナー" in n for n in notes)


def test_ECS_セキュリティグループで0_0_0_0に開いた口は_公開の口に数える():
    plan = copy.deepcopy(PLAN)
    rc(plan, "aws_security_group.app")["change"]["after"]["ingress"].append({"from_port": 22, "to_port": 22, "protocol": "tcp", "cidr_blocks": ["0.0.0.0/0"]})
    facts, _ = facts_from_plan(plan)
    assert facts["internet_ingress_ports"] == ["22", "443"]


def test_削除されるリソースは_事実に数えない():
    plan = copy.deepcopy(PLAN)
    rc(plan, "aws_ecs_service.app")["change"]["actions"] = ["delete"]
    facts, notes = facts_from_plan(plan)
    assert facts["instances_max"] is None and any("plan に無い" in n for n in notes)


def add(plan, address, type_, after, after_unknown=None, expressions=None):
    plan["resource_changes"].append({"address": address, "type": type_, "name": address.split(".")[-1],
                                     "change": {"actions": ["create"], "after": after, "after_unknown": after_unknown or {}}})
    if expressions is not None:
        plan["configuration"]["root_module"]["resources"].append({"address": address, "expressions": expressions})


def test_ECS_別のリソースに書いたセキュリティグループの規則も_公開の口に数える():
    plan = copy.deepcopy(PLAN)
    add(plan, "aws_security_group_rule.ssh", "aws_security_group_rule",
        {"type": "ingress", "from_port": 22, "to_port": 22, "protocol": "tcp", "cidr_blocks": ["0.0.0.0/0"]})
    add(plan, "aws_vpc_security_group_ingress_rule.v6", "aws_vpc_security_group_ingress_rule",
        {"from_port": 8787, "to_port": 8787, "ip_protocol": "tcp", "cidr_ipv6": "::/0"})
    add(plan, "aws_security_group_rule.out", "aws_security_group_rule",
        {"type": "egress", "from_port": 0, "to_port": 0, "protocol": "-1", "cidr_blocks": ["0.0.0.0/0"]})
    facts, _ = facts_from_plan(plan)
    assert sorted(facts["internet_ingress_ports"]) == ["22", "443", "8787"]
    got = judged(SHOP + 'needs exposure public "一般向け"\n', facts)["exposure"]
    assert got["判定"] == FAIL and "22" in got["詳細"] and "8787" in got["詳細"]


def test_ECS_CIDR_やポートが確定していない入る向きの規則があれば_公開の口は言い切らない():
    plan = copy.deepcopy(PLAN)
    add(plan, "aws_security_group_rule.any", "aws_security_group_rule",
        {"type": "ingress", "from_port": 22, "to_port": 22, "protocol": "tcp"}, {"cidr_blocks": True})
    facts, notes = facts_from_plan(plan)
    assert facts["internet_ingress_ports"] is None and any("確定していない" in n for n in notes)
    assert judged(SHOP + 'needs exposure internal "社内向け"\n', facts)["exposure"]["判定"] == UNKNOWN  # 合格にはしない
    plan = copy.deepcopy(PLAN)
    rc(plan, "aws_security_group.app")["change"]["after_unknown"] = {"ingress": True}
    assert facts_from_plan(plan)[0]["internet_ingress_ports"] is None


def test_ECS_internal_を書いていない_LB_はインターネット向きとして_リスナーの口を数える():
    plan = copy.deepcopy(PLAN)
    lb = rc(plan, "aws_lb.front")["change"]
    del lb["after"]["internal"]
    lb["after_unknown"] = {"internal": True}  # 書かなければ、plan では確定していないと出る（既定は false＝インターネット向き）
    plan["configuration"]["root_module"]["resources"].append({"address": "aws_lb.front", "expressions": {"load_balancer_type": {"constant_value": "application"}}})
    facts, _ = facts_from_plan(plan)
    assert facts["internet_ingress_ports"] == ["443"]
    assert judged(SHOP + 'needs exposure internal "社内向け"\n', facts)["exposure"]["判定"] == FAIL
    plan["configuration"]["root_module"]["resources"][-1]["expressions"]["internal"] = {"references": ["var.internal"]}  # 書いてあって確定していない
    facts, notes = facts_from_plan(plan)
    assert facts["internet_ingress_ports"] is None and any("internal" in n for n in notes)


def test_ECS_リスナーのポートが確定していなければ_公開の口は言い切らない_aws_alb_も読む():
    plan = copy.deepcopy(PLAN)
    li = rc(plan, "aws_lb_listener.https")["change"]
    del li["after"]["port"]
    li["after_unknown"] = {"port": True}
    facts, notes = facts_from_plan(plan)
    assert facts["internet_ingress_ports"] is None and any("ポート" in n for n in notes)
    alb = copy.deepcopy(PLAN)
    for r in alb["resource_changes"]:
        r["type"] = {"aws_lb": "aws_alb", "aws_lb_listener": "aws_alb_listener"}.get(r["type"], r["type"])
    rc(alb, "aws_lb_listener.https")["change"]["after"]["port"] = 8443
    assert facts_from_plan(alb)[0]["internet_ingress_ports"] == ["8443"]


# ---- CLI ----
def run(*args):
    return subprocess.run([sys.executable, "-m", "geas", *args], capture_output=True, text=True, cwd=ROOT)


def test_CLI_env_facts_と_env_verify_は_終了コードで合否を返す(tmp_path):
    app = tmp_path / "a.geas"
    app.write_text(SHOP)
    plan = ROOT / "tests" / "fixtures" / "env" / "aws_ecs_plan_sample.json"
    r = run("env-facts", str(plan))
    assert r.returncode == 0 and json.loads(r.stdout)["facts"]["instances_max"] == 6
    r = run("env-verify", str(app), "--plan", str(plan), "--json")
    j = json.loads(r.stdout)
    assert r.returncode == 1 and j["counts"][FAIL] == 2 and j["results"][0]["判定"] == FAIL  # 台数 6 の ECS で、sqlite のアプリは動かない
    good = tmp_path / "facts.json"
    good.write_text(json.dumps({"facts": GOOD}))
    r = run("env-verify", str(app), "--facts", str(good))
    assert r.returncode == 0, r.stdout  # 不合格なし（reach は確かめられなかった）
    assert run("env-verify", str(app), "--facts", str(good), "--strict").returncode == 1  # 確かめられなかったも止める
    assert run("env-verify", str(app), "--plan", str(tmp_path / "none.json")).returncode == 2
    assert run("env-verify", str(app), "--plan", str(plan), "--adapter", "gcp").returncode == 2
    app.write_text(SHOP + 'needs secret Shipping.api_key "x"\n')
    r = run("env-verify", str(app), "--facts", str(good))
    assert r.returncode == 1 and r.stdout == "" and "名前空間がありません" in r.stderr
