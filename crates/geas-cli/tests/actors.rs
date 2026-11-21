// SPDX-License-Identifier: GPL-3.0-or-later
//! 呼べる人の区別（actor 名前 ／ resource の動作の for 役割, … ／ for anyone）と、範囲（scope）・絞り込みの条件 q[名前]・続きを読む・並びを選ぶ・事情のある POST。
//! 本人の確かめは前に置くプロキシの仕事で、サーバーは、プロキシが載せたヘッダ X-Geas-Actor の値と、動作の for を比べる。
//! 元は Python 版の tests/test_actors.py（タグ python-final）。
mod common;
use common::serve::{build_app, http, App, Srv};
use common::*;
use geas_core::check::defs::check;
use geas_core::envreq::items;
use geas_core::hostgen::definitions;
use geas_core::mcp::tool_defs;
use geas_core::parser::parse;
use geas_core::receipt::build_receipt;
use geas_core::rest::internal_targets;
use geas_core::review::review;
use geas_core::spec::{build, render};
use regex::Regex;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

fn src_path() -> PathBuf {
    root().join("tests").join("fixtures").join("runtime_demo").join("runtime_demo.geas")
}

fn plain() -> String {
    std::fs::read_to_string(src_path()).unwrap()
}

const ACTORS: &str = "\n/// 客\nactor Customer\n/// お店の人\nactor Staff\n";
const SCOPE: &str = include_str!("data/actors_scope.txt");

// 呼ぶもの → for に書く役割。ここに無い動作は Staff だけ
fn who() -> Vec<(&'static str, &'static str)> {
    vec![("accept", "Customer"), ("parcel_list", "anyone"), ("restamp", "Customer, Staff"), ("parcel_of", "Customer, Staff")]
}

fn over(mut w: Vec<(&'static str, &'static str)>, k: &'static str, v: &'static str) -> Vec<(&'static str, &'static str)> {
    w.retain(|(n, _)| *n != k);
    w.push((k, v));
    w
}

/// 見本に役割を足した版。外から呼べる動作すべてに for を書く（流れの途中の処理には書かない）
fn with_actors_x(who: &[(&str, &str)], default: &str, actors: &str) -> String {
    let text = plain();
    let mut prog = parse(&text, Some(&src_path())).unwrap();
    let inner = internal_targets(&prog);
    let _ = check(&mut prog, false);
    let mut lines: Vec<String> = text.split('\n').map(String::from).collect();
    for res in &prog.resources {
        for a in &res.actions {
            if inner.contains(&a.target) {
                continue;
            }
            let role = who.iter().find(|(n, _)| *n == a.target).map(|(_, r)| *r).unwrap_or(default);
            if !role.is_empty() {
                let head = format!("{} {}", a.kind, a.target);
                let i = (a.line - 1) as usize;
                assert!(lines[i].contains(&head), "{} {}", head, lines[i]);
                lines[i] = lines[i].replacen(&head, &format!("{} for {}", head, role), 1);
            }
        }
    }
    lines.join("\n") + actors
}

fn with_actors() -> String {
    with_actors_x(&who(), "Staff", ACTORS)
}

fn scoped_with(scope: &str) -> String {
    with_actors() + scope
}

fn scoped() -> String {
    scoped_with(SCOPE)
}

fn with_ck<R>(text: &str, f: impl FnOnce(&geas_core::check::Checker) -> R) -> R {
    let mut prog = parse(text, Some(&src_path())).unwrap_or_else(|e| panic!("構文の誤り: {}", e));
    let ck = check(&mut prog, true);
    f(&ck)
}

fn errors(text: &str) -> Vec<String> {
    with_ck(text, |ck| ck.diags.iter().filter(|d| d.level == "エラー").map(|d| d.msg.clone()).collect())
}

fn has(text: &str, word: &str) -> bool {
    errors(text).iter().any(|m| m.contains(word))
}

fn perr(text: &str) -> Option<String> {
    parse(text, Some(&src_path())).err().map(|e| format!("{}", e))
}

fn perr_match(text: &str, pat: &str) {
    let e = perr(text).unwrap_or_else(|| panic!("構文の誤りのはず: {}", pat));
    assert!(Regex::new(pat).unwrap().is_match(&e), "{}: {}", pat, e);
}

fn rs(s: &str, old: &str, new: &str) -> String {
    assert!(s.contains(old), "{}", old);
    s.replacen(old, new, 1)
}

#[test]
fn 役割の検査() {
    assert!(errors(&with_actors()).is_empty());
    assert!(errors(&plain()).is_empty()); // 役割の無いアプリは、今までどおり
    // 役割を宣言したら、外から呼べる動作すべてに for が要る（書き忘れで、誰でも呼べる口を作らない）
    assert!(has(&with_actors_x(&over(who(), "accept", ""), "Staff", ACTORS), "呼べる役割がありません"));
    assert!(has(&with_actors_x(&over(who(), "accept", "Custmer"), "Staff", ACTORS), "役割「Custmer」が未定義です"));
    assert!(has(&with_actors_x(&over(who(), "accept", "anyone, Staff"), "Staff", ACTORS), "ほかの役割と並べて書けません"));
    assert!(has(&with_actors_x(&over(who(), "accept", "Staff, Staff"), "Staff", ACTORS), "重ねて書いています"));
    assert!(has(&with_actors_x(&who(), "Staff", ""), "for に書く役割が宣言されていません"));
    assert!(has(&with_actors_x(&who(), "Staff", &format!("{}actor Staff\n", ACTORS)), "同じ名前の役割が2つ以上"));
    assert!(has(&with_actors_x(&who(), "Staff", &format!("{}actor anyone\n", ACTORS)), "役割の名前にできません"));
    // 流れの途中の処理（外から呼べない）には書けない
    assert!(has(&rs(&with_actors(), "create settle {", "create settle for Staff {"), "流れの途中の処理は外から呼べないので"));
    // 使っていない役割は警告
    with_ck(&with_actors_x(&who(), "Staff", &format!("{}actor Auditor\n", ACTORS)), |ck| assert!(ck.diags.iter().any(|d| d.msg.contains("どの動作の for にも書かれていません"))));
    assert!(perr(&rs(&with_actors(), "for Customer, Staff", "for")).is_some());
}

