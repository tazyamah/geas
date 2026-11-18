// SPDX-License-Identifier: GPL-3.0-or-later
//! ①の具体例を、コンパイルしたコードで実際に動かして確かめる（Python 版 examples.py の移し）。
//! 具体例の表から Rust の実行用 bin を生成し、cargo で動かして、結果・変化・外部作用を①の期待と比べる。
use crate::ast::Type;
use crate::check::{phys, Checker};
use crate::plan::{n, split_top, Finding, Row, EXAMPLE, UNSPEC};
use crate::rustgen::{deed_inputs_proc, deed_inputs_query, rust_str};
use indexmap::IndexMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

type E = String;

/// 半角の数字だけの整数か
fn is_int(t: &str) -> bool {
    let b = t.strip_prefix('-').unwrap_or(t);
    !b.is_empty() && b.chars().all(|c| c.is_ascii_digit())
}

fn lit(t: &Type, text: &str, ck: &Checker) -> Result<String, E> {
    let text = text.trim();
    let k = t.kind.as_str();
    Ok(match k {
        "ID" | "参照" | "文字列" => format!("{}.to_string()", rust_str(text)),
        "真偽" => {
            if text != "true" && text != "false" {
                return Err(format!("真偽は true か false で書きます（「{}」）", text));
            }
            text.to_string()
        }
        "バイト列" => {
            let h = text.strip_prefix("0x");
            match h {
                Some(h) if h.chars().count() % 2 == 0 && h.chars().all(|c| c.is_ascii_hexdigit()) => {
                    let cs: Vec<char> = h.chars().collect();
                    format!("vec![{}]", cs.chunks(2).map(|c| format!("0x{}u8", c.iter().collect::<String>())).collect::<Vec<_>>().join(", "))
                }
                _ => return Err(format!("バイト列は 0x に続く16進数で書きます（「{}」）", text)),
            }
        }
        "日時" => {
            if !is_int(text) {
                return Err(format!("日時は秒の数で書きます（「{}」）", text));
            }
            format!("{}i64", text)
        }
        "集合" => {
            if !(text.starts_with('{') && text.ends_with('}')) {
                return Err(format!("集合は {{要素, …}} で書きます（「{}」）", text));
            }
            let inner = &text[1..text.len() - 1];
            let items = split_top(inner, ',');
            format!("BTreeSet::from([{}])", items.iter().map(|x| format!("{}.to_string()", rust_str(x))).collect::<Vec<_>>().join(", "))
        }
        "整数" | "金額" => {
            if !is_int(text) {
                return Err(format!("数のはずが「{}」です", text));
            }
            format!("{}{}", text, phys(t))
        }
        "割合" => {
            let (a, b) = match text.split_once('/') {
                Some((a, b)) => (a, b),
                None => (text, ""),
            };
            let (a, b) = (a.trim(), b.trim());
            if !(is_int(a) && is_int(b) && !a.starts_with('-') && !b.starts_with('-')) {
                return Err(format!("割合は 分子/分母 で書きます（「{}」）", text));
            }
            format!("Ratio {{ num: {}, den: {} }}", a, b)
        }
        "名前" => {
            let tg = t.target.clone().unwrap_or_default();
            let states: Vec<String> = ck.p.states.iter().filter(|s| s.name == tg).flat_map(|s| std::iter::once(s.initial.clone()).chain(s.middle.iter().cloned()).chain(s.final_.iter().cloned())).collect();
            let text = ck.p.resolve(text, &states);
            if !states.contains(&text) {
                return Err(format!("状態 {} に「{}」はありません", tg, text));
            }
            format!("{}::{}", tg, text)
        }
        "写像" => {
            if text == "{}" || text.is_empty() {
                return Ok("BTreeMap::new()".into());
            }
            if !(text.starts_with('{') && text.ends_with('}')) {
                return Err(format!("写像は {{キー:値, …}} で書きます（「{}」）", text));
            }
            let mut pairs = vec![];
            for item in split_top(&text[1..text.len() - 1], ',') {
                let (kk, vv) = match item.split_once(':') {
                    Some((a, b)) => (a, b),
                    None => (item.as_str(), ""),
                };
                pairs.push(format!("({}.to_string(), {})", rust_str(kk.trim()), lit(t.value.as_ref().unwrap(), vv, ck)?));
            }
            format!("BTreeMap::from([{}])", pairs.join(", "))
        }
        _ => return Err(format!("型 {} の値は書けません", k)),
    })
}

