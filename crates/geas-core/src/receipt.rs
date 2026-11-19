// SPDX-License-Identifier: GPL-3.0-or-later
//! 変更ごとのレシート：前の版と比べて、「信頼に関わる変化」だけを抜き出す（Python 版 receipt.py の移し）。
use crate::check::{Checker, ERROR, QUESTION};
use crate::plan::Row;
use crate::pynum::escape;
use crate::rest::{INTERNAL, OPEN};
use crate::review::{lead, row_text, table};
use crate::spec::{build, level_counts, Spec, LEVELS};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub struct Receipt {
    pub label: String,
    pub weakened: Vec<(String, String, String, String)>,
    pub added_weak: Vec<(String, String, String)>,
    pub strengthened: Vec<(String, String, String, String)>,
    pub before: Vec<(String, usize)>,
    pub after: Vec<(String, usize)>,
    pub survivors: Vec<(usize, String, String)>,
    pub mutation_note: String,
    pub errors: usize,
    pub questions: usize,
    pub impact: Vec<(String, Vec<String>)>,
}

fn s(v: &Value, i: usize) -> String {
    match &v[i] {
        Value::String(x) => x.clone(),
        o => o.to_string(),
    }
}

fn key_of(row: &Value) -> String {
    let (target, content) = (s(row, 1), s(row, 2));
    if target == "不変条件" {
        content
    } else {
        format!("{}｜{}", target, content.split('：').next().unwrap_or(""))
    }
}

fn arr<'a>(spec: &'a Spec, k: &str) -> &'a [Value] {
    spec.get(k).and_then(|v| v.as_array()).map(|a| a.as_slice()).unwrap_or(&[])
}

fn rank(l: &str) -> usize {
    LEVELS.iter().position(|x| *x == l).unwrap_or(0)
}

fn weak(l: &str) -> bool {
    LEVELS[2..].contains(&l)
}

