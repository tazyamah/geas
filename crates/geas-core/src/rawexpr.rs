// SPDX-License-Identifier: GPL-3.0-or-later
//! raw から変換する convert（型の read に結んだ変換）の「拒否の条件」（require）を、生成物の外でも評価する。
//! Python 版 geas/rawexpr.py の移し。条件の式を、JSON にできる木にする。木にできない式があれば None（事前には確かめない）。
use crate::ast::*;
use crate::parser::parse_chars;
use serde_json::{json, Value};

/// 式の木（Python 版の入れ子のリストに当たる）
#[derive(Debug, Clone)]
pub enum RT {
    Num(i128),
    Bool(bool),
    Raw,
    Var(String),
    Not(Box<RT>),
    If(Box<RT>, Box<RT>, Box<RT>),
    Sum(String, Box<RT>),
    Len,
    Only(Vec<(u32, u32)>),
    Digit(Box<RT>),
    /// min・max・abs
    Func(String, Vec<RT>),
    /// 2 項（+ - * < > <= >= == != and or / %）
    Op(String, Box<RT>, Box<RT>),
}

impl RT {
    /// Python 版の木（JSON）と同じ形
    pub fn to_json(&self) -> Value {
        match self {
            RT::Num(v) => json!(["num", v.to_string().parse::<serde_json::Number>().unwrap()]),
            RT::Bool(b) => json!(["bool", b]),
            RT::Raw => json!(["raw"]),
            RT::Var(n) => json!(["var", n]),
            RT::Not(e) => json!(["not", e.to_json()]),
            RT::If(c, a, b) => json!(["if", c.to_json(), a.to_json(), b.to_json()]),
            RT::Sum(n, e) => json!(["sum", n, e.to_json()]),
            RT::Len => json!(["len"]),
            RT::Only(ps) => json!(["only", ps.iter().map(|(a, b)| json!([a, b])).collect::<Vec<_>>()]),
            RT::Digit(e) => json!(["digit", e.to_json()]),
            RT::Func(n, xs) => {
                let mut v = vec![json!(n)];
                v.extend(xs.iter().map(|x| x.to_json()));
                Value::Array(v)
            }
            RT::Op(op, l, r) => json!([op, l.to_json(), r.to_json()]),
        }
    }
}

pub fn tree(e: &Expr, raw: &str) -> Option<RT> {
    Some(match e {
        Expr::Num { v } => RT::Num(*v),
        Expr::BoolLit { v } => RT::Bool(*v),
        Expr::Name { n } => {
            if n == raw {
                RT::Raw
            } else {
                RT::Var(n.clone())
            }
        }
        Expr::Not { e } => RT::Not(Box::new(tree(e, raw)?)),
        Expr::Cond { c, a, b } => RT::If(Box::new(tree(c, raw)?), Box::new(tree(a, raw)?), Box::new(tree(b, raw)?)),
        Expr::SumE { elem, binder, .. } => RT::Sum(binder.names[0].clone(), Box::new(tree(elem, raw)?)),
        Expr::Call { n, args } if n == "len" => {
            let _ = args;
            RT::Len
        }
        Expr::Call { n, args } if n == "only" => match args.get(1)? {
            Expr::StrLit { s } => RT::Only(parse_chars(s).ok()?),
            _ => return None,
        },
        Expr::Call { n, args } if n == "digit" => RT::Digit(Box::new(tree(args.get(1)?, raw)?)),
        Expr::Call { n, args } if n == "min" || n == "max" || n == "abs" => {
            let mut xs = vec![];
            for a in args {
                xs.push(tree(a, raw)?);
            }
            RT::Func(n.clone(), xs)
        }
        Expr::Bin { op, l, r, .. } if ["+", "-", "*", "<", ">", "<=", ">=", "==", "!=", "かつ", "または"].contains(&op.as_str()) => {
            let o = match op.as_str() {
                "かつ" => "and",
                "または" => "or",
                o => o,
            };
            RT::Op(o.to_string(), Box::new(tree(l, raw)?), Box::new(tree(r, raw)?))
        }
        Expr::Bin { op, l, r, .. } if (op == "/" || op == "%") && matches!(**r, Expr::Num { .. }) => RT::Op(op.clone(), Box::new(tree(l, raw)?), Box::new(tree(r, raw)?)),
        _ => return None,
    })
}

