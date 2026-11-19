// SPDX-License-Identifier: GPL-3.0-or-later
//! AWS の ECS（terraform show -json の出力）から、環境の事実を作るアダプタ（Python 版 adapters/aws_ecs.py の移し）。
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

pub const ADAPTER_NAMES: [&str; 1] = ["aws-ecs"];
const DEFAULT_MAX_PERCENT: i64 = 200;

type Live = IndexMap<String, (String, Value, Value)>;

fn live_of(plan: &Value) -> Live {
    let mut out = IndexMap::new();
    for rc in plan.get("resource_changes").and_then(|v| v.as_array()).cloned().unwrap_or_default() {
        let ch = rc.get("change").cloned().unwrap_or(Value::Null);
        if ch.get("actions") == Some(&json!(["delete"])) || ch.get("after").map_or(true, |a| a.is_null()) {
            continue;
        }
        let aft = ch.get("after").cloned().filter(|v| !v.is_null()).unwrap_or_else(|| json!({}));
        let unk = ch.get("after_unknown").cloned().filter(|v| !v.is_null()).unwrap_or_else(|| json!({}));
        out.insert(rc["address"].as_str().unwrap_or("").to_string(), (rc["type"].as_str().unwrap_or("").to_string(), aft, unk));
    }
    out
}

fn refs(node: &Value, found: &mut BTreeSet<String>) {
    match node {
        Value::Object(m) => {
            for (k, v) in m {
                if k == "references" && v.is_array() {
                    found.extend(v.as_array().unwrap().iter().filter_map(|x| x.as_str().map(|s| s.to_string())));
                } else {
                    refs(v, found);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|v| refs(v, found)),
        _ => {}
    }
}

fn root_resources(plan: &Value) -> Vec<Value> {
    plan.pointer("/configuration/root_module/resources").and_then(|v| v.as_array()).cloned().unwrap_or_default()
}

fn config_refs(plan: &Value) -> IndexMap<String, BTreeSet<String>> {
    root_resources(plan)
        .iter()
        .map(|r| {
            let mut f = BTreeSet::new();
            refs(r.get("expressions").unwrap_or(&Value::Null), &mut f);
            (r["address"].as_str().unwrap_or("").to_string(), f)
        })
        .collect()
}

fn config_keys(plan: &Value) -> IndexMap<String, BTreeSet<String>> {
    root_resources(plan)
        .iter()
        .map(|r| (r["address"].as_str().unwrap_or("").to_string(), r.get("expressions").and_then(|e| e.as_object()).map(|m| m.keys().cloned().collect()).unwrap_or_default()))
        .collect()
}

/// 値が確定していて、null でもないとき、その値
fn known<'a>(after: &'a Value, unknown: &Value, key: &str) -> Option<&'a Value> {
    if unknown.get(key) == Some(&Value::Bool(true)) {
        return None;
    }
    after.get(key).filter(|v| !v.is_null())
}

fn proto_all(r: &Value) -> bool {
    matches!(r.get("protocol"), Some(Value::String(s)) if s == "-1") || r.get("protocol") == Some(&json!(-1))
}

fn ports(rule: &Value) -> String {
    let (lo, hi) = (rule.get("from_port").filter(|v| !v.is_null()), rule.get("to_port").filter(|v| !v.is_null()));
    if proto_all(rule) || lo.is_none() || hi.is_none() {
        return "all".into();
    }
    let s = |v: &Value| match v {
        Value::String(x) => x.clone(),
        o => o.to_string(),
    };
    if lo == hi { s(lo.unwrap()) } else { format!("{}-{}", s(lo.unwrap()), s(hi.unwrap())) }
}

fn covers(rule: &Value, port: i64) -> bool {
    if proto_all(rule) {
        return true;
    }
    match (rule.get("from_port").and_then(|v| v.as_i64()), rule.get("to_port").and_then(|v| v.as_i64())) {
        (Some(lo), Some(hi)) => lo <= port && port <= hi,
        _ => false,
    }
}

fn has_str(v: Option<&Value>, s: &str) -> bool {
    v.and_then(|x| x.as_array()).map_or(false, |a| a.iter().any(|x| x.as_str() == Some(s)))
}

fn open_cidr(rule: &Value) -> bool {
    has_str(rule.get("cidr_blocks"), "0.0.0.0/0") || has_str(rule.get("ipv6_cidr_blocks"), "::/0")
}

fn has_unknown(au: &Value) -> bool {
    match au {
        Value::Bool(true) => true,
        Value::Object(m) => m.values().any(has_unknown),
        Value::Array(a) => a.iter().any(has_unknown),
        _ => false,
    }
}