#[test]
fn 役割は仕様書と定義と動かす前提に出る() {
    with_ck(&with_actors(), |ck| {
        let spec = build(ck);
        let md = render(ck, &spec, "");
        let html = review(ck, None, &[], "", false).unwrap();
        assert!(md.contains("| 外から | 呼べる人 |"));
        let routes = spec["ルート"].as_array().unwrap();
        let row = |path: &str, m: &str| routes.iter().find(|x| x[0] == m && x[1] == path).unwrap_or_else(|| panic!("{} {}", m, path)).clone();
        assert!(row("/parcels", "POST")[6] == "客" && row("/parcels", "GET")[6] == "誰でも");
        assert!(row("/parcels/:id", "PUT")[6] == "客、お店の人" && row("/parcels/:id", "DELETE")[6] == "お店の人");
        assert_eq!(row("/settlements", "POST")[6], "（外から呼べない）");
        let roles = spec["役割"].as_array().unwrap();
        // ヘッダに載せるのは、コードの名前
        assert_eq!(roles.iter().map(|x| (x[0].as_str().unwrap().to_string(), x[1].as_str().unwrap().to_string())).collect::<Vec<_>>(), vec![("客".to_string(), "Customer".to_string()), ("お店の人".to_string(), "Staff".to_string()), ("誰でも".to_string(), "（要らない）".to_string())]);
        let calls = |name: &str| roles.iter().find(|x| x[0] == name).unwrap()[2].as_str().unwrap().to_string();
        assert!(calls("客").contains("POST /parcels") && !calls("客").contains("DELETE /parcels/:id") && calls("お店の人").contains("DELETE /parcels/:id"));
        assert_eq!(calls("誰でも"), "GET /parcels");
        assert!(md.contains("## 役割") && md.contains("X-Geas-Actor") && md.contains("401") && md.contains("403"));
        assert!(html.contains("呼べる人") && html.contains("役割（呼べる人の区別）"));
        // 生成物の定義：役割のある口だけに for が付く（誰でも呼べる口と、流れの途中の処理には付かない）
        let defs = definitions(ck).unwrap();
        let route = |m: &str, p: &str| defs["routes"].as_array().unwrap().iter().find(|r| r["method"] == m && r["path"] == p).unwrap().clone();
        assert_eq!(route("POST", "/parcels")["for"], json!(["Customer"]));
        assert_eq!(route("PUT", "/parcels/:id")["for"], json!(["Customer", "Staff"]));
        assert!(route("GET", "/parcels").get("for").is_none() && route("POST", "/settlements").get("for").is_none());
        // 動かす前提：プロキシが役割を確かめる、という人の保証が出る
        let item = items(ck.p).into_iter().find(|x| x.id == "actor").unwrap();
        assert!(item.trust && item.text.contains("X-Geas-Actor") && item.text.contains("Customer・Staff"));
    });
    // 役割の無いアプリの仕様書は、今までどおり（列も節も出ない）
    with_ck(&plain(), |ck| {
        let md0 = render(ck, &build(ck), "");
        assert!(!md0.contains("呼べる人") && !md0.contains("## 役割") && !items(ck.p).iter().any(|x| x.id == "actor"));
    });
}

#[test]
fn 呼べる人が広がった変更はレシートの弱くなったに出る() {
    let t = Tmp::new("actorsreceipt");
    let weak = |new_text: &str, old_text: &str| -> (Vec<String>, Vec<String>) {
        let f = t.0.join("new.geas");
        std::fs::write(&f, new_text).unwrap();
        with_ck(new_text, |n| {
            with_ck(old_text, |o| {
                let r = build_receipt(n, o, &f.to_string_lossy(), None, "前の版", None);
                (r.weakened.iter().filter(|w| w.3.contains("呼べる人")).map(|w| w.0.clone()).collect(), r.strengthened.iter().filter(|w| w.3.contains("呼べる人")).map(|w| w.0.clone()).collect())
            })
        })
    };
    let base = with_actors();
    assert_eq!(weak(&base, &base), (vec![], vec![]));
    let (w, s) = weak(&with_actors_x(&over(who(), "remove_parcel", "Customer, Staff"), "Staff", ACTORS), &base); // 消せる人に、客が増えた
    assert!(w == vec!["URL DELETE /parcels/:id".to_string()] && s.is_empty(), "{:?} {:?}", w, s);
    let (w, s) = weak(&with_actors_x(&over(who(), "accept", "anyone"), "Staff", ACTORS), &base); // 誰でも呼べるようになった
    assert!(w == vec!["URL POST /parcels".to_string()] && s.is_empty(), "{:?} {:?}", w, s);
    let (w, s) = weak(&with_actors_x(&over(who(), "restamp", "Staff"), "Staff", ACTORS), &base); // 狭まった
    assert!(w.is_empty() && s == vec!["URL PUT /parcels/:id".to_string()], "{:?} {:?}", w, s);
}

// ---- 組んで動かす ----
fn served_app() -> &'static App {
    static A: OnceLock<App> = OnceLock::new();
    A.get_or_init(|| {
        let t = std::env::temp_dir().join(format!("geas-actors-src-{}", std::process::id()));
        std::fs::create_dir_all(&t).unwrap();
        let code = t.join("actors.geas");
        std::fs::write(&code, scoped()).unwrap();
        build_app("actors", &code, &[])
    })
}

fn served() -> Srv {
    Srv::start(served_app(), &[])
}

fn ids_of(v: &Value) -> Vec<String> {
    v["rows"].as_array().unwrap().iter().map(|x| x["parcel_id"].as_str().unwrap().to_string()).collect()
}

