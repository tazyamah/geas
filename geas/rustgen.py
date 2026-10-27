# SPDX-License-Identifier: GPL-3.0-or-later
"""Rust の生成。検査器を通ったコードだけを受け付ける。【処理】は純粋な関数
（文脈, 保存データ, 入力）→（結果, 書込の列, 外部作用の要求の列）として生成し、実行は基盤に任せる。"""
from dataclasses import replace

from .ast import *  # noqa: F401,F403
from .check import Checker, ProcInfo, column_type, phys, tok_of

PRELUDE = r'''// SPDX-License-Identifier: MIT OR Apache-2.0
// 自動生成。手で編集しない（コードから geas が毎回生成する）。
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code, unused_variables, unused_mut)]
#![allow(uncommon_codepoints, mixed_script_confusables, unused_parens)]
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;

/// 端数の出る値（分子/分母）。丸めの指定が無いと整数にできない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ratio {
    pub num: i128,
    pub den: i128,
}
impl Display for Ratio {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}
pub fn scale(x: i128, r: Ratio) -> Ratio {
    Ratio { num: x * r.num, den: r.den }
}
pub fn floor(x: Ratio) -> i128 {
    x.num.div_euclid(x.den)
}
pub fn ceil(x: Ratio) -> i128 {
    -((-x.num).div_euclid(x.den))
}
pub fn half_up(x: Ratio) -> i128 {
    (2 * x.num + x.den).div_euclid(2 * x.den)
}
pub fn check_range(name: &str, x: i128, lo: Option<i128>, hi: Option<i128>) -> Result<(), String> {
    if lo.map_or(false, |l| x < l) || hi.map_or(false, |h| x > h) {
        return Err(format!("{}={} は範囲外", name, x));
    }
    Ok(())
}
/// 時刻（秒）の絶対値の上限。期間の足し引きが i64 であふれないよう、外から入る値を境界で止める
pub const TIME_LIMIT: i128 = 1_000_000_000_000_000;
pub fn check_chars(name: &str, s: &str, allowed: &[(char, char)]) -> Result<(), String> {
    match s.chars().find(|c| !allowed.iter().any(|&(a, b)| a <= *c && *c <= b)) {
        Some(c) => Err(format!("{} に使えない文字 {:?} があります", name, c)),
        None => Ok(()),
    }
}
/// 冪等キーの組を1つの文字列に（["a","b"] の形。" と \\ は逃がすので、区切りがあいまいにならない）
pub fn join_idem_key(xs: &[String]) -> String {
    let q: Vec<String> = xs.iter().map(|x| format!("\"{}\"", x.replace('\\', "\\\\").replace('"', "\\\""))).collect();
    format!("[{}]", q.join(","))
}
pub fn check_ratio(name: &str, x: &Ratio, lo: Option<i128>, hi: Option<i128>) -> Result<(), String> {
    if x.den <= 0 || lo.map_or(false, |l| x.num < l * x.den) || hi.map_or(false, |h| x.num > h * x.den) {
        return Err(format!("{}={}/{} は範囲外", name, x.num, x.den));
    }
    Ok(())
}
pub fn in_range(name: &str, x: i128, lo: Option<i128>, hi: Option<i128>) -> i128 {
    check_range(name, x, lo, hi).expect("範囲の宣言に反する値を書こうとした");
    x
}
pub fn show_bytes(b: &[u8]) -> String { format!("0x{}", b.iter().map(|x| format!("{:02x}", x)).collect::<String>()) }
/// 検索で同一視する軸をそろえた文字列。大小は小文字へ、かなはカタカナをひらがなへ、幅は全角の英数記号を半角へ
pub fn fold_text(s: &str, fold_case: bool, fold_kana: bool, fold_width: bool) -> String {
    s.chars().flat_map(|c| {
        let mut c = c;
        if fold_width && ('\u{FF01}'..='\u{FF5E}').contains(&c) { c = char::from_u32(c as u32 - 0xFEE0).unwrap_or(c); }
        if fold_kana && ('\u{30A1}'..='\u{30F6}').contains(&c) { c = char::from_u32(c as u32 - 0x60).unwrap_or(c); }
        if fold_case { c.to_lowercase().collect::<Vec<_>>() } else { vec![c] }
    }).collect()
}
pub fn show_set(s: &BTreeSet<String>) -> String {
    format!("{{{}}}", s.iter().cloned().collect::<Vec<_>>().join(","))
}
pub fn show_map<V>(m: &BTreeMap<String, V>, show: impl Fn(&V) -> String) -> String {
    format!("{{{}}}", m.iter().map(|(k, v)| format!("{}:{}", k, show(v))).collect::<Vec<_>>().join(","))
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dest {
    Deed(&'static str),
    Wait(&'static str),
    Reply,
}
pub struct TriggerDef {
    pub name: &'static str,
    /// (種別, 要因, 行き先の処理)。期限の要因は秒数。先に起きた要因が勝つ（実行は基盤）。
    pub causes: &'static [(&'static str, &'static str, &'static str)],
}
'''


def rust_type(t: Type) -> str:
    k = t.kind
    if k in ("ID", "参照", "文字列"):
        return "String"
    if k == "日時":
        return "i64"
    if k == "バイト列":
        return "Vec<u8>"
    if k in ("整数", "金額"):
        return phys(t)
    if k == "割合":
        return "Ratio"
    if k in ("名前", "合成"):
        return t.target
    if k == "写像":
        return f"BTreeMap<String, {rust_type(t.value)}>"
    if k == "集合":
        return "BTreeSet<String>"
    raise ValueError(k)


def chars_rust(cs: tuple) -> str:
    return "&[" + ", ".join(f"('\\u{{{a:x}}}', '\\u{{{b:x}}}')" for a, b in cs) + "]"


