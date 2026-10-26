"""ブラウザ（WASM）から【処理】を呼ぶための橋渡し host.rs の生成。
JSON の文字列で（名前, 文脈, 保存, 入力）を受け取り、処理を実行して書込を適用し、（結果, 外部作用, 新しい保存）を JSON で返す。
フローの行き先とトリガの宣言は、生成時に JSON 定義として埋め込む。待ち・期限・外部作用の実行は、JS 側の基盤（runtime.js）の仕事。"""
import json
from dataclasses import replace

from .ast import *  # noqa: F401,F403
from .check import Checker, phys
from .rustgen import opt
from .spec import fmt_cause

JSON_PRELUDE = r'''// 自動生成。手で編集しない。
#![allow(non_snake_case, non_camel_case_types, dead_code, unused_variables, unused_mut)]
use crate::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub enum J { Null, Bool(bool), Num(i128), Str(String), Arr(Vec<J>), Obj(BTreeMap<String, J>) }

impl J {
    pub fn get(&self, k: &str) -> Result<&J, String> {
        match self { J::Obj(m) => m.get(k).ok_or_else(|| format!("項目「{}」がありません", k)), _ => Err("オブジェクトではありません".into()) }
    }
    pub fn get_opt(&self, k: &str) -> Option<&J> {
        match self { J::Obj(m) => match m.get(k) { Some(J::Null) | None => None, Some(x) => Some(x) }, _ => None }
    }
    pub fn as_str(&self) -> Result<&str, String> { match self { J::Str(s) => Ok(s), _ => Err("文字列のはずです".into()) } }
    pub fn as_int(&self) -> Result<i128, String> { match self { J::Num(n) => Ok(*n), _ => Err("整数のはずです".into()) } }
    pub fn as_obj(&self) -> Result<&BTreeMap<String, J>, String> { match self { J::Obj(m) => Ok(m), _ => Err("オブジェクトのはずです".into()) } }
    pub fn as_set(&self) -> Result<BTreeSet<String>, String> {
        match self { J::Arr(a) => a.iter().map(|x| x.as_str().map(|s| s.to_string())).collect(), _ => Err("配列のはずです".into()) }
    }
}

struct P<'a> { s: &'a [u8], i: usize }
impl<'a> P<'a> {
    fn ws(&mut self) { while self.i < self.s.len() && (self.s[self.i] as char).is_whitespace() { self.i += 1; } }
    fn eat(&mut self, c: u8) -> Result<(), String> { self.ws(); if self.i < self.s.len() && self.s[self.i] == c { self.i += 1; Ok(()) } else { Err(format!("JSON: '{}' が必要です（{}文字目）", c as char, self.i)) } }
    fn value(&mut self) -> Result<J, String> {
        self.ws();
        if self.i >= self.s.len() { return Err("JSON: 途中で終わっています".into()); }
        match self.s[self.i] {
            b'{' => { self.i += 1; let mut m = BTreeMap::new(); self.ws();
                if self.s[self.i] == b'}' { self.i += 1; return Ok(J::Obj(m)); }
                loop { let k = match self.value()? { J::Str(s) => s, _ => return Err("JSON: キーは文字列です".into()) }; self.eat(b':')?; let v = self.value()?; m.insert(k, v); self.ws();
                    if self.s[self.i] == b',' { self.i += 1; } else { self.eat(b'}')?; return Ok(J::Obj(m)); } } }
            b'[' => { self.i += 1; let mut a = Vec::new(); self.ws();
                if self.s[self.i] == b']' { self.i += 1; return Ok(J::Arr(a)); }
                loop { a.push(self.value()?); self.ws(); if self.s[self.i] == b',' { self.i += 1; } else { self.eat(b']')?; return Ok(J::Arr(a)); } } }
            b'"' => { self.i += 1; let mut out: Vec<u8> = Vec::new();
                loop { if self.i >= self.s.len() { return Err("JSON: 文字列が閉じていません".into()); } let c = self.s[self.i]; self.i += 1;
                    match c { b'"' => break, b'\\' => { let e = self.s[self.i]; self.i += 1;
                        match e { b'n' => out.push(b'\n'), b't' => out.push(b'\t'), b'r' => out.push(b'\r'), b'b' => out.push(8), b'f' => out.push(12),
                            b'u' => { let h = std::str::from_utf8(&self.s[self.i..self.i + 4]).map_err(|_| "JSON: \\u".to_string())?; let mut cp = u32::from_str_radix(h, 16).map_err(|_| "JSON: \\u".to_string())?; self.i += 4;
                                if (0xD800..0xDC00).contains(&cp) && self.s.get(self.i) == Some(&b'\\') && self.s.get(self.i + 1) == Some(&b'u') {
                                    let h2 = std::str::from_utf8(&self.s[self.i + 2..self.i + 6]).map_err(|_| "JSON: \\u".to_string())?; let lo = u32::from_str_radix(h2, 16).map_err(|_| "JSON: \\u".to_string())?; self.i += 6;
                                    cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00); }
                                let ch = char::from_u32(cp).unwrap_or('\u{FFFD}'); let mut b = [0u8; 4]; out.extend_from_slice(ch.encode_utf8(&mut b).as_bytes()); }
                            x => out.push(x) } }
                        x => out.push(x) } }
                Ok(J::Str(String::from_utf8(out).map_err(|_| "JSON: UTF-8 でありません".to_string())?)) }
            b't' => { self.i += 4; Ok(J::Bool(true)) }
            b'f' => { self.i += 5; Ok(J::Bool(false)) }
            b'n' => { self.i += 4; Ok(J::Null) }
            _ => { let st = self.i; if self.s[self.i] == b'-' { self.i += 1; } while self.i < self.s.len() && (self.s[self.i] as char).is_ascii_digit() { self.i += 1; }
                std::str::from_utf8(&self.s[st..self.i]).unwrap().parse::<i128>().map(J::Num).map_err(|_| format!("JSON: 数が読めません（{}文字目）", st)) }
        }
    }
}
pub fn parse(s: &str) -> Result<J, String> { let mut p = P { s: s.as_bytes(), i: 0 }; let v = p.value()?; Ok(v) }

pub fn js_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() { match c { '"' => o.push_str("\\\""), '\\' => o.push_str("\\\\"), '\n' => o.push_str("\\n"), '\r' => o.push_str("\\r"), '\t' => o.push_str("\\t"),
        c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)), c => o.push(c) } }
    o.push('"'); o
}
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
pub fn b64_encode(b: &[u8]) -> String {
    let mut o = String::with_capacity((b.len() + 2) / 3 * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        o.push(B64[(n >> 18) as usize & 63] as char);
        o.push(B64[(n >> 12) as usize & 63] as char);
        o.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        o.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    o
}
/// 割合は "分子/分母" の文字列で受け取る（小数にすると丸めが入るため）。範囲の宣言もここで確かめる
pub fn 有理を読む(s: &str, lo: Option<i128>, hi: Option<i128>) -> Result<有理, String> {
    let (a, b) = s.split_once('/').ok_or_else(|| "割合は \"分子/分母\" で書いてください".to_string())?;
    let 分子: i128 = a.trim().parse().map_err(|_| "割合の分子が数ではありません".to_string())?;
    let 分母: i128 = b.trim().parse().map_err(|_| "割合の分母が数ではありません".to_string())?;
    if 分母 <= 0 { return Err("割合の分母は 1 以上にしてください".into()); }
    if lo.map_or(false, |l| 分子 < l * 分母) || hi.map_or(false, |h| 分子 > h * 分母) { return Err("範囲外の割合です".into()); }
    Ok(有理 { 分子, 分母 })
}
pub fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    let mut o = Vec::with_capacity(s.len() / 4 * 3);
    let (mut n, mut k) = (0u32, 0);
    for ch in s.bytes() {
        if ch == b'=' { break; }
        let v = B64.iter().position(|&x| x == ch).ok_or("バイト列の符号化が不正です")? as u32;
        n = n << 6 | v; k += 1;
        if k == 4 { o.extend([(n >> 16) as u8, (n >> 8) as u8, n as u8]); n = 0; k = 0; }
    }
    if k == 2 { o.push((n >> 4) as u8); } else if k == 3 { o.extend([(n >> 10) as u8, (n >> 2) as u8]); }
    Ok(o)
}
pub fn js_set(s: &BTreeSet<String>) -> String { format!("[{}]", s.iter().map(|x| js_str(x)).collect::<Vec<_>>().join(",")) }
pub fn js_arr(v: &[String]) -> String { format!("[{}]", v.iter().map(|x| js_str(x)).collect::<Vec<_>>().join(",")) }
'''