#[test]
fn 役割の無い呼び出しは401合わない役割は403合えば通る() {
    let s = served();
    let new = json!({"kind": "light"});
    // 誰でも呼べる口は、ヘッダが無くても通る
    assert_eq!(s.call("GET", "/parcels", None, None).0, 200);
    // 役割のある口：ヘッダが無い（空も）なら 401、役割が合わなければ 403。どちらも、処理は動かない
    assert_eq!(s.call("POST", "/parcels", None, Some(new.clone())).0, 401);
    assert_eq!(s.call("POST", "/parcels", Some(""), Some(new.clone())).0, 401);
    assert_eq!(s.call("POST", "/parcels", Some("Staff"), Some(new.clone())).0, 403);
    assert_eq!(s.call("POST", "/parcels", Some("Nobody"), Some(new.clone())).0, 403);
    assert_eq!(s.call("POST", "/parcels", Some("customer"), Some(new.clone())).0, 403); // 大文字と小文字は区別する
    assert_eq!(s.call("GET", "/parcels", None, None).1["rows"], json!([]));
    let (code, made) = s.call("POST", "/parcels", Some("Customer"), Some(new));
    assert_eq!(code, 201, "{}", made);
    let pid = ids_of(&s.call("GET", "/parcels", None, None).1)[0].clone();
    // 消せるのは、お店の人だけ
    assert_eq!(s.call("DELETE", &format!("/parcels/{}", pid), Some("Customer"), None).0, 403);
    assert_eq!(s.call("DELETE", &format!("/parcels/{}", pid), None, None).0, 401);
    assert_eq!(s.call("GET", "/parcels", None, None).1["rows"].as_array().unwrap().len(), 1);
    // 役割を複数持つ人（カンマ区切り）は、どれかが合えば通る
    assert_eq!(s.call("GET", &format!("/parcels/{}/details", pid), Some("Auditor, Staff"), None).0, 200);
    assert_eq!(s.call("GET", &format!("/parcels/{}/details", pid), Some("Auditor"), None).0, 403);
    // 流れの途中の処理は、役割があっても外から呼べない
    assert_eq!(s.call("POST", "/settlements", Some("Staff"), Some(json!({"parcel_id": pid}))).0, 404);
}

#[test]
fn 範囲の検査と仕様書() {
    assert!(errors(&scoped()).is_empty(), "{:?}", errors(&scoped()));
    let mine = "from Parcel.own_parcels\n";
    let sc = |a: &str, b: &str| scoped_with(&rs(SCOPE, a, b));
    assert!(has(&sc(mine, "from Parcel.own_parcel\n"), "範囲「own_parcel」が未定義です"));
    assert!(has(&sc(mine, "from Parcel.own_parcels.own_parcels\n"), "重ねて書いています"));
    assert!(has(&sc("scope own_parcels on Parcel", "scope own_parcels on Package"), "データ「Package」が未定義です"));
    assert!(has(&sc("filter refund_ref equals", "filter kind equals"), "ID か参照"));
    assert!(has(&sc("where status == Canceled", "where status == Gone"), "「Gone」は項目 status の状態ではありません"));
    assert!(has(&scoped_with(&SCOPE.replace("scope live on", "scope kind on").replace("Parcel.live", "Parcel.kind")), "項目か範囲か分からなくなります"));
    // 範囲の項目も、読む項目に数える
    assert!(has(&sc("reads Parcel.refund_ref, Parcel.status, Parcel.parcel_id", "reads Parcel.refund_ref, Parcel.parcel_id"), "宣言（読む）にありません"));
    assert!(has(&sc("index my_parcels for Customer", "index my_parcels for anyone"), "呼べるのは役割のある人だけです"));
    assert!(has(&sc(mine, "input caller: id required\n  from Parcel.own_parcels\n  filter parcel_id equals caller\n"), "入力の名前に caller は使えません"));
    // 出来事には呼んだ本人がいないので、本人で絞る範囲の問い合わせは each に使えない（本人で絞らない範囲は使える）
    let ids = |n: &str, s: &str, r: &str| format!("\nquery {n} {{\n  from Parcel.{s}\n  reads {r}\n}}\n\nevent Sweep{n} {{\n  each {n} → ask_nudge\n}}\n", n = n, s = s, r = r);
    assert!(has(&scoped_with(&(SCOPE.to_string() + &ids("mine_ids", "own_parcels", "Parcel.refund_ref"))), "出来事からは呼べません"));
    assert!(!has(&scoped_with(&(SCOPE.to_string() + &ids("live_ids", "live", "Parcel.status"))), "出来事からは呼べません"));
    assert!(has(&sc("filter refund_ref equals caller", "filter refund_ref equals me"), "範囲の引数にありません"));
    for bad in ["filter refund_ref == caller", "filter refund_ref equals caller\n  filter parcel_id equals caller", "order fee desc"] {
        assert!(perr(&sc("filter refund_ref equals caller", bad)).is_some(), "{}", bad);
    }
    perr_match(&sc(mine, "from Parcel in own_parcels\n"), "範囲は in ではなく");
    // in [候補, …] は、候補ごとの == を or でつないだものと同じ（範囲の中でも、問い合わせの中でも書ける）
    let p_in = parse(&scoped(), Some(&src_path())).unwrap();
    let t3 = |a: &str, b: &str, c: &str| (a.to_string(), b.to_string(), c.to_string());
    assert_eq!(p_in.scopes.iter().find(|x| x.name == "in_hand").unwrap().conds, vec![vec![t3("status", "==", "Received"), t3("status", "==", "Waiting")]]);
    assert_eq!(p_in.queries.iter().find(|x| x.name == "parcels_in_hand").unwrap().conds, vec![vec![t3("fee", "==", "0"), t3("fee", "==", "300"), t3("fee", "==", "500")]]);
    assert!(has(&sc("in [Received, Waiting]", "in [Received, Gone]"), "「Gone」は項目 status の状態ではありません"));
    for (bad, why) in [("status in Received", "候補を \\[ \\] で並べます"), ("status in [Received, Received]", "重ねて書いています"), ("status == [Received, Waiting]", "in で書きます"), ("status = [Received, Waiting]", "in で書きます")] {
        perr_match(&sc("status in [Received, Waiting]", bad), why);
    }
    // 引数つきの範囲：渡すのは問い合わせの必ずある入力で、数と型が合うこと
    let using = "from Parcel.of_kind(kind).own_parcels";
    assert!(has(&sc(using, "from Parcel.of_kind.own_parcels"), "範囲 of_kind の引数は 1 個です"));
    assert!(has(&sc(using, "from Parcel.of_kind(kind, kind).own_parcels"), "渡しているのは 2 個です"));
    assert!(has(&sc(using, "from Parcel.of_kind(kin).own_parcels"), "入力にありません"));
    assert!(has(&sc("input kind: Kind required\n  from Parcel.of_kind", "input kind: Kind\n  from Parcel.of_kind"), "required にしてください"));
    assert!(has(&sc("scope of_kind(k: Kind)", "scope of_kind(k: id)"), "同じ種類の文字列・ID・整数どうし"));
    assert!(has(&sc("filter kind equals k\n", "filter kind equals kk\n"), "範囲の引数にありません"));
    assert!(has(&scoped_with(&SCOPE.replace("scope of_kind(k: Kind)", "scope of_kind(k: Kind, extra: Kind)").replace(using, "from Parcel.of_kind(kind, kind).own_parcels")), "どの filter にも使われていません"));
    assert!(has(&scoped_with(&SCOPE.replace("filter kind equals k\n", "filter fee at_least k\n").replace("reads Parcel.kind, Parcel.refund_ref", "reads Parcel.fee, Parcel.refund_ref")), "同じ種類の数・日時どうし"));
    perr_match(&sc("filter refund_ref equals caller", "filter refund_ref at_least caller"), "equals だけ");
    with_ck(&scoped(), |ck| {
        let spec = build(ck);
        let md = render(ck, &spec, "");
        let ranges = spec["範囲"].as_array().unwrap();
        assert!(ranges.iter().any(|x| x[0] == "of_kind（k）" && x[2].as_str().unwrap().contains("が、引数 k と同じ")));
        let qs = spec["問い合わせ"].as_array().unwrap();
        let cond_of = |name: &str| -> Vec<String> { qs.iter().find(|x| x["問い合わせ"] == name).unwrap()["条件"].as_array().unwrap().iter().map(|c| c.as_str().unwrap().to_string()).collect() };
        assert!(cond_of("種類ごとの、自分の小包").iter().any(|c| c.contains("一致 kind（範囲 of_kind）")));
        let who: std::collections::HashMap<String, String> = spec["ルート"].as_array().unwrap().iter().map(|x| (x[1].as_str().unwrap().to_string(), x[6].as_str().unwrap().to_string())).collect();
        assert!(who["/my_parcels"] == "客（本人の分だけ）" && who["/my_live_parcels"] == "客（本人の分だけ）" && who["/live_parcels"] == "誰でも");
        assert!(md.contains("## 範囲") && md.contains("X-Geas-Actor-Id") && md.contains("すべての範囲に入る行だけ"));
        let row = |n: &str| ranges.iter().find(|x| x[0] == n).unwrap().clone();
        assert!(row("own_parcels")[2].as_str().unwrap().ends_with("が、呼んだ本人のID と同じ") && row("live")[2].as_str().unwrap().contains("!="));
        assert_eq!(row("live")[3], "自分の、取り消していない小包、取り消していない小包、自分の小包の種類を書き換える");
        let c = cond_of("自分の、取り消していない小包");
        assert!(c.len() == 2 && c.iter().any(|x| x.contains("（範囲 own_parcels）")) && c.iter().any(|x| x.contains("（範囲 live）")));
        let defs = definitions(ck).unwrap();
        let q = &defs["queries"];
        assert_eq!(q["my_parcels"], json!({"inputs": [], "refs": [], "caller": true}));
        assert_eq!(q["my_canceled_parcels"]["caller"], json!(true));
        assert!(q["parcel_list"].get("caller").is_none() && q["live_parcels"].get("caller").is_none()); // caller は、呼ぶ側の入力にしない。本人で絞らない範囲には要らない
        assert!(items(ck.p).into_iter().find(|x| x.id == "actor").unwrap().text.contains("X-Geas-Actor-Id"));
    });
    with_ck(&with_actors(), |ck| assert!(!render(ck, &build(ck), "").contains("## 範囲")));
}

