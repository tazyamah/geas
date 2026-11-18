// SPDX-License-Identifier: GPL-3.0-or-later
//! 部品（library）の境界値の検査：宣言だけを頼りに、引数の範囲の端の値で実際に呼んで、宣言どおりかを確かめる
//! （Python 版 partcheck.py の検査の部分と stdref.py の移し）。
use crate::ast::*;
use crate::check::phys;
use crate::partcheck::{examples_fingerprint, fingerprint};
use crate::rustgen::{dependencies, rust_type};
use indexmap::IndexMap;
use num_bigint::{BigInt, Sign};
use num_traits::{One, Signed, ToPrimitive, Zero};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

pub struct PFinding {
    pub part: String,
    pub kind: String,
    pub detail: String,
}

#[derive(Default)]
pub struct Report {
    pub findings: Vec<PFinding>,
    pub calls: IndexMap<String, usize>,
    pub skipped: IndexMap<String, String>,
    pub compared: BTreeSet<String>,
    pub examples: IndexMap<String, usize>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.findings.is_empty()
    }
}

fn big(v: i128) -> BigInt {
    BigInt::from(v)
}

/// 型の値の範囲（下限, 上限）。整数と金額だけ
pub fn bounds(t: &Type) -> Option<(BigInt, BigInt)> {
    if t.kind != "整数" && t.kind != "金額" {
        return None;
    }
    let p = phys(t);
    let bits: u32 = p[1..].parse().ok()?;
    let (mut lo, mut hi): (BigInt, BigInt) = if p.starts_with('u') { (BigInt::zero(), (BigInt::one() << bits) - 1) } else { (-(BigInt::one() << (bits - 1)), (BigInt::one() << (bits - 1)) - 1) };
    if let Some(r) = &t.range {
        if let Some(l) = r.lo {
            lo = lo.max(big(l));
        }
        if let Some(h) = r.hi {
            hi = hi.min(big(h));
        }
    }
    Some((lo, hi))
}

#[derive(Clone, Debug, PartialEq)]
pub enum V {
    I(BigInt),
    B(bool),
}

impl V {
    fn int(&self) -> BigInt {
        match self {
            V::I(i) => i.clone(),
            V::B(b) => BigInt::from(*b as i32),
        }
    }
    fn truthy(&self) -> bool {
        match self {
            V::I(i) => !i.is_zero(),
            V::B(b) => *b,
        }
    }
}

fn floor_div(a: &BigInt, b: &BigInt) -> BigInt {
    let q = a / b;
    if (a % b).is_zero() || (a.sign() == Sign::Minus) == (b.sign() == Sign::Minus) { q } else { q - 1 }
}

fn floor_mod(a: &BigInt, b: &BigInt) -> BigInt {
    a - floor_div(a, b) * b
}

fn edge_values(t: &Type) -> Option<Vec<V>> {
    if t.kind == "真偽" {
        return Some(vec![V::B(false), V::B(true)]);
    }
    let (lo, hi) = bounds(t)?;
    let two = big(2);
    let mut picks: BTreeSet<BigInt> = [lo.clone(), &lo + 1, &hi - 1, hi.clone(), big(-1), big(0), big(1), big(2), floor_div(&(&lo + &hi), &two)].into_iter().collect();
    for x in [7, 8, 15, 16, 31, 32, 63, 64, 255, 256] {
        picks.insert(big(x));
    }
    Some(picks.into_iter().filter(|v| &lo <= v && v <= &hi).map(V::I).collect())
}

fn cases(pt: &Part) -> Option<Vec<Vec<V>>> {
    let cols: Vec<Option<Vec<V>>> = pt.params.iter().map(|(_, t)| edge_values(t)).collect();
    if cols.iter().any(|c| c.is_none()) { None } else { Some(cols.into_iter().map(|c| c.unwrap()).collect()) }
}

fn lit(v: &V, t: &Type) -> String {
    if t.kind == "真偽" { (if v.truthy() { "true" } else { "false" }).to_string() } else { format!("{}{}", v.int(), phys(t)) }
}

fn example_lit(v: &Expr, t: &Type) -> String {
    match v {
        Expr::StrLit { s } => format!("{}.to_string()", serde_json::to_string(s).unwrap()),
        Expr::Num { v } => lit(&V::I(big(*v)), t),
        Expr::BoolLit { v } => lit(&V::B(*v), t),
        _ => String::new(),
    }
}