fn impact_of(new: &Spec, old: &Spec, plan_rows: Option<&[Row]>) -> Vec<(String, Vec<String>)> {
    let rows = |spec: &Spec, key: &str| -> BTreeMap<String, String> {
        arr(spec, key)
            .iter()
            .map(|x| {
                let k = x.get(key).map(|v| v.as_str().map(|t| t.to_string()).unwrap_or_else(|| v.to_string())).unwrap_or_default();
                (k, row_text(x))
            })
            .collect()
    };
    let mut deeds: BTreeSet<String> = BTreeSet::new();
    let mut entities: BTreeSet<String> = BTreeSet::new();
    for key in ["処理", "問い合わせ"] {
        let (a, b) = (rows(old, key), rows(new, key));
        let all: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
        for k in all {
            if a.get(k) != b.get(k) {
                deeds.insert(k.clone());
            }
        }
    }
    for key in ["データ", "状態", "遷移", "拒否", "削除", "不変条件"] {
        let txt = |spec: &Spec| -> Vec<String> { arr(spec, key).iter().map(|x| if let Value::String(t) = x { t.clone() } else { row_text(x) }).collect() };
        let (a, b) = (txt(old), txt(new));
        for x in b.iter().chain(a.iter()) {
            if !a.contains(x) || !b.contains(x) {
                entities.insert(x.split("　／　").next().unwrap_or("").split(':').next().unwrap_or("").trim().to_string());
            }
        }
    }
    let list = |v: &Value| -> Vec<String> { v.as_array().map(|a| a.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default() };
    for d in arr(new, "処理").iter().chain(arr(new, "問い合わせ").iter()) {
        let name = d.get("処理").or_else(|| d.get("問い合わせ")).and_then(|v| v.as_str()).unwrap_or("").to_string();
        if list(&d["読む"]).iter().chain(list(&d["書く"]).iter()).any(|r| entities.contains(r.split('.').next().unwrap_or(""))) {
            deeds.insert(name);
        }
    }
    let routes: BTreeSet<String> = arr(new, "ルート").iter().filter(|r| deeds.contains(&s(r, 3))).map(|r| format!("{} {}", s(r, 0), s(r, 1))).collect();
    let mut screens: BTreeSet<String> = arr(new, "画面遷移").iter().filter(|x| deeds.contains(&s(x, 1))).map(|x| s(x, 0)).collect();
    for x in arr(new, "画面") {
        if s(x, 1).split(',').any(|q| deeds.contains(q.trim())) {
            screens.insert(s(x, 0));
        }
    }
    let mut ucs: Vec<String> = vec![];
    for r in plan_rows.unwrap_or(&[]) {
        let g = |k: &str| r.get(k).map(|v| v.trim().to_string()).unwrap_or_default();
        if deeds.contains(&g("操作")) || screens.contains(&g("画面")) {
            let u = r.get("ユースケース").cloned().unwrap_or_default();
            if !ucs.contains(&u) {
                ucs.push(u);
            }
        }
    }
    vec![
        ("処理".into(), deeds.into_iter().collect()),
        ("データ".into(), entities.into_iter().filter(|e| !e.is_empty()).collect()),
        ("ルート".into(), routes.into_iter().collect()),
        ("画面".into(), screens.into_iter().collect()),
        ("ユースケース".into(), ucs),
    ]
}

pub fn build_receipt(ck: &Checker, base_ck: &Checker, code_path: &str, base_text: Option<&str>, label: &str, plan_rows: Option<&[Row]>) -> Receipt {
    let (new_spec, old_spec) = (build(ck), build(base_ck));
    let (new, old) = (arr(&new_spec, "保証の強さ"), arr(&old_spec, "保証の強さ"));
    let old_by: BTreeMap<String, &Value> = old.iter().map(|r| (key_of(r), r)).collect();
    let new_keys: BTreeSet<String> = new.iter().map(key_of).collect();
    let mut r = Receipt { label: label.into(), before: level_counts(old), after: level_counts(new), ..Default::default() };
    for row in new {
        match old_by.get(&key_of(row)) {
            None => {
                if weak(&s(row, 0)) {
                    r.added_weak.push((s(row, 0), s(row, 1), s(row, 2)));
                }
            }
            Some(prev) => {
                if rank(&s(row, 0)) > rank(&s(prev, 0)) {
                    r.weakened.push((s(row, 1), s(prev, 0), s(row, 0), s(row, 2)));
                } else if rank(&s(row, 0)) < rank(&s(prev, 0)) {
                    r.strengthened.push((s(row, 1), s(prev, 0), s(row, 0), s(row, 2)));
                }
            }
        }
    }
    for p in old {
        if !new_keys.contains(&key_of(p)) && weak(&s(p, 0)) {
            r.strengthened.push((s(p, 1), s(p, 0), "（無くなった）".into(), s(p, 2)));
        }
    }
    let was_out: BTreeMap<(String, String), String> = arr(&old_spec, "ルート").iter().map(|x| ((s(x, 0), s(x, 1)), s(x, 5))).collect();
    for x in arr(&new_spec, "ルート") {
        let (m, path, now) = (s(x, 0), s(x, 1), s(x, 5));
        match was_out.get(&(m.clone(), path.clone())) {
            Some(was) if was == INTERNAL && now == OPEN => r.weakened.push((format!("URL {} {}", m, path), was.clone(), now, "流れの途中から外れ、外から直接呼べるようになりました".into())),
            Some(was) if was == OPEN && now == INTERNAL => r.strengthened.push((format!("URL {} {}", m, path), was.clone(), now, "流れの途中の処理になり、サーバーは外に出さなくなりました".into())),
            _ => {}
        }
    }
    let was_who: BTreeMap<(String, String), String> = arr(&old_spec, "ルート").iter().filter(|x| x.as_array().map_or(false, |a| a.len() > 6)).map(|x| ((s(x, 0), s(x, 1)), s(x, 6))).collect();
    for x in arr(&new_spec, "ルート") {
        let n = x.as_array().map_or(0, |a| a.len());
        let (m, path, now) = (s(x, 0), s(x, 1), s(x, n - 1));
        let Some(was) = was_who.get(&(m.clone(), path.clone())) else { continue };
        if *was == now || format!("{}{}", was, now).contains("外から呼べない") {
            continue;
        }
        let split = |t: &str| -> BTreeSet<String> { t.split('、').filter(|x| !x.is_empty()).map(|x| x.to_string()).collect() };
        let (a, b) = (split(was), split(&now));
        let wide = |t: &BTreeSet<String>| t.is_empty() || t.contains("誰でも");
        let (wa, wb) = (wide(&a), wide(&b));
        let shown = |t: &str| if t.is_empty() { "（区別なし）".to_string() } else { t.to_string() };
        if (wb && !wa) || (!wa && !wb && b.is_superset(&a) && b != a) {
            r.weakened.push((format!("URL {} {}", m, path), shown(was), shown(&now), "呼べる人が広がりました".into()));
        } else if (wa && !wb) || (!wa && !wb && a.is_superset(&b) && a != b) {
            r.strengthened.push((format!("URL {} {}", m, path), shown(was), shown(&now), "呼べる人が狭まりました".into()));
        }
    }
    r.impact = impact_of(&new_spec, &old_spec, plan_rows);
    r.errors = ck.diags.iter().filter(|d| d.level == ERROR).count();
    r.questions = ck.diags.iter().filter(|d| d.level == QUESTION).count();
    match crate::mutate::run(code_path, base_text) {
        Ok((ms, why)) => {
            r.mutation_note = why.unwrap_or_default();
            r.survivors = ms.iter().filter(|m| m.result == crate::mutate::SURVIVED).map(|m| (m.line, m.kind.clone(), m.before.trim().to_string())).collect();
        }
        Err(e) => r.mutation_note = format!("変異検査を回せませんでした: {}", e),
    }
    r
}

pub fn counts_text(cs: &[(String, usize)]) -> String {
    if cs.is_empty() {
        "なし".into()
    } else {
        cs.iter().map(|(l, n)| format!("{} {}", l, n)).collect::<Vec<_>>().join("、")
    }
}

const IMPACT_LABELS: [(&str, &str); 5] = [("処理", "処理・問い合わせ"), ("データ", "データ（項目・状態・遷移・削除・不変条件）"), ("ルート", "URL"), ("画面", "画面"), ("ユースケース", "ユースケース（計画書を渡したときだけ）")];

fn impact_lines(imp: &[(String, Vec<String>)]) -> Vec<String> {
    if imp.iter().all(|(_, v)| v.is_empty()) {
        return vec!["処理・データの変更はありません（または、仕様書に出る関係では影響が見つかりません）。".into()];
    }
    let mut out = vec!["変わった処理・データと、それにつながる URL・画面・ユースケースです。人が動きを確かめる範囲の目安で、漏れがないことまでは保証しません。".to_string(), String::new()];
    for (k, label) in IMPACT_LABELS {
        if let Some((_, v)) = imp.iter().find(|(n, _)| n == k) {
            if !v.is_empty() {
                out.push(format!("- {}：{}", label, v.join("、")));
            }
        }
    }
    out
}

pub fn to_markdown(r: &Receipt) -> String {
    let mut out: Vec<String> = vec![format!("## 信頼のレシート（比較元：{}）", r.label), String::new()];
    if !r.weakened.is_empty() || !r.added_weak.is_empty() {
        out.extend(["### 保証が弱くなったもの・増えた人の保証（最初に見てください）", "", "| 対象 | 変化 | 内容 |", "|---|---|---|"].map(String::from));
        out.extend(r.weakened.iter().map(|(t, a, b, c)| format!("| {} | {} → {} | {} |", t, a, b, c)));
        out.extend(r.added_weak.iter().map(|(lv, t, c)| format!("| {} | （新しい）→ {} | {} |", t, lv, c)));
    } else {
        out.push("保証が弱くなったもの、増えた人の保証はありません。".into());
    }
    if !r.strengthened.is_empty() {
        out.extend(["", "### 強くなった・無くなったもの", "", "| 対象 | 変化 | 内容 |", "|---|---|---|"].map(String::from));
        out.extend(r.strengthened.iter().map(|(t, a, b, c)| format!("| {} | {} → {} | {} |", t, a, b, c)));
    }
    out.extend(["".to_string(), "### 動きが変わるかもしれないところ".into(), "".into()]);
    out.extend(impact_lines(&r.impact));
    out.extend(["".to_string(), "### 保証の強さの件数".into(), "".into(), format!("- 前：{}", counts_text(&r.before)), format!("- 今：{}", counts_text(&r.after))]);
    out.extend(["".to_string(), "### 変異検査の生き残り（変わった行を崩しても、検査器が何も言わなかったもの）".into(), "".into()]);
    if !r.mutation_note.is_empty() {
        out.push(r.mutation_note.clone());
    } else if !r.survivors.is_empty() {
        out.extend(["守るべき約束なら、不変条件・遷移・具体例に足してください。元から冗長な確認なら、そのままで構いません。", "", "| 行 | 崩し方 | 元の行 |", "|---|---|---|"].map(String::from));
        out.extend(r.survivors.iter().map(|(n, k, b)| format!("| {} | {} | `{}` |", n, k, b)));
    } else {
        out.push("ありません。".into());
    }
    out.extend(["".to_string(), "### 検査の結果".into(), "".into(), format!("- エラー {} 件、問い {} 件{}", r.errors, r.questions, if r.errors == 0 && r.questions == 0 { "" } else { "（先へ進めません）" })]);
    out.join("\n") + "\n"
}

pub fn to_html(r: &Receipt) -> String {
    let mut parts: Vec<String> = vec![];
    if !r.weakened.is_empty() || !r.added_weak.is_empty() {
        let mut rows: Vec<Vec<String>> = r.weakened.iter().map(|(t, a, b, c)| vec![t.clone(), format!("{} → {}", a, b), c.clone()]).collect();
        rows.extend(r.added_weak.iter().map(|(lv, t, c)| vec![t.clone(), format!("（新しい）→ {}", lv), c.clone()]));
        parts.push(format!("<h3>保証が弱くなったもの・増えた人の保証（最初に見てください）</h3>{}", table(&["対象", "変化", "内容"], &rows)));
    } else {
        parts.push(lead("保証が弱くなったもの、増えた人の保証はありません。"));
    }
    if !r.strengthened.is_empty() {
        let rows: Vec<Vec<String>> = r.strengthened.iter().map(|(t, a, b, c)| vec![t.clone(), format!("{} → {}", a, b), c.clone()]).collect();
        parts.push(format!("<h3>強くなった・無くなったもの</h3>{}", table(&["対象", "変化", "内容"], &rows)));
    }
    parts.push(format!("<h3>動きが変わるかもしれないところ</h3>{}", impact_lines(&r.impact).iter().filter(|x| !x.is_empty()).map(|x| lead(&escape(x))).collect::<String>()));
    parts.push(format!("<h3>保証の強さの件数</h3>{}{}", lead(&format!("前：{}", escape(&counts_text(&r.before)))), lead(&format!("今：{}", escape(&counts_text(&r.after))))));
    parts.push("<h3>変異検査の生き残り</h3>".into());
    if !r.mutation_note.is_empty() {
        parts.push(lead(&escape(&r.mutation_note)));
    } else if !r.survivors.is_empty() {
        let rows: Vec<Vec<String>> = r.survivors.iter().map(|(n, k, b)| vec![n.to_string(), k.clone(), b.clone()]).collect();
        parts.push(format!("{}{}", lead("変わった行を崩しても、検査器が何も言わなかったものです。守るべき約束なら、不変条件・遷移・具体例に足してください。元から冗長な確認なら、そのままで構いません。"), table(&["行", "崩し方", "元の行"], &rows)));
    } else {
        parts.push(lead("ありません。"));
    }
    parts.push(format!("<h3>検査の結果</h3>{}", lead(&format!("エラー {} 件、問い {} 件", r.errors, r.questions))));
    parts.concat()
}