fn get(s: &Srv, actor: Option<&str>, who: Option<&str>, tag: Option<&str>, path: &str) -> (u16, Option<Vec<String>>, Option<String>) {
    let mut hs: Vec<(&str, &str)> = vec![];
    if let Some(a) = actor {
        if !a.is_empty() {
            hs.push(("X-Geas-Actor", a));
        }
    }
    if let Some(w) = who {
        hs.push(("X-Geas-Actor-Id", w));
    }
    if let Some(t) = tag {
        hs.push(("If-None-Match", t));
    }
    let r = http(s.port, "GET", path, &hs, None);
    let ids = if r.status == 200 { Some(ids_of(&r.json())) } else { None };
    (r.status, ids, r.header("ETag"))
}

fn first(s: &Srv, actor: Option<&str>, who: Option<&str>, path: &str) -> (u16, Option<Vec<String>>) {
    let r = get(s, actor, who, None, path);
    (r.0, r.1)
}

fn contains_all(have: &Option<Vec<String>>, want: &[&String]) -> bool {
    let h = have.clone().unwrap_or_default();
    want.iter().all(|w| h.contains(w))
}

#[test]
fn 範囲つきの問い合わせは本人の分だけを返し並べた範囲はすべてに入る行だけになる() {
    let s = served();
    for _ in 0..2 {
        assert_eq!(s.call("POST", "/parcels", Some("Customer"), Some(json!({"kind": "light"}))).0, 201);
    }
    let mut all = ids_of(&s.call("GET", "/parcels", None, None).1);
    all.sort();
    let (a, b) = (all[all.len() - 2].clone(), all[all.len() - 1].clone());
    for (pid, owner) in [(&a, "c-1"), (&b, "c-2")] {
        // 控え（refund_ref）を「持ち主」に見立てて、お店の人が決める
        assert_eq!(s.call("POST", "/claims", Some("Staff"), Some(json!({"parcel_id": pid, "owner": owner}))).0, 201);
    }
    let (code, rows, tag_a) = get(&s, Some("Customer"), Some("c-1"), None, "/my_parcels");
    assert_eq!((code, rows), (200, Some(vec![a.clone()])));
    assert_eq!(first(&s, Some("Customer"), Some("c-2"), "/my_parcels"), (200, Some(vec![b.clone()])));
    assert_eq!(first(&s, Some("Customer"), Some("nobody"), "/my_parcels"), (200, Some(vec![])));
    // 本人の ID が無ければ 401、役割が無ければ 401、役割が違えば 403
    assert!(first(&s, Some("Customer"), None, "/my_parcels").0 == 401 && first(&s, Some("Customer"), Some(" "), "/my_parcels").0 == 401);
    assert!(first(&s, None, Some("c-1"), "/my_parcels").0 == 401 && first(&s, Some("Staff"), Some("c-1"), "/my_parcels").0 == 403);
    // 別の人が、前の人の答えの版（ETag）を送っても、304 にならず自分の分が返る
    assert_eq!(get(&s, Some("Customer"), Some("c-1"), tag_a.as_deref(), "/my_parcels").0, 304);
    let r = get(&s, Some("Customer"), Some("c-2"), tag_a.as_deref(), "/my_parcels");
    assert_eq!((r.0, r.1), (200, Some(vec![b.clone()])));
    // 並べた範囲は、すべてに入る行だけ（順は関係しない）。本人で絞らない範囲は、誰でも呼べる口にもできる
    assert_eq!(first(&s, Some("Customer"), Some("c-1"), "/my_live_parcels"), (200, Some(vec![a.clone()])));
    assert_eq!(first(&s, Some("Customer"), Some("c-1"), "/my_canceled_parcels"), (200, Some(vec![])));
    assert_eq!(first(&s, Some("Customer"), None, "/my_canceled_parcels").0, 401);
    assert!(contains_all(&first(&s, None, None, "/live_parcels").1, &[&a, &b]));
    // 引数つきの範囲：URL の種類（入力）を範囲に渡す。本人の分との積集合になる
    assert_eq!(first(&s, Some("Customer"), Some("c-1"), "/my_parcels_of_kinds/light"), (200, Some(vec![a.clone()])));
    assert_eq!(first(&s, Some("Customer"), Some("c-2"), "/my_parcels_of_kinds/light"), (200, Some(vec![b.clone()])));
    assert_eq!(first(&s, Some("Customer"), Some("c-1"), "/my_parcels_of_kinds/heavy"), (200, Some(vec![])));
    // 処理の 1 件引きも範囲を通す：自分の小包は書き換えられ、他人の小包は「無い」（404）。本人の ID が無ければ 401
    let relabel = |pid: &str, who: Option<&str>, kind: &str| -> (u16, Option<String>) {
        // PUT には、読んだときの行の版が要る（一覧の行の _etag を If-Match に載せる）
        let rows = s.call("GET", "/parcels", None, None).1;
        let tag = rows["rows"].as_array().unwrap().iter().find(|x| x["parcel_id"] == pid).map(|x| format!("\"{}\"", x["_etag"].as_str().unwrap())).unwrap_or_else(|| "\"none\"".to_string());
        let mut hs: Vec<(&str, &str)> = vec![("Content-Type", "application/json"), ("X-Geas-Actor", "Customer"), ("If-Match", &tag)];
        if let Some(w) = who {
            hs.push(("X-Geas-Actor-Id", w));
        }
        let body = serde_json::to_vec(&json!({"kind": kind, "caller": "c-1"})).unwrap();
        let r = http(s.port, "PUT", &format!("/my_parcel_labels/{}", pid), &hs, Some(&body));
        (r.status, r.json().get("result").and_then(|x| x.as_str()).map(String::from))
    };
    assert_eq!(relabel(&a, Some("c-1"), "mine"), (200, Some("Relabeled".into())));
    assert_eq!(relabel(&b, Some("c-1"), "mine"), (404, Some("NotMine".into()))); // 本文に caller を書いても、ヘッダの本人で決まる
    // 本人の分だけを扱う URL では、無い行も「版が違う」（412）と答える（他人の行があるかどうかを、答えの違いで教えない）
    assert_eq!(relabel(&a, Some("c-2"), "mine"), (404, Some("NotMine".into())));
    assert_eq!(relabel("nope", Some("c-1"), "mine"), (412, None));
    assert_eq!(relabel(&a, None, "mine").0, 401);
    let kinds: std::collections::HashMap<String, String> = s.call("GET", "/parcels", None, None).1["rows"].as_array().unwrap().iter().map(|x| (x["parcel_id"].as_str().unwrap().to_string(), x["kind"].as_str().unwrap().to_string())).collect();
    assert_eq!((kinds[&a].as_str(), kinds[&b].as_str()), ("mine", "light"));
    assert_eq!(relabel(&a, Some("c-1"), "light"), (200, Some("Relabeled".into())));
    // 続きを読む：答えの next を辿ると、全部の行を 1 回ずつ、並びの順で読める。最後のページに next は無い
    let walk = |start: &str| -> (Vec<String>, usize) {
        let (mut seen, mut pages, mut path) = (vec![], 0usize, Some(start.to_string()));
        while let Some(p) = path {
            let (code, body) = s.call("GET", &p, None, None);
            assert_eq!(code, 200, "{} {}", p, body);
            let got: Vec<String> = if body.get("rows").is_some() { ids_of(&body) } else { body["ids"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect() };
            seen.extend(got);
            pages += 1;
            path = body.get("next").and_then(|x| x.as_str()).map(String::from);
            assert!(pages < 50);
        }
        (seen, pages)
    };
    let every: Vec<Value> = s.call("GET", "/parcels", None, None).1["rows"].as_array().unwrap().clone();
    let key = |x: &Value| (x["fee"].as_i64().unwrap(), x["parcel_id"].as_str().unwrap().to_string());
    let mut sorted_asc = every.clone();
    sorted_asc.sort_by_key(key);
    let up: Vec<String> = sorted_asc.iter().map(|x| x["parcel_id"].as_str().unwrap().to_string()).collect();
    let down_want: Vec<String> = up.iter().rev().cloned().collect();
    let (seen, pages) = walk("/parcel_pages");
    assert!(seen == down_want && pages == (down_want.len() + 1) / 2, "{:?} {}", seen, pages);
    let (ids, id_pages) = walk("/parcel_id_pages");
    assert!(ids == up && id_pages == ids.len());
    // 並びを選ぶ：書いた候補から q[order] で選ぶ。選ばなければ 1 行目。次のページの URL にも残る。候補に無い並びは 400
    let (down, down_pages) = walk("/parcel_id_pages?q[order]=fee.desc");
    assert!(down == down_want && down_pages == ids.len());
    assert_eq!(walk("/parcel_id_pages?q[order]=fee.asc").0, ids);
    assert!(s.call("GET", "/parcel_id_pages?q[order]=fee.desc", None, None).1["next"].as_str().unwrap().contains("q[order]=fee.desc"));
    for x in ["fee", "kind.desc", "fee.DESC", ""] {
        assert_eq!(s.call("GET", &format!("/parcel_id_pages?q[order]={}", x), None, None).0, 400, "{}", x);
    }
    assert_eq!(s.call("GET", "/parcel_pages?q[order]=fee.desc", None, None).0, 400); // 並びが 1 行の問い合わせでは選べない
    // 条件は、次のページの URL にも残る
    let lights: Vec<String> = every.iter().filter(|x| x["kind"] == "light").map(|x| x["parcel_id"].as_str().unwrap().to_string()).collect();
    let first_page = s.call("GET", "/parcel_pages?q[kind]=light", None, None).1;
    if lights.len() > 2 {
        let n = first_page["next"].as_str().unwrap();
        assert!(n.contains("q[kind]=light") && n.contains("q[after]="));
    } else {
        assert!(first_page.get("next").is_none());
    }
    let mut got = walk("/parcel_pages?q[kind]=light").0;
    got.sort();
    let mut want = lights.clone();
    want.sort();
    assert_eq!(got, want);
    // 無い行の印は 400。ページの無い問い合わせに q[after] は書けない
    assert!(s.call("GET", "/parcel_pages?q[after]=nope", None, None).0 == 400 && s.call("GET", &format!("/live_parcels?q[after]={}", a), None, None).0 == 400);
    // 事情のある POST：条件は本文の {"q": {…}}。その場で結果を返し（200）、続きは答えの after を q.after に入れて読む
    let quiet = |body: Value, actor: &str, path: &str| s.call("POST", path, Some(actor), Some(body));
    let qp = "/quiet_parcel_searches";
    let (code, page1) = quiet(json!({"q": {"kind": "light", "min_fee": 300}}), "Staff", qp);
    assert!(code == 200 && page1["rows"].as_array().unwrap().len() == 1 && page1.get("next").is_none() && !page1["after"].is_null(), "{}", page1);
    let (code, page2) = quiet(json!({"q": {"kind": "light", "after": page1["after"]}}), "Staff", qp);
    assert!(code == 200 && !page2["rows"].as_array().unwrap().is_empty() && page2["rows"] != page1["rows"]);
    assert!(quiet(json!({"q": {"kind": "heavy"}}), "Staff", qp).1["rows"] == json!([]) && quiet(json!({"q": {"kind": "light", "min_fee": 301}}), "Staff", qp).1["rows"] == json!([]));
    // 要る条件が無い・宣言に無い名前・q のほかのキー・q がオブジェクトでない・クエリ文字列・型違いは 400。役割も確かめる
    for wrong in [json!({}), json!({"q": {}}), json!({"q": {"kind": "light", "other": 1}}), json!({"kind": "light"}), json!({"q": "light"}), json!({"q": {"kind": "light", "min_fee": "300"}}), json!({"q": {"kind": "light"}, "x": 1})] {
        assert_eq!(quiet(wrong.clone(), "Staff", qp).0, 400, "{}", wrong);
    }
    assert_eq!(quiet(json!({"q": {"kind": "light"}}), "Staff", "/quiet_parcel_searches?q[kind]=light").0, 400);
    assert!(quiet(json!({"q": {"kind": "light"}}), "Customer", qp).0 == 403 && s.call("GET", "/quiet_parcel_searches?q[kind]=light", Some("Staff"), None).0 != 200);
    // 絞り込みの条件は q[名前]=値：種類はパス、料金の下限（必ず）と上限（省ける）はクエリ文字列
    let two = "/parcels_by_kind_and_fees/light";
    let g = |p: String| first(&s, None, None, &p);
    assert!(contains_all(&g(format!("{}?q[min_fee]=300", two)).1, &[&a, &b]) && g(format!("{}?q[min_fee]=301", two)) == (200, Some(vec![])));
    assert!(contains_all(&g(format!("{}?q[max_fee]=300&q[min_fee]=0", two)).1, &[&a, &b]) && g(format!("{}?q[min_fee]=0&q[max_fee]=299", two)) == (200, Some(vec![])));
    assert!(contains_all(&g(format!("{}?q%5Bmin_fee%5D=300", two)).1, &[&a, &b])); // 角括弧は %5B %5D でも同じ
    assert_eq!(g("/parcels_by_kind_and_fees/heavy?q[min_fee]=0".to_string()), (200, Some(vec![])));
    // 要る条件が無い・数字でない・宣言に無い名前・q[…] でない形・同じ名前の重複は 400
    for x in ["", "?q[min_fee]=abc", "?q[min_fee]=3.5", "?q[min_fee]=300&q[other]=1", "?min_fee=300", "?q[min_fee]=300&q[min_fee]=300", "?q=300", "?q[min_fee]=300&x"] {
        assert_eq!(g(format!("{}{}", two, x)).0, 400, "{}", x);
    }
    // 条件を取らない URL にクエリ文字列を付けても 400（問い合わせでも、処理でも）
    assert!(g("/live_parcels?q[x]=1".to_string()).0 == 400 && g("/live_parcels?page=2".to_string()).0 == 400);
    assert_eq!(s.call("POST", "/claims?x=1", Some("Staff"), Some(json!({"parcel_id": a, "owner": "c-1"}))).0, 400);
    // 条件が違えば、答えの版（ETag）も別
    let t300 = get(&s, None, None, None, &format!("{}?q[min_fee]=300", two)).2;
    let t301 = get(&s, None, None, None, &format!("{}?q[min_fee]=301", two)).2;
    assert!(t300.is_some() && t300 != t301);
    assert_eq!(get(&s, None, None, t300.as_deref(), &format!("{}?q[min_fee]=301", two)).0, 200);
    assert_eq!(get(&s, None, None, t300.as_deref(), &format!("{}?q[min_fee]=300", two)).0, 304);
    // in [候補, …]：状態がどれかで、料金もどれかの小包（作ったばかりの 2 件は、どちらも当てはまる）
    let hand = first(&s, None, None, "/parcels_in_hand");
    assert!(hand.0 == 200 && contains_all(&hand.1, &[&a, &b]));
}

#[test]
fn 絞り込みの条件はq名前で渡る() {
    let text = scoped();
    assert!(errors(&text).is_empty());
    with_ck(&text, |ck| {
        let defs = definitions(ck).unwrap();
        let route = defs["routes"].as_array().unwrap().iter().find(|r| r["target"] == "parcels_by_kind_and_fee").unwrap().clone();
        assert!(route["path"] == "/parcels_by_kind_and_fees/:kind" && route["path_inputs"] == json!([{"name": "kind"}]));
        assert_eq!(route["q_inputs"], json!([{"name": "min_fee", "int": true}, {"name": "max_fee", "int": true, "optional": true}]));
        // 仕様書の URL の表には、条件の名前まで出る（省ける条件は ?）
        assert!(build(ck)["ルート"].as_array().unwrap().iter().any(|row| row[1] == "/parcels_by_kind_and_fees/:kind?q[min_fee]=…&q[max_fee?]=…"));
        let tools = tool_defs(ck).unwrap();
        let tool = tools.iter().find(|t| t["geas"]["path"] == "/parcels_by_kind_and_fees/:kind").unwrap();
        assert_eq!(tool["geas"]["q"], json!(["min_fee", "max_fee"]));
        assert_eq!(tool["inputSchema"]["required"], json!(["kind", "min_fee"]));
    });
    // パスのキーは 1 つ。キーが入力に無い・URL に載せられない型は、検査で止まる
    let head = "key kind {\n  show parcels_by_kind_and_fee";
    for two in ["key kind, min_fee", "key kind key min_fee"] {
        perr_match(&text.replace(head, &format!("{} {{\n  show parcels_by_kind_and_fee", two)), "パスのキーは 1 つです");
    }
    assert!(has(&text.replace(head, "key nope {\n  show parcels_by_kind_and_fee"), "key「nope」が"));
    assert!(has(&text.replace("input max_fee: Fee\n", "input max_fee: bool\n").replace("  filter fee at_most max_fee\n", ""), "URL に載せられない型です"));
}

#[test]
fn 処理の1件引きは範囲を通せる() {
    let text = scoped();
    assert!(errors(&text).is_empty());
    with_ck(&text, |ck| {
        let info = &ck.infos["relabel_mine"];
        assert!(info.caller && info.scopes == vec!["own_parcels".to_string(), "live".to_string()]);
        let defs = definitions(ck).unwrap();
        assert!(defs["deeds"]["relabel_mine"]["caller"] == json!(true) && defs["deeds"]["claim"].get("caller").is_none());
        let spec = build(ck);
        assert!(spec["ルート"].as_array().unwrap().iter().any(|row| row[6].as_str().unwrap().contains("（本人の分だけ）") && row[1] == "/my_parcel_labels/:id"));
        assert!(render(ck, &spec, "").contains("（範囲 own_parcels、live）["));
    });
    let one = "let p = Parcel.own_parcels.live[parcel_id] else { return NotMine }";
    assert!(has(&text.replace(one, "let p = Parcel.own_parcel[parcel_id] else { return NotMine }"), "範囲「own_parcel」が未定義です"));
    assert!(has(&text.replace(one, "let p = Parcel.of_kind[parcel_id] else { return NotMine }"), "引数つきの範囲"));
    assert!(has(&text.replace(one, "let p = Parcel.live.live[parcel_id] else { return NotMine }"), "重ねて書いています"));
    // 範囲が比べる項目も、読む項目に数える
    assert!(has(&text.replace("reads Parcel.refund_ref, Parcel.status\n  writes Parcel.kind", "reads Parcel.refund_ref\n  writes Parcel.kind"), "宣言"));
    assert!(has(&text.replace("update relabel_mine for Customer", "update relabel_mine for anyone"), "呼べるのは役割のある人だけです"));
    assert!(has(&text.replace(one, "let p = Parcel[parcel_id] else { return NotMine }\n  let k = Parcel.live[parcel_id].kind"), "の形だけです"));
}

#[test]
fn ページのある問い合わせは続きの印を受ける() {
    let text = scoped();
    with_ck(&text, |ck| {
        let d = definitions(ck).unwrap();
        assert!(d["queries"]["parcels_paged"]["paged"] == json!(true) && d["queries"]["live_parcels"].get("paged").is_none());
        let route = d["routes"].as_array().unwrap().iter().find(|r| r["target"] == "parcels_paged").unwrap().clone();
        assert_eq!(route["q_inputs"], json!([{"name": "kind", "optional": true}, {"name": "after", "optional": true}]));
        assert!(build(ck)["ルート"].as_array().unwrap().iter().any(|row| row[1] == "/parcel_pages?q[kind?]=…&q[after?]=…"));
    });
    assert!(has(&text.replace("query parcels_paged {\n  input kind: Kind", "query parcels_paged {\n  input kind: Kind, after: Kind"), "入力の名前に after は使えません"));
}

#[test]
fn 並びを2行以上書くと呼ぶ側が選べる() {
    let text = scoped();
    assert!(errors(&text).is_empty());
    with_ck(&text, |ck| {
        let defs = definitions(ck).unwrap();
        let route = defs["routes"].as_array().unwrap().iter().find(|r| r["target"] == "parcel_ids_paged").unwrap().clone();
        assert_eq!(route["q_inputs"], json!([{"name": "order", "optional": true}, {"name": "after", "optional": true}]));
        let spec = build(ck);
        assert!(spec["ルート"].as_array().unwrap().iter().any(|row| row[1] == "/parcel_id_pages?q[order?]=…&q[after?]=…"));
        let qs = spec["問い合わせ"].as_array().unwrap();
        let row = qs.iter().find(|x| x["問い合わせ"] == "小包の ID を 1 件ずつ").unwrap();
        assert_eq!(row["並び"], "fee 小さい順（q[order]=fee.asc。選ばないとき）、fee 大きい順（q[order]=fee.desc）");
        assert_eq!(qs.iter().find(|x| x["問い合わせ"].as_str().unwrap().starts_with("小包を 2 件ずつ")).unwrap()["並び"], "fee 大きい順");
    });
    assert!(has(&text.replace("  order fee asc\n  order fee desc\n", "  order fee asc\n  order fee asc\n"), "を 2 回書いています"));
    assert!(has(&text.replace("query parcel_ids_paged {\n", "query parcel_ids_paged {\n  input order: Kind\n  filter kind equals order\n"), "入力の名前に order は使えません"));
}

#[test]
fn 事情のあるpostで受ける問い合わせ() {
    let text = scoped();
    assert!(errors(&text).is_empty());
    with_ck(&text, |ck| {
        let defs = definitions(ck).unwrap();
        let route = defs["routes"].as_array().unwrap().iter().find(|r| r["target"] == "parcels_quietly").unwrap().clone();
        assert!(route["method"] == "POST" && route["path"] == "/quiet_parcel_searches" && route["kind"] == "index" && route["post"] == json!(true) && route["for"] == json!(["Staff"]), "{}", route);
        let spec = build(ck);
        assert_eq!(spec["POSTの問い合わせ"], json!([["/quiet_parcel_searches", "種類と料金で小包を探す（条件を URL に載せない）", "探している種類を、URL とログに残さない"]]));
        assert!(spec["ルート"].as_array().unwrap().iter().any(|row| row[0] == "POST" && row[1] == "/quiet_parcel_searches（本文 {\"q\": {kind, min_fee?, after?}}）"));
        let md = render(ck, &spec, "");
        assert!(md.contains("## POST で受ける問い合わせ") && md.contains("探している種類を、URL とログに残さない"));
        let tools = tool_defs(ck).unwrap();
        let tool = tools.iter().find(|t| t["geas"]["path"] == "/quiet_parcel_searches").unwrap();
        assert!(tool["geas"]["post_q"] == json!(true) && tool["annotations"]["readOnlyHint"] == json!(true));
    });
    let one = "index parcels_quietly for Staff post \"探している種類を、URL とログに残さない\"";
    perr_match(&text.replace(one, "index parcels_quietly for Staff post"), "理由");
    assert!(errors(&text.replace(one, "index parcels_quietly post \"理由\" for Staff")).is_empty()); // for は、post の後ろでもよい
    let claims = "resource Claims {\n  create claim for Staff {\n    Claimed → 201\n    NoParcel → 404\n  }\n}";
    assert!(has(&text.replace(one, &format!("{}\n  create claim for Staff {{\n    Claimed → 201\n    NoParcel → 404\n  }}", one)).replace(claims, ""), "create は書けません"));
    assert!(has(&text.replace("show my_parcels_of_kind for Customer", "show my_parcels_of_kind for Customer post \"理由\""), "index だけです"));
}

#[test]
fn 画面の一覧は続きを読めて並びを選べる() {
    // node と、グローバルに入れた Playwright があるときだけ
    let root_g = Command::new("npm").args(["root", "-g"]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let ok = Command::new("node").args(["-e", "require('playwright')"]).env("NODE_PATH", &root_g).output().map(|o| o.status.success()).unwrap_or(false);
    if !ok {
        eprintln!("node と Playwright が無いので飛ばします");
        return;
    }
    let s = served();
    for _ in 0..3 {
        assert_eq!(s.call("POST", "/parcels", Some("Customer"), Some(json!({"kind": "light"}))).0, 201);
    }
    assert_eq!(s.call("POST", "/parcels", Some("Customer"), Some(json!({"kind": "heavy"}))).0, 201);
    let mut every: Vec<Value> = s.call("GET", "/parcels", None, None).1["rows"].as_array().unwrap().clone();
    every.sort_by_key(|x| (x["fee"].as_i64().unwrap(), x["parcel_id"].as_str().unwrap().to_string()));
    let up: Vec<String> = every.iter().map(|x| x["parcel_id"].as_str().unwrap().to_string()).collect();
    let script = r#"
const { chromium } = require('playwright');
(async () => {
  const b = await chromium.launch();
  const p = await b.newPage();
  await p.goto(process.argv[1] + '/parcel_id_pages', { waitUntil: 'networkidle' });
  const box = '[data-show="parcel_ids_paged"]';
  const ids = async () => (await p.locator(box + ' .rows').innerText()).split(/\s+/).filter((x) => x);
  const out = { labels: await p.locator(box + ' select[data-order] option').allInnerTexts(), first: await ids() };
  const more = p.locator(box + ' button.more');
  out.moreShown = await more.isVisible();
  await more.click(); await p.waitForFunction((n) => document.querySelector('[data-show="parcel_ids_paged"] .rows').children.length === n, 2);
  out.second = await ids();
  for (let n = 3; await more.isVisible() && n < 40; n++) { await more.click(); await p.waitForFunction((k) => document.querySelector('[data-show="parcel_ids_paged"] .rows').children.length === k, n); }
  out.all = await ids(); out.moreAtEnd = await more.isVisible();
  await p.locator(box + ' select[data-order]').selectOption('fee.desc');
  await p.waitForFunction((want) => document.querySelector('[data-show="parcel_ids_paged"] .rows').innerText.includes(want), process.argv[2]);
  out.down = await ids(); out.moreAfterOrder = await more.isVisible();
  console.log(JSON.stringify(out));
  await b.close();
})().catch(async (e) => { console.error(e); process.exit(1); });
"#;
    let r = Command::new("node").arg("-e").arg(script).arg(s.base()).arg(up.last().unwrap()).env("NODE_PATH", &root_g).output().unwrap();
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 2000));
    let out: Value = serde_json::from_str(String::from_utf8_lossy(&r.stdout).trim().lines().last().unwrap()).unwrap();
    assert_eq!(out["labels"], json!(["fee 小さい順", "fee 大きい順"]));
    // 最初は 1 件（page 1）。「続きを読む」を押すたびに 1 件ずつ増え、最後まで読むとボタンが消える
    assert!(out["first"] == json!(&up[..1]) && out["moreShown"] == json!(true) && out["second"] == json!(&up[..2]), "{}", out);
    assert!(out["all"] == json!(up) && out["moreAtEnd"] == json!(false), "{}", out);
    // 並びを変えると、最初のページから読み直す
    assert!(out["down"] == json!(&up[up.len() - 1..]) && out["moreAfterOrder"] == json!(true), "{}", out);
}