def input_j(x: str, t: Type) -> str:
    """処理と問い合わせの入力。無いかもしれない入力は、キーが無いのも null も同じ「無い」。
    必ず要る入力（required）は、キーが無くても null でも入力不正"""
    if t.optional:
        return f'match 入力.get_opt("{x}") {{ Some(v) => Some({from_j(replace(t, optional=False), "v")}), None => None }}'
    return from_j(t, f'入力.get("{x}")?')


def from_j(t: Type, j: str) -> str:
    """JSON 値 j から Rust の値へ（エラーは ? で伝える）。
    無いかもしれない項目は、null なら None。キーそのものが無い（undefined）のはエラー"""
    if t.optional:
        return f"match {j} {{ J::Null => None, v => Some({from_j(replace(t, optional=False), 'v')}) }}"
    k = t.kind
    if k in ("ID", "参照", "文字列"):
        return f"{j}.as_str()?.to_string()"
    if k in ("整数", "金額"):
        return f'{phys(t)}::try_from({j}.as_int()?).map_err(|_| "範囲外の数です".to_string())?'
    if k == "日時":
        return f"({j}.as_int()? as i64)"
    if k == "割合":
        r = t.range
        return f'有理を読む({j}.as_str()?, {opt(r.lo if r else None)}, {opt(r.hi if r else None)})?'
    if k == "バイト列":
        return f"b64_decode({j}.as_str()?)?"
    if k == "集合":
        return f"{j}.as_set()?"
    if k == "名前":
        return f'{t.target}::名前から({j}.as_str()?).ok_or_else(|| "未知の状態です".to_string())?'
    if k == "写像":
        return f"{j}.as_obj()?.iter().map(|(k, v)| Ok((k.clone(), {from_j(t.value, 'v')}))).collect::<Result<BTreeMap<String, _>, String>>()?"
    raise ValueError(f"JSON への対応がない型: {k}")


