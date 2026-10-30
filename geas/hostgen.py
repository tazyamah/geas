# SPDX-License-Identifier: GPL-3.0-or-later
"""ブラウザ（WASM）から【処理】を呼ぶための橋渡し host.rs の生成。
JSON の文字列で（名前, 文脈, 保存, 入力）を受け取り、処理を実行して書込を適用し、（結果, 外部作用, 新しい保存）を JSON で返す。
フローの行き先とトリガの宣言は、生成時に JSON 定義として埋め込む。待ち・期限・外部作用の実行は、JS 側の基盤（runtime.js）の仕事。"""
import json
from dataclasses import replace

from .ast import *  # noqa: F401,F403
from .check import Checker, phys
from .rustgen import column_type, opt
from .rest import routes as rest_routes
from .screengen import screens_def
from .spec import fmt_cause
from .mcp import tool_defs

JSON_PRELUDE = r'''// SPDX-License-Identifier: MIT OR Apache-2.0
// 自動生成。手で編集しない。
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
pub fn read_ratio(s: &str, lo: Option<i128>, hi: Option<i128>) -> Result<Ratio, String> {
    let (a, b) = s.split_once('/').ok_or_else(|| "割合は \"分子/分母\" で書いてください".to_string())?;
    let num: i128 = a.trim().parse().map_err(|_| "割合の分子が数ではありません".to_string())?;
    let den: i128 = b.trim().parse().map_err(|_| "割合の分母が数ではありません".to_string())?;
    if den <= 0 || den > RATIO_DEN_MAX { return Err(format!("割合の分母は 1 以上 {} 以下にしてください", RATIO_DEN_MAX)); }
    let (lo, hi) = (lo.unwrap_or(-RATIO_VAL_MAX), hi.unwrap_or(RATIO_VAL_MAX));
    if num < lo.saturating_mul(den) || num > hi.saturating_mul(den) { return Err("範囲外の割合です".into()); }
    Ok(Ratio { num, den })
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
        return f'match Input.get_opt("{x}") {{ Some(v) => Some({from_j(replace(t, optional=False), "v")}), None => None }}'
    return from_j(t, f'Input.get("{x}")?')


def from_j(t: Type, j: str) -> str:
    """JSON 値 j から Rust の値へ（エラーは ? で伝える）。
    無いかもしれない項目は、null なら None。キーそのものが無い（undefined）のはエラー"""
    if t.optional:
        return f"match {j} {{ J::Null => None, v => Some({from_j(replace(t, optional=False), 'v')}) }}"
    k = t.kind
    if t.via is not None:  # raw で受け、via の変換（geas の式）で論理型にする。拒否は入力不正
        return f"{t.via}({j}.as_str()?)?"
    if k in ("ID", "参照", "文字列"):
        return f"{j}.as_str()?.to_string()"
    if k in ("整数", "金額"):
        return f'{phys(t)}::try_from({j}.as_int()?).map_err(|_| "範囲外の数です".to_string())?'
    if k == "日時":
        return f"({j}.as_int()? as i64)"
    if k == "割合":
        r = None if t.trust else t.range  # 人の保証つきの入力は、範囲の確認を validate に任せる（verify／assume の段階に従う）
        return f'read_ratio({j}.as_str()?, {opt(r.lo if r else None)}, {opt(r.hi if r else None)})?'
    if k == "バイト列":
        return f"b64_decode({j}.as_str()?)?"
    if k == "集合":
        return f"{j}.as_set()?"
    if k == "名前":
        return f'{t.target}::from_name({j}.as_str()?).ok_or_else(|| "未知の状態です".to_string())?'
    if k == "合成":
        def comp(pn, pt):  # 未指定にできる成分は、キーが無いのも null も「未指定」
            if pt.optional:
                return (f'match {j}.get_opt("{pn}") {{ Some(v) => Some({from_j(replace(pt, optional=False), "v")}), None => None }}')
            return from_j(pt, j + f'.get("{pn}")?')
        return f"{t.target} {{ {', '.join(pn + ': ' + comp(pn, pt) for pn, pt in t.parts)} }}"
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
        return f"js_str({x}.name())"
    if k == "合成":
        items = ", ".join(f'format!("{{}}:{{}}", js_str("{pn}"), {to_j(pt, f"{x}.{pn}")})' for pn, pt in t.parts)
        return f'format!("{{{{{{}}}}}}", vec![{items}].join(","))'
    if k == "写像":
        return f'format!("{{{{{{}}}}}}", {x}.iter().map(|(k, v)| format!("{{}}:{{}}", js_str(k), {to_j(t.value, "v")})).collect::<Vec<_>>().join(","))'
    raise ValueError(f"JSON への対応がない型: {k}")


CALL_WASM = r'''#[no_mangle]
pub extern "C" fn geas_call(ptr: *const u8, len: usize) -> u64 {
    let s = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) };
    ret(match call(s) { Ok(x) => x, Err(e) => format!("{{\"error\":{}}}", js_str(&e)) })
}
'''


def mock_def(ck: Checker) -> dict:
    """開発用の模擬：外部作用ごとに返る応答と、応答の結果の種類（outcome）。模擬と操作パネルが、これだけで動く"""
    p, show = ck.p, ck.p.show_names
    reps = {r.name: r for r in p.replies}
    kinds = {"ID": "id", "参照": "id", "文字列": "text", "整数": "int", "集合": "set"}
    return {
        "effects": {e.name: {"label": show(e.name), "reply": e.replies} for e in p.effects if e.replies in reps},
        "replies": {r.name: {"label": show(r.name), "fields": [{"name": n, "kind": kinds.get(t.kind, "text")} for n, t in r.params],
                             "outcomes": [{"name": on, "label": show(on), "gives": g} for on, g in (r.outcomes or [("Normal", [n for n, _ in r.params])])]}
                    for r in p.replies if any(e.replies == r.name for e in p.effects)},
        "events": [{"name": ev.name, "label": show(ev.name)} for ev in p.events if ev.on],
    }


def definitions(ck: Checker) -> dict:
    p = ck.p
    flows: dict[str, dict] = {}
    for fl in p.flows:
        for proc, routes in fl.entries:  # 行き先が1つなら文字列、複数なら書いた順の配列
            flows.setdefault(proc, {}).update({v: ds[0] if len(ds) == 1 else ds for v, ds in route_map(routes).items()})
    return {
        "deeds": {x.name: {"inputs": [n for n, _ in x.inputs], "refs": [n for n, _ in x.refs], "new_ids": sorted(set(ck.infos[x.name].new_ids))} for x in p.processes},
        "queries": {q.name: {"inputs": [n for n, _ in q.inputs], "refs": [n for n, _ in q.refs]} for q in p.queries},
        "flows": flows,
        "events": {ev.name: [{"kind": k, "query": q, "deed": d} for k, q, d in ev.steps] for ev in p.events},
        "effects": {e.name: {"args": [n for n, _ in e.params], "joins": e.joins, "no_wait": e.no_wait is not None} for e in p.effects},
        "replies": {r.name: [{"name": n, "required": not t.optional} for n, t in r.params] for r in p.replies},
        "rules": {f.slot: [{"when": [list(c) for c in conds], "value": val} for conds, val in f.rows] for f in p.rules},
        "mock": mock_def(ck),
        "screens": screens_def(ck),
        "routes": [{"method": m, "path": path, "kind": kind, "target": tgt, "resource": rn, "statuses": dict(st),
                    **({"id_input": next((n for n, t in ck.procs[tgt].inputs if t.kind == "ID"), "")} if kind in ("update", "destroy") and tgt in ck.procs else {})}
                   for m, path, kind, tgt, rn, st in rest_routes(ck.p)],
        "mcp": tool_defs(ck),
        "triggers": [{"name": t.name, "args": [n for n, _ in t.params],
                  "causes": [{"kind": k, "key": key, "secs": int(key) if k == "deadline" else None, "replies": key.split(",") if k == "all" else None, "dest": d} for k, key, d in t.causes]} for t in p.triggers],
    }


def host_source(ck: Checker) -> str:
    p = ck.p
    out = [JSON_PRELUDE]
    w = out.append
    # データ・保存の変換
    for d in p.data:
        key = ck.key_field(d.name).name
        w(f"fn {d.name}_from_json(j: &J) -> Result<{d.name}, String> {{\n    Ok({d.name} {{")
        for f in d.fields:
            w(f'        {f.name}: {from_j(f.type, f"j.get(\"{f.name}\")?")},')
        w("    })\n}")
        w(f"fn {d.name}_to_json(r: &{d.name}) -> String {{\n    let mut v: Vec<String> = Vec::new();")
        for f in d.fields:
            w(f'    v.push(format!("{{}}:{{}}", js_str("{f.name}"), {to_j(f.type, f"r.{f.name}")}));')
        w('    format!("{{{}}}", v.join(","))\n}')
    w("pub fn read_store(j: &J) -> Result<Store, String> {\n    let mut s = Store::default();")
    for d in p.data:
        w(f'    if let Some(m) = j.get_opt("{d.name}") {{ for (k, v) in m.as_obj()? {{ s.{d.name}.insert(k.clone(), {d.name}_from_json(v)?); }} }}')
    w("    Ok(s)\n}")
    w("pub fn write_store(s: &Store) -> String {\n    let mut v: Vec<String> = Vec::new();")
    for d in p.data:
        w(f'    v.push(format!("{{}}:{{{{{{}}}}}}", js_str("{d.name}"), s.{d.name}.iter().map(|(k, r)| format!("{{}}:{{}}", js_str(k), {d.name}_to_json(r))).collect::<Vec<_>>().join(",")));')
    w('    format!("{{{}}}", v.join(","))\n}')
    # 外部作用
    w("fn write_effects(e: &Effect) -> String {\n    match " + ("e" if p.effects else "*e") + " {")
    for e in p.effects:
        names = [n for n, _ in e.params]
        pat = ", ".join(names + ["idem_key"])
        args = ",".join(f'{{}}:{{}}' for _ in names)
        vals = ", ".join(f'js_str("{n}"), {to_j(t, n)}' for n, t in e.params)
        w(f'        Effect::{e.name.replace(".", "_")} {{ {pat} }} => format!("{{{{\\"name\\":{{}},\\"args\\":{{{{{args}}}}},\\"idem_key\\":{{}}}}}}", js_str("{e.name}"), {vals}, js_str(idem_key)),')
    w("    }\n}")
    # 結果の中身（名前つき）。ネクサスの次の処理へは、これをそのまま入力として渡す
    for r in p.results:
        w(f"fn {r.name}_body(r: &{r.name}) -> String {{\n    match r {{")
        for v, ps in r.variants:
            if ps:
                names = [f"a{i}" for i in range(len(ps))]
                fmt = ",".join("{}:{}" for _ in ps)
                vals = ", ".join(f'js_str("{n}"), {to_j(t, a)}' for (n, t), a in zip(ps, names))
                w(f'        {r.name}::{v}({", ".join(names)}) => format!("{{{{{fmt}}}}}", {vals}),')
            else:
                w(f'        {r.name}::{v} => "{{}}".to_string(),')
        w("    }\n}")
    for q in p.queries:  # 列つきの問い合わせの行は、列名を持つ JSON のオブジェクトにして返す
        if q.select:
            n = q.name
            cols = [(c, column_type(ck, q, c)) for c in q.select]
            items = ", ".join(f'format!("{{}}:{{}}", js_str("{c.name}"), {to_j(t, f"r.{c.name}")})' for c, t in cols)
            w(f'fn {n}_row_json(r: &{n}Row) -> String {{\n    format!("{{{{{{}}}}}}", vec![{items}].join(","))\n}}')
    # 処理の呼び出し
    w("fn write_result(name: &str, args: Vec<String>, body: String, effects: Vec<String>, Store: &Store) -> String {\n"
      '    format!("{{\\"result\\":{},\\"args\\":{},\\"body\\":{},\\"effects\\":[{}],\\"store\\":{}}}", js_str(name), js_arr(&args), body, effects.join(","), write_store(Store))\n}')
    has_parts = any(pt.external for pt in p.parts)
    w(f"fn call_inner(name: &str, ctx: &J, Input: &J, Store: &mut Store{', parts: &dyn Parts' if has_parts else ''}) -> Result<String, String> {{\n    match name {{")
    for proc in p.processes:
        info = ck.infos[proc.name]
        n = proc.name
        cf = [f"{x}: {from_j(t, f'ctx.get(\"{x}\")?')}" for x, t in proc.refs] + [f'new_{d}_id: ctx.get("new_{d}_id")?.as_str()?.to_string()' for d in sorted(set(info.new_ids))]
        inf = [f"{x}: {input_j(x, t)}" for x, t in proc.inputs]
        w(f'        "{n}" => {{\n            let c = {n}Ctx {{ {", ".join(cf)} }};\n            let i = {n}Input {{ {", ".join(inf)} }};\n            c.validate()?;\n            i.validate()?;')
        extra = ", parts" if info.parts else ""
        w(f"            let o = {n}(&c, Store, &i{extra}).map_err(|f| f.to_string())?;\n            Store.apply(o.Write.clone()).map_err(|f| f.to_string())?;")
        w(f"            Ok(write_result(o.result.name(), o.result.args(), {proc.result}_body(&o.result), o.effects.iter().map(write_effects).collect(), Store))\n        }}")
    for q in p.queries:
        n = q.name
        cf = [f"{x}: {from_j(t, f'ctx.get(\"{x}\")?')}" for x, t in q.refs]
        inf = [f"{x}: {input_j(x, t)}" for x, t in q.inputs]
        w(f'        "{n}" => {{\n            let c = {n}Ctx {{ {", ".join(cf)} }};\n            let i = {n}Input {{ {", ".join(inf)} }};\n            c.validate()?;\n            i.validate()?;')
        if q.select:
            w(f'            let rows = {n}(&c, Store, &i);\n            Ok(write_result("一覧", Vec::new(), format!("{{{{\\"rows\\":[{{}}]}}}}", rows.iter().map({n}_row_json).collect::<Vec<_>>().join(",")), vec![], Store))\n        }}')
        else:
            w(f'            Ok(write_result("一覧", {n}(&c, Store, &i), "{{}}".to_string(), vec![], Store))\n        }}')
    w('        _ => Err(format!("処理「{}」はありません", name)),\n    }\n}')
    w(f"pub fn call(request: &str{', parts: &dyn Parts' if has_parts else ''}) -> Result<String, String> {{\n"
      '    let r = parse(request)?;\n    let mut s = read_store(r.get("store")?)?;\n'
      f'    call_inner(r.get("name")?.as_str()?, r.get("ctx")?, r.get("input")?, &mut s{", parts" if has_parts else ""})\n}}')
    defs = json.dumps(definitions(ck), ensure_ascii=False, separators=(",", ":"))
    w(f'pub const DEFS_JSON: &str = r####"{defs}"####;')
    w(r'''
fn ret(s: String) -> u64 {
    let b = s.into_bytes().into_boxed_slice();
    let l = b.len();
    let p = Box::into_raw(b) as *mut u8;
    ((p as u64) << 32) | (l as u64)
}
#[no_mangle]
pub extern "C" fn alloc(n: usize) -> *mut u8 { let mut v = Vec::<u8>::with_capacity(n); let p = v.as_mut_ptr(); std::mem::forget(v); p }
#[no_mangle]
pub extern "C" fn dealloc(p: *mut u8, n: usize) { unsafe { drop(Vec::from_raw_parts(p, 0, n)); } }
@@CALL@@#[no_mangle]
pub extern "C" fn geas_defs() -> u64 { ret(DEFS_JSON.to_string()) }
'''.replace("@@CALL@@", "" if has_parts else CALL_WASM))
    return "\n".join(out) + "\n"
