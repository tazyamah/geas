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