def value_checks(label: str, t: Type, x: str, deref: bool) -> list[str]:
    """値 x（Rust の場所の式）の、型の範囲の宣言の検査の行。deref なら x は参照（無いかもしれない値の中身）"""
    chk = []
    num = f"*{x}" if deref else x
    ref = x if deref else f"&{x}"
    if t.kind == "日時":
        chk.append(f'check_range("{label}", {num} as i128, Some(-TIME_LIMIT), Some(TIME_LIMIT))?;')
    if t.kind == "文字列" and t.range:
        chk.append(f'check_range("{label}の長さ", {x}.chars().count() as i128, {opt(t.range.lo)}, {opt(t.range.hi)})?;')
    if t.kind == "文字列" and t.chars is not None:
        chk.append(f'check_chars("{label}", &{x}, {chars_rust(t.chars)})?;')
    if t.kind == "バイト列" and t.range:
        chk.append(f'check_range("{label}の長さ", {x}.len() as i128, {opt(t.range.lo)}, {opt(t.range.hi)})?;')
    if t.kind in ("整数", "金額") and t.range:
        chk.append(f'check_range("{label}", {num} as i128, {opt(t.range.lo)}, {opt(t.range.hi)})?;')
    if t.kind == "割合" and t.range:
        chk.append(f'check_ratio("{label}", {ref}, {opt(t.range.lo)}, {opt(t.range.hi)})?;')
    if t.kind == "写像" and t.value.kind in ("整数", "金額") and t.value.range:
        r = t.value.range
        chk.append(f'for v in {x}.values() {{ check_range("{label}", *v as i128, {opt(r.lo)}, {opt(r.hi)})?; }}')
    if t.kind == "合成":
        for pn, pt in t.parts:
            if pt.optional:  # 未指定にできる成分は、あるときだけ確かめる
                inner = value_checks(f"{label}.{pn}", replace(pt, optional=False), "v", True)
                chk += [f"if let Some(v) = &{x}.{pn} {{ {c} }}" for c in inner]
            else:
                chk += value_checks(f"{label}.{pn}", pt, f"{x}.{pn}", False)
    return chk


def range_checks(items) -> list[str]:
    """外から受け取る値（入力・slot）の、範囲の宣言の検査の行。無いかもしれない値は、あるときだけ確かめる。
    """
    out = []
    for name, t in items:
        opt_ = getattr(t, "optional", False)
        chk = value_checks(name, t, "x" if opt_ else f"self.{name}", opt_)
        out += [f"        if let Some(x) = &self.{name} {{ {c} }}" if opt_ else f"        {c}" for c in chk]
    return out


def field_rust(t: Type) -> str:
    """項目の Rust の型。無いかもしれない項目は Option"""
    return f"Option<{rust_type(t)}>" if t.optional else rust_type(t)


def show_expr(t: Type, x: str) -> str:
    """値の表示用の Rust 式（突合のための正規の書式）。null は「null」"""
    if t.optional:
        return f'{x}.as_ref().map_or("null".to_string(), |v| format!("{{}}", {show_expr(replace(t, optional=False), "v")}))'
    k = t.kind
    if k == "名前":
        return f"{x}.name()"
    if k == "合成":
        return f'format!("{{:?}}", &{x})'
    if k == "写像":
        return f"show_map(&{x}, |v| format!(\"{{}}\", {show_expr(t.value, 'v')}))"
    if k == "集合":
        return f"show_set(&{x})"
    if k == "バイト列":
        return f"show_bytes(&{x})"
    return x


CONVERSIONS = ("money", "int", "time")


def opt(v):
    return "None" if v is None else f"Some({v})"


