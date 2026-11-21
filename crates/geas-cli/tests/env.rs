// SPDX-License-Identifier: GPL-3.0-or-later
//! 動かす前提（needs と env-checks）：アプリが実行環境に要求すること。導出する項目と、人が理由つきで書く項目。
//! 動かす前提の照合：事実（プラットフォームに依存しない形）と、ECS（terraform plan の JSON）のアダプタ。
//! 見本の plan（tests/fixtures/env/aws_ecs_plan_sample.json）は手書きで、実物の plan とは未照合。
//! 元は Python 版の tests/test_env.py・test_env_verify.py（タグ python-final）。
mod common;
use common::*;
use geas_core::adapter::facts_from_plan;
use geas_core::check::{ERROR, WARN};
use geas_core::envcheck::{counts, verify, FAIL, PASS, TRUST, UNKNOWN};
use geas_core::envreq::{items, EnvItem};
use geas_core::parser::parse;
use geas_core::spec::{build, render};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_geas");

fn shop() -> String {
    fixture("shop/shop.geas")
}

fn demo() -> String {
    fixture("runtime_demo/runtime_demo.geas")
}

fn plan() -> Value {
    serde_json::from_str(&fixture("env/aws_ecs_plan_sample.json")).unwrap()
}

fn env_msgs(src: &str, level: &str) -> Vec<String> {
    with_check(src, false, |ck| ck.diags.iter().filter(|d| d.code == "環境" && d.level == level).map(|d| d.msg.clone()).collect())
}

fn env_errors(src: &str) -> Vec<String> {
    env_msgs(src, ERROR)
}

fn any(v: &[String], needle: &str) -> bool {
    v.iter().any(|m| m.contains(needle))
}

fn its(src: &str) -> Vec<EnvItem> {
    items(&parse(src, None).unwrap_or_else(|e| panic!("{}", e)))
}

fn got(src: &str) -> HashMap<String, EnvItem> {
    its(src).into_iter().map(|x| (x.id.clone(), x)).collect()
}

fn ids(src: &str) -> Vec<String> {
    its(src).into_iter().map(|x| x.id).collect()
}

// ---- 導出と書き方 ----
#[test]
fn 導出される項目は宣言から決まり番号は導出元の名前になる() {
    let g = got(&shop());
    assert_eq!(ids(&shop()), vec!["instances", "durable.default", "reach.Payment"]);
    assert!(g["instances"].aspect == "安全性" && g["instances"].text.contains("同じ機械"));
    assert!(g["reach.Payment"].how == vec!["実環境"] && g["reach.Payment"].origin.contains("導出"));
    assert!(!g.values().any(|x| x.trust));
}

#[test]
fn isolation_processの保存先があると同時に動くのは1台になる() {
    let src = shop() + "\nstorage Orders isolation process\nstore Order in Orders\n";
    let g = got(&src);
    assert!(g["instances"].text.contains("1 台") && g["instances"].origin.contains("isolation process"));
    assert!(g.contains_key("durable.default") && g.contains_key("durable.Orders"));
}

#[test]
fn needs_secretは理由つきで書き外部作用の名前空間だけが秘密を使える() {
    let src = shop() + "\nneeds secret Payment.api_key \"決済事業者の API 鍵\"\n";
    let g = got(&src);
    let x = &g["secret.Payment.api_key"];
    assert!(x.aspect == "機密" && x.origin.contains("人が書いた") && x.why == "決済事業者の API 鍵");
    assert!(env_errors(&src).is_empty());
    assert!(any(&env_errors(&(shop() + "\nneeds secret Shipping.api_key \"x\"\n")), "名前空間がありません"));
    assert!(any(&env_errors(&(shop() + "\nneeds secret Payment.鍵 \"x\"\n")), "lower_snake")); // 識別子は半角英数
    assert!(any(&env_errors(&(shop() + "\nneeds secret Payment.api_key \"a\"\nneeds secret Payment.api_key \"b\"\n")), "2つ以上"));
}