fn example_text(v: &Expr) -> String {
    match v {
        Expr::StrLit { s } => s.clone(),
        Expr::BoolLit { v } => (if *v { "true" } else { "false" }).into(),
        Expr::Num { v } => v.to_string(),
        _ => String::new(),
    }
}

fn harness(parts: &[&Part], case_map: &IndexMap<String, Option<Vec<Vec<V>>>>) -> (String, BTreeMap<usize, String>) {
    let mut lines: Vec<String> = vec!["#![allow(warnings)]".into(), "use std::panic::{catch_unwind, set_hook};".into(), String::new()];
    let mut where_ = BTreeMap::new();
    for pt in parts {
        let ps: Vec<String> = pt.params.iter().map(|(n, t)| format!("{}: {}", n, rust_type(t))).collect();
        where_.insert(lines.len() + 1, pt.name.clone());
        lines.push(format!("fn p_{}({}) -> {} {{ {} }}", pt.name, ps.join(", "), rust_type(&pt.ret), pt.lib.as_ref().unwrap().2));
    }
    for l in [
        "",
        "fn run<T: std::fmt::Display + PartialEq>(name: &str, shown: String, f: impl Fn() -> T + std::panic::RefUnwindSafe) {",
        "    match (catch_unwind(&f), catch_unwind(&f)) {",
        "        (Ok(a), Ok(b)) => println!(\"{}\\t{}\\t{}\\t{}\", name, shown, a, a == b),",
        "        _ => println!(\"{}\\t{}\\tPANIC\\ttrue\", name, shown),",
        "    }",
        "}",
        "",
        "fn main() {",
        "    set_hook(Box::new(|_| {}));",
    ] {
        lines.push(l.to_string());
    }
    for pt in parts {
        let names: Vec<&str> = pt.params.iter().map(|(n, _)| n.as_str()).collect();
        let fmt = names.iter().map(|_| "{}").collect::<Vec<_>>().join(",");
        if let Some(cs) = &case_map[&pt.name] {
            let heads: String = pt.params.iter().zip(cs).map(|((n, t), vals)| format!("for {} in [{}] {{ ", n, vals.iter().map(|v| lit(v, t)).collect::<Vec<_>>().join(", "))).collect();
            lines.push(format!("    {}run(\"{}\", format!(\"{}\", {}), || p_{}({})); {}", heads, pt.name, fmt, names.join(", "), pt.name, names.join(", "), "}".repeat(names.len())));
        }
        for (i, (args, _, _)) in pt.examples.iter().enumerate() {
            let call = format!("p_{}({})", pt.name, args.iter().zip(&pt.params).map(|(a, (_, t))| example_lit(a, t)).collect::<Vec<_>>().join(", "));
            lines.push(format!("    run(\"{}\", \"#{}\".to_string(), || format!(\"{{}}\", {}).bytes().map(|b| format!(\"{{:02x}}\", b)).collect::<String>());", pt.name, i, call));
        }
    }
    lines.push("}".into());
    (lines.join("\n") + "\n", where_)
}

fn cmp_ord(a: &V, b: &V) -> std::cmp::Ordering {
    a.int().cmp(&b.int())
}