fn kv(text: &str) -> Result<IndexMap<String, String>, E> {
    let mut out = IndexMap::new();
    for item in split_top(text, ',') {
        match item.split_once('=') {
            None => return Err(format!("「名前=値」の形のはずが「{}」です", item)),
            Some((k, v)) => {
                out.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }
    Ok(out)
}

fn example_code(ck: &Checker, row: &Row) -> Result<String, E> {
    let g = |k: &str| row.get(k).cloned().unwrap_or_default();
    let names: Vec<String> = ck.queries.keys().chain(ck.procs.keys()).cloned().collect();
    let pname = ck.p.resolve(&g("処理"), &names);
    let q = ck.queries.get(&pname).copied();
    let proc = ck.procs.get(&pname).copied();
    let (pn, refs) = match (q, proc) {
        (Some(q), _) => (q.name.clone(), &q.refs),
        (None, Some(p)) => (p.name.clone(), &p.refs),
        _ => return Err(format!("処理「{}」がコードにありません", g("処理"))),
    };
    let info = if q.is_none() { ck.infos.get(&pn) } else { None };
    let mut l: Vec<String> = vec!["    {".into(), "        let mut Store = Store::default();".into()];
    let mut ctx: IndexMap<String, String> = IndexMap::new();
    let dnames: Vec<String> = ck.data.keys().cloned().collect();
    for item in split_top(&g("前提"), ';') {
        let head = item.split('=').next().unwrap_or("");
        if head.contains('(') {
            let (dn, rest) = item.split_once('(').unwrap();
            let dname = ck.p.resolve(dn, &dnames);
            let Some(d) = ck.data.get(&dname) else {
                return Err(format!("前提のデータ「{}」がコードにありません", dname));
            };
            let args = split_top(rest.trim_end().trim_end_matches(')'), ',');
            let key = ck.key_field(&dname).unwrap();
            let fnames: Vec<String> = d.fields.iter().map(|f| f.name.clone()).collect();
            let mut vals: IndexMap<String, String> = IndexMap::new();
            vals.insert(key.name.clone(), args.first().cloned().unwrap_or_default());
            if args.len() > 1 {
                for (k, v) in kv(&args[1..].join(","))? {
                    vals.insert(ck.p.resolve(&k, &fnames), v);
                }
            }
            let mut fields = vec![];
            for f in &d.fields {
                if f.r#type.optional && vals.get(&f.name).map(|s| s.trim()).unwrap_or("null") == "null" {
                    fields.push(format!("{}: None", f.name));
                    continue;
                }
                let Some(vv) = vals.get(&f.name) else {
                    return Err(format!("前提 {}({}) に項目「{}」がありません", dname, args[0], f.name));
                };
                let v = lit(&f.r#type, vv, ck)?;
                fields.push(if f.r#type.optional { format!("{}: Some({})", f.name, v) } else { format!("{}: {}", f.name, v) });
            }
            let mut extra: Vec<&String> = vals.keys().filter(|k| !fnames.contains(k)).collect();
            extra.sort();
            if let Some(x) = extra.first() {
                return Err(format!("データ {} に項目「{}」がありません", dname, x));
            }
            l.push(format!("        Store.{}.insert({}.to_string(), {} {{ {} }});", dname, rust_str(&args[0]), dname, fields.join(", ")));
        } else if !item.is_empty() {
            for (k, v) in kv(&item)? {
                ctx.insert(k, v);
            }
        }
    }
    let rnames: Vec<String> = refs.iter().map(|(n, _)| n.clone()).collect();
    let ctx: IndexMap<String, String> = ctx.into_iter().map(|(k, v)| (ck.p.resolve(&k, &rnames), v)).collect();
    let mut fields = vec![];
    for (name, t) in refs {
        let Some(v) = ctx.get(name) else {
            return Err(format!("前提に 参照「{}」がありません", name));
        };
        fields.push(format!("{}: {}", name, lit(t, v, ck)?));
    }
    if let Some(info) = info {
        let mut ids: Vec<&String> = info.new_ids.iter().collect::<BTreeSet<_>>().into_iter().collect();
        ids.sort();
        for d in ids {
            let k1 = format!("新{}ID", d);
            let k2 = format!("新{}ID", ck.p.show_names(d));
            let key = [k1, k2].into_iter().find(|k| ctx.contains_key(k));
            let Some(key) = key else {
                return Err(format!("前提に 採番「新{}ID」がありません", ck.p.show_names(d)));
            };
            fields.push(format!("new_{}_id: {}.to_string()", d, rust_str(&ctx[&key])));
        }
    }
    l.push(format!("        let ctx = {}Ctx {{ {} }};", pn, fields.join(", ")));
    let given_s = g("入力");
    let given = if !given_s.trim().is_empty() { kv(&given_s)? } else { IndexMap::new() };
    let inputs = match (q, proc) {
        (Some(q), _) => deed_inputs_query(ck, q),
        (None, Some(p)) => deed_inputs_proc(ck, p),
        _ => vec![],
    };
    let inames: Vec<String> = inputs.iter().map(|(n, _)| n.clone()).collect();
    let given: IndexMap<String, String> = given.into_iter().map(|(k, v)| (ck.p.resolve(&k, &inames), v)).collect();
    let mut ins = vec![];
    for (name, t) in &inputs {
        let Some(gv) = given.get(name) else {
            if t.optional {
                ins.push(format!("{}: None", name));
                continue;
            }
            return Err(format!("入力に「{}」がありません", name));
        };
        let v = lit(t, gv, ck)?;
        ins.push(if t.optional { format!("{}: Some({})", name, v) } else { format!("{}: {}", name, v) });
    }
    l.push(format!("        let Input = {}Input {{ {} }};", pn, ins.join(", ")));
    let eid = rust_str(row.get("ID").map(|s| s.trim()).unwrap_or(""));
    if let Some(q) = q {
        l.push("        match ctx.validate().and_then(|_| Input.validate()) {".into());
        l.push(format!("            Err(e) => println!(\"{{}}\\t入力不正({{}})\\t\\t\", {}, e),", eid));
        let sel = if !q.select.is_empty() { ".iter().map(|x| x.show()).collect::<Vec<_>>()" } else { "" };
        l.push(format!("            Ok(()) => println!(\"{{}}\\t一覧({{}})\\t\\t\", {}, {}(&ctx, &Store, &Input){}.join(\",\")),", eid, q.name, sel));
        l.push("        }".into());
        l.push("    }".into());
        return Ok(l.join("\n"));
    }
    l.push("        match ctx.validate().and_then(|_| Input.validate()) {".into());
    l.push(format!("            Err(e) => println!(\"{{}}\\t入力不正({{}})\\t\\t\", {}, e),", eid));
    l.push(format!("            Ok(()) => match {}(&ctx, &Store, &Input) {{", pn));
    l.push(format!("                Err(f) => println!(\"{{}}\\t異常({{}})\\t\\t\", {}, f),", eid));
    l.push("                Ok(Output) => {".into());
    l.push("                    let 前 = Store.clone();".into());
    l.push("                    Store.apply(Output.Write.clone()).expect(\"書込の適用\");".into());
    l.push(format!(
        "                    println!(\"{{}}\\t{{}}\\t{{}}\\t{{}}\", {}, Output.result.show(), 前.diff(&Store).join(\";\"), Output.effects.iter().map(|e| e.show()).collect::<Vec<_>>().join(\";\"));",
        eid
    ));
    l.push("                }".into());
    l.push("            },".into());
    l.push("        }".into());
    l.push("    }".into());
    Ok(l.join("\n"))
}

pub fn runner_source(ck: &Checker, rows: &[Row]) -> (String, Vec<Finding>) {
    let (mut problems, mut blocks) = (vec![], vec![]);
    for row in rows {
        let eid = row.get("ID").map(|s| s.trim()).unwrap_or("").to_string();
        if row.get("期待する結果").map_or(false, |s| s.contains("未規定")) {
            problems.push(Finding { kind: UNSPEC.into(), where_: format!("具体例 {}", eid), msg: "計画で未規定。コードがどう動くかは検証しない。問いとして残る".into() });
            continue;
        }
        match example_code(ck, row) {
            Ok(c) => blocks.push(c),
            Err(e) => problems.push(Finding { kind: EXAMPLE.into(), where_: format!("具体例 {}", eid), msg: format!("実行できない形です: {}", e) }),
        }
    }
    let src = "use geas_app::*;\nuse std::collections::{BTreeMap, BTreeSet};\n\n#[allow(unused_mut, non_snake_case, unused_variables)]\nfn main() {\n";
    (format!("{}{}\n}}\n", src, blocks.join("\n")), problems)
}

/// 実行して結果を集める。show は、コードの名前を人の書く名前（表示名）にそろえる
pub fn run_crate(dest: &Path, show: &dyn Fn(&str) -> String) -> Result<IndexMap<String, (String, String, String)>, E> {
    let r = Command::new("cargo").args(["run", "--quiet", "--offline", "--bin", "plan_examples"]).current_dir(dest).output();
    let r = match r {
        Ok(r) => r,
        Err(_) => return Err("cargo が無いので具体例を実行できません".into()),
    };
    if !r.status.success() {
        let err = String::from_utf8_lossy(&r.stderr);
        let cs: Vec<char> = err.chars().collect();
        let tail: String = cs[cs.len().saturating_sub(2000)..].iter().collect();
        return Err(format!("生成した Rust のビルドか実行に失敗しました:\n{}", tail));
    }
    let mut out = IndexMap::new();
    for line in String::from_utf8_lossy(&r.stdout).lines() {
        let p: Vec<&str> = line.split('\t').collect();
        if p.len() == 4 {
            out.insert(p[0].to_string(), (show(p[1]), show(p[2]), show(p[3])));
        }
    }
    Ok(out)
}

fn items(s: &str) -> Vec<String> {
    let mut v: Vec<String> = split_top(&n(s), ';').into_iter().filter(|x| !x.is_empty() && x != "なし").collect();
    v.sort();
    v
}

pub fn compare_examples(rows: &[Row], actual: &IndexMap<String, (String, String, String)>) -> Vec<Finding> {
    let mut out = vec![];
    for row in rows {
        let eid = row.get("ID").map(|s| s.trim()).unwrap_or("").to_string();
        let g = |k: &str| row.get(k).cloned().unwrap_or_default();
        if g("期待する結果").contains("未規定") {
            continue;
        }
        let Some((res, chg, eff)) = actual.get(&eid) else { continue };
        for (label, want, got) in [
            ("結果", vec![n(&g("期待する結果"))], vec![n(res)]),
            ("変化", items(&g("期待する変化")), items(chg)),
            ("外部作用", items(&g("期待する外部作用")), items(eff)),
        ] {
            if want != got {
                let j = |v: &Vec<String>| if v.is_empty() { "なし".to_string() } else { v.join("; ") };
                out.push(Finding { kind: EXAMPLE.into(), where_: format!("具体例 {}（{}）の{}", eid, g("処理"), label), msg: format!("計画: {} ／ 実行した結果: {}", j(&want), j(&got)) });
            }
        }
    }
    out
}
