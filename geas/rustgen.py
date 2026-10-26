"""Rust の生成。検査器を通ったコードだけを受け付ける。【処理】は純粋な関数
（文脈, 保存データ, 入力）→（結果, 書込の列, 外部作用の要求の列）として生成し、実行は基盤に任せる。"""
from dataclasses import replace

from .ast import *  # noqa: F401,F403
from .check import Checker, column_type, phys, tok_of

PRELUDE = r'''// 自動生成。手で編集しない（コードから geas が毎回生成する）。
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code, unused_variables, unused_mut)]
#![allow(uncommon_codepoints, mixed_script_confusables, unused_parens)]
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;

/// 端数の出る値（分子/分母）。丸めの指定が無いと整数にできない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct 有理 {
    pub 分子: i128,
    pub 分母: i128,
}
impl Display for 有理 {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}/{}", self.分子, self.分母)
    }
}
pub fn 積(x: i128, r: 有理) -> 有理 {
    有理 { 分子: x * r.分子, 分母: r.分母 }
}
pub fn 切り捨て(x: 有理) -> i128 {
    x.分子.div_euclid(x.分母)
}
pub fn 切り上げ(x: 有理) -> i128 {
    -((-x.分子).div_euclid(x.分母))
}
pub fn 四捨五入(x: 有理) -> i128 {
    (2 * x.分子 + x.分母).div_euclid(2 * x.分母)
}
pub fn 範囲検査(名: &str, x: i128, lo: Option<i128>, hi: Option<i128>) -> Result<(), String> {
    if lo.map_or(false, |l| x < l) || hi.map_or(false, |h| x > h) {
        return Err(format!("{}={} は範囲外", 名, x));
    }
    Ok(())
}
pub fn 範囲内(名: &str, x: i128, lo: Option<i128>, hi: Option<i128>) -> i128 {
    範囲検査(名, x, lo, hi).expect("範囲の宣言に反する値を書こうとした");
    x
}
pub fn バイト列表示(b: &[u8]) -> String { format!("0x{}", b.iter().map(|x| format!("{:02x}", x)).collect::<String>()) }
/// 検索で同一視する軸をそろえた文字列。大小は小文字へ、かなはカタカナをひらがなへ、幅は全角の英数記号を半角へ
pub fn 照合(s: &str, 大小: bool, かな: bool, 幅: bool) -> String {
    s.chars().flat_map(|c| {
        let mut c = c;
        if 幅 && ('\u{FF01}'..='\u{FF5E}').contains(&c) { c = char::from_u32(c as u32 - 0xFEE0).unwrap_or(c); }
        if かな && ('\u{30A1}'..='\u{30F6}').contains(&c) { c = char::from_u32(c as u32 - 0x60).unwrap_or(c); }
        if 大小 { c.to_lowercase().collect::<Vec<_>>() } else { vec![c] }
    }).collect()
}
pub fn 集合表示(s: &BTreeSet<String>) -> String {
    format!("{{{}}}", s.iter().cloned().collect::<Vec<_>>().join(","))
}
pub fn 写像表示<V: Display>(m: &BTreeMap<String, V>) -> String {
    format!("{{{}}}", m.iter().map(|(k, v)| format!("{}:{}", k, v)).collect::<Vec<_>>().join(","))
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum 行き先 {
    処理(&'static str),
    待つ(&'static str),
    応答,
}
pub struct トリガ定義 {
    pub 名前: &'static str,
    /// (種別, 要因, 行き先の処理)。期限の要因は秒数。先に起きた要因が勝つ（実行は基盤）。
    pub 要因: &'static [(&'static str, &'static str, &'static str)],
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
        return "有理"
    if k == "名前":
        return t.target
    if k == "写像":
        return f"BTreeMap<String, {rust_type(t.value)}>"
    if k == "集合":
        return "BTreeSet<String>"
    raise ValueError(k)


def field_rust(t: Type) -> str:
    """項目の Rust の型。無いかもしれない項目は Option"""
    return f"Option<{rust_type(t)}>" if t.optional else rust_type(t)


def show_expr(t: Type, x: str) -> str:
    """値の表示用の Rust 式（突合のための正規の書式）。null は「null」"""
    if t.optional:
        return f'{x}.as_ref().map_or("null".to_string(), |v| format!("{{}}", {show_expr(replace(t, optional=False), "v")}))'
    k = t.kind
    if k == "名前":
        return f"{x}.名前()"
    if k == "写像":
        return f"写像表示(&{x})"
    if k == "集合":
        return f"集合表示(&{x})"
    if k == "バイト列":
        return f"バイト列表示(&{x})"
    return x


CONVERSIONS = ("money", "int")


def opt(v):
    return "None" if v is None else f"Some({v})"


class Gen:
    def __init__(self, ck: Checker):
        assert ck.ok, "検査にエラーがあるので生成しません"
        self.ck = ck
        self.p = ck.p
        self.out: list[str] = []

    def w(self, s=""):
        self.out.append(s)

    # ---- 全体 ----
    def lib(self) -> str:
        self.out = [PRELUDE]
        self.states()
        self.records()
        self.writes_enum()
        self.effects()
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
        self.w(f"pub struct {n}入力 {{")
        for name, t in q.inputs:
            self.w(f"    pub {name}: {ty(t)},")
        self.w("}")
        self.w(f"impl {n}入力 {{\n    pub fn 検査(&self) -> Result<(), String> {{")
        for name, t in q.inputs:
            if t.kind in ("整数", "金額") and t.range:
                x = f"*x as i128" if t.optional else f"self.{name} as i128"
                chk = f'範囲検査("{name}", {x}, {opt(t.range.lo)}, {opt(t.range.hi)})?;'
                self.w(f"        if let Some(x) = &self.{name} {{ {chk} }}" if t.optional else f"        {chk}")
        self.w("        Ok(())\n    }\n}")
        self.w(f"pub struct {n}文脈 {{")
        for name, t in q.refs:
            self.w(f"    pub {name}: {rust_type(t)},")
        self.w("}")
        if q.select:
            cols = [(c, column_type(self.ck, q, c)) for c in q.select]
            self.w("#[derive(Clone, Debug, PartialEq)]")
            self.w(f"pub struct {n}行 {{")
            for c, t in cols:
                self.w(f"    pub {c.name}: {'i128' if c.kind in ('合計', '件数') else field_rust(t)},")  # 集計は桁あふれしない広い整数
            self.w("}")
            fmt = "/".join(f"{c.name}={{}}" for c, _ in cols)
            vals = ", ".join(show_expr(t, f"self.{c.name}") for c, t in cols)
            self.w(f'impl {n}行 {{\n    pub fn 表示(&self) -> String {{ format!("{fmt}", {vals}) }}\n}}')
        out_ty = f"{n}行" if q.select else "String"
        self.w(f"pub fn {n}(ctx: &{n}文脈, 保存: &保存, 入力: &{n}入力) -> Vec<{out_ty}> {{")
        self.w(f"    let mut 行: Vec<&{q.target}> = 保存.{q.target}.values().filter(|r| {{")
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
                cond = f"照合(&{v}, {flags}).contains(&照合(x, {flags}))" if folds else f"{v}.contains(x.as_str())"
            else:
                cond = {"以上": f"({v} as i128) >= (*x as i128)", "以下": f"({v} as i128) <= (*x as i128)"}[op]  # 物理数の幅が違っても比べられる
            cond = guard(cond)
            if t.optional:
                self.w(f"        if let Some(x) = &入力.{a} {{ if !({cond}) {{ return false; }} }}")
            else:
                self.w(f"        {{ let x = &入力.{a}; if !({cond}) {{ return false; }} }}")
        self.w("        true\n    }).collect();")
        if q.order:
            f, d = q.order
            a, b = ("b", "a") if d == "新しい順" else ("a", "b")
            nulls = f"a.{f}.is_none().cmp(&b.{f}.is_none()).then(" if fields[f].type.optional else "("  # null は最後
            self.w(f"    行.sort_by(|a, b| {nulls}{a}.{f}.cmp(&{b}.{f})).then({a}.{key}.cmp(&{b}.{key})));")
        else:
            self.w(f"    行.sort_by(|a, b| a.{key}.cmp(&b.{key}));")
        if q.limit:
            t = ins[q.limit]
            n_expr = f"入力.{q.limit}.map_or(行.len(), |x| x as usize)" if t.optional else f"入力.{q.limit} as usize"
            self.w(f"    行.truncate({n_expr});")
        if not q.select:
            self.w(f"    行.into_iter().map(|r| r.{key}.clone()).collect()\n}}\n")
            return
        self.w("    行.into_iter().filter_map(|r| {")
        for via in dict.fromkeys(c.via for c in q.select if c.kind == "参照先"):
            tgt = fields[via].type.target
            if fields[via].type.optional:  # 参照が null なら、その先の列は null
                self.w(f"        let j_{via} = match &r.{via} {{ Some(k) => Some(保存.{tgt}.get(k)?), None => None }};  // 参照先が無い行は出さない")
            else:
                self.w(f"        let j_{via} = 保存.{tgt}.get(&r.{via})?;  // 参照先が無い行は出さない")
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
        self.w(f"        Some({n}行 {{ {', '.join(parts)} }})")
        self.w("    }).collect()\n}\n")

    def states(self):
        for s in self.p.states:
            vs = [s.initial, *s.middle, *s.final]
            self.w("#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]")
            self.w(f"pub enum {s.name} {{ {', '.join(vs)} }}")
            self.w(f"impl {s.name} {{")
            self.w("    pub fn 名前から(s: &str) -> Option<Self> {\n        match s {")
            for v in vs:
                self.w(f'            "{v}" => Some({s.name}::{v}),')
            self.w("            _ => None,\n        }\n    }")
            self.w("    pub fn 名前(&self) -> &'static str {\n        match self {")
            for v in vs:
                self.w(f'            {s.name}::{v} => "{v}",')
            self.w("        }\n    }\n}\n")

    def key(self, d):
        return self.ck.key_field(d).name

    def records(self):
        for d in self.p.data:
            self.w("#[derive(Clone, Debug, PartialEq)]")
            self.w(f"pub struct {d.name} {{")
            for f in d.fields:
                self.w(f"    pub {f.name}: {field_rust(f.type)},")
            self.w("}\n")
        self.w("#[derive(Clone, Debug, Default)]\npub struct 保存 {")
        for d in self.p.data:
            self.w(f"    pub {d.name}: BTreeMap<String, {d.name}>,")
        self.w("}\n")
        # 書込の適用と差分
        self.w("impl 保存 {")
        self.w("    pub fn 適用(&mut self, 書込列: Vec<書込>) {\n        for w in 書込列 {\n            match w {")
        for kind, d, f in self.write_kinds():
            if f is None:
                self.w(f"                書込::{d}_新規(r) => {{ self.{d}.insert(r.{self.key(d)}.clone(), r); }}")
            else:
                self.w(f"                書込::{d}_{f}(id, v) => {{ self.{d}.get_mut(&id).expect(\"書込先が無い\").{f} = v; }}")
        self.w("            }\n        }\n    }")
        self.w("    pub fn 差分(&self, 後: &保存) -> Vec<String> {\n        let mut out: Vec<String> = Vec::new();")
        for d in self.p.data:
            key = self.key(d.name)
            others = [f for f in d.fields if f.name != key]
            self.w(f"        for (id, r) in &後.{d.name} {{\n            match self.{d.name}.get(id) {{")
            fmt = ", ".join(f"{f.name}={{}}" for f in others)
            args = ", ".join(show_expr(f.type, f"r.{f.name}") for f in others)
            self.w(f'                None => out.push(format!("{d.name}({{}})新規({fmt})", id{", " + args if args else ""})),')
            self.w("                Some(b) => {")
            for f in others:
                self.w(f"                    if b.{f.name} != r.{f.name} {{ out.push(format!(\"{d.name}({{}}).{f.name}:{{}}→{{}}\", id, "
                       f"{show_expr(f.type, 'b.' + f.name)}, {show_expr(f.type, 'r.' + f.name)})); }}")
            self.w("                }\n            }\n        }")
            self.w(f"        for id in self.{d.name}.keys() {{ if !後.{d.name}.contains_key(id) {{ out.push(format!(\"{d.name}({{}})削除\", id)); }} }}")
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
        self.w("#[derive(Clone, Debug, PartialEq)]\npub enum 書込 {")
        for _, d, f in self.write_kinds():
            if f is None:
                self.w(f"    {d}_新規({d}),")
            else:
                t = next(x.type for x in self.ck.data[d].fields if x.name == f)
                self.w(f"    {d}_{f}(String, {field_rust(t)}),")
        self.w("}\n")

    def effects(self):
        self.w("#[derive(Clone, Debug, PartialEq)]\npub enum 外部作用 {")
        for e in self.p.effects:
            fs = ", ".join(f"{n}: {rust_type(t)}" for n, t in e.params)
            self.w(f"    {e.name.replace('.', '_')} {{ {fs}, 冪等キー: String }},")
        # 外部作用が1つも無いときは、中身の無い enum になる。参照のまま match できないので、値で match する
        self.w("}\nimpl 外部作用 {\n    pub fn 表示(&self) -> String {\n        match " + ("self" if self.p.effects else "*self") + " {")
        for e in self.p.effects:
            names = [n for n, _ in e.params]
            pat = ", ".join(names)
            fmt = ", ".join("{}" for _ in names)
            shown = ", ".join(show_expr(t, n) for n, t in e.params)
            self.w(f'            外部作用::{e.name.replace(".", "_")} {{ {pat}, 冪等キー }} => format!("{e.name}({fmt}, 冪等キー={{}})", {shown}, 冪等キー),')
        self.w("        }\n    }\n}\n")

    def results(self):
        for r in self.p.results:
            self.w("#[derive(Clone, Debug, PartialEq)]")
            self.w(f"pub enum {r.name} {{")
            vs = [(v, [t for _, t in ps]) for v, ps in r.variants]  # Rust では位置で持つ（名前は検査と受け渡しの定義に使う）
            for v, ts in vs:
                self.w(f"    {v}({', '.join(rust_type(t) for t in ts)})," if ts else f"    {v},")
            self.w("}")
            self.w(f"impl {r.name} {{\n    pub fn 表示(&self) -> String {{\n        match self {{")
            for v, ts in vs:
                if ts:
                    names = [f"a{i}" for i in range(len(ts))]
                    shown = ", ".join(show_expr(t, n) if t.kind != "ID" else n for t, n in zip(ts, names))
                    fmt = ", ".join("{}" for _ in ts)
                    self.w(f'            {r.name}::{v}({", ".join(names)}) => format!("{v}({fmt})", {shown}),')
                else:
                    self.w(f'            {r.name}::{v} => "{v}".to_string(),')
            self.w("        }\n    }")
            self.w("    pub fn 名前(&self) -> &'static str {\n        match self {")
            for v, ts in vs:
                self.w(f'            {r.name}::{v}' + ("(..)" if ts else "") + f' => "{v}",')
            self.w("        }\n    }")
            self.w("    pub fn 引数(&self) -> Vec<String> {\n        match self {")
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
                m = dict(routes)
                self.w(f"pub fn {fl.name}_{proc}(r: &{rdef.name}) -> 行き先 {{\n    match r {{")
                for v, ts in rdef.variants:  # ts は中身（名前, 型）。ここでは有無だけ見る
                    pat = f"{rdef.name}::{v}" + ("(..)" if ts else "")
                    dest = m[v]
                    self.w(f"        {pat} => " + ("行き先::応答," if dest == "応答" else f'行き先::待つ("{dest[3:]}"),' if dest.startswith("待つ ") else f'行き先::処理("{dest}"),'))
                self.w("    }\n}\n")

    def triggers(self):
        if not self.p.triggers:
            return
        self.w("// 【トリガ】：待ちの宣言。要因ごとの行き先。作成・発火・期限の監視は基盤の仕事。")
        self.w("pub const トリガ一覧: &[トリガ定義] = &[")
        for tg in self.p.triggers:
            cs = ", ".join(f'("{k}", "{key}", "{d}")' for k, key, d in tg.causes)
            self.w(f'    トリガ定義 {{ 名前: "{tg.name}", 要因: &[{cs}] }},')
        self.w("];\n")


class ProcGen:
    def __init__(self, g: Gen, proc: Process):
        self.g, self.ck, self.proc = g, g.ck, proc
        self.info = g.ck.infos[proc.name]
        self.rdef = g.ck.results[proc.result]
        self.scope: dict[str, tuple[str, str]] = {}  # 名前 → (種類, 型トークン)
        self.lines: list[str] = []
        self.ind = 1
        self.in_loop = False

    def L(self, s=""):
        self.lines.append("    " * self.ind + s)

    # ---- 式 ----
    def tok(self, e):
        return self.info.types[id(e)]

    def wrap(self, tok, x):
        if tok in ("Int", "Money"):
            return f"({x} as i128)"
        if tok in ("ID", "Str", "Bytes") or tok.startswith("Map:") or tok == "Set":
            return f"{x}.clone()"
        return x

    def nullable(self, e: Attr) -> bool:
        bt = self.tok(e.e)
        if not bt.startswith("Rec:"):
            return False
        return next(f for f in self.ck.data[bt[4:]].fields if f.name == e.n).type.optional

    def place(self, e) -> str:
        if isinstance(e, Name):
            kind, _ = self.scope[e.n]
            return {"input": f"入力.{e.n}", "ref": f"ctx.{e.n}"}.get(kind, e.n)
        if isinstance(e, Attr):
            return f"{self.place(e.e)}.{e.n}"
        if isinstance(e, Index):
            return f"保存.{e.d}[&({self.rx(e.e)})]"
        raise ValueError(e)

    def rx(self, e) -> str:
        if isinstance(e, Num):
            return f"{e.v}i128"
        if isinstance(e, Name):
            if e.n in self.scope:
                kind, tok = self.scope[e.n]
                x = self.place(e)
                if kind == "local" and tok in ("Int", "Money", "Rational", "Ratio"):
                    return x
                if kind == "ref":
                    return x
                if tok.startswith("Rec:"):
                    return x
                return self.wrap(tok, x)
            return f"{self.ck.variants[e.n]}::{e.n}"
        if isinstance(e, Attr):
            if self.nullable(e):  # 検査で、null でないと確かめてある
                return self.wrap(self.tok(e), f'{self.place(e)}.clone().expect("検査で確かめた値")')
            return self.wrap(self.tok(e), self.place(e))
        if isinstance(e, Index):
            return self.place(e)
        if isinstance(e, Pred) and e.kind == "null":
            return f"{self.place(e.e)}.is_none()"
        if isinstance(e, Null):
            return "None"
        if isinstance(e, Pred):
            if e.kind in ("ある", "ない"):
                c = f"保存.{e.e.d}.contains_key(&({self.rx(e.e.e)}))"
                return c if e.kind == "ある" else f"!{c}"
            c = f"{self.place(e.e)}.is_empty()"
            return c if e.kind == "空" else f"!{c}"
        if isinstance(e, Call):
            if e.n in CONVERSIONS:  # 整数 ⇄ 金額 の変換は、意味の型を変えるだけ（物理の値はそのまま）
                return self.rx(e.args[0])
            return f"{e.args[1].n}({self.rx(e.args[0])})"
        if isinstance(e, Bin):
            lt, rt = self.tok(e.l), self.tok(e.r)
            l, r = self.rx(e.l), self.rx(e.r)
            if e.op == "*" and "Ratio" in (lt, rt):
                return f"積({r}, {l})" if lt == "Ratio" else f"積({l}, {r})"
            op = {"かつ": "&&"}.get(e.op, e.op)
            return f"({l} {op} {r})"
        if isinstance(e, (SetB, SumE)):
            return self.builder(e)
        raise ValueError(e)

    def bind_lines(self, b: Binder, pad: str):
        """束縛の for 文の先頭と、束縛した名前の登録。戻り値は (for 行, 束縛の行, 旧 scope)。"""
        st = self.tok(b.src)
        src = self.place(b.src)
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
        init = "let mut 集め: BTreeSet<String> = BTreeSet::new();" if is_set else "let mut 集め: i128 = 0;"
        add = f"集め.insert({elem});" if is_set else f"集め += {elem};"
        inner = " ".join(body) + (f" if {cond} {{ {add} }}" if cond else f" {add}")
        return f"{{ {init} {head} {inner} }} 集め }}"

    # ---- 文 ----
    def ret_expr(self, r: Return) -> str:
        args = ", ".join(self.rx(a) for a in r.args)
        v = f"{self.rdef.name}::{r.variant}" + (f"({args})" if r.args else "")
        return f"{self.proc.name}出力 {{ 結果: {v}, 書込: 書込列, 外部: 外部列 }}"

    def conv(self, t: Type, x: str, name: str) -> str:
        """i128 の式を、項目の物理数へ。範囲の宣言があれば検査を付ける。"""
        r = t.range
        lo = r.lo if r else None
        hi = r.hi if r else None
        return f'{phys(t)}::try_from(範囲内("{name}", {x}, {opt(lo) if lo is None else f"Some({lo})"}, {"None" if hi is None else f"Some({hi})"})).expect("物理数に収まらない")'

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
                self.L(f"let {st.name} = match 保存.{d}.get(&({self.rx(st.e.e)})) {{")
                self.L("    Some(r) => r,")
                if st.absent and self.in_loop:
                    self.L(f"    None => {{ 外部列.truncate(束の始め); return {self.ret_expr(st.absent)}; }}")
                elif st.absent:
                    self.L(f"    None => return {self.ret_expr(st.absent)},")
                self.L("};")
                self.scope[st.name] = ("local", f"Rec:{d}")
            elif st.absent is not None and isinstance(st.e, Attr):  # let x = y.項目 else { return … }（null なら返す）
                tok = self.tok(st.e)
                ty = {"Int": "i128", "Money": "i128", "Ratio": "有理", "ID": "String", "Str": "String", "Time": "i64", "Set": "BTreeSet<String>"}.get(tok, "_")
                self.L(f"let {st.name}: {ty} = match {self.place(st.e)}.clone() {{")
                self.L(f"    Some(v) => {self.wrap(tok, 'v') if tok in ('Int', 'Money') else 'v'},")
                self.L(f"    None => return {self.ret_expr(st.absent)},")
                self.L("};")
                self.scope[st.name] = ("local", tok)
            else:
                tok = self.tok(st.e)
                ty = {"Int": "i128", "Money": "i128", "Rational": "有理", "ID": "String", "Str": "String", "Time": "i64", "Set": "BTreeSet<String>"}[tok]
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
                self.L(f"        書込列.push(書込::{d}_{f}({self.rx(tgt.e.e)}, None));")
            elif ft.kind in ("整数", "金額"):
                v = self.conv(ft, '新値', d + '.' + f)
                self.L(f"        let 新値: i128 = {new};")
                self.L(f"        書込列.push(書込::{d}_{f}({self.rx(tgt.e.e)}, {f'Some({v})' if ft.optional else v}));")
            else:
                self.L(f"        書込列.push(書込::{d}_{f}({self.rx(tgt.e.e)}, {f'Some({val})' if ft.optional else val}));")
            self.L("    }")
            self.L("}")
            self.scope = saved
        elif isinstance(st, SetField):
            d = self.info.lookup_vars[st.var]
            key = self.g.key(d)
            t = self.field_type(d, st.f)
            v = self.conv(t, self.rx(st.e), f"{d}.{st.f}") if t.kind in ("整数", "金額") else self.rx(st.e)
            if t.optional and not isinstance(st.e, Null):
                v = f"Some({v})"
            self.L(f"書込列.push(書込::{d}_{st.f}({st.var}.{key}.clone(), {v}));")
        elif isinstance(st, Create):
            dt = self.ck.data[st.data]
            key = self.ck.key_field(st.data)
            given = dict(st.fields)
            self.L(f"let {st.name} = {st.data} {{")
            for f in dt.fields:
                if f is key:
                    v = f"ctx.新{st.data}ID.clone()"
                else:
                    e = given.get(f.name, Null())  # 書かなかった項目は null
                    if isinstance(e, Null):
                        v = "None"
                    elif f.type.kind in ("整数", "金額"):
                        v = self.conv(f.type, self.rx(e), f"{st.data}.{f.name}")
                    else:
                        v = self.rx(e)
                    if f.type.optional and not isinstance(e, Null):
                        v = f"Some({v})"
                self.L(f"    {f.name}: {v},")
            self.L("};")
            self.L(f"書込列.push(書込::{st.data}_新規({st.name}.clone()));")
            self.scope[st.name] = ("local", f"Rec:{st.data}")
        elif isinstance(st, ForEach):
            head, body, saved = self.bind_lines(st.binder, "")
            self.L("let 束の始め = 外部列.len();")
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
            parts.append(f"冪等キー: {self.rx(st.idem)}")
            self.L(f"外部列.push(外部作用::{st.effect.replace('.', '_')} {{ {', '.join(parts)} }});")

    # ---- 関数全体 ----
    def emit(self):
        g, proc, info = self.g, self.proc, self.info
        n = proc.name
        g.w(f"// ---- 処理 {n} ----")
        g.w(f"pub struct {n}入力 {{")
        for name, t in proc.inputs:
            g.w(f"    pub {name}: {rust_type(t)},")
        g.w("}")
        g.w(f"impl {n}入力 {{\n    /// 外部から取り込むときの、範囲の宣言の検査\n    pub fn 検査(&self) -> Result<(), String> {{")
        for name, t in proc.inputs:
            if t.kind == "バイト列" and t.range:
                g.w(f'        範囲検査("{name}の長さ", self.{name}.len() as i128, {opt(t.range.lo)}, {opt(t.range.hi)})?;')
            if t.kind in ("整数", "金額") and t.range:
                g.w(f'        範囲検査("{name}", self.{name} as i128, {opt(t.range.lo)}, {opt(t.range.hi)})?;')
            if t.kind == "写像" and t.value.kind in ("整数", "金額") and t.value.range:
                r = t.value.range
                g.w(f'        for v in self.{name}.values() {{ 範囲検査("{name}", *v as i128, {opt(r.lo)}, {opt(r.hi)})?; }}')
        g.w("        Ok(())\n    }\n}")
        g.w(f"/// 文脈：【参照】（配備時に埋まる）と、採番（実行のたびに基盤が代入）\npub struct {n}文脈 {{")
        for name, t in proc.refs:
            g.w(f"    pub {name}: {rust_type(t)},")
        for d in sorted(set(info.new_ids)):
            g.w(f"    pub 新{d}ID: String,")
        g.w("}")
        g.w(f"pub struct {n}出力 {{\n    pub 結果: {self.rdef.name},\n    pub 書込: Vec<書込>,\n    pub 外部: Vec<外部作用>,\n}}")
        g.w(f"pub fn {n}(ctx: &{n}文脈, 保存: &保存, 入力: &{n}入力) -> {n}出力 {{")
        g.w("    let mut 書込列: Vec<書込> = Vec::new();\n    let mut 外部列: Vec<外部作用> = Vec::new();")
        for name, t in proc.inputs:
            self.scope[name] = ("input", tok_of(t))
        for name, t in proc.refs:
            self.scope[name] = ("ref", tok_of(t))
        for st in proc.body:
            self.stmt(st)
        g.out.extend(self.lines)
        g.w("}\n")


def cargo_toml(name: str, host: bool = False) -> str:
    crate_type = 'crate-type = ["cdylib", "rlib"]\n' if host else ""
    opt = 'opt-level = "s"\n' if host else ""
    return f'''[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"
{crate_type}
[profile.release]
{opt}overflow-checks = true
'''


def generate(ck: Checker) -> str:
    return Gen(ck).lib()
