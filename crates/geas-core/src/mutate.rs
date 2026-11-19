// SPDX-License-Identifier: GPL-3.0-or-later
//! 仕様の変異検査：約束をわざと 1 つ崩した版を作り、検査器が止めるかを見る（Python 版 mutate.py の移し）。
use crate::check::{ERROR, QUESTION};
use crate::parser::parse;
use regex::Regex;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

pub const KILLED: &str = "検出";
pub const SURVIVED: &str = "生き残り";
pub const INVALID: &str = "対象外";
pub const KILLED_EX: &str = "具体例で検出";

#[derive(Clone, Debug)]
pub struct Mutant {
    pub line: usize,
    pub kind: String,
    pub before: String,
    pub after: Option<String>,
    pub result: String,
    pub why: String,
}

fn re(c: &'static OnceLock<Regex>, p: &str) -> &'static Regex {
    c.get_or_init(|| Regex::new(p).unwrap())
}

/// Python の str.splitlines()（改行は \n \r\n \r など。末尾の改行で空行は増えない）
pub fn splitlines(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\n' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}' => out.push(std::mem::take(&mut cur)),
            '\r' => {
                if it.peek() == Some(&'\n') {
                    it.next();
                }
                out.push(std::mem::take(&mut cur));
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn big(s: &str) -> Option<num_bigint::BigInt> {
    s.parse().ok()
}

pub fn mutants_of(text: &str, only: Option<&std::collections::BTreeSet<usize>>) -> Vec<Mutant> {
    static GUARD: OnceLock<Regex> = OnceLock::new();
    static ELSE: OnceLock<Regex> = OnceLock::new();
    static THRESHOLD: OnceLock<Regex> = OnceLock::new();
    static TRANSITION: OnceLock<Regex> = OnceLock::new();
    static LTYPE_RANGE: OnceLock<Regex> = OnceLock::new();
    static LTYPE_MAX: OnceLock<Regex> = OnceLock::new();
    static REQUIRED: OnceLock<Regex> = OnceLock::new();
    let guard = re(&GUARD, r"^(\s*)if .+ \{ return [^{}]*\}\s*(//.*)?$");
    let else_ = re(&ELSE, r"\belse\b");
    let threshold = re(&THRESHOLD, r"^(\s*if [\w.]+ (?:>|>=) )(\d+)( \{ return .*)$");
    let transition = re(&TRANSITION, r"^\s*transition .+$");
    let lrange = re(&LTYPE_RANGE, r"^(\s*ltype \w+ = (?:int|money\([\w]+\)) -?\d+\.\.)(\d+)(.*)$");
    let lmax = re(&LTYPE_MAX, r"^(\s*ltype \w+ = .* max )(\d+)(.*)$");
    let required = re(&REQUIRED, r"^(.*\w+: [^,/\n]*?) required\b(.*)$");
    let mut out = vec![];
    for (i, line) in splitlines(text).iter().enumerate() {
        let i = i + 1;
        if let Some(o) = only {
            if !o.contains(&i) {
                continue;
            }
        }
        if line.trim_start().starts_with("//") {
            continue;
        }
        let mk = |kind: &str, after: Option<String>| Mutant { line: i, kind: kind.into(), before: line.clone(), after, result: String::new(), why: String::new() };
        if let Some(m) = threshold.captures(line) {
            if let Some(n) = big(&m[2]) {
                out.push(mk("上限の確認を緩める", Some(format!("{}{}{}", &m[1], n * 10 + 1000, &m[3]))));
            }
        }
        if let Some(m) = lrange.captures(line) {
            if let Some(n) = big(&m[2]) {
                if n >= num_bigint::BigInt::from(10) {
                    out.push(mk("範囲の上限を狭める", Some(format!("{}{}{}", &m[1], n / 10, &m[3]))));
                }
            }
        }
        if let Some(m) = lmax.captures(line) {
            if let Some(n) = big(&m[2]) {
                out.push(mk("要素数の上限を広げる", Some(format!("{}{}{}", &m[1], n * 10, &m[3]))));
            }
        }
        let is_guard = guard.captures(line).map_or(false, |m| {
            let start = m.get(1).unwrap().end() + 3;
            !else_.is_match(&line[start..])
        });
        if is_guard {
            out.push(mk("確認を消す", None));
        } else if transition.is_match(line) {
            out.push(mk("遷移の宣言を消す", None));
        } else if required.is_match(line) && !line.trim_start().starts_with("input ") {
            out.push(mk("required を外す", Some(required.replace(line, "$1$2").into_owned())));
        }
    }
    out
}

pub fn apply(text: &str, mu: &Mutant) -> String {
    let mut lines = splitlines(text);
    match &mu.after {
        None => {
            lines.remove(mu.line - 1);
        }
        Some(a) => lines[mu.line - 1] = a.clone(),
    }
    lines.join("\n") + "\n"
}

/// new のうち、old から変わった行（1 始まり）
pub fn changed_lines(old: &str, new: &str) -> std::collections::BTreeSet<usize> {
    crate::difflib::changed_new_lines(&splitlines(old), &splitlines(new))
}

pub fn read_git(code: &Path, rev: &str) -> Result<String, String> {
    if rev.is_empty() || rev.starts_with('-') {
        return Err(format!("前の版の指定が正しくありません（{}）。HEAD~1 やコミットの ID で書いてください", crate::pyrepr::repr_str(rev)));
    }
    let code = std::fs::canonicalize(code).map_err(|e| e.to_string())?;
    let parent = code.parent().unwrap().to_path_buf();
    let top = Command::new("git").arg("-C").arg(&parent).args(["rev-parse", "--show-toplevel"]).output().map_err(|e| e.to_string())?;
    if !top.status.success() {
        return Err(format!("Command 'git rev-parse --show-toplevel' returned non-zero exit status {}.", top.status.code().unwrap_or(1)));
    }
    let top = String::from_utf8_lossy(&top.stdout).trim().to_string();
    let top = std::fs::canonicalize(&top).map_err(|e| e.to_string())?;
    let rel = code.strip_prefix(&top).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
    let old = Command::new("git").arg("-C").arg(&top).arg("show").arg(format!("{}:{}", rev, rel)).output().map_err(|e| e.to_string())?;
    if !old.status.success() {
        return Err(format!("前の版を読めません（{}:{}）: {}", rev, rel, String::from_utf8_lossy(&old.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&old.stdout).into_owned())
}

/// 検査器が止めたか（エラーか問いが出た）と、その最初の指摘。構文エラーは Err のまま上へ
pub fn stopped(text: &str, path: &str) -> Result<(bool, String), crate::error::Error> {
    let mut prog = parse(text, Some(Path::new(path)))?;
    let ck = crate::check::defs::check(&mut prog, true);
    let bad: Vec<_> = ck.diags.iter().filter(|d| d.level == ERROR || d.level == QUESTION).collect();
    Ok((!bad.is_empty(), bad.first().map(|d| d.to_string()).unwrap_or_default()))
}

pub fn run(path: &str, base_text: Option<&str>) -> Result<(Vec<Mutant>, Option<String>), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let (broken, why) = stopped(&text, path).map_err(|e| e.to_string())?;
    if broken {
        return Ok((vec![], Some(format!("元の版が検査を通らないので、変異検査は始めません: {}", why))));
    }
    let only = base_text.map(|b| changed_lines(b, &text));
    let mut ms = mutants_of(&text, only.as_ref());
    for mu in ms.iter_mut() {
        match stopped(&apply(&text, mu), path) {
            Ok((killed, why)) => {
                mu.why = why;
                mu.result = if killed { KILLED } else { SURVIVED }.into();
            }
            Err(e) => {
                mu.result = INVALID.into();
                mu.why = format!("崩した版が構文として成り立たない: {}", e);
            }
        }
    }
    Ok((ms, None))
}

/// 生き残りを、計画書の具体例を動かして確かめる（Rust をビルドして動かすので遅い）
pub fn run_examples(path: &str, ms: &mut [Mutant], plan_text: &str) -> Result<Option<String>, String> {
    use crate::examples::{compare_examples, run_crate, runner_source};
    use crate::plan::{read_plan, UNSPEC};
    let plan = read_plan(plan_text);
    let rows: Vec<crate::plan::Row> = plan.get("具体例").cloned().unwrap_or_default();
    if rows.is_empty() {
        return Ok(Some("計画書に具体例がないので、具体例での確認はしません".into()));
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let target = std::env::temp_dir().join(format!("geas-mut-target-{}", std::process::id()));
    std::env::set_var("CARGO_TARGET_DIR", &target);
    let mismatches = |src: &str| -> Result<Vec<crate::plan::Finding>, String> {
        let mut prog = parse(src, Some(Path::new(path))).map_err(|e| e.to_string())?;
        let ck = crate::check::defs::check(&mut prog, true);
        let (code, problems) = runner_source(&ck, &rows);
        let tmp = std::env::temp_dir().join(format!("geas-mut-{}-{}", std::process::id(), src.len()));
        let _ = std::fs::remove_dir_all(&tmp);
        let r = (|| -> Result<Vec<crate::plan::Finding>, String> {
            crate::crategen::write_crate(&ck, &crate::rustgen::generate(&ck), &code, &tmp, false)?;
            let got = run_crate(&tmp, &|s| ck.p.show_names(s))?;
            let mut all = problems;
            all.extend(compare_examples(&rows, &got));
            Ok(all.into_iter().filter(|f| f.kind != UNSPEC).collect())
        })();
        let _ = std::fs::remove_dir_all(&tmp);
        r
    };
    let result = (|| -> Result<Option<String>, String> {
        let base = match mismatches(&text) {
            Ok(b) => b,
            Err(e) => return Ok(Some(format!("元の版の具体例を動かせないので、具体例での確認は始めません: {}", e))),
        };
        if let Some(f) = base.first() {
            return Ok(Some(format!("元の版で、計画書の具体例が合っていません（{}）。先に直してください", f.where_)));
        }
        for mu in ms.iter_mut() {
            if mu.result != SURVIVED {
                continue;
            }
            match mismatches(&apply(&text, mu)) {
                Err(e) => mu.why = format!("具体例を動かせなかった: {}", e.chars().take(120).collect::<String>()),
                Ok(found) => {
                    if let Some(f) = found.first() {
                        mu.result = KILLED_EX.into();
                        mu.why = format!("{}: {}", f.where_, f.msg);
                    }
                }
            }
        }
        Ok(None)
    })();
    std::env::remove_var("CARGO_TARGET_DIR");
    let _ = std::fs::remove_dir_all(&target);
    result
}

pub fn report(ms: &[Mutant], name: &str, scope: &str) -> String {
    let n = |k: &str| ms.iter().filter(|m| m.result == k).count();
    let ex_note = if n(KILLED_EX) > 0 { format!("、計画書の具体例が止めたもの（{}）{}", KILLED_EX, n(KILLED_EX)) } else { String::new() };
    let mut out: Vec<String> = vec![
        format!("# 変異検査（{}）", name),
        String::new(),
        format!("崩し方の範囲：{}。崩した版 {} 件のうち、検査器が止めたもの（{}）{}{}、止めなかったもの（{}）{}、構文として成り立たず試せなかったもの（{}）{}。", scope, ms.len(), KILLED, n(KILLED), ex_note, SURVIVED, n(SURVIVED), INVALID, n(INVALID)),
        String::new(),
    ];
    let ex: Vec<&Mutant> = ms.iter().filter(|m| m.result == KILLED_EX).collect();
    if !ex.is_empty() {
        out.extend(["## 具体例が止めたもの（検査器は何も言わなかったが、計画書の具体例が食い違った）", "", "検査器では守られていませんが、具体例のテストが守っています。", "", "| 行 | 崩し方 | 元の行 | 食い違った具体例 |", "|---|---|---|---|"].map(String::from));
        out.extend(ex.iter().map(|m| format!("| {} | {} | `{}` | {} |", m.line, m.kind, m.before.trim(), m.why.replace('|', "/"))));
        out.push(String::new());
    }
    let live: Vec<&Mutant> = ms.iter().filter(|m| m.result == SURVIVED).collect();
    if !live.is_empty() {
        out.extend([
            "## 生き残り（検査器が何も言わなかった崩し方）",
            "",
            "異常とは限りません。その約束は、検査器ではなく、具体例のテストか人の目で守られています。元から冗長な確認なら、このままで構いません。守るべき約束なら、仕様（不変条件・遷移）か具体例に足してください。",
            "",
            "| 行 | 崩し方 | 元の行 |",
            "|---|---|---|",
        ].map(String::from));
        out.extend(live.iter().map(|m| format!("| {} | {} | `{}` |", m.line, m.kind, m.before.trim())));
    } else {
        out.push(format!("生き残りはありません（崩した版は、すべて検査器{}が止めました）。", if !ex.is_empty() { "か具体例" } else { "" }));
    }
    out.join("\n") + "\n"
}