#[test]
fn needsは理由が必須で書き方の誤りは止まる() {
    for (bad, pat) in [
        ("needs secret Payment.api_key", "理由"),
        ("needs exposure open \"x\"", "public"),
        ("needs exposure public", "理由"),
        ("needs instances at_most 0 \"x\"", "1 以上"),
        ("needs instances 3 \"x\"", "at_most"),
        ("needs cpu 2 \"x\"", "needs は"),
    ] {
        let e = parse_err(&format!("{}\n{}\n", shop(), bad)).unwrap_or_else(|| panic!("構文の誤りのはず: {}", bad));
        assert!(e.contains(pat), "{}: {}", bad, e);
    }
}

#[test]
fn instancesは導出した上限より緩く書けず厳しくは書ける() {
    let base = shop() + "\nstorage Orders isolation process\nstore Order in Orders\n";
    assert!(any(&env_errors(&(base.clone() + "needs instances at_most 3 \"x\"\n")), "より緩い"));
    assert!(env_errors(&(base + "needs instances at_most 1 \"二重に動かさない\"\n")).is_empty());
    let free = shop() + "needs instances at_most 3 \"決済事業者の上限が 3 並列\"\n"; // 導出の上限が無ければ、人が書いた上限がそのまま使われる
    assert!(env_errors(&free).is_empty());
    let g = got(&free);
    assert!(g["instances"].text.contains("3 台以下") && g["instances"].why == "決済事業者の上限が 3 並列");
}

#[test]
fn 公開する口があるアプリは公開範囲を書かないと警告し書けば項目と前段の人の保証が出る() {
    let w = env_msgs(&demo(), WARN);
    assert!(w.len() == 1 && w[0].contains("公開範囲"), "{:?}", w);
    let src = demo() + "\nneeds exposure internal \"社内の運送担当だけが使う\"\n";
    assert!(env_msgs(&src, WARN).is_empty());
    let g = got(&src);
    assert!(g["exposure"].text.contains("届く口が無い") && g["exposure"].aspect == "到達性");
    assert!(g["limits"].trust && g["limits"].how == vec!["人の保証"]); // 前段の約束は外から確かめられない
    assert!(g["limits"].text.contains("時間切れ") && !g["limits"].text.contains("本文")); // 本文の上限はサーバーが 413 で守るので、前段には求めない
    assert!(!ids(&shop()).contains(&"limits".to_string())); // 公開する口が無ければ前段の約束も出ない
}

#[test]
fn 仕様書に動かす前提の節が出て行は一覧と同じ() {
    let src = shop() + "\nneeds secret Payment.api_key \"決済事業者の API 鍵\"\n";
    with_check(&src, false, |ck| {
        let md = render(ck, &build(ck), "");
        assert!(md.contains("## 動かす前提") && md.contains("secret.決済.api_key") && md.contains("env-verify")); // 名前は表示名に置き換わって出る
        let rows: Vec<String> = build(ck)["動かす前提"].as_array().unwrap().iter().map(|r| r[0].as_str().unwrap().to_string()).collect();
        let want: Vec<String> = items(ck.p).iter().map(|x| ck.p.show_names(&x.id)).collect();
        assert_eq!(rows, want); // 仕様書は表示名、一覧（JSON）はコードの名前
    });
}

fn run_geas(args: &[&str]) -> (i32, String, String) {
    let r = Command::new(BIN).env("GEAS_LANG", "ja").args(args).current_dir(root()).output().unwrap();
    (r.status.code().unwrap_or(-1), String::from_utf8_lossy(&r.stdout).to_string(), String::from_utf8_lossy(&r.stderr).to_string())
}

#[test]
fn env_checksはjsonで一覧を出し検査のエラーがあれば止まる() {
    let t = Tmp::new("envchecks");
    let f = t.0.join("a.geas");
    let fs = f.to_string_lossy().to_string();
    std::fs::write(&f, shop() + "\nneeds secret Payment.api_key \"決済事業者の API 鍵\"\n").unwrap();
    let (rc, out, err) = run_geas(&["env-checks", &fs, "--json"]);
    assert_eq!(rc, 0, "{}", err);
    let j: Value = serde_json::from_str(&out).unwrap();
    let list = j["items"].as_array().unwrap();
    assert_eq!(list.iter().map(|x| x["id"].as_str().unwrap()).collect::<Vec<_>>(), vec!["instances", "durable.default", "reach.Payment", "secret.Payment.api_key"]);
    let mut keys: Vec<&String> = list[0].as_object().unwrap().keys().collect();
    keys.sort();
    let mut want = vec!["id", "観点", "要求", "由来", "確かめ方", "環境の事実", "人の保証", "理由", "条件"].into_iter().map(String::from).collect::<Vec<_>>();
    want.sort();
    assert_eq!(keys.into_iter().cloned().collect::<Vec<_>>(), want);
    let (_, text, _) = run_geas(&["env-checks", &fs]);
    assert!(text.contains("secret.Payment.api_key（機密）"));
    std::fs::write(&f, shop() + "\nneeds secret Shipping.api_key \"x\"\n").unwrap();
    let (rc, out, err) = run_geas(&["env-checks", &fs, "--json"]);
    assert!(rc == 1 && out.is_empty() && err.contains("名前空間がありません"), "{} {} {}", rc, out, err);
}