def to_j(t: Type, x: str) -> str:
    """Rust の値 x から JSON 文字列へ（式。String を返す）。None は null"""
    if t.optional:
        return f'match &{x} {{ None => "null".to_string(), Some(v) => {to_j(replace(t, optional=False), "v")} }}'
    k = t.kind
    if k in ("ID", "参照", "文字列"):
        return f"js_str(&{x})"
    if k in ("整数", "金額", "日時"):
        return f"{x}.to_string()"
    if k == "バイト列":
        return f"js_str(&b64_encode(&{x}))"
    if k == "割合":
        return f"js_str(&{x}.to_string())"
    if k == "集合":
        return f"js_set(&{x})"
    if k == "名前":
        return f"js_str({x}.名前())"
    if k == "写像":
        return f'format!("{{{{{{}}}}}}", {x}.iter().map(|(k, v)| format!("{{}}:{{}}", js_str(k), {to_j(t.value, "v")})).collect::<Vec<_>>().join(","))'
    raise ValueError(f"JSON への対応がない型: {k}")


def definitions(ck: Checker) -> dict:
    p = ck.p
    flows: dict[str, dict[str, str]] = {}
    for fl in p.flows:
        for proc, routes in fl.entries:
            flows.setdefault(proc, {}).update(dict(routes))
    return {
        "処理": {x.name: {"入力": [n for n, _ in x.inputs], "参照": [n for n, _ in x.refs], "採番": sorted(set(ck.infos[x.name].new_ids))} for x in p.processes},
        "問い合わせ": {q.name: {"入力": [n for n, _ in q.inputs], "参照": [n for n, _ in q.refs]} for q in p.queries},
        "フロー": flows,
        "出来事": {ev.name: [{"種別": k, "問い合わせ": q, "処理": d} for k, q, d in ev.steps] for ev in p.events},
        "トリガ": [{"名前": t.name, "引数": [n for n, _ in t.params],
                  "要因": [{"種別": k, "キー": key, "秒": int(key) if k == "期限" else None, "行き先": d} for k, key, d in t.causes]} for t in p.triggers],
    }


