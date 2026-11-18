// SPDX-License-Identifier: GPL-3.0-or-later
//! 動かす前提：アプリが実行環境に要求すること（Python 版 geas/envreq.py の移し）。
use crate::ast::*;
use indexmap::IndexSet;

pub fn name_ok(s: &str) -> bool {
    let mut cs = s.chars();
    matches!(cs.next(), Some('a'..='z')) && cs.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

pub fn ns_ok(s: &str) -> bool {
    let mut cs = s.chars();
    matches!(cs.next(), Some('A'..='Z')) && cs.all(|c| c.is_ascii_alphanumeric())
}

pub fn effect_namespaces(p: &Program) -> Vec<String> {
    let mut seen: IndexSet<String> = IndexSet::new();
    for e in &p.effects {
        seen.insert(e.name.split('.').next().unwrap().to_string());
    }
    seen.into_iter().collect()
}

/// 保存先の isolation から導く、同時に動けるサーバーの上限。process なら 1、そうでなければ導出できない（None）
pub fn derived_instance_limit(p: &Program) -> Option<i128> {
    if p.isolations.values().any(|(who, _, _, _)| who == "process") {
        Some(1)
    } else {
        None
    }
}

pub const BODY_LIMIT: &str = "16 MiB";
pub const JSON_DEPTH: i64 = 64;

/// 保存先の名前 → それを使う entity。`store` の行がない entity は標準の保存先（sqlite）で、名前は default
pub fn stores(p: &Program) -> indexmap::IndexMap<String, Vec<String>> {
    let mut out: indexmap::IndexMap<String, Vec<String>> = indexmap::IndexMap::new();
    for d in &p.data {
        out.entry(d.store.clone().unwrap_or_else(|| "default".to_string())).or_default().push(d.name.clone());
    }
    out
}

/// 保存先の種類。標準の保存先と、isolation process の保存先は、サーバーが持つ sqlite
pub fn store_kind(p: &Program, where_: &str) -> String {
    match p.isolations.get(where_) {
        Some((who, kind, _, _)) if where_ != "default" => {
            if who == "store" {
                kind.clone()
            } else {
                "sqlite".to_string()
            }
        }
        _ => "sqlite".to_string(),
    }
}

/// 照合器が読む 1 つの検査項目
#[derive(Debug, Clone)]
pub struct EnvItem {
    pub id: String,
    pub aspect: String,
    pub text: String,
    pub origin: String,
    pub how: Vec<String>,
    pub facts: Vec<String>,
    pub trust: bool,
    pub why: String,
    pub cond: serde_json::Map<String, serde_json::Value>,
}

fn item(id: &str, aspect: &str, text: &str, origin: &str, how: &[&str], facts: &[&str], trust: bool, why: &str, cond: serde_json::Map<String, serde_json::Value>) -> EnvItem {
    EnvItem {
        id: id.into(),
        aspect: aspect.into(),
        text: text.into(),
        origin: origin.into(),
        how: how.iter().map(|s| s.to_string()).collect(),
        facts: facts.iter().map(|s| s.to_string()).collect(),
        trust,
        why: why.into(),
        cond,
    }
}

fn cond1(k: &str, v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    let mut m = serde_json::Map::new();
    m.insert(k.to_string(), v);
    m
}

pub fn items(p: &Program) -> Vec<EnvItem> {
    use crate::rest::{ACTOR_HEADER, ACTOR_ID_HEADER};
    use serde_json::json;
    let mut out: Vec<EnvItem> = vec![];
    let needs = &p.needs;
    // ---- 同時に動くサーバー ----
    let (mut parts, mut origins): (Vec<String>, Vec<String>) = (vec![], vec![]);
    let procs: Vec<String> = p.isolations.iter().filter(|(_, (who, _, _, _))| who == "process").map(|(w, _)| w.clone()).collect();
    let sq: Vec<String> = stores(p).keys().cloned().collect();
    let sqlite_apart = sq.iter().any(|w| store_kind(p, w) == "sqlite" && !procs.contains(w));
    if !procs.is_empty() {
        parts.push("同時に動くサーバーは 1 台（入れ替えのときも、新旧が重ならない）".into());
        origins.push(format!("導出（保存先 {} が isolation process）", procs.join("・")));
    }
    if sqlite_apart {
        parts.push("同じ DB ファイルを使うサーバーは、すべて同じ機械の上で動く".into());
        origins.push("導出（保存先 sqlite は、同じ機械のファイルの書き込みロックで直列にする）".into());
    }
    let mine: Vec<&(String, String, NeedVal, String, i64)> = needs.iter().filter(|n| n.0 == "instances").collect();
    let num = |v: &NeedVal| match v {
        IntOrStr::Int(i) => *i,
        IntOrStr::Str(s) => s.parse().unwrap_or(0),
    };
    for n in &mine {
        parts.push(format!("同時に動くサーバーは {} 台以下", num(&n.2)));
        origins.push(format!("人が書いた（{}）", n.3));
    }
    if !parts.is_empty() {
        let mut limits: Vec<i128> = if procs.is_empty() { vec![] } else { vec![1] };
        limits.extend(mine.iter().map(|n| num(&n.2)));
        let mut cond = serde_json::Map::new();
        if let Some(m) = limits.iter().min() {
            cond.insert("at_most".into(), json!(m));
        }
        if sqlite_apart {
            cond.insert("same_host".into(), json!(true));
        }
        let why: Vec<&str> = mine.iter().map(|n| n.3.as_str()).collect();
        out.push(item("instances", "安全性", &parts.join("。"), &origins.join("／"), &["plan", "実環境"], &["台数", "更新のときの同時最大数", "保存先の実体"], false, &why.join("／"), cond));
    }
    // ---- 保存先の永続 ----
    for (where_, ents) in stores(p) {
        let kind = store_kind(p, &where_);
        let what = if kind == "sqlite" { "DB ファイル".to_string() } else { format!("保存先 {} のデータ", kind) };
        out.push(item(
            &format!("durable.{}", where_),
            "安全性",
            &format!("{}が、再起動や入れ替えで消えない領域にある", what),
            &format!("導出（{} が保存先 {}（{}）に入る）", ents.join("・"), where_, kind),
            &["plan", "実環境"],
            &["保存先の実体", "領域の種類"],
            false,
            "",
            cond1("persistent", json!(true)),
        ));
    }
    // ---- 外へ届くこと ----
    let host_re = regex::Regex::new(r"^https://([^/:]+)").unwrap();
    for ns in effect_namespaces(p) {
        let ops: Vec<&str> = p.effects.iter().filter(|e| e.name.split('.').next() == Some(ns.as_str())).map(|e| e.name.as_str()).collect();
        let mut hosts: Vec<String> = p
            .effects
            .iter()
            .filter(|e| e.name.split('.').next() == Some(ns.as_str()))
            .filter_map(|e| e.http.as_ref())
            .filter_map(|h| host_re.captures(&h.url).map(|c| c[1].to_string()))
            .collect();
        hosts.sort();
        hosts.dedup();
        let where_ = if !hosts.is_empty() {
            format!("外部 {} の宛先（{}）に、アプリから届く（宣言した先だけに出る）", ns, hosts.join("・"))
        } else {
            format!("外部 {} の宛先に、サーバーから届く（宛先は環境から読む。書いていない宛先には出ない前提）", ns)
        };
        out.push(item(&format!("reach.{}", ns), "到達性", &where_, &format!("導出（外部作用 {}）", ops.join("・")), &["実環境"], &["通信の許可", "宛先の設定"], false, "", cond1("egress_https", json!(true))));
    }
    // ---- 秘密 ----
    for (kind, target, _, why, _) in needs {
        if kind == "secret" {
            out.push(item(&format!("secret.{}", target), "機密", &format!("シークレット {} が環境にあり、plan に平文の値が書かれていない", target), &format!("人が書いた（{}）", why), &["plan", "実環境"], &["シークレットの有無", "plan の値"], false, why, cond1("secret", json!(target))));
        }
    }
    // ---- ファイル操作 ----
    for e in &p.effects {
        if let Some(f) = &e.file {
            let verb = match f.op.as_str() {
                "write" => "新規に書ける",
                "overwrite" => "書ける",
                "read" => "読める",
                _ => "消せる",
            };
            out.push(item(
                &format!("file.{}", e.name.replace(' ', "")),
                "到達性",
                &format!("外部作用 {} が触る場所（実行時に入力されるパス（{}）の下の {}）を、アプリの利用者の権限で{}", e.name, f.dir, f.rest, verb),
                &format!("導出（外部作用 {} の over file）", e.name),
                &["実環境"],
                &["フォルダの実体", "権限"],
                false,
                "",
                cond1("file_access", json!(f.op)),
            ));
        }
    }
    // ---- 環境変数・データ用フォルダ ----
    for (kind, target, _, why, _) in needs {
        if kind == "env" {
            out.push(item(&format!("env.{}", target), "環境", &format!("環境変数 {} が設定されている", target), &format!("人が書いた（{}）", why), &["実環境"], &["環境変数の有無"], false, why, cond1("env", json!(target))));
        }
    }
    for (os_name, path, _) in &p.data_folders {
        let osn = if os_name.is_empty() { "relative" } else { os_name.as_str() };
        let osj = if os_name.is_empty() { "相対パス" } else { os_name.as_str() };
        out.push(item(&format!("datafolder.{}", osn), "安全性", &format!("データ用フォルダ（{}：{}）が、書き込めて、再起動や入れ替えで消えない領域にある", osj, path), "人が書いた（data_folder）", &["実環境"], &["フォルダの実体", "書き込みの権限"], false, "", cond1("writable_folder", json!(path))));
    }
    // ---- 公開 ----
    for (kind, _, value, why, _) in needs {
        if kind == "exposure" {
            let n = p.resources.len();
            let v = match value {
                IntOrStr::Str(s) => s.clone(),
                IntOrStr::Int(i) => i.to_string(),
            };
            let text = if v == "public" { format!("インターネットに公開する口は、リソース {} 件の経路を出すものだけ", n) } else { "インターネットから届く口が無い".to_string() };
            out.push(item("exposure", "到達性", &text, &format!("人が書いた（{}）", why), &["plan", "実環境"], &["公開ポート", "公開の許可"], false, why, cond1("internet", json!(v))));
        }
    }
    // ---- 役割の確かめ（前に置くプロキシが担う） ----
    if !p.actors.is_empty() {
        let names = p.actors.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join("・");
        let mut text = format!("サーバーに届く要求は、すべて前に置くプロキシを通る。プロキシは本人を確かめ、外から来たヘッダ {} を捨てて、確かめた役割の名前（{}。複数はカンマ区切り）を載せる", ACTOR_HEADER, names);
        if p.scopes.iter().any(|s| !s.caller.is_empty()) {
            let mut seen: indexmap::IndexSet<String> = indexmap::IndexSet::new();
            for s in p.scopes.iter().filter(|s| !s.caller.is_empty()) {
                seen.insert(format!("{} の {}", s.data, s.caller));
            }
            text += &format!("。ヘッダ {} も同じく捨てて、確かめた本人の ID（{} と比べる値）を載せる", ACTOR_ID_HEADER, seen.into_iter().collect::<Vec<_>>().join("・"));
        }
        out.push(item("actor", "安全性", &text, &format!("導出（役割 {} の宣言。サーバーはこのヘッダの値だけで役割を決め、本人は確かめない）", names), &["人の保証"], &[], true, "外から確かめられない", serde_json::Map::new()));
    }
    // ---- 前段の約束（外から確かめられない） ----
    if !p.resources.is_empty() {
        out.push(item(
            "limits",
            "資源",
            "前段（LB・プロキシ）が、遅い読み込みを切る時間切れを持つ",
            &format!("導出（生成サーバーは本文が {} を超えると 413 で断り、JSON の入れ子は {} 段まで。読み込みの時間切れだけは部品に口がなく、前段に任せる。前段の本文上限は、低くても無くても、サーバーが守るので要求しない）", BODY_LIMIT, JSON_DEPTH),
            &["人の保証"],
            &[],
            true,
            "外から確かめられない",
            serde_json::Map::new(),
        ));
    }
    out
}

/// 仕様書の表の行：(番号, 観点, 要求, 由来, 確かめ方)
pub fn rows(p: &Program) -> Vec<[String; 5]> {
    items(p).into_iter().map(|x| [x.id, x.aspect, x.text, x.origin, x.how.join("・")]).collect()
}

fn item_json(x: &EnvItem) -> serde_json::Value {
    use serde_json::json;
    json!({"id": x.id, "観点": x.aspect, "要求": x.text, "由来": x.origin, "確かめ方": x.how, "環境の事実": x.facts, "人の保証": x.trust, "理由": x.why, "条件": x.cond})
}

pub fn to_json(p: &Program) -> String {
    let v = serde_json::json!({"items": items(p).iter().map(item_json).collect::<Vec<_>>()});
    crate::crategen::dumps_indent(&v, 2) + "\n"
}

pub fn to_text(p: &Program) -> String {
    let mut lines: Vec<String> = vec![];
    for x in items(p) {
        lines.push(format!("{}（{}）{}", x.id, x.aspect, x.text));
        lines.push(format!("    由来：{}　確かめ方：{}", x.origin, x.how.join("・")));
    }
    if lines.is_empty() { String::new() } else { lines.join("\n") + "\n" }
}