class Gen:
    def __init__(self, ck: Checker, convert_fns: bool = False):
        assert ck.ready, "検査にエラーか問いが残っているので生成しません"
        self.ck = ck
        self.p = ck.p
        self.convert_fns = convert_fns
        self.out: list[str] = []

    def w(self, s=""):
        self.out.append(s)

    # ---- 全体 ----
    def lib(self) -> str:
        self.out = [PRELUDE]
        for crate, _ in self.p.extern_crates:
            self.w(f"pub use {crate}::*;  // ソースなしのパッケージ：型と結果の形の定義はパッケージの crate にある")
        self.states()
        self.records()
        self.writes_enum()
        self.effects()
        self.parts()
        if self.convert_fns:
            self.convert_fns_out()
        self.results()
        for proc in self.p.processes:
            ProcGen(self, proc).emit()
        for q in self.p.queries:
            self.query(q)
        self.flows()
        self.triggers()
        return "\n".join(self.out) + "\n"

    def query(self, q):
        """問い合わせ（読むだけの処理）：保存を読み、条件・絞り込み・並びで選んだ対象のIDの列を返す。書込も外部作用も無い。"""
        n = q.name
        fields = {f.name: f for f in self.ck.data[q.target].fields}
        key = self.key(q.target)
        ins = dict(q.inputs)

        def ty(t):
            r = rust_type(t)
            return f"Option<{r}>" if t.optional else r

        self.w(f"// ---- 問い合わせ {n}（読むだけ） ----")
        self.w(f"pub struct {n}Input {{")
        for name, t in q.inputs:
            self.w(f"    pub {name}: {ty(t)},")
        self.w("}")
        self.w(f"impl {n}Input {{\n    pub fn validate(&self) -> Result<(), String> {{")
        for line in range_checks(q.inputs):
            self.w(line)
        self.w("        Ok(())\n    }\n}")
        self.w(f"pub struct {n}Ctx {{")
        for name, t in q.refs:
            self.w(f"    pub {name}: {rust_type(t)},")
        self.w("}")
        self.w(f"impl {n}Ctx {{\n    /// 外から埋まる値（slot）の、範囲の宣言の検査\n    pub fn validate(&self) -> Result<(), String> {{")
        for line in range_checks(q.refs):
            self.w(line)
        self.w("        Ok(())\n    }\n}")
        if q.select:
            cols = [(c, column_type(self.ck, q, c)) for c in q.select]
            self.w("#[derive(Clone, Debug, PartialEq)]")
            self.w(f"pub struct {n}Row {{")
            for c, t in cols:
                self.w(f"    pub {c.name}: {'i128' if c.kind in ('合計', '件数') else field_rust(t)},")  # 集計は桁あふれしない広い整数
            self.w("}")
            fmt = "/".join(f"{c.name}={{}}" for c, _ in cols)
            vals = ", ".join(show_expr(t, f"self.{c.name}") for c, t in cols)
            self.w(f'impl {n}Row {{\n    pub fn show(&self) -> String {{ format!("{fmt}", {vals}) }}\n}}')
        out_ty = f"{n}Row" if q.select else "String"
        self.w(f"pub fn {n}(ctx: &{n}Ctx, Store: &Store, Input: &{n}Input) -> Vec<{out_ty}> {{")
        self.w(f"    let mut rows: Vec<&{q.target}> = Store.{q.target}.values().filter(|r| {{")
        def atom(f, op, st):
            if not st.isdigit():
                return f"r.{f} {op} {fields[f].type.target}::{st}"
            if fields[f].type.optional:  # null はどの比較も満たさない
                return f"r.{f}.map_or(false, |v| (v as i128) {op} {st})"
            return f"(r.{f} as i128) {op} {st}"

        for group in q.conds:
            atoms = [atom(f, op, st) for f, op, st in group]
            self.w(f"        if !({' || '.join(atoms)}) {{ return false; }}")
        for f, op, a in q.filters:
            nul = fields[f].type.optional
            v = "(*fv)" if nul else f"r.{f}"

            def guard(cond):  # null は絞り込みを満たさない。値があれば fv で比べる
                return f"match &r.{f} {{ Some(fv) => {cond}, None => false }}" if nul else cond
            if op == "直近":
                self.w(f"        if !({guard(f'{v} >= ctx.現在 - {a}')}) {{ return false; }}")
                continue
            t = ins[a]
            if op.startswith("含む"):
                folds = op[3:-1].split(",") if "(" in op else []
                flags = ", ".join("true" if k in folds else "false" for k in ("大小無視", "かな無視", "幅無視"))
                cond = f"fold_text(&{v}, {flags}).contains(&fold_text(x, {flags}))" if folds else f"{v}.contains(x.as_str())"
            else:
                cond = {"以上": f"({v} as i128) >= (*x as i128)", "以下": f"({v} as i128) <= (*x as i128)"}[op]  # 物理数の幅が違っても比べられる
            cond = guard(cond)
            if t.optional:
                self.w(f"        if let Some(x) = &Input.{a} {{ if !({cond}) {{ return false; }} }}")
            else:
                self.w(f"        {{ let x = &Input.{a}; if !({cond}) {{ return false; }} }}")
        self.w("        true\n    }).collect();")
        if q.order:
            f, d = q.order
            a, b = ("b", "a") if d == "新しい順" else ("a", "b")
            nulls = f"a.{f}.is_none().cmp(&b.{f}.is_none()).then(" if fields[f].type.optional else "("  # null は最後
            self.w(f"    rows.sort_by(|a, b| {nulls}{a}.{f}.cmp(&{b}.{f})).then({a}.{key}.cmp(&{b}.{key})));")
        else:
            self.w(f"    rows.sort_by(|a, b| a.{key}.cmp(&b.{key}));")
        if q.page or q.page_n:
            if q.page_n:
                n_expr = str(q.page_n)
            else:
                t = ins[q.page]
                n_expr = f"Input.{q.page}.map_or(rows.len(), |x| x as usize)" if t.optional else f"Input.{q.page} as usize"
            self.w(f"    rows.truncate({n_expr});  // 1ページ分（並びは上で決めてある）")
        if not q.select:
            self.w(f"    rows.into_iter().map(|r| r.{key}.clone()).collect()\n}}\n")
            return
        self.w("    rows.into_iter().filter_map(|r| {")
        for via in dict.fromkeys(c.via for c in q.select if c.kind == "参照先"):
            tgt = fields[via].type.target
            if fields[via].type.optional:  # 参照が null なら、その先の列は null
                self.w(f"        let j_{via} = match &r.{via} {{ Some(k) => Some(Store.{tgt}.get(k)?), None => None }};  // 参照先が無い行は出さない")
            else:
                self.w(f"        let j_{via} = Store.{tgt}.get(&r.{via})?;  // 参照先が無い行は出さない")
        parts = []
        for c, _ in cols:
            if c.kind == "参照先":
                tf = next(x for x in self.ck.data[fields[c.via].type.target].fields if x.name == c.field)
                if fields[c.via].type.optional:
                    get = "and_then(|j| j.{0}.clone())" if tf.type.optional else "map(|j| j.{0}.clone())"
                    parts.append(f"{c.name}: j_{c.via}." + get.format(c.field))
                else:
                    parts.append(f"{c.name}: j_{c.via}.{c.field}.clone()")
            elif c.kind == "合計":
                parts.append(f"{c.name}: r.{c.field}.iter().flat_map(|m| m.values()).map(|v| *v as i128).sum::<i128>()"
                             if fields[c.field].type.optional else f"{c.name}: r.{c.field}.values().map(|v| *v as i128).sum::<i128>()")
            elif c.kind == "件数":
                parts.append(f"{c.name}: r.{c.field}.as_ref().map_or(0, |m| m.len()) as i128"
                             if fields[c.field].type.optional else f"{c.name}: r.{c.field}.len() as i128")
            else:
                parts.append(f"{c.name}: r.{c.field}.clone()")
        self.w(f"        Some({n}Row {{ {', '.join(parts)} }})")
        self.w("    }).collect()\n}\n")

    def states(self):
        for s in self.p.states:
            vs = [s.initial, *s.middle, *s.final]
            self.w("#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]")
            self.w(f"pub enum {s.name} {{ {', '.join(vs)} }}")
            self.w(f"impl {s.name} {{")
            self.w("    pub fn from_name(s: &str) -> Option<Self> {\n        match s {")
            for v in vs:
                self.w(f'            "{v}" => Some({s.name}::{v}),')
            self.w("            _ => None,\n        }\n    }")
            self.w("    pub fn name(&self) -> &'static str {\n        match self {")
            for v in vs:
                self.w(f'            {s.name}::{v} => "{v}",')
            self.w("        }\n    }\n}\n")

    def key(self, d):
        return self.ck.key_field(d).name

    def records(self):
        for c in self.p.composites:
            if c.target in self.p.extern_names:
                continue  # ソースなしのパッケージの型は、パッケージの crate にある
            self.w("#[derive(Clone, Debug, PartialEq)]")
            self.w(f"pub struct {c.target} {{")
            for pn, pt in c.parts:
                self.w(f"    pub {pn}: {field_rust(pt)},")
            self.w("}\n")
        for d in self.p.data:
            self.w("#[derive(Clone, Debug, PartialEq)]")
            self.w(f"pub struct {d.name} {{")
            for f in d.fields:
                self.w(f"    pub {f.name}: {field_rust(f.type)},")
            self.w("}\n")
        self.w("#[derive(Clone, Debug, Default)]\npub struct Store {")
        for d in self.p.data:
            self.w(f"    pub {d.name}: BTreeMap<String, {d.name}>,")
        self.w("}\n")
        # 書込の適用と差分
        self.w("impl Store {")
        self.w("    pub fn apply(&mut self, writes: Vec<Write>) {\n        for w in writes {\n            match w {")
        for kind, d, f in self.write_kinds():
            if f is None:
                self.w(f"                Write::{d}_New(r) => {{ self.{d}.insert(r.{self.key(d)}.clone(), r); }}")
            else:
                # 書込先は、処理が存在を確かめて引いた1件（検査で示してある）。実行時には確かめない
                self.w(f"                Write::{d}_{f}(id, v) => {{ unsafe {{ self.{d}.get_mut(&id).unwrap_unchecked() }}.{f} = v; }}")
        self.w("            }\n        }\n    }")
        self.w("    pub fn diff(&self, after: &Store) -> Vec<String> {\n        let mut out: Vec<String> = Vec::new();")
        for d in self.p.data:
            key = self.key(d.name)
            others = [f for f in d.fields if f.name != key]
            self.w(f"        for (id, r) in &after.{d.name} {{\n            match self.{d.name}.get(id) {{")
            fmt = ", ".join(f"{f.name}={{}}" for f in others)
            args = ", ".join(show_expr(f.type, f"r.{f.name}") for f in others)
            self.w(f'                None => out.push(format!("{d.name}({{}})新規({fmt})", id{", " + args if args else ""})),')
            self.w("                Some(b) => {")
            for f in others:
                self.w(f"                    if b.{f.name} != r.{f.name} {{ out.push(format!(\"{d.name}({{}}).{f.name}:{{}}→{{}}\", id, "
                       f"{show_expr(f.type, 'b.' + f.name)}, {show_expr(f.type, 'r.' + f.name)})); }}")
            self.w("                }\n            }\n        }")
            self.w(f"        for id in self.{d.name}.keys() {{ if !after.{d.name}.contains_key(id) {{ out.push(format!(\"{d.name}({{}})削除\", id)); }} }}")
        self.w("        out.sort();\n        out\n    }\n}\n")

    def write_kinds(self):
        kinds = []
        for proc in self.p.processes:
            info = self.ck.infos[proc.name]
            for w in sorted(info.writes):
                d, _, f = w.partition(".")
                k = (d, f or None)
                if k not in [(a, b) for _, a, b in kinds]:
                    kinds.append(("w", d, f or None))
        return kinds

    def writes_enum(self):
        self.w("#[derive(Clone, Debug, PartialEq)]\npub enum Write {")
        for _, d, f in self.write_kinds():
            if f is None:
                self.w(f"    {d}_New({d}),")
            else:
                t = next(x.type for x in self.ck.data[d].fields if x.name == f)
                self.w(f"    {d}_{f}(String, {field_rust(t)}),")
        self.w("}\n")

    def parts(self):
        """外から足す部品。保証は呼んだ直後に確かめる（ProcGen.rx）。違反は、保存へ書く前に処理を止める"""
        if not self.p.parts:
            return
        self.w("/// 部品の保証に違反した。処理は書込を返す前に止まり、保存は変わらない\n"
               "pub fn guarantee_violated(part: &str, guarantee: &str) -> ! {\n"
               '    panic!("部品「{}」が保証に違反しました: {}", part, guarantee)\n}')
        if not any(pt.external for pt in self.p.parts):
            return
        self.w("/// アプリが実装する部品。戻り値は、呼ぶたびに宣言の保証で確かめられる\npub trait Parts {")
        for pt in self.p.parts:
            if not pt.external:
                continue
            ps = ", ".join(f"{n}: {rust_type(t)}" for n, t in pt.params)
            self.w(f"    fn {pt.name}(&self{', ' if ps else ''}{ps}) -> {rust_type(pt.ret)};")
        self.w("}")

    def convert_fns_out(self):
        """変換（geas の式）を、公開関数にする。引数と戻り値は、呼ぶ側の局所の型（数は i128）"""
        local = lambda t: "i128" if t.kind in ("整数", "金額") else rust_type(t)
        for pt in self.p.parts:
            if pt.body is None:
                continue
            pg = ProcGen.__new__(ProcGen)
            pg.g, pg.ck, pg.proc = self, self.ck, Process(pt.name)
            pg.info, pg.rdef, pg.scope, pg.lines, pg.ind = ProcInfo(pt.name), None, {}, [], 1
            pg.null_inputs, pg.in_loop, pg.fills = set(), False, {}
            pg.info.types.update(self.ck.part_types)
            for n, t in pt.params:
                pg.scope[n] = ("local", tok_of(t))
            pg.fills = {(n, c): v for n, t in pt.params for c, v in (t.fills or ())}
            body = pg.rx(pt.body)
            ps = ", ".join(f"{n}: {local(t)}" for n, t in pt.params)
            self.w(f"/// 変換 {pt.name}（パッケージの利用者が呼ぶ）\npub fn {pt.name}({ps}) -> {local(pt.ret)} {{\n    {body + ' as i128' if pt.ret.kind in ('整数', '金額') else body}\n}}")

    def effects(self):
        self.w("#[derive(Clone, Debug, PartialEq)]\npub enum Effect {")
        for e in self.p.effects:
            fs = ", ".join(f"{n}: {rust_type(t)}" for n, t in e.params)
            self.w(f"    {e.name.replace('.', '_')} {{ {fs}, idem_key: String }},")
        # 外部作用が1つも無いときは、中身の無い enum になる。参照のまま match できないので、値で match する
        self.w("}\nimpl Effect {\n    pub fn show(&self) -> String {\n        match " + ("self" if self.p.effects else "*self") + " {")
        for e in self.p.effects:
            names = [n for n, _ in e.params]
            pat = ", ".join(names)
            fmt = ", ".join("{}" for _ in names)
            shown = ", ".join(show_expr(t, n) for n, t in e.params)
            self.w(f'            Effect::{e.name.replace(".", "_")} {{ {pat}, idem_key }} => format!("{e.name}({fmt}, 冪等キー={{}})", {shown}, idem_key),')
        self.w("        }\n    }\n}\n")

    def results(self):
        for r in self.p.results:
            if r.name in self.p.extern_names:
                continue
            self.w("#[derive(Clone, Debug, PartialEq)]")
            self.w(f"pub enum {r.name} {{")
            vs = [(v, [t for _, t in ps]) for v, ps in r.variants]  # Rust では位置で持つ（名前は検査と受け渡しの定義に使う）
            for v, ts in vs:
                self.w(f"    {v}({', '.join(rust_type(t) for t in ts)})," if ts else f"    {v},")
            self.w("}")
            self.w(f"impl {r.name} {{\n    pub fn show(&self) -> String {{\n        match self {{")
            for v, ts in vs:
                if ts:
                    names = [f"a{i}" for i in range(len(ts))]
                    shown = ", ".join(show_expr(t, n) if t.kind != "ID" else n for t, n in zip(ts, names))
                    fmt = ", ".join("{}" for _ in ts)
                    self.w(f'            {r.name}::{v}({", ".join(names)}) => format!("{v}({fmt})", {shown}),')
                else:
                    self.w(f'            {r.name}::{v} => "{v}".to_string(),')
            self.w("        }\n    }")
            self.w("    pub fn name(&self) -> &'static str {\n        match self {")
            for v, ts in vs:
                self.w(f'            {r.name}::{v}' + ("(..)" if ts else "") + f' => "{v}",')
            self.w("        }\n    }")
            self.w("    pub fn args(&self) -> Vec<String> {\n        match self {")
            for v, ts in vs:
                if ts:
                    names = [f"a{i}" for i in range(len(ts))]
                    shown = ", ".join(f'format!("{{}}", {show_expr(t, n)})' for t, n in zip(ts, names))
                    self.w(f'            {r.name}::{v}({", ".join(names)}) => vec![{shown}],')
                else:
                    self.w(f'            {r.name}::{v} => vec![],')
            self.w("        }\n    }\n}\n")

    def flows(self):
        self.w("// 【フロー】：結果の種類ごとの行き先。match の網羅は Rust が検査する。")
        for fl in self.p.flows:
            for proc, routes in fl.entries:
                rdef = self.ck.results[self.ck.procs[proc].result]
                m = route_map(routes)
                many = any(len(ds) > 1 for ds in m.values())  # 行き先が複数ある結果があれば、全部の結果を、書いた順の列で返す
                one = lambda dest: "Dest::Reply" if dest == "reply" else f'Dest::Wait("{dest[5:]}")' if dest.startswith("wait ") else f'Dest::Deed("{dest}")'
                self.w(f"pub fn {fl.name}_{proc}(r: &{rdef.name}) -> {'Vec<Dest>' if many else 'Dest'} {{\n    match r {{")
                for v, ts in rdef.variants:  # ts は中身（名前, 型）。ここでは有無だけ見る
                    pat = f"{rdef.name}::{v}" + ("(..)" if ts else "")
                    ds = [one(d) for d in m[v]]
                    self.w(f"        {pat} => " + (f"vec![{', '.join(ds)}]," if many else ds[0] + ","))
                self.w("    }\n}\n")

    def triggers(self):
        if not self.p.triggers:
            return
        self.w("// 【トリガ】：待ちの宣言。要因ごとの行き先。作成・発火・期限の監視は基盤の仕事。")
        self.w("pub const triggers: &[TriggerDef] = &[")
        for tg in self.p.triggers:
            cs = ", ".join(f'("{k}", "{key}", "{d}")' for k, key, d in tg.causes)
            self.w(f'    TriggerDef {{ name: "{tg.name}", causes: &[{cs}] }},')
        self.w("];\n")


