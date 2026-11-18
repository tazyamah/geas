// SPDX-License-Identifier: GPL-3.0-or-later
//! Python の repr（生成するテストの中に、データを Python のリテラルとして埋めるため）。
use serde_json::Value;

fn printable(c: char) -> bool {
    let u = c as u32;
    if u < 0x20 || u == 0x7f || (0x80..0xa0).contains(&u) {
        return false;
    }
    // 空白の仲間（普通の空白以外）と、書式の制御文字、行・段落の区切り
    !matches!(u, 0xa0 | 0xad | 0x1680 | 0x2000..=0x200f | 0x2028..=0x202f | 0x205f..=0x2064 | 0x3000 | 0xfeff | 0xe000..=0xf8ff)
}

pub fn repr_str(s: &str) -> String {
    let q = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut o = String::from(q);
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if c == q => {
                o.push('\\');
                o.push(c);
            }
            c if printable(c) => o.push(c),
            c => {
                let u = c as u32;
                if u < 0x100 {
                    o.push_str(&format!("\\x{:02x}", u));
                } else if u < 0x10000 {
                    o.push_str(&format!("\\u{:04x}", u));
                } else {
                    o.push_str(&format!("\\U{:08x}", u));
                }
            }
        }
    }
    o.push(q);
    o
}

pub fn repr_opt(s: &Option<String>) -> String {
    match s {
        Some(s) => repr_str(s),
        None => "None".into(),
    }
}

/// JSON の値を Python のリテラルにする（辞書・並び・文字列・数・真偽・None）
pub fn repr_value(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(b) => if *b { "True" } else { "False" }.into(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => repr_str(s),
        Value::Array(a) => format!("[{}]", a.iter().map(repr_value).collect::<Vec<_>>().join(", ")),
        Value::Object(m) => format!("{{{}}}", m.iter().map(|(k, x)| format!("{}: {}", repr_str(k), repr_value(x))).collect::<Vec<_>>().join(", ")),
    }
}