// ---- 照合器 ----
fn good() -> Value {
    json!({"instances_max": 1, "rollout_max": 1, "shared_host": false, "storage_persistent": true, "internet_ingress_ports": ["443"], "egress_https": true})
}

fn with(base: Value, over: Value) -> Value {
    let mut m = base.as_object().unwrap().clone();
    for (k, v) in over.as_object().unwrap() {
        m.insert(k.clone(), v.clone());
    }
    Value::Object(m)
}

/// id → (判定, 詳細)
fn judged(src: &str, facts: &Value) -> HashMap<String, (String, String)> {
    verify(&its(src), facts).into_iter().map(|r| (r.id, (r.status.to_string(), r.detail))).collect()
}

fn st(j: &HashMap<String, (String, String)>, id: &str) -> String {
    j[id].0.clone()
}

#[test]
fn 一台で消えない領域があればinstancesとdurableは合格する() {
    let g = judged(&shop(), &good());
    assert!(st(&g, "instances") == PASS && st(&g, "durable.default") == PASS);
}

#[test]
fn 台数が2以上でプラットフォームが同じ機械を保証しなければ不合格() {
    let g = judged(&shop(), &with(good(), json!({"instances_max": 6, "rollout_max": 12})));
    assert!(st(&g, "instances") == FAIL && g["instances"].1.contains("同じ機械"));
    assert_eq!(st(&judged(&shop(), &with(good(), json!({"instances_max": 6, "rollout_max": 12, "shared_host": true}))), "instances"), PASS);
}

#[test]
fn isolation_processは入れ替えのときも1台でなければ不合格() {
    let src = shop() + "\nstorage Orders isolation process\nstore Order in Orders\n";
    assert_eq!(st(&judged(&src, &good()), "instances"), PASS);
    let g = judged(&src, &with(good(), json!({"rollout_max": 2}))); // 台数は 1 でも、入れ替えで新旧が重なる
    assert!(st(&g, "instances") == FAIL && g["instances"].1.contains("入れ替え"));
}

#[test]
fn 人が書いた上限は台数と入れ替えの両方に効く() {
    let src = shop() + "needs instances at_most 3 \"決済事業者の上限\"\n";
    assert_eq!(st(&judged(&src, &with(good(), json!({"instances_max": 2, "rollout_max": 3, "shared_host": true}))), "instances"), PASS);
    assert_eq!(st(&judged(&src, &with(good(), json!({"instances_max": 2, "rollout_max": 4, "shared_host": true}))), "instances"), FAIL);
}

#[test]
fn 読めなかった事実は合格にせず確かめられなかったになる() {
    let none = json!({"instances_max": null, "rollout_max": null, "shared_host": null, "storage_persistent": null, "internet_ingress_ports": null, "egress_https": null});
    let g = judged(&(shop() + "needs exposure internal \"社内向け\"\nneeds secret Payment.api_key \"鍵\"\n"), &none);
    for i in ["instances", "durable.default", "exposure", "reach.Payment", "secret.Payment.api_key"] {
        assert_eq!(st(&g, i), UNKNOWN, "{}", i);
    }
    // 台数が読めなければ、同じ機械かも言えない
    assert_eq!(st(&judged(&shop(), &with(good(), json!({"instances_max": null}))), "instances"), UNKNOWN);
}

