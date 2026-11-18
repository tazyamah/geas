// SPDX-License-Identifier: GPL-3.0-or-later
//! AI が計画書から `.geas` を書く力の測定（Python 版 bench.py の移し）。AI は呼ばず、書かれたものを採点するだけ。
use crate::check::{ERROR, QUESTION};
use crate::examples::{compare_examples, run_crate, runner_source};
use crate::parser::parse;
use crate::plan::{compare_static, read_plan, Finding, EXAMPLE, UNSPEC};
use crate::spec::build;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct Score {
    pub case: String,
    pub status: String,
    pub errors: usize,
    pub questions: usize,
    pub mismatches: usize,
    pub examples: String,
    pub rounds: Option<i64>,
    pub notes: Vec<String>,
}

impl Score {
    pub fn passed(&self) -> bool {
        self.status == "通過" && self.mismatches == 0 && ["合った", "具体例なし", "実行しない"].contains(&self.examples.as_str())
    }
}

fn head(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

pub fn score_case(case: &str, plan_text: &str, app: &Path, rounds_file: &Path, run: bool) -> Score {
    let mut s = Score { case: case.into(), ..Default::default() };
    if rounds_file.exists() {
        match std::fs::read_to_string(rounds_file).ok().and_then(|t| t.trim().parse::<i64>().ok()) {
            Some(n) => s.rounds = Some(n),
            None => s.notes.push("rounds.txt が整数ではありません".into()),
        }
    }
    if !app.exists() {
        s.status = "書かれていない".into();
        return s;
    }
    let text = std::fs::read_to_string(app).unwrap_or_default();
    let mut prog = match parse(&text, Some(app)) {
        Ok(p) => p,
        Err(e) => {
            s.status = "構文エラー".into();
            s.notes.push(head(&e.to_string(), 200));
            return s;
        }
    };
    let ck = crate::check::defs::check(&mut prog, true);
    s.errors = ck.diags.iter().filter(|d| d.level == ERROR).count();
    s.questions = ck.diags.iter().filter(|d| d.level == QUESTION).count();
    if !ck.ready() {
        s.status = "検査で止まる".into();
        return s;
    }
    s.status = "通過".into();
    let plan = read_plan(plan_text);
    let mut findings = compare_static(&build(&ck), &plan);
    let rows: Vec<crate::plan::Row> = plan.get("具体例").cloned().unwrap_or_default();
    let (code, problems) = runner_source(&ck, &rows);
    findings.extend(problems);
    if rows.is_empty() {
        s.examples = "具体例なし".into();
    } else if !run {
        s.examples = "実行しない".into();
    } else {
        let tmp = std::env::temp_dir().join(format!("geas-bench-{}-{}", std::process::id(), case.len()));
        let _ = std::fs::remove_dir_all(&tmp);
        let r = (|| -> Result<Vec<Finding>, String> {
            crate::crategen::write_crate(&ck, &crate::rustgen::generate(&ck), &code, &tmp, false)?;
            let got = run_crate(&tmp, &|x| ck.p.show_names(x))?;
            Ok(compare_examples(&rows, &got))
        })();
        let _ = std::fs::remove_dir_all(&tmp);
        match r {
            Ok(bad) => {
                s.examples = if bad.is_empty() { "合った" } else { "食い違い" }.into();
                findings.extend(bad);
            }
            Err(e) => {
                s.examples = "食い違い".into();
                s.notes.push(head(&e, 200));
                findings.push(Finding { kind: EXAMPLE.into(), where_: "具体例の実行".into(), msg: "動かせません".into() });
            }
        }
    }
    s.mismatches = findings.iter().filter(|f| f.kind != UNSPEC).count();
    s
}

pub fn run_bench(cases_dir: &Path, runs_dir: &Path, run: bool) -> Vec<Score> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(cases_dir).map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.join("plan.md").exists()).collect()).unwrap_or_default();
    dirs.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    dirs.iter()
        .map(|c| {
            let name = c.file_name().unwrap().to_string_lossy().into_owned();
            score_case(&name, &std::fs::read_to_string(c.join("plan.md")).unwrap_or_default(), &runs_dir.join(&name).join("app.geas"), &runs_dir.join(&name).join("rounds.txt"), run)
        })
        .collect()
}

pub fn to_json(scores: &[Score]) -> String {
    let cases: Vec<Value> = scores.iter().map(|s| json!({"case": s.case, "status": s.status, "errors": s.errors, "questions": s.questions, "mismatches": s.mismatches, "examples": s.examples, "rounds": s.rounds, "passed": s.passed()})).collect();
    crate::crategen::dumps_indent(&json!({ "cases": cases }), 2) + "\n"
}

pub fn to_markdown(scores: &[Score], baseline: Option<&Value>) -> String {
    let n = scores.len();
    let ok = scores.iter().filter(|s| s.passed()).count();
    let rs: Vec<i64> = scores.iter().filter_map(|s| s.rounds).collect();
    let mut out: Vec<String> = vec![
        "# AI が書く力の測定".into(),
        String::new(),
        format!("ケース {} 件のうち、通ったもの {} 件（{}%）。{}", n, ok, if n > 0 { 100 * ok / n } else { 0 }, if rs.is_empty() { String::new() } else { format!(" 直した回数の平均 {:.1} 回。", rs.iter().sum::<i64>() as f64 / rs.len() as f64) }),
        String::new(),
        "| ケース | 検査 | 食い違い | 具体例 | 直した回数 | 判定 |".into(),
        "|---|---|---|---|---|---|".into(),
    ];
    for s in scores {
        let chk = format!("{}{}", s.status, if s.status == "検査で止まる" { format!("（エラー {}・問い {}）", s.errors, s.questions) } else { String::new() });
        out.push(format!(
            "| {} | {} | {} | {} | {} | {} |",
            s.case,
            chk,
            if s.status == "通過" { s.mismatches.to_string() } else { "-".into() },
            if s.examples.is_empty() { "-" } else { &s.examples },
            s.rounds.map(|r| r.to_string()).unwrap_or_default(),
            if s.passed() { "通った" } else { "通らない" }
        ));
    }
    let notes: Vec<(&String, &String)> = scores.iter().flat_map(|s| s.notes.iter().map(move |x| (&s.case, x))).collect();
    if !notes.is_empty() {
        out.extend(["".to_string(), "## 補足".into(), "".into()]);
        out.extend(notes.iter().map(|(c, x)| format!("- {}：{}", c, x)));
    }
    if let Some(b) = baseline {
        let prev: std::collections::BTreeMap<String, bool> = b["cases"].as_array().map(|a| a.iter().map(|c| (c["case"].as_str().unwrap_or("").to_string(), c["passed"].as_bool().unwrap_or(false))).collect()).unwrap_or_default();
        let j = |v: Vec<&str>| if v.is_empty() { "なし".to_string() } else { v.join("、") };
        let worse: Vec<&str> = scores.iter().filter(|s| prev.get(&s.case) == Some(&true) && !s.passed()).map(|s| s.case.as_str()).collect();
        let better: Vec<&str> = scores.iter().filter(|s| prev.get(&s.case) == Some(&false) && s.passed()).map(|s| s.case.as_str()).collect();
        let new: Vec<&str> = scores.iter().filter(|s| !prev.contains_key(&s.case)).map(|s| s.case.as_str()).collect();
        out.extend(["".to_string(), "## 前回との比較".into(), "".into(), format!("- 悪くなった（通っていたのに通らない）：{}", j(worse)), format!("- 良くなった：{}", j(better)), format!("- 新しいケース：{}", j(new))]);
    }
    out.join("\n") + "\n"
}