/// guarantee の式を計算する（Python 版と同じく、計算できない形は None）
fn holds(e: &Expr, env: &BTreeMap<String, V>) -> Option<V> {
    match e {
        Expr::Num { v } => Some(V::I(big(*v))),
        Expr::BoolLit { v } => Some(V::B(*v)),
        Expr::Name { n } => env.get(n).cloned(),
        Expr::Not { e } => holds(e, env).map(|v| V::B(!v.truthy())),
        Expr::Cond { c, a, b } => {
            let c = holds(c, env)?;
            holds(if c.truthy() { a } else { b }, env)
        }
        Expr::Call { n, args } if n == "min" || n == "max" || n == "abs" => {
            let vs: Vec<Option<V>> = args.iter().map(|a| holds(a, env)).collect();
            if vs.iter().any(|v| v.is_none()) {
                return None;
            }
            let vs: Vec<V> = vs.into_iter().map(|v| v.unwrap()).collect();
            if n == "abs" {
                return vs.first().map(|v| V::I(v.int().abs()));
            }
            let it = vs.into_iter();
            if n == "min" { it.min_by(|a, b| cmp_ord(a, b)) } else { it.max_by(|a, b| cmp_ord(a, b)) }
        }
        Expr::Bin { op, l, r, .. } => {
            let (a, b) = (holds(l, env)?, holds(r, env)?);
            let (x, y) = (a.int(), b.int());
            Some(match op.as_str() {
                "+" => V::I(x + y),
                "-" => V::I(x - y),
                "*" => V::I(x * y),
                "<" => V::B(x < y),
                "<=" => V::B(x <= y),
                ">" => V::B(x > y),
                ">=" => V::B(x >= y),
                "==" => V::B(x == y),
                "!=" => V::B(x != y),
                "かつ" => if a.truthy() { b } else { a },
                "または" => if a.truthy() { a } else { b },
                "/" => {
                    if y.is_zero() {
                        return None;
                    }
                    V::I(floor_div(&x, &y))
                }
                "%" => {
                    if y.is_zero() {
                        return None;
                    }
                    V::I(floor_mod(&x, &y))
                }
                _ => return None,
            })
        }
        _ => None,
    }
}

fn which_cargo() -> bool {
    Command::new("cargo").arg("--version").output().map_or(false, |o| o.status.success())
}

struct Out {
    ok: bool,
    stdout: String,
    stderr: String,
}

fn build_and_run(dest: &Path, src: &str, deps: &BTreeMap<String, String>) -> Out {
    std::fs::create_dir_all(dest.join("src")).unwrap();
    std::fs::write(dest.join("Cargo.toml"), format!("[package]\nname = \"geas_parts\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n{}[profile.dev]\noverflow-checks = true\n", dependencies(deps))).unwrap();
    std::fs::write(dest.join("src").join("main.rs"), src).unwrap();
    let r = Command::new("cargo").args(["run", "--quiet", "--offline"]).current_dir(dest).output().unwrap();
    Out { ok: r.status.success(), stdout: String::from_utf8_lossy(&r.stdout).into_owned(), stderr: String::from_utf8_lossy(&r.stderr).into_owned() }
}

pub type RefFn = Box<dyn Fn(&[V]) -> V>;

fn tail(s: &str, n: usize) -> String {
    let cs: Vec<char> = s.chars().collect();
    cs[cs.len().saturating_sub(n)..].iter().collect()
}