class ProcGen:
    def __init__(self, g: Gen, proc: Process):
        self.g, self.ck, self.proc = g, g.ck, proc
        self.info = g.ck.infos[proc.name]
        self.rdef = g.ck.results[proc.result]
        self.scope: dict[str, tuple[str, str]] = {}  # 名前 → (種類, 型トークン)
        self.lines: list[str] = []
        self.ind = 1
        self.null_inputs: set[str] = set()
        self.in_loop = False

    def L(self, s=""):
        self.lines.append("    " * self.ind + s)

    # ---- 式 ----
    def tok(self, e):
        return self.info.types[id(e)]

    def wrap(self, tok, x):
        if tok in ("Int", "Money"):
            return f"({x} as i128)"
        if tok in ("ID", "Str", "Bytes") or tok.startswith("Map:") or tok.startswith("Comp:") or tok == "Set":
            return f"{x}.clone()"
        return x

    def nullable(self, e: Attr) -> bool:
        bt = self.tok(e.e)
        if bt.startswith("Comp:"):  # 未指定にできる成分。読むのは、検査で未指定でないと確かめてある所だけ
            return next(t for n, t in self.ck.comps[bt[5:]].parts if n == e.n).optional
        if not bt.startswith("Rec:"):
            return False
        return next(f for f in self.ck.data[bt[4:]].fields if f.name == e.n).type.optional

    def place(self, e) -> str:
        if isinstance(e, Name):
            kind, _ = self.scope[e.n]
            return {"input": f"Input.{e.n}", "ref": f"ctx.{e.n}"}.get(kind, e.n)
        if isinstance(e, Attr):
            return f"{self.place(e.e)}.{e.n}"
        if isinstance(e, Index):
            return f"Store.{e.d}[&({self.rx(e.e)})]"
        raise ValueError(e)

    def part_call(self, e: Call) -> str:
        """部品を呼び、戻り値が型の範囲と保証に合うかをその場で確かめる。違反は保証違反（処理を止める）"""
        pt = self.ck.parts[e.n]
        local = lambda t: "i128" if t.kind in ("整数", "金額") else rust_type(t)
        lines, args = [], []
        for (n, t), a in zip(pt.params, e.args):
            lines.append(f"let {n}: {local(t)} = {self.rx(a)};")
            args.append(self.conv(t, n, f"{e.n}の{n}") if t.kind in ("整数", "金額") else f"{n}.clone()")
        self.info.types.update(self.ck.part_types)
        if pt.body is not None:  # convert：引数を局所に束ねて、定義の式をその場で計算する
            saved = dict(self.scope)
            for n, t in pt.params:
                self.scope[n] = ("local", tok_of(t))
            self.fills = {(n, c): v for n, t in pt.params for c, v in (t.fills or ())}
            call = self.rx(pt.body)
            self.fills = {}
            self.scope = saved
        elif pt.extern_crate:  # ソースなしのパッケージの変換：パッケージの crate の関数を呼ぶ（引数は局所の型のまま）
            call = f"{e.n}({', '.join(n if t.kind in ('整数', '金額') else f'{n}.clone()' for n, t in pt.params)})"
        else:
            call = f"parts.{e.n}({', '.join(args)})"
        lines.append(f"let result: {local(pt.ret)} = {call + ' as i128' if pt.ret.kind in ('整数', '金額') else call};")
        if pt.body is not None or pt.extern_crate:  # convert の範囲は検査器が静的に示してある（パッケージの変換は、パッケージを作るときに）。実行時には確かめない
            return "{ " + " ".join(lines) + " result }"
        if pt.ret.kind in ("整数", "金額") and pt.ret.range:
            r = pt.ret.range
            lines.append(f'if let Err(m) = check_range("result", result, {opt(r.lo)}, {opt(r.hi)}) {{ guarantee_violated("{e.n}", &m) }}')
        if pt.ret.kind == "文字列" and pt.ret.range:
            r = pt.ret.range
            lines.append(f'if let Err(m) = check_range("result", result.chars().count() as i128, {opt(r.lo)}, {opt(r.hi)}) {{ guarantee_violated("{e.n}", &m) }}')
        if pt.ret.kind == "文字列" and pt.ret.chars is not None:
            lines.append(f'if let Err(m) = check_chars("result", &result, {chars_rust(pt.ret.chars)}) {{ guarantee_violated("{e.n}", &m) }}')
        saved = dict(self.scope)
        for n, t in [*pt.params, ("result", pt.ret)]:
            self.scope[n] = ("local", tok_of(t))
        self.info.types.update(self.ck.part_types)
        cond = self.rx(pt.guarantee)
        self.scope = saved
        lines.append(f'if !({cond}) {{ guarantee_violated("{e.n}", "{show(pt.guarantee)}") }}')
        return "{ " + " ".join(lines) + " result }"

    def src(self, e) -> str:
        """読むだけの場所。無いかもしれない入力は、検査で確かめてあるので中身を借りる"""
        if isinstance(e, Name) and e.n in self.null_inputs and self.scope.get(e.n, ("",))[0] == "input":
            return f"unsafe {{ {self.place(e)}.as_ref().unwrap_unchecked() }}"
        return self.place(e)

    def rx(self, e) -> str:
        if isinstance(e, Num):
            return f"{e.v}i128"
        if isinstance(e, Dur):
            return f"{e.secs}i64"
        if isinstance(e, Name):
            if e.n in self.scope:
                kind, tok = self.scope[e.n]
                x = self.place(e)
                if kind == "input" and e.n in self.null_inputs:  # 検査で、無くないと確かめてある（実行時に確かめない）
                    return self.wrap(tok, f"unsafe {{ {x}.clone().unwrap_unchecked() }}")
                if kind == "local" and tok in ("Int", "Money", "Rational", "Ratio"):
                    return x
                if kind == "ref":  # slot も入力と同じく、整数と金額は i128 で計算する
                    return self.wrap(tok, x)
                if tok.startswith("Rec:"):
                    return x
                return self.wrap(tok, x)
            return f"{self.ck.variants[e.n]}::{e.n}"
        if isinstance(e, Attr):
            if isinstance(e.e, Name) and (e.e.n, e.n) in getattr(self, "fills", {}):  # 補うルールがある成分
                return self.wrap(self.tok(e), f"{self.place(e)}.unwrap_or({self.fills[(e.e.n, e.n)]})")
            if self.nullable(e):  # 検査で、null でないと確かめてある
                # 検査で、null でないと確かめてある（No.28：示せた所は実行時に確かめない）
                return self.wrap(self.tok(e), f'unsafe {{ {self.place(e)}.clone().unwrap_unchecked() }}')
            return self.wrap(self.tok(e), self.place(e))
        if isinstance(e, Index):
            return self.place(e)
        if isinstance(e, Make):  # 範囲は検査で示してあるので、成分は物理数へそのまま移す
            ct = self.ck.comps[e.n]
            vals = []
            for n, x in e.fields:
                pt = next(t for pn, t in ct.parts if pn == n)
                num = pt.kind in ("整数", "金額")
                if isinstance(x, Null):
                    vals.append(f"{n}: None")
                    continue
                v = self.conv(pt, self.rx(x), f"{e.n}.{n}", proven=True) if num else self.rx(x)
                vals.append(f"{n}: Some({v})" if pt.optional else f"{n}: {v}")
            vals += [f"{pn}: None" for pn, pt in ct.parts if pt.optional and pn not in dict(e.fields)]
            return f"{e.n} {{ {', '.join(vals)} }}"
        if isinstance(e, Pred) and e.kind == "null":
            return f"{self.place(e.e)}.is_none()"
        if isinstance(e, Null):
            return "None"
        if isinstance(e, Pred):
            if e.kind in ("ある", "ない"):
                c = f"Store.{e.e.d}.contains_key(&({self.rx(e.e.e)}))"
                return c if e.kind == "ある" else f"!{c}"
            c = f"{self.src(e.e)}.is_empty()"
            return c if e.kind == "空" else f"!{c}"
        if isinstance(e, Call):
            if e.n in CONVERSIONS:  # 整数 ⇄ 金額 の変換は、意味の型を変えるだけ（物理の値はそのまま）
                return f"(({self.rx(e.args[0])}) as i64)" if e.n == "time" else self.rx(e.args[0])
            if e.n in self.ck.parts:
                return self.part_call(e)
            return f"{e.args[1].n}({self.rx(e.args[0])})"
        if isinstance(e, Bin):
            lt, rt = self.tok(e.l), self.tok(e.r)
            l, r = self.rx(e.l), self.rx(e.r)
            if e.op in ("/", "%"):  # 正の定数で割る。負の値も、切り捨て（負の無限大へ）と 0 以上の余りにそろえる
                return f"({l}).{'div_euclid' if e.op == '/' else 'rem_euclid'}({r})"
            if e.op == "*" and "Ratio" in (lt, rt):
                return f"scale({r}, {l})" if lt == "Ratio" else f"scale({l}, {r})"
            if e.op == "-" and (lt, rt) == ("Time", "Time"):  # 時刻の差は秒数（整数は i128 で持つ）
                return f"(({l} as i128) - ({r} as i128))"
            op = {"かつ": "&&"}.get(e.op, e.op)
            return f"({l} {op} {r})"
        if isinstance(e, (SetB, SumE)):
            return self.builder(e)
        raise ValueError(e)

    def bind_lines(self, b: Binder, pad: str):
        """束縛の for 文の先頭と、束縛した名前の登録。戻り値は (for 行, 束縛の行, 旧 scope)。"""
        st = self.tok(b.src)
        src = self.src(b.src)
        saved = dict(self.scope)
        if st.startswith("Map:"):
            _, _, vt = st.split(":", 2)
            k, v = b.names
            head = f"for (k_, v_) in {src}.iter() {{"
            body = [f"let {k}: String = k_.clone();"]
            if vt in ("Int", "Money"):
                body.append(f"let {v}: i128 = *v_ as i128;")
            else:
                body.append(f"let {v} = v_.clone();")
            self.scope[k] = ("local", "ID")
            self.scope[v] = ("local", vt)
        else:
            k = b.names[0]
            head = f"for k_ in {src}.iter() {{"
            body = [f"let {k}: String = k_.clone();"]
            self.scope[k] = ("local", "ID")
        return head, body, saved

    def builder(self, e) -> str:
        head, body, saved = self.bind_lines(e.binder, "")
        cond = self.rx(e.cond) if e.cond is not None else None
        elem = self.rx(e.elem)
        self.scope = saved
        is_set = isinstance(e, SetB)
        init = "let mut acc: BTreeSet<String> = BTreeSet::new();" if is_set else "let mut acc: i128 = 0;"
        add = f"acc.insert({elem});" if is_set else f"acc += {elem};"
        inner = " ".join(body) + (f" if {cond} {{ {add} }}" if cond else f" {add}")
        return f"{{ {init} {head} {inner} }} acc }}"

    # ---- 文 ----
    def ret_expr(self, r: Return) -> str:
        args = ", ".join(self.rx(a) for a in r.args)
        v = f"{self.rdef.name}::{r.variant}" + (f"({args})" if r.args else "")
        return f"{self.proc.name}Output {{ result: {v}, Write: writes, effects: effects }}"

    def conv(self, t: Type, x: str, name: str, proven: bool = False) -> str:
        """i128 の式を、項目の物理数へ。proven なら、範囲は検査で示してある（実行時には確かめない）。
        上限か下限が決まっていない範囲は、物理数に収まるかまでは示せないので、そこだけ確かめる"""
        if proven:
            r = t.range
            if r and r.lo is not None and r.hi is not None:
                return f"(({x}) as {phys(t)})"  # 範囲は検査で示した。範囲から選んだ物理数には必ず収まる
            return f'{phys(t)}::try_from({x}).expect("物理数に収まらない")'
        r = t.range
        lo = r.lo if r else None
        hi = r.hi if r else None
        return f'{phys(t)}::try_from(in_range("{name}", {x}, {opt(lo) if lo is None else f"Some({lo})"}, {"None" if hi is None else f"Some({hi})"})).expect("物理数に収まらない")'

    def field_value(self, t: Type, e, name: str) -> str:
        """項目へ書く値。null になりうる項目は Option で、無いかもしれない入力はそのまま渡す"""
        num = t.kind in ("整数", "金額")
        if isinstance(e, Null):
            return "None"
        if t.optional and isinstance(e, Name) and e.n in self.null_inputs and self.scope[e.n][0] == "input":
            x = self.place(e)
            return f"{x}.map(|v| {self.conv(t, '(v as i128)', name, proven=True)})" if num else f"{x}.clone()"
        v = self.conv(t, self.rx(e), name, proven=True) if num else self.rx(e)
        return f"Some({v})" if t.optional else v

    def field_type(self, d, f) -> Type:
        return next(x.type for x in self.ck.data[d].fields if x.name == f)

    def stmt(self, st):
        if isinstance(st, Return):
            self.L(f"return {self.ret_expr(st)};")
        elif isinstance(st, If):
            self.L(f"if {self.rx(st.cond)} {{")
            self.ind += 1
            for inner in st.body:
                self.stmt(inner)
            self.ind -= 1
            self.L("}")
        elif isinstance(st, Assign):
            if isinstance(st.e, Index):
                d = st.e.d
                self.L(f"let {st.name} = match Store.{d}.get(&({self.rx(st.e.e)})) {{")
                self.L("    Some(r) => r,")
                if st.absent and self.in_loop:
                    self.L(f"    None => {{ effects.truncate(束の始め); return {self.ret_expr(st.absent)}; }}")
                elif st.absent:
                    self.L(f"    None => return {self.ret_expr(st.absent)},")
                self.L("};")
                self.scope[st.name] = ("local", f"Rec:{d}")
            elif st.absent is not None and isinstance(st.e, (Attr, Name)):  # let x = y.項目／入力 else { return … }（無ければ返す）
                tok = self.tok(st.e)
                ty = {"Int": "i128", "Money": "i128", "Ratio": "Ratio", "ID": "String", "Str": "String", "Time": "i64", "Set": "BTreeSet<String>"}.get(tok, "_")
                self.L(f"let {st.name}: {ty} = match {self.place(st.e)}.clone() {{")
                self.L(f"    Some(v) => {self.wrap(tok, 'v') if tok in ('Int', 'Money') else 'v'},")
                self.L(f"    None => return {self.ret_expr(st.absent)},")
                self.L("};")
                self.scope[st.name] = ("local", tok)
            else:
                tok = self.tok(st.e)
                ty = {"Int": "i128", "Money": "i128", "Rational": "Ratio", "ID": "String", "Str": "String", "Time": "i64", "Set": "BTreeSet<String>"}[tok]
                self.L(f"let {st.name}: {ty} = {self.rx(st.e)};")
                self.scope[st.name] = ("local", tok)
        elif isinstance(st, Update):
            head, body, saved = self.bind_lines(st.binder, "")
            tgt = st.target
            d, f = tgt.e.d, tgt.n
            self.L(head)
            for b in body:
                self.L("    " + b)
            val = self.rx(st.value)
            cur = self.rx(tgt)
            new = {"-=": f"{cur} - {val}", "+=": f"{cur} + {val}", "=": val}[st.op]
            pre = f"if {self.rx(st.cond)} " if st.cond is not None else ""
            ft = self.field_type(d, f)
            self.L(f"    {pre}{{")
            if isinstance(st.value, Null):
                self.L(f"        writes.push(Write::{d}_{f}({self.rx(tgt.e.e)}, None));")
            elif ft.kind in ("整数", "金額"):
                v = self.conv(ft, 'new_value', d + '.' + f, proven=True)
                self.L(f"        let new_value: i128 = {new};")
                self.L(f"        writes.push(Write::{d}_{f}({self.rx(tgt.e.e)}, {f'Some({v})' if ft.optional else v}));")
            else:
                self.L(f"        writes.push(Write::{d}_{f}({self.rx(tgt.e.e)}, {f'Some({val})' if ft.optional else val}));")
            self.L("    }")
            self.L("}")
            self.scope = saved
        elif isinstance(st, SetField):
            d = self.info.lookup_vars[st.var]
            key = self.g.key(d)
            v = self.field_value(self.field_type(d, st.f), st.e, f"{d}.{st.f}")
            self.L(f"writes.push(Write::{d}_{st.f}({st.var}.{key}.clone(), {v}));")
        elif isinstance(st, Create):
            dt = self.ck.data[st.data]
            key = self.ck.key_field(st.data)
            given = dict(st.fields)
            self.L(f"let {st.name} = {st.data} {{")
            for f in dt.fields:
                if f is key:
                    v = f"ctx.new_{st.data}_id.clone()"
                else:
                    v = self.field_value(f.type, given.get(f.name, Null()), f"{st.data}.{f.name}")  # 書かなかった項目は null
                self.L(f"    {f.name}: {v},")
            self.L("};")
            self.L(f"writes.push(Write::{st.data}_New({st.name}.clone()));")
            self.scope[st.name] = ("local", f"Rec:{st.data}")
        elif isinstance(st, ForEach):
            head, body, saved = self.bind_lines(st.binder, "")
            self.L("let 束の始め = effects.len();")
            self.L(head)
            for b in body:
                self.L("    " + b)
            self.in_loop = True
            for inner in st.body:
                self.stmt(inner)
            self.in_loop = False
            self.L("}")
            self.scope = saved
        elif isinstance(st, Request):
            eff = self.ck.effects[st.effect]
            parts = []
            for (n, t), a in zip(eff.params, st.args):
                v = self.conv(t, self.rx(a), n) if t.kind in ("整数", "金額") else self.rx(a)
                parts.append(f"{n}: {v}")
            if isinstance(st.idem, Tuple):  # 組は、区切りがあいまいにならない形で1つの文字列に
                parts.append(f"idem_key: join_idem_key(&[{', '.join(self.rx(x) for x in st.idem.items)}])")
            else:
                parts.append(f"idem_key: {self.rx(st.idem)}")
            self.L(f"effects.push(Effect::{st.effect.replace('.', '_')} {{ {', '.join(parts)} }});")

    # ---- 関数全体 ----
    def emit(self):
        g, proc, info = self.g, self.proc, self.info
        n = proc.name
        g.w(f"// ---- 処理 {n} ----")
        g.w(f"pub struct {n}Input {{")
        for name, t in proc.inputs:
            g.w(f"    pub {name}: {field_rust(t)},")  # 無いかもしれない入力は Option
        g.w("}")
        g.w(f"impl {n}Input {{\n    /// 外部から取り込むときの、範囲の宣言の検査\n    pub fn validate(&self) -> Result<(), String> {{")
        for line in range_checks(proc.inputs):
            g.w(line)
        g.w("        Ok(())\n    }\n}")
        g.w(f"/// 文脈：【参照】（配備時に埋まる）と、採番（実行のたびに基盤が代入）\npub struct {n}Ctx {{")
        for name, t in proc.refs:
            g.w(f"    pub {name}: {rust_type(t)},")
        for d in sorted(set(info.new_ids)):
            g.w(f"    pub new_{d}_id: String,")
        g.w("}")
        g.w(f"impl {n}Ctx {{\n    /// 外から埋まる値（slot）の、範囲の宣言の検査\n    pub fn validate(&self) -> Result<(), String> {{")
        for line in range_checks(proc.refs):
            g.w(line)
        g.w("        Ok(())\n    }\n}")
        g.w(f"pub struct {n}Output {{\n    pub result: {self.rdef.name},\n    pub Write: Vec<Write>,\n    pub effects: Vec<Effect>,\n}}")
        extra = ", parts: &dyn Parts" if info.parts else ""
        g.w(f"pub fn {n}(ctx: &{n}Ctx, Store: &Store, Input: &{n}Input{extra}) -> {n}Output {{")
        g.w("    let mut writes: Vec<Write> = Vec::new();\n    let mut effects: Vec<Effect> = Vec::new();")
        self.null_inputs = {name for name, t in proc.inputs if t.optional}
        for name, t in proc.inputs:
            self.scope[name] = ("input", tok_of(t))
        for name, t in proc.refs:
            self.scope[name] = ("ref", tok_of(t))
        for st in proc.body:
            self.stmt(st)
        g.out.extend(self.lines)
        g.w("}\n")


def cargo_toml(name: str, host: bool = False, crates=()) -> str:
    crate_type = 'crate-type = ["cdylib", "rlib"]\n' if host else ""
    opt = 'opt-level = "s"\n' if host else ""
    return f'''[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"
{crate_type}
[dependencies]
{"".join(f'{c} = {{ path = "{path}" }}' + chr(10) for c, path in crates)}
[profile.release]
{opt}overflow-checks = true
'''


def generate(ck: Checker, convert_fns: bool = False) -> str:
    """convert_fns：変換を、crate の公開関数としても出す（パッケージの crate。ソースなしで取り込む側が呼ぶ）"""
    return Gen(ck, convert_fns).lib()