const RULE_KEYS: [&str; 8] = ["from_port", "to_port", "protocol", "ip_protocol", "cidr_blocks", "ipv6_cidr_blocks", "cidr_ipv4", "cidr_ipv6"];

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map_or(true, |f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// (向き, 規則, 確定していないか, どこの規則か)
fn sg_rules(live: &Live) -> Vec<(String, Value, bool, String)> {
    let mut out = vec![];
    for (a, (t, aft, au)) in live {
        if t == "aws_security_group" {
            for d in ["ingress", "egress"] {
                let unknown = au.get(d).map_or(false, has_unknown);
                for (i, r) in aft.get(d).and_then(|v| v.as_array()).cloned().unwrap_or_default().into_iter().enumerate() {
                    out.push((d.to_string(), r, unknown, format!("{} の {}[{}]", a, d, i)));
                }
                if unknown && !truthy(aft.get(d)) {
                    out.push((d.to_string(), json!({}), true, format!("{} の {}", a, d)));
                }
            }
        } else if t == "aws_security_group_rule" {
            let d = if au.get("type").map_or(false, has_unknown) { "ingress".to_string() } else { aft.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string() };
            let unk = RULE_KEYS.iter().chain(["type"].iter()).any(|k| au.get(*k).map_or(false, has_unknown));
            out.push((d, aft.clone(), unk, a.clone()));
        } else if t == "aws_vpc_security_group_ingress_rule" || t == "aws_vpc_security_group_egress_rule" {
            let c4 = aft.get("cidr_ipv4").filter(|v| truthy(Some(v))).cloned();
            let c6 = aft.get("cidr_ipv6").filter(|v| truthy(Some(v))).cloned();
            let r = json!({"from_port": aft.get("from_port").cloned().unwrap_or(Value::Null), "to_port": aft.get("to_port").cloned().unwrap_or(Value::Null),
                           "protocol": aft.get("ip_protocol").cloned().unwrap_or(Value::Null),
                           "cidr_blocks": c4.map(|c| vec![c]).unwrap_or_default(), "ipv6_cidr_blocks": c6.map(|c| vec![c]).unwrap_or_default()});
            let unk = RULE_KEYS.iter().any(|k| au.get(*k).map_or(false, has_unknown));
            out.push((if t.ends_with("ingress_rule") { "ingress" } else { "egress" }.to_string(), r, unk, a.clone()));
        }
    }
    out
}

/// (事実, 読めなかった理由などのメモ)
pub fn facts_from_plan(plan: &Value, service: &str) -> (Value, Vec<String>) {
    let live = live_of(plan);
    let (cfg, written) = (config_refs(plan), config_keys(plan));
    let mut notes: Vec<String> = vec![];
    let mut facts: IndexMap<&str, Value> = ["instances_max", "rollout_max", "shared_host", "storage_persistent", "internet_ingress_ports", "egress_https"].iter().map(|k| (*k, Value::Null)).collect();
    let mut services: Vec<String> = live.iter().filter(|(_, (t, _, _))| t == "aws_ecs_service").map(|(a, _)| a.clone()).collect();
    if !service.is_empty() {
        services.retain(|a| a == service);
        if services.is_empty() {
            notes.push(format!("指名された {} が plan に無い", service));
        }
    }
    let app = if services.len() == 1 { Some(services[0].clone()) } else { None };
    if services.len() > 1 {
        notes.push("aws_ecs_service が複数あり、アプリがどれか決められない（--service で指名する）".into());
    }
    if services.is_empty() && service.is_empty() {
        notes.push("aws_ecs_service が plan に無い".into());
    }
    let empty = BTreeSet::new();
    if let Some(app) = &app {
        let (_, after, unk) = &live[app];
        facts.insert("shared_host", json!(false));
        let desired = known(after, unk, "desired_count").and_then(|v| v.as_i64());
        let mut cap = desired;
        if cap.is_none() {
            notes.push(format!("{} の desired_count が確定していない", app));
        }
        for (a, (t, aft, au)) in &live {
            if t == "aws_appautoscaling_target" && cfg.get(a).unwrap_or(&empty).contains(app) {
                match known(aft, au, "max_capacity").and_then(|v| v.as_i64()) {
                    Some(mx) => cap = Some(cap.map_or(mx, |c| c.max(mx))),
                    None => {
                        cap = None;
                        notes.push(format!("{} の max_capacity が確定していない", a));
                        break;
                    }
                }
            }
        }
        facts.insert("instances_max", cap.map_or(Value::Null, |c| json!(c)));
        if let Some(cap) = cap {
            let mp = known(after, unk, "deployment_maximum_percent").and_then(|v| v.as_i64()).unwrap_or(DEFAULT_MAX_PERCENT);
            facts.insert("rollout_max", json!((cap * mp + 99).div_euclid(100)));
        }
        let tds: Vec<&String> = cfg.get(app).unwrap_or(&empty).iter().filter(|a| live.get(*a).map_or(false, |l| l.0 == "aws_ecs_task_definition")).collect();
        if tds.len() == 1 {
            match live[tds[0]].1.get("volume") {
                Some(Value::Array(vols)) => {
                    let p = vols.iter().any(|v| truthy(v.get("efs_volume_configuration")) || truthy(v.get("configure_at_launch")) || truthy(v.get("docker_volume_configuration")));
                    facts.insert("storage_persistent", json!(p));
                }
                _ => notes.push(format!("{} の volume が確定していない", tds[0])),
            }
        } else {
            notes.push("サービスから、タスク定義を 1 つにたどれなかった".into());
        }
    }
    let sgs: Vec<&String> = live.iter().filter(|(_, (t, _, _))| t == "aws_security_group").map(|(a, _)| a).collect();
    let rules = sg_rules(&live);
    if !sgs.is_empty() {
        let mut ports_v: Option<Vec<String>> = Some(vec![]);
        for (d, r, unknown, where_) in &rules {
            if d == "ingress" && *unknown {
                ports_v = None;
                notes.push(format!("{} の CIDR かポートが確定していない（公開の口は言い切らない）", where_));
                break;
            }
            if d == "ingress" && open_cidr(r) {
                let p = ports(r);
                let v = ports_v.as_mut().unwrap();
                if !v.contains(&p) {
                    v.push(p);
                }
            }
        }
        facts.insert("internet_ingress_ports", ports_v.map_or(Value::Null, |v| json!(v)));
        let any_open = rules.iter().any(|(d, r, u, _)| d == "egress" && !u && open_cidr(r) && covers(r, 443));
        let any_unk = rules.iter().any(|(d, _, u, _)| d == "egress" && *u);
        facts.insert("egress_https", if any_open { json!(true) } else if any_unk { Value::Null } else { json!(false) });
    } else {
        notes.push("aws_security_group が plan に無い".into());
    }
    let listeners: Vec<(&String, &Value, &Value)> = live.iter().filter(|(_, (t, _, _))| t == "aws_lb_listener" || t == "aws_alb_listener").map(|(a, (_, f, u))| (a, f, u)).collect();
    for (a, (t, aft, _)) in &live {
        if t != "aws_lb" && t != "aws_alb" {
            continue;
        }
        let internal = aft.get("internal");
        if internal == Some(&Value::Bool(true)) {
            continue;
        }
        if internal != Some(&Value::Bool(false)) && (!written.contains_key(a) || written[a].contains("internal")) {
            facts.insert("internet_ingress_ports", Value::Null);
            notes.push(format!("{} の internal が plan で確定していない。公開の口は言い切らない", a));
            break;
        }
        let mine: Vec<&(&String, &Value, &Value)> = listeners.iter().filter(|x| cfg.get(x.0).unwrap_or(&empty).contains(a)).collect();
        let mine: Vec<&(&String, &Value, &Value)> = if mine.is_empty() { listeners.iter().collect() } else { mine };
        if mine.is_empty() {
            facts.insert("internet_ingress_ports", Value::Null);
            notes.push(format!("{} は公開の LB だが、リスナーが plan に無く、公開のポートが分からない", a));
            break;
        }
        if facts["internet_ingress_ports"].is_null() {
            break;
        }
        for (la, aft_l, au_l) in mine {
            let port = aft_l.get("port").and_then(|v| v.as_i64());
            if port.is_none() || au_l.get("port").map_or(false, has_unknown) {
                facts.insert("internet_ingress_ports", Value::Null);
                notes.push(format!("{} のポートが確定していない（公開の口は言い切らない）", la));
                break;
            }
            let p = port.unwrap().to_string();
            if let Some(Value::Array(a)) = facts.get_mut("internet_ingress_ports") {
                if !a.iter().any(|x| x.as_str() == Some(&p)) {
                    a.push(json!(p));
                }
            }
        }
        if facts["internet_ingress_ports"].is_null() {
            break;
        }
    }
    let mut m = Map::new();
    for (k, v) in facts {
        m.insert(k.to_string(), v);
    }
    (Value::Object(m), notes)
}