/// library の部品を、境界値で呼んで確かめる。reference は 部品名 → 手本の関数
pub fn check_parts(parts: &[&Part], dest: &Path, reference: &IndexMap<String, RefFn>) -> Result<Report, String> {
    if !which_cargo() {
        return Err("cargo が無いので、部品を呼んで確かめられません".into());
    }
    let mut rep = Report::default();
    let mut todo: Vec<&Part> = vec![];
    let mut case_map: IndexMap<String, Option<Vec<Vec<V>>>> = IndexMap::new();
    for pt in parts {
        if !pt.external() {
            continue;
        }
        let mut cs = cases(pt);
        if cs.is_none() || (pt.ret.kind != "真偽" && bounds(&pt.ret).is_none()) {
            cs = None;
            rep.skipped.insert(pt.name.clone(), format!("引数か戻り値が、整数・金額・真偽ではない（端の値では確かめられない{}", if !pt.examples.is_empty() { "。具体例だけを確かめた）" } else { "）" }));
            if pt.examples.is_empty() {
                continue;
            }
        }
        todo.push(pt);
        case_map.insert(pt.name.clone(), cs);
    }
    let mut deps: BTreeMap<String, String> = BTreeMap::new();
    for pt in &todo {
        let l = pt.lib.as_ref().unwrap();
        if l.0 != "std" {
            deps.insert(l.0.clone(), l.1.clone());
        }
    }
    let mut r: Option<Out> = None;
    let loc = Regex::new(r"--> src/main\.rs:(\d+):").unwrap();
    for _ in 0..=todo.len() {
        if todo.is_empty() {
            return Ok(rep);
        }
        let (src, where_) = harness(&todo, &case_map);
        let out = build_and_run(dest, &src, &deps);
        if out.ok {
            r = Some(out);
            break;
        }
        let mut bad: BTreeMap<String, String> = BTreeMap::new();
        // re.split(r"\n(?=error)", stderr)
        let mut blocks: Vec<&str> = vec![];
        let mut start = 0;
        let bytes = out.stderr.as_bytes();
        for i in 0..bytes.len() {
            if bytes[i] == b'\n' && out.stderr[i + 1..].starts_with("error") {
                blocks.push(&out.stderr[start..i]);
                start = i + 1;
            }
        }
        blocks.push(&out.stderr[start..]);
        for block in blocks {
            if let Some(m) = loc.captures(block) {
                if let Some(name) = m[1].parse::<usize>().ok().and_then(|n| where_.get(&n)) {
                    bad.entry(name.clone()).or_insert_with(|| crate::mutate::splitlines(block).first().cloned().unwrap_or_default());
                }
            }
        }
        if bad.is_empty() {
            return Err(format!("部品を確かめるプログラムを組めませんでした:\n{}", tail(&out.stderr, 2000)));
        }
        for (name, why) in &bad {
            rep.findings.push(PFinding { part: name.clone(), kind: "組めない".into(), detail: why.clone() });
        }
        todo.retain(|pt| !bad.contains_key(&pt.name));
        r = Some(out);
    }
    let r = r.unwrap();
    let by_name: IndexMap<&str, &Part> = todo.iter().map(|pt| (pt.name.as_str(), *pt)).collect();
    for line in crate::mutate::splitlines(&r.stdout) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() != 4 {
            return Err(format!("not enough values to unpack (expected 4, got {})", f.len()));
        }
        let (name, shown, got, same) = (f[0], f[1], f[2], f[3]);
        let pt = by_name[name];
        if let Some(idx) = shown.strip_prefix('#') {
            let (args, want, _) = &pt.examples[idx.parse::<usize>().unwrap()];
            let call = format!("{}({})", name, args.iter().map(show).collect::<Vec<_>>().join(", "));
            if got == "PANIC" {
                rep.findings.push(PFinding { part: name.into(), kind: "panic".into(), detail: call.clone() });
            } else {
                let bytes: Vec<u8> = (0..got.len() / 2).map(|i| u8::from_str_radix(&got[2 * i..2 * i + 2], 16).unwrap_or(0)).collect();
                let text = String::from_utf8_lossy(&bytes).into_owned();
                if text != example_text(want) {
                    rep.findings.push(PFinding { part: name.into(), kind: "具体例と違う".into(), detail: format!("{} = {}（具体例は {}）", call, text, show(want)) });
                } else {
                    *rep.examples.entry(name.into()).or_insert(0) += 1;
                }
            }
            if same != "true" {
                rep.findings.push(PFinding { part: name.into(), kind: "結果が変わる".into(), detail: call });
            }
            continue;
        }
        *rep.calls.entry(name.into()).or_insert(0) += 1;
        let call = format!("{}({})", name, shown);
        if got == "PANIC" {
            rep.findings.push(PFinding { part: name.into(), kind: "panic".into(), detail: call });
            continue;
        }
        if same != "true" {
            rep.findings.push(PFinding { part: name.into(), kind: "結果が変わる".into(), detail: call.clone() });
        }
        let val = if pt.ret.kind == "真偽" { V::B(got == "true") } else { V::I(got.parse::<BigInt>().map_err(|_| format!("invalid literal for int() with base 10: {}", crate::pyrepr::repr_str(got)))?) };
        let args: Vec<V> = shown.split(',').zip(&pt.params).map(|(v, (_, t))| if t.kind == "真偽" { V::B(v == "true") } else { V::I(v.parse().unwrap_or_default()) }).collect();
        if pt.ret.kind != "真偽" {
            let (lo, hi) = bounds(&pt.ret).unwrap();
            let x = val.int();
            if !(lo <= x && x <= hi) {
                rep.findings.push(PFinding { part: name.into(), kind: "範囲外".into(), detail: format!("{} = {}（宣言は {}..{}）", call, x, lo, hi) });
            }
        }
        if let Some(g) = &pt.guarantee {
            let mut env: BTreeMap<String, V> = pt.params.iter().zip(&args).map(|((n, _), v)| (n.clone(), v.clone())).collect();
            env.insert("result".into(), val.clone());
            match holds(g, &env) {
                None => {
                    rep.skipped.entry(format!("{} の guarantee", name)).or_insert_with(|| format!("この形の式は計算できない（{}）", show(g)));
                }
                Some(ok) if !ok.truthy() => rep.findings.push(PFinding { part: name.into(), kind: "保証違反".into(), detail: format!("{} = {}（guarantee {}）", call, py_v(&val), show(g)) }),
                _ => {}
            }
        }
        if let Some(f) = reference.get(name) {
            rep.compared.insert(name.into());
            let want = f(&args);
            if want.int() != val.int() {
                rep.findings.push(PFinding { part: name.into(), kind: "手本と違う".into(), detail: format!("{} = {}（手本は {}）", call, py_v(&val), py_v(&want)) });
            }
        }
    }
    Ok(rep)
}