def host_source(ck: Checker) -> str:
    p = ck.p
    out = [JSON_PRELUDE]
    w = out.append
    # データ・保存の変換
    for d in p.data:
        key = ck.key_field(d.name).name
        w(f"fn {d.name}_から(j: &J) -> Result<{d.name}, String> {{\n    Ok({d.name} {{")
        for f in d.fields:
            w(f'        {f.name}: {from_j(f.type, f"j.get(\"{f.name}\")?")},')
        w("    })\n}")
        w(f"fn {d.name}_へ(r: &{d.name}) -> String {{\n    let mut v: Vec<String> = Vec::new();")
        for f in d.fields:
            w(f'    v.push(format!("{{}}:{{}}", js_str("{f.name}"), {to_j(f.type, f"r.{f.name}")}));')
        w('    format!("{{{}}}", v.join(","))\n}')
    w("pub fn 保存を読む(j: &J) -> Result<保存, String> {\n    let mut s = 保存::default();")
    for d in p.data:
        w(f'    if let Some(m) = j.get_opt("{d.name}") {{ for (k, v) in m.as_obj()? {{ s.{d.name}.insert(k.clone(), {d.name}_から(v)?); }} }}')
    w("    Ok(s)\n}")
    w("pub fn 保存を書く(s: &保存) -> String {\n    let mut v: Vec<String> = Vec::new();")
    for d in p.data:
        w(f'    v.push(format!("{{}}:{{{{{{}}}}}}", js_str("{d.name}"), s.{d.name}.iter().map(|(k, r)| format!("{{}}:{{}}", js_str(k), {d.name}_へ(r))).collect::<Vec<_>>().join(",")));')
    w('    format!("{{{}}}", v.join(","))\n}')
    # 外部作用
    w("fn 外部を書く(e: &外部作用) -> String {\n    match " + ("e" if p.effects else "*e") + " {")
    for e in p.effects:
        names = [n for n, _ in e.params]
        pat = ", ".join(names + ["冪等キー"])
        args = ",".join(f'{{}}:{{}}' for _ in names)
        vals = ", ".join(f'js_str("{n}"), {to_j(t, n)}' for n, t in e.params)
        w(f'        外部作用::{e.name.replace(".", "_")} {{ {pat} }} => format!("{{{{\\"名前\\":{{}},\\"引数\\":{{{{{args}}}}},\\"冪等キー\\":{{}}}}}}", js_str("{e.name}"), {vals}, js_str(冪等キー)),')
    w("    }\n}")
    # 結果の中身（名前つき）。ネクサスの次の処理へは、これをそのまま入力として渡す
    for r in p.results:
        w(f"fn {r.name}_中身(r: &{r.name}) -> String {{\n    match r {{")
        for v, ps in r.variants:
            if ps:
                names = [f"a{i}" for i in range(len(ps))]
                fmt = ",".join("{}:{}" for _ in ps)
                vals = ", ".join(f'js_str("{n}"), {to_j(t, a)}' for (n, t), a in zip(ps, names))
                w(f'        {r.name}::{v}({", ".join(names)}) => format!("{{{{{fmt}}}}}", {vals}),')
            else:
                w(f'        {r.name}::{v} => "{{}}".to_string(),')
        w("    }\n}")
    # 処理の呼び出し
    w("fn 結果を書く(名前: &str, 引数: Vec<String>, 中身: String, 外部: Vec<String>, 保存: &保存) -> String {\n"
      '    format!("{{\\"結果\\":{},\\"引数\\":{},\\"中身\\":{},\\"外部\\":[{}],\\"保存\\":{}}}", js_str(名前), js_arr(&引数), 中身, 外部.join(","), 保存を書く(保存))\n}')
    w("fn 呼ぶ内部(名前: &str, ctx: &J, 入力: &J, 保存: &mut 保存) -> Result<String, String> {\n    match 名前 {")
    for proc in p.processes:
        info = ck.infos[proc.name]
        n = proc.name
        cf = [f"{x}: {from_j(t, f'ctx.get(\"{x}\")?')}" for x, t in proc.refs] + [f'新{d}ID: ctx.get("新{d}ID")?.as_str()?.to_string()' for d in sorted(set(info.new_ids))]
        inf = [f"{x}: {input_j(x, t)}" for x, t in proc.inputs]
        w(f'        "{n}" => {{\n            let c = {n}文脈 {{ {", ".join(cf)} }};\n            let i = {n}入力 {{ {", ".join(inf)} }};\n            c.検査()?;\n            i.検査()?;')
        w(f"            let o = {n}(&c, 保存, &i);\n            保存.適用(o.書込.clone());")
        w(f"            Ok(結果を書く(o.結果.名前(), o.結果.引数(), {proc.result}_中身(&o.結果), o.外部.iter().map(外部を書く).collect(), 保存))\n        }}")
    for q in p.queries:
        n = q.name
        cf = [f"{x}: {from_j(t, f'ctx.get(\"{x}\")?')}" for x, t in q.refs]
        inf = [f"{x}: {input_j(x, t)}" for x, t in q.inputs]
        shown = ".iter().map(|x| x.表示()).collect()" if q.select else ""
        w(f'        "{n}" => {{\n            let c = {n}文脈 {{ {", ".join(cf)} }};\n            let i = {n}入力 {{ {", ".join(inf)} }};\n            c.検査()?;\n            i.検査()?;')
        w(f'            Ok(結果を書く("一覧", {n}(&c, 保存, &i){shown}, "{{}}".to_string(), vec![], 保存))\n        }}')
    w('        _ => Err(format!("処理「{}」はありません", 名前)),\n    }\n}')
    w("pub fn 呼ぶ(要求: &str) -> Result<String, String> {\n"
      '    let r = parse(要求)?;\n    let mut s = 保存を読む(r.get("保存")?)?;\n'
      '    呼ぶ内部(r.get("名前")?.as_str()?, r.get("文脈")?, r.get("入力")?, &mut s)\n}')
    defs = json.dumps(definitions(ck), ensure_ascii=False, separators=(",", ":"))
    w(f'pub const 定義JSON: &str = r####"{defs}"####;')
    w(r'''
fn 返す(s: String) -> u64 {
    let b = s.into_bytes().into_boxed_slice();
    let l = b.len();
    let p = Box::into_raw(b) as *mut u8;
    ((p as u64) << 32) | (l as u64)
}
#[no_mangle]
pub extern "C" fn alloc(n: usize) -> *mut u8 { let mut v = Vec::<u8>::with_capacity(n); let p = v.as_mut_ptr(); std::mem::forget(v); p }
#[no_mangle]
pub extern "C" fn dealloc(p: *mut u8, n: usize) { unsafe { drop(Vec::from_raw_parts(p, 0, n)); } }
#[no_mangle]
pub extern "C" fn geas_call(ptr: *const u8, len: usize) -> u64 {
    let s = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) };
    返す(match 呼ぶ(s) { Ok(x) => x, Err(e) => format!("{{\"エラー\":{}}}", js_str(&e)) })
}
#[no_mangle]
pub extern "C" fn geas_defs() -> u64 { 返す(定義JSON.to_string()) }
''')
    return "\n".join(out) + "\n"