#[test]
fn 消える領域は不合格で外向きの443が無ければreachは不合格あっても宛先までは言えない() {
    assert_eq!(st(&judged(&shop(), &with(good(), json!({"storage_persistent": false}))), "durable.default"), FAIL);
    assert_eq!(st(&judged(&shop(), &with(good(), json!({"egress_https": false}))), "reach.Payment"), FAIL);
    assert_eq!(st(&judged(&shop(), &good()), "reach.Payment"), UNKNOWN); // 合格にはしない
}

#[test]
fn 公開範囲internalは口が無ければ合格publicは80と443以外が開いていれば不合格() {
    let internal = shop() + "needs exposure internal \"社内向け\"\n";
    assert_eq!(st(&judged(&internal, &with(good(), json!({"internet_ingress_ports": []}))), "exposure"), PASS);
    assert_eq!(st(&judged(&internal, &good()), "exposure"), FAIL);
    let public = shop() + "needs exposure public \"一般向け\"\n";
    assert_eq!(st(&judged(&public, &good()), "exposure"), PASS);
    let g = judged(&public, &with(good(), json!({"internet_ingress_ports": ["443", "22"]})));
    assert!(st(&g, "exposure") == FAIL && g["exposure"].1.contains("22"));
}

#[test]
fn 外から確かめられない項目は人の保証として数える() {
    let rs = verify(&its(&(demo() + "needs exposure internal \"社内\"\n")), &good());
    assert_eq!(rs.iter().find(|r| r.id == "limits").unwrap().status, TRUST);
    assert_eq!(counts(&rs).iter().map(|(_, n)| *n).sum::<usize>(), rs.len());
}

// ---- ECS のアダプタ ----
fn rc<'a>(p: &'a mut Value, address: &str) -> &'a mut Value {
    p["resource_changes"].as_array_mut().unwrap().iter_mut().find(|r| r["address"] == address).unwrap()
}

fn facts(p: &Value, service: &str) -> (Value, Vec<String>) {
    facts_from_plan(p, service)
}

#[test]
fn ecsの見本から台数の上限は自動拡張の最大入れ替えは最大割合から出る() {
    let (f, notes) = facts(&plan(), "");
    assert!(f["instances_max"] == 6 && f["rollout_max"] == 12 && f["shared_host"] == false); // desired 2、最大 6、200%
    assert_eq!(f["storage_persistent"], json!(false)); // volume が空
    assert_eq!(f["internet_ingress_ports"], json!(["443"])); // アプリの SG は 10.0.0.0/16 だけ。公開 LB のリスナー 443
    assert!(f["egress_https"] == true && notes.is_empty(), "{:?}", notes);
}

#[test]
fn ecs自動拡張が無ければdesired_countが上限で永続領域があればpersistent() {
    let mut p = plan();
    p["resource_changes"].as_array_mut().unwrap().retain(|r| r["type"] != "aws_appautoscaling_target");
    let a = &mut rc(&mut p, "aws_ecs_service.app")["change"]["after"];
    a["desired_count"] = json!(1);
    a["deployment_maximum_percent"] = json!(100);
    rc(&mut p, "aws_ecs_task_definition.app")["change"]["after"]["volume"] = json!([{"name": "data", "efs_volume_configuration": [{"file_system_id": "fs-1"}]}]);
    let (f, _) = facts(&p, "");
    assert_eq!((f["instances_max"].clone(), f["rollout_max"].clone(), f["storage_persistent"].clone()), (json!(1), json!(1), json!(true)));
}

#[test]
fn ecs確定していない値や複数のサービスは言い切らずnullにしてメモを残す() {
    let mut p = plan();
    rc(&mut p, "aws_ecs_service.app")["change"]["after_unknown"]["desired_count"] = json!(true);
    rc(&mut p, "aws_appautoscaling_target.app")["change"]["after_unknown"] = json!({"max_capacity": true});
    let (f, notes) = facts(&p, "");
    assert!(f["instances_max"].is_null() && f["rollout_max"].is_null() && any(&notes, "確定していない"), "{:?} {:?}", f, notes);
    let mut two = plan();
    let mut other = rc(&mut two, "aws_ecs_service.app").clone();
    other["address"] = json!("aws_ecs_service.worker");
    other["name"] = json!("worker");
    two["resource_changes"].as_array_mut().unwrap().push(other);
    let (f, notes) = facts(&two, "");
    assert!(f["instances_max"].is_null() && any(&notes, "--service"));
    let (f, _) = facts(&two, "aws_ecs_service.app");
    assert_eq!(f["instances_max"], 6);
}