fn py_v(v: &V) -> String {
    match v {
        V::I(i) => i.to_string(),
        V::B(b) => (if *b { "True" } else { "False" }).into(),
    }
}

pub fn render(rep: &Report) -> String {
    let mut l: Vec<String> = vec![format!(
        "部品の境界値の検査：{} 個の部品を、合わせて {} 回呼びました{}",
        rep.calls.len(),
        rep.calls.values().sum::<usize>(),
        if rep.compared.is_empty() { String::new() } else { format!("（うち {} 個は手本とも比べました）", rep.compared.len()) }
    )];
    if !rep.examples.is_empty() {
        l.push(format!("具体例：{} 個の部品の、合わせて {} 件が一致しました", rep.examples.len(), rep.examples.values().sum::<usize>()));
    }
    for f in &rep.findings {
        l.push(format!("  ✕ {}：{}　{}", f.part, f.kind, f.detail));
    }
    for (name, why) in &rep.skipped {
        l.push(format!("  － {}：確かめられませんでした（{}）", name, why));
    }
    l.push(if rep.ok() { "宣言と食い違う部品はありません（端の値で確かめた範囲）".to_string() } else { format!("宣言と食い違う部品が {} 個あります", rep.findings.iter().map(|f| f.part.as_str()).collect::<BTreeSet<_>>().len()) });
    l.join("\n")
}

pub fn toolchain() -> String {
    Command::new("rustc").arg("--version").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

/// 検査の結果を台帳の形にする（キーは名前順にそろえる：Python 版は sort_keys で書く）
pub fn ledger_of(rep: &Report, parts: &[&Part]) -> Value {
    let bad: BTreeSet<&str> = rep.findings.iter().map(|f| f.part.as_str()).collect();
    let mut out = serde_json::Map::new();
    for pt in parts {
        if (rep.calls.contains_key(&pt.name) || rep.examples.contains_key(&pt.name)) && !bad.contains(pt.name.as_str()) {
            out.insert(pt.name.clone(), json!({"fingerprint": fingerprint(pt), "calls": rep.calls.get(&pt.name).copied().unwrap_or(0), "reference": rep.compared.contains(&pt.name),
                                                "examples": rep.examples.get(&pt.name).copied().unwrap_or(0), "examples_fingerprint": examples_fingerprint(pt)}));
        }
    }
    json!({"toolchain": toolchain(), "parts": out})
}

/// キーを名前順に並べ直す（json.dumps(sort_keys=True) の代わり）
pub fn sort_keys(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            Value::Object(keys.into_iter().map(|k| (k.clone(), sort_keys(&m[k]))).collect())
        }
        Value::Array(a) => Value::Array(a.iter().map(sort_keys).collect()),
        o => o.clone(),
    }
}

// ---- 標準部品の手本（Python 版 stdref.py） ----
fn modulus(bits: u32) -> BigInt {
    BigInt::one() << bits
}

fn wrap(v: &BigInt, bits: u32, signed: bool) -> BigInt {
    let m = modulus(bits);
    let x = floor_mod(v, &m);
    if signed && x >= (BigInt::one() << (bits - 1)) { x - m } else { x }
}

fn sat(v: &BigInt, bits: u32, signed: bool) -> BigInt {
    let (lo, hi) = if signed { (-(BigInt::one() << (bits - 1)), (BigInt::one() << (bits - 1)) - 1) } else { (BigInt::zero(), modulus(bits) - 1) };
    v.clone().max(lo).min(hi)
}

fn u_(a: &BigInt, bits: u32) -> BigInt {
    floor_mod(a, &modulus(bits))
}