/// 変換の拒否の条件 → [(木, 理由)]。木にできない条件があれば None
pub fn requires_tree(part: Option<&Part>) -> Option<Vec<(RT, String)>> {
    let part = part?;
    if part.requires.is_empty() || part.params.is_empty() {
        return None;
    }
    let raw = &part.params[0].0;
    let mut out = vec![];
    for (cond, why) in &part.requires {
        out.push((tree(cond, raw)?, why.clone()));
    }
    Some(out)
}

pub fn requires_json(req: &[(RT, String)]) -> Value {
    Value::Array(req.iter().map(|(t, w)| json!([t.to_json(), w])).collect())
}

struct Overflow;

fn value(t: &RT, s: &[char], env: &mut Vec<(String, i128)>) -> Result<i128, Overflow> {
    let b = |x: bool| x as i128;
    Ok(match t {
        RT::Num(v) => *v,
        RT::Bool(v) => b(*v),
        RT::Raw => 0,
        RT::Var(n) => env.iter().rev().find(|(k, _)| k == n).map(|x| x.1).unwrap_or(0),
        RT::Len => s.len() as i128,
        RT::Only(ps) => b(s.iter().all(|c| ps.iter().any(|(a, z)| *a <= *c as u32 && (*c as u32) <= *z))),
        RT::Digit(e) => {
            let i = value(e, s, env)?;
            if i >= 0 && (i as usize) < s.len() && s[i as usize].is_ascii_digit() {
                s[i as usize] as i128 - '0' as i128
            } else {
                0
            }
        }
        RT::Not(e) => b(value(e, s, env)? == 0),
        RT::If(c, x, y) => {
            if value(c, s, env)? != 0 {
                value(x, s, env)?
            } else {
                value(y, s, env)?
            }
        }
        RT::Sum(n, e) => {
            let mut out: i128 = 0;
            for i in 0..s.len() {
                env.push((n.clone(), i as i128));
                let v = value(e, s, env);
                env.pop();
                out = out.checked_add(v?).ok_or(Overflow)?;
            }
            out
        }
        RT::Func(n, xs) => {
            let mut v = vec![];
            for x in xs {
                v.push(value(x, s, env)?);
            }
            match n.as_str() {
                "min" => *v.iter().min().ok_or(Overflow)?,
                "max" => *v.iter().max().ok_or(Overflow)?,
                _ => v[0].checked_abs().ok_or(Overflow)?,
            }
        }
        RT::Op(op, l, r) => {
            if op == "and" {
                let a = value(l, s, env)?;
                return Ok(if a == 0 { 0 } else { b(value(r, s, env)? != 0) });
            }
            if op == "or" {
                let a = value(l, s, env)?;
                return Ok(if a != 0 { 1 } else { b(value(r, s, env)? != 0) });
            }
            let x = value(l, s, env)?;
            let y = value(r, s, env)?;
            match op.as_str() {
                "<" => b(x < y),
                ">" => b(x > y),
                "<=" => b(x <= y),
                ">=" => b(x >= y),
                "==" => b(x == y),
                "!=" => b(x != y),
                "+" => x.checked_add(y).ok_or(Overflow)?,
                "-" => x.checked_sub(y).ok_or(Overflow)?,
                "*" => x.checked_mul(y).ok_or(Overflow)?,
                "/" => {
                    if y == 0 {
                        return Err(Overflow);
                    }
                    x.div_euclid(y)
                }
                _ => {
                    if y == 0 {
                        return Err(Overflow);
                    }
                    x.rem_euclid(y)
                }
            }
        }
    })
}

/// 値 s を変換が断る理由。通るなら None
pub fn refusal(req: &[(RT, String)], s: &str) -> Option<String> {
    let cs: Vec<char> = s.chars().collect();
    for (t, why) in req {
        match value(t, &cs, &mut vec![]) {
            Ok(v) => {
                if v == 0 {
                    return Some(why.clone());
                }
            }
            Err(_) => return Some("計算があふれました".to_string()),
        }
    }
    None
}