#[test]
fn ecs公開のlbにリスナーが無ければ公開の口は分からない() {
    let mut p = plan();
    p["resource_changes"].as_array_mut().unwrap().retain(|r| r["type"] != "aws_lb_listener");
    let (f, notes) = facts(&p, "");
    assert!(f["internet_ingress_ports"].is_null() && any(&notes, "リスナー"));
}

#[test]
fn ecsセキュリティグループで0_0_0_0に開いた口は公開の口に数える() {
    let mut p = plan();
    rc(&mut p, "aws_security_group.app")["change"]["after"]["ingress"].as_array_mut().unwrap().push(json!({"from_port": 22, "to_port": 22, "protocol": "tcp", "cidr_blocks": ["0.0.0.0/0"]}));
    let (f, _) = facts(&p, "");
    assert_eq!(f["internet_ingress_ports"], json!(["22", "443"]));
}

#[test]
fn 削除されるリソースは事実に数えない() {
    let mut p = plan();
    rc(&mut p, "aws_ecs_service.app")["change"]["actions"] = json!(["delete"]);
    let (f, notes) = facts(&p, "");
    assert!(f["instances_max"].is_null() && any(&notes, "plan に無い"));
}

fn add(p: &mut Value, address: &str, type_: &str, after: Value, after_unknown: Option<Value>, expressions: Option<Value>) {
    let name = address.rsplit('.').next().unwrap();
    p["resource_changes"].as_array_mut().unwrap().push(json!({"address": address, "type": type_, "name": name, "change": {"actions": ["create"], "after": after, "after_unknown": after_unknown.unwrap_or(json!({}))}}));
    if let Some(e) = expressions {
        p["configuration"]["root_module"]["resources"].as_array_mut().unwrap().push(json!({"address": address, "expressions": e}));
    }
}