fn bitlen(a: &BigInt) -> u32 {
    a.magnitude().bits() as u32
}

fn trailing_zeros(x: &BigInt, bits: u32) -> u32 {
    if x.is_zero() { bits } else { x.magnitude().trailing_zeros().unwrap_or(0) as u32 }
}

fn rev(x: &BigInt, bits: u32) -> BigInt {
    let s = format!("{:0>w$}", x.magnitude().to_str_radix(2), w = bits as usize);
    let r: String = s.chars().rev().collect();
    BigInt::parse_bytes(r.as_bytes(), 2).unwrap_or_default()
}

fn swap(x: &BigInt, bits: u32) -> BigInt {
    let (_, mut b) = x.to_bytes_le();
    b.resize((bits / 8) as usize, 0);
    BigInt::from_bytes_be(Sign::Plus, &b)
}

fn rot(x: &BigInt, n: &BigInt, bits: u32) -> BigInt {
    let n = floor_mod(n, &big(bits as i128)).to_u32().unwrap_or(0);
    ((x << n) | (x >> (bits - n))) & (modulus(bits) - 1)
}

fn trunc_half(s: &BigInt) -> BigInt {
    if s.is_negative() { -((-s) / BigInt::from(2)) } else { s / BigInt::from(2) }
}

fn sat_pow(bits: u32, signed: bool, a: &BigInt, n: &BigInt) -> BigInt {
    if a.is_zero() || a == &big(1) || a == &big(-1) {
        return if n.is_zero() { big(1) } else if !floor_mod(n, &big(2)).is_zero() { a.clone() } else { a * a };
    }
    let b = big(bits as i128);
    let cap = if n > &b { &b + floor_mod(&(n - &b), &big(2)) } else { n.clone() };
    let e = n.clone().min(cap).to_u32().unwrap_or(0);
    sat(&num_traits::pow::pow(a.clone(), e as usize), bits, signed)
}

fn ref_of(name: &str, bits: u32, signed: bool, a: &[BigInt]) -> Option<V> {
    let (b_, s_) = (bits, signed);
    let x = |i: usize| a[i].clone();
    let i = |v: BigInt| Some(V::I(v));
    let bl = |v: bool| Some(V::B(v));
    match name {
        "abs_diff" => i((x(0) - x(1)).abs()),
        "bit_width" => i(big(bitlen(&a[0]) as i128)),
        "cast_signed" => i(wrap(&x(0), b_, true)),
        "cast_unsigned" => i(u_(&x(0), b_)),
        "count_ones" => i(big(format!("{:b}", u_(&x(0), b_).magnitude()).matches('1').count() as i128)),
        "count_zeros" => i(big(b_ as i128 - format!("{:b}", u_(&x(0), b_).magnitude()).matches('1').count() as i128)),
        "is_negative" => bl(x(0).is_negative()),
        "is_positive" => bl(x(0).is_positive()),
        "is_power_of_two" => bl(x(0).is_positive() && ((x(0) & (x(0) - BigInt::one())).is_zero())),
        "isolate_highest_one" => {
            let u = u_(&x(0), b_);
            i(wrap(&if u.is_zero() { big(0) } else { BigInt::one() << (bitlen(&u) - 1) }, b_, s_))
        }
        "isolate_lowest_one" => {
            let u = u_(&x(0), b_);
            i(wrap(&if u.is_zero() { big(0) } else { BigInt::one() << trailing_zeros(&u, b_) }, b_, s_))
        }
        "isqrt" => i(x(0).sqrt()),
        "leading_ones" => i(big(b_ as i128 - bitlen(&u_(&(-x(0) - 1), b_)) as i128)),
        "leading_zeros" => i(big(b_ as i128 - bitlen(&u_(&x(0), b_)) as i128)),
        "trailing_ones" => i(big(trailing_zeros(&u_(&(-x(0) - 1), b_), b_) as i128)),
        "trailing_zeros" => i(big(trailing_zeros(&u_(&x(0), b_), b_) as i128)),
        "midpoint" => i(if s_ { trunc_half(&(x(0) + x(1))) } else { floor_div(&(x(0) + x(1)), &big(2)) }),
        "reverse_bits" => i(wrap(&rev(&u_(&x(0), b_), b_), b_, s_)),
        "swap_bytes" | "to_be" => i(wrap(&swap(&u_(&x(0), b_), b_), b_, s_)),
        "to_le" => i(x(0)),
        "rotate_left" => i(wrap(&rot(&u_(&x(0), b_), &x(1), b_), b_, s_)),
        "rotate_right" => i(wrap(&rot(&u_(&x(0), b_), &-x(1), b_), b_, s_)),
        "saturating_abs" => i(sat(&x(0).abs(), b_, s_)),
        "saturating_add" | "saturating_add_signed" | "saturating_add_unsigned" => i(sat(&(x(0) + x(1)), b_, s_)),
        "saturating_sub" | "saturating_sub_signed" | "saturating_sub_unsigned" => i(sat(&(x(0) - x(1)), b_, s_)),
        "saturating_mul" => i(sat(&(x(0) * x(1)), b_, s_)),
        "saturating_neg" => i(sat(&-x(0), b_, s_)),
        "saturating_pow" => i(sat_pow(b_, s_, &x(0), &x(1))),
        "signum" => i(big((x(0).is_positive() as i128) - (x(0).is_negative() as i128))),
        "unbounded_shl" => i(if x(1) >= big(b_ as i128) { big(0) } else { wrap(&(u_(&x(0), b_) << x(1).to_u32().unwrap_or(0)), b_, s_) }),
        "unbounded_shr" => i(if x(1) >= big(b_ as i128) { if x(0).is_negative() { big(-1) } else { big(0) } } else { x(0) >> x(1).to_u32().unwrap_or(0) }),
        "wrapping_abs" => i(wrap(&x(0).abs(), b_, s_)),
        "wrapping_add" | "wrapping_add_signed" | "wrapping_add_unsigned" => i(wrap(&(x(0) + x(1)), b_, s_)),
        "wrapping_sub" | "wrapping_sub_signed" | "wrapping_sub_unsigned" => i(wrap(&(x(0) - x(1)), b_, s_)),
        "wrapping_mul" => i(wrap(&(x(0) * x(1)), b_, s_)),
        "wrapping_neg" => i(wrap(&-x(0), b_, s_)),
        "wrapping_pow" => i(wrap(&floor_mod(&x(0), &modulus(b_)).modpow(&x(1), &modulus(b_)), b_, s_)),
        _ => None,
    }
}

const REF_NAMES: [&str; 45] = [
    "abs_diff", "bit_width", "cast_signed", "cast_unsigned", "count_ones", "count_zeros", "is_negative", "is_positive", "is_power_of_two", "isolate_highest_one", "isolate_lowest_one", "isqrt", "leading_ones",
    "leading_zeros", "trailing_ones", "trailing_zeros", "midpoint", "reverse_bits", "swap_bytes", "to_be", "to_le", "rotate_left", "rotate_right", "saturating_abs", "saturating_add",
    "saturating_add_signed", "saturating_add_unsigned", "saturating_sub", "saturating_sub_signed", "saturating_sub_unsigned", "saturating_mul", "saturating_neg", "saturating_pow", "signum", "unbounded_shl",
    "unbounded_shr", "wrapping_abs", "wrapping_add", "wrapping_add_signed", "wrapping_add_unsigned", "wrapping_sub", "wrapping_sub_signed", "wrapping_sub_unsigned", "wrapping_mul", "wrapping_neg",
];

/// 部品名 → 手本の関数。手本の無い部品は入らない
pub fn reference(parts: &[&Part]) -> IndexMap<String, RefFn> {
    let re = Regex::new(r"^Num_([iu])(8|16|32|64)_(\w+)$").unwrap();
    let mut out: IndexMap<String, RefFn> = IndexMap::new();
    for pt in parts {
        if let Some(m) = re.captures(&pt.name) {
            let f = m[3].to_string();
            if REF_NAMES.contains(&f.as_str()) || f == "wrapping_pow" {
                let (bits, signed) = (m[2].parse::<u32>().unwrap(), &m[1] == "i");
                out.insert(pt.name.clone(), Box::new(move |args: &[V]| {
                    let a: Vec<BigInt> = args.iter().map(|v| v.int()).collect();
                    ref_of(&f, bits, signed, &a).unwrap_or(V::I(BigInt::zero()))
                }));
            }
        }
    }
    out
}