#[test]
fn ecs別のリソースに書いたセキュリティグループの規則も公開の口に数える() {
    let mut p = plan();
    add(&mut p, "aws_security_group_rule.ssh", "aws_security_group_rule", json!({"type": "ingress", "from_port": 22, "to_port": 22, "protocol": "tcp", "cidr_blocks": ["0.0.0.0/0"]}), None, None);
    add(&mut p, "aws_vpc_security_group_ingress_rule.v6", "aws_vpc_security_group_ingress_rule", json!({"from_port": 8787, "to_port": 8787, "ip_protocol": "tcp", "cidr_ipv6": "::/0"}), None, None);
    add(&mut p, "aws_security_group_rule.out", "aws_security_group_rule", json!({"type": "egress", "from_port": 0, "to_port": 0, "protocol": "-1", "cidr_blocks": ["0.0.0.0/0"]}), None, None);
    let (f, _) = facts(&p, "");
    let mut ports: Vec<String> = f["internet_ingress_ports"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect();
    ports.sort();
    assert_eq!(ports, vec!["22", "443", "8787"]);
    let g = judged(&(shop() + "needs exposure public \"一般向け\"\n"), &f);
    assert!(st(&g, "exposure") == FAIL && g["exposure"].1.contains("22") && g["exposure"].1.contains("8787"));
}

#[test]
fn ecs_cidrやポートが確定していない入る向きの規則があれば公開の口は言い切らない() {
    let mut p = plan();
    add(&mut p, "aws_security_group_rule.any", "aws_security_group_rule", json!({"type": "ingress", "from_port": 22, "to_port": 22, "protocol": "tcp"}), Some(json!({"cidr_blocks": true})), None);
    let (f, notes) = facts(&p, "");
    assert!(f["internet_ingress_ports"].is_null() && any(&notes, "確定していない"));
    assert_eq!(st(&judged(&(shop() + "needs exposure internal \"社内向け\"\n"), &f), "exposure"), UNKNOWN); // 合格にはしない
    let mut p = plan();
    rc(&mut p, "aws_security_group.app")["change"]["after_unknown"] = json!({"ingress": true});
    assert!(facts(&p, "").0["internet_ingress_ports"].is_null());
}

#[test]
fn ecs_internalを書いていないlbはインターネット向きとしてリスナーの口を数える() {
    let mut p = plan();
    {
        let lb = &mut rc(&mut p, "aws_lb.front")["change"];
        lb["after"].as_object_mut().unwrap().remove("internal");
        lb["after_unknown"] = json!({"internal": true}); // 書かなければ、plan では確定していないと出る（既定は false＝インターネット向き）
    }
    p["configuration"]["root_module"]["resources"].as_array_mut().unwrap().push(json!({"address": "aws_lb.front", "expressions": {"load_balancer_type": {"constant_value": "application"}}}));
    let (f, _) = facts(&p, "");
    assert_eq!(f["internet_ingress_ports"], json!(["443"]));
    assert_eq!(st(&judged(&(shop() + "needs exposure internal \"社内向け\"\n"), &f), "exposure"), FAIL);
    let n = p["configuration"]["root_module"]["resources"].as_array().unwrap().len();
    p["configuration"]["root_module"]["resources"][n - 1]["expressions"]["internal"] = json!({"references": ["var.internal"]}); // 書いてあって確定していない
    let (f, notes) = facts(&p, "");
    assert!(f["internet_ingress_ports"].is_null() && any(&notes, "internal"));
}

#[test]
fn ecsリスナーのポートが確定していなければ公開の口は言い切らないaws_albも読む() {
    let mut p = plan();
    {
        let li = &mut rc(&mut p, "aws_lb_listener.https")["change"];
        li["after"].as_object_mut().unwrap().remove("port");
        li["after_unknown"] = json!({"port": true});
    }
    let (f, notes) = facts(&p, "");
    assert!(f["internet_ingress_ports"].is_null() && any(&notes, "ポート"));
    let mut alb = plan();
    for r in alb["resource_changes"].as_array_mut().unwrap() {
        let t = match r["type"].as_str().unwrap() {
            "aws_lb" => "aws_alb",
            "aws_lb_listener" => "aws_alb_listener",
            o => o,
        }
        .to_string();
        r["type"] = json!(t);
    }
    rc(&mut alb, "aws_lb_listener.https")["change"]["after"]["port"] = json!(8443);
    assert_eq!(facts(&alb, "").0["internet_ingress_ports"], json!(["8443"]));
}

// ---- CLI ----
#[test]
fn cliのenv_factsとenv_verifyは終了コードで合否を返す() {
    let t = Tmp::new("envverify");
    let app = t.0.join("a.geas");
    let a = app.to_string_lossy().to_string();
    std::fs::write(&app, shop()).unwrap();
    let plan_path = root().join("tests").join("fixtures").join("env").join("aws_ecs_plan_sample.json");
    let pp = plan_path.to_string_lossy().to_string();
    let (rc, out, _) = run_geas(&["env-facts", &pp]);
    assert!(rc == 0 && serde_json::from_str::<Value>(&out).unwrap()["facts"]["instances_max"] == 6);
    let (rc, out, _) = run_geas(&["env-verify", &a, "--plan", &pp, "--json"]);
    let j: Value = serde_json::from_str(&out).unwrap();
    assert!(rc == 1 && j["counts"][FAIL] == 2 && j["results"][0]["判定"] == FAIL, "{}", out); // 台数 6 の ECS で、sqlite のアプリは動かない
    let goodf = t.0.join("facts.json");
    let gf = goodf.to_string_lossy().to_string();
    std::fs::write(&goodf, json!({"facts": good()}).to_string()).unwrap();
    let (rc, out, _) = run_geas(&["env-verify", &a, "--facts", &gf]);
    assert_eq!(rc, 0, "{}", out); // 不合格なし（reach は確かめられなかった）
    assert_eq!(run_geas(&["env-verify", &a, "--facts", &gf, "--strict"]).0, 1); // 確かめられなかったも止める
    let none = t.0.join("none.json");
    assert_eq!(run_geas(&["env-verify", &a, "--plan", &none.to_string_lossy()]).0, 2);
    assert_eq!(run_geas(&["env-verify", &a, "--plan", &pp, "--adapter", "gcp"]).0, 2);
    std::fs::write(&app, shop() + "needs secret Shipping.api_key \"x\"\n").unwrap();
    let (rc, out, err) = run_geas(&["env-verify", &a, "--facts", &gf]);
    assert!(rc == 1 && out.is_empty() && err.contains("名前空間がありません"), "{} {} {}", rc, out, err);
}
