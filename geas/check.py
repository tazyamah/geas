# SPDX-License-Identifier: GPL-3.0-or-later
"""検査器。型の推論つき。宣言（読む・書く・遷移・外部・結果）と本体のずれ、
行き先の漏れ、丸めの指定漏れ、冪等キーの漏れなどを見つける。"""
from dataclasses import dataclass, field, replace
from fractions import Fraction
from math import ceil, floor

from .ast import *  # noqa: F401,F403
from .names import label_gaps, name_problems

ERROR, WARN, QUESTION = "エラー", "警告", "問い"
METHODS = ("floor", "ceil", "half_up")
def unit_label(u: str) -> str:
    """単位の記号を人向けの言葉に（$名前 は同じ入力・slot の通貨、@… は保存した1件の通貨の項目）"""
    return SHOW_JA.get(u, "別の通貨" if u[:1] in "$@" else u)


CURRENCIES = ("JPY", "USD", "EUR")  # 金額の単位。金額は最小単位（円・セント）の整数で持つ
CONVERSIONS = {"money": ("Int", "Money"), "int": ("Money", "Int"), "time": ("Int", "Time")}  # 名前 → (変える前, 変えた後)


@dataclass
class Diag:
    level: str
    code: str
    where: str
    msg: str

    def __str__(self):
        return f"[{self.level}] {self.where}: {self.msg}（{self.code}）"


def tok_of(t: Type) -> str:
    k = t.kind
    if k in ("ID", "参照"):
        return "ID"
    if k == "文字列":
        return "Str"
    if k == "バイト列":
        return "Bytes"
    if k == "日時":
        return "Time"
    if k == "整数":
        return "Int"
    if k == "金額":
        return "Money"
    if k == "割合":
        return "Ratio"
    if k == "名前":
        return f"State:{t.target}"
    if k == "合成":
        return f"Comp:{t.target}"
    if k == "写像":
        return f"Map:{t.key}:{tok_of(t.value)}"
    if k == "集合":
        return "Set"
    raise ValueError(k)


def phys(t: Type) -> str:
    """範囲から物理数（Rust の型）を自動で選ぶ。"""
    if t.kind == "金額":
        return "i64"
    if t.kind != "整数":
        raise ValueError(t.kind)
    lo, hi = (t.range.lo, t.range.hi) if t.range else (None, None)
    if lo is not None and hi is not None:
        if lo >= 0:
            for name, top in (("u8", 2**8), ("u16", 2**16), ("u32", 2**32), ("u64", 2**64)):
                if hi < top:
                    return name
        for name, bits in (("i8", 8), ("i16", 16), ("i32", 32), ("i64", 64)):
            if -(2 ** (bits - 1)) <= lo and hi < 2 ** (bits - 1):
                return name
        return "i128"
    if lo is not None and lo >= 0:
        return "u64"
    return "i64"


@dataclass
class ProcInfo:
    name: str
    field_reads: set = field(default_factory=set)
    exist_reads: set = field(default_factory=set)
    writes: set = field(default_factory=set)
    write_list: list = field(default_factory=list)
    write_ops: dict = field(default_factory=dict)  # "データ.項目" → 書き方の集合（"=", "+=", "-="）。並列の枝の書き込みが重なってよいか見る
    transitions: set = field(default_factory=set)  # (データ, 元, 先)
    effects: list = field(default_factory=list)  # (名前, 冪等キー)
    returns: set = field(default_factory=set)
    roundings: list = field(default_factory=list)  # (方式, 単位)
    parts: set = field(default_factory=set)  # 呼んだ宣言済みの部品
    new_ids: list = field(default_factory=list)  # 採番するデータ名
    types: dict = field(default_factory=dict)  # id(式) → 型トークン
    lookup_vars: dict = field(default_factory=dict)  # 変数 → データ名

    @property
    def reads(self) -> set:
        data_with_fields = {r.split(".")[0] for r in self.field_reads}
        return set(self.field_reads) | {d for d in self.exist_reads if d not in data_with_fields}


SELECT_KINDS = ("ID", "整数", "金額", "日時", "文字列", "名前", "参照")


def column_type(ck, q, c) -> Type:
    """問い合わせの1列の型（検査済みの前提）"""
    own = {f.name: f for f in ck.data[q.target].fields}
    if c.kind == "参照先":
        tdt = ck.data[own[c.via].type.target]
        t = next(x.type for x in tdt.fields if x.name == c.field)
        return replace(t, optional=t.optional or own[c.via].type.optional)  # 参照が null なら、その先の列も null
    if c.kind == "件数":
        return Type("整数")
    f = own[c.field]
    return Type("整数") if c.kind == "合計" else f.type


def inv_form(inv):
    """不変条件の形。("下限", データ, 項目, 下限) か ("ならば", データ, 状態の項目, 状態, 項目, 数)。それ以外は None"""
    c = inv.cond
    if isinstance(c, Bin) and c.op in (">=", ">") and isinstance(c.l, Name) and isinstance(c.r, Num):
        return ("下限", inv.data, c.l.n, c.r.v + (1 if c.op == ">" else 0))
    if isinstance(c, Implies):
        l, r = c.l, c.r
        if (isinstance(l, Bin) and l.op == "==" and isinstance(l.l, Name) and isinstance(l.r, Name)
                and isinstance(r, Bin) and r.op == ">" and isinstance(r.l, Name) and isinstance(r.r, Num)):
            return ("ならば", inv.data, l.l.n, l.r.n, r.l.n, r.r.v)
    return None


class Checker:
    def __init__(self, prog: Program, strict_names: bool = False):
        self.p = prog
        self.strict_names = strict_names  # 名前の書き方（半角英数・大文字小文字）も検査する。コマンドは常に真
        self.diags: list[Diag] = []
        self.data = {d.name: d for d in prog.data}
        self.states = {s.name: s for s in prog.states}
        self.results = {r.name: r for r in prog.results}
        self.effects = {e.name: e for e in prog.effects}
        self.parts = {x.name: x for x in prog.parts}
        self.comps = {c.target: c for c in prog.composites}
        self.part_types: dict[int, str] = {}  # 保証の式 → 型トークン（生成で使う）
        self.procs = {x.name: x for x in prog.processes}
        self.triggers = {x.name: x for x in prog.triggers}
        self.queries = {x.name: x for x in prog.queries}
        self.query_reads: dict[str, set] = {}
        self.variants = {}
        for s in prog.states:
            for v in [s.initial, *s.middle, *s.final]:
                if v:
                    self.variants[v] = s.name
        self.infos: dict[str, ProcInfo] = {}
        self.invariant_status: list[tuple[str, list[tuple[str, str]]]] = []
        self.inv_forms = [inv_form(inv) for inv in prog.invariants]
        # すべての項目は原則「無いかもしれない」。required と書いたもの・キー・状態の項目だけが必須
        for d in prog.data:
            for f in d.fields:
                f.type.optional = not (f.required or (f.type.kind == "ID" and f.unique) or f.type.kind == "名前")
        self.inv_rows: dict[int, dict[str, list[tuple[bool, str]]]] = {}  # 不変条件 → 処理 → (示せたか, 理由)

    def d(self, level, code, where, msg):
        self.diags.append(Diag(level, code, self.p.show_names(where), self.p.show_names(msg)))

    def key_field(self, dname):
        for f in self.data[dname].fields:
            if f.type.kind == "ID" and f.unique:
                return f
        return None

    def check_module_deed(self, proc, info):
        """モジュールの処理は状態を持たない：データの読み書き・状態の遷移・採番・外部作用ができない（入力から結果を返すだけ）"""
        if proc.name not in self.p.origin:
            return
        used = [what for what, v in (("データを読む", info.reads), ("データを書く", info.writes or info.transitions), ("採番する", info.new_ids), ("外部作用を使う", info.effects)) if v]
        if used:
            self.d(ERROR, "モジュール", proc.name, f"モジュールの処理は、入力から結果を返すだけです（{'・'.join(used)}ことはできません）")

    # ---- 全体 ----
    def run(self):
        if self.strict_names:
            for where, msg in name_problems(self.p, self.p.ltype_names):
                self.diags.append(Diag(ERROR, "名前", where, msg))  # 名前そのものの指摘なので、表示名に置き換えない
            for where, n in label_gaps(self.p, self.p.ltype_names):
                self.diags.append(Diag(WARN, "表示名", where, f"「{self.p.origin.get(n, (None, n))[1]}」に表示名（直前の /// ）がありません。仕様書には名前のまま出ます"))
        self.check_defs()
        for proc in self.p.processes:
            self.infos[proc.name] = ProcChecker(self, proc).run()
            self.check_module_deed(proc, self.infos[proc.name])
        self.check_flows()
        self.check_queries()
        self.check_triggers()
        self.check_effect_waits()
        self.check_events()
        self.check_states()
        self.check_invariants()
        order = {ERROR: 0, QUESTION: 1, WARN: 2}
        self.diags.sort(key=lambda x: order[x.level])
        return self.diags

    @property
    def ok(self):
        return not any(x.level == ERROR for x in self.diags)

    @property
    def ready(self):
        """生成してよいか。問いが残っている（仕様が決まっていない）うちは生成しない"""
        return not any(x.level in (ERROR, QUESTION) for x in self.diags)

    def currency_field(self, data: str, fname: str) -> bool:
        dt = self.data.get(data)
        f = next((x for x in dt.fields if x.name == fname), None) if dt else None
        return f is not None and f.type.kind == "名前" and f.type.target == "Currency"

    def check_units(self, where: str, items):
        """money(通貨) の通貨は、同じ並び（入力・slot、項目、引数、中身）にある通貨 currency 型の名前でなければならない"""
        names = {n for n, t in items if t.kind == "名前" and t.target == "Currency"}
        for n, t in items:
            u = t.unit if t.kind == "金額" else None
            if u is None:
                continue
            if u.startswith("$"):
                if u[1:] not in names:
                    self.d(ERROR, "単位", f"{where} / {n}", f"金額の通貨「{u[1:]}」が、同じ並びにありません（通貨コードの currency 型の名前を書きます）")
            elif u not in CURRENCIES:
                self.d(ERROR, "単位", f"{where} / {n}", f"金額の単位「{u}」は使えません（{'・'.join(CURRENCIES)}）")

    def check_type(self, t: Type, where: str, comp_ok: bool = False):
        """型に出てくる名前（参照先・状態・写像のキー）が定義されているか。合成型は、項目と処理の入力・slot にだけ使える"""
        if t.kind == "合成" and not comp_ok:
            self.d(ERROR, "型", where, f"合成型「{t.target}」は、いまは項目・処理の入力と slot・結果の中身・外部作用の引数・応答の中身・トリガの引数にだけ使えます")
        if t.kind == "参照" and t.target not in self.data:
            self.d(ERROR, "名前", where, f"参照先のデータ「{t.target}」が未定義です")
        if t.kind == "名前" and t.target not in self.states:
            self.d(ERROR, "名前", where, f"型「{t.target}」が未定義です")
        if t.kind == "写像" and t.key != "id" and t.key not in self.data:
            self.d(ERROR, "名前", where, f"キーのデータ「{t.key}」が未定義です")
        if t.kind in ("写像", "集合") and t.value is not None:
            self.check_type(t.value, where, comp_ok)

    def check_unique(self, names, where, what):
        for n in dict.fromkeys(n for n in names if names.count(n) > 1):
            self.d(ERROR, "名前", where, f"{what}「{n}」が2つ以上あります")

    def check_part(self, pt: Part):
        """部品の宣言。保証の式は、引数と result だけを使う、真偽の式"""
        where = f"部品 {pt.name}"
        if pt.name in ("round", *CONVERSIONS):
            self.d(ERROR, "名前", where, "組み込みの部品と同じ名前は使えません")
        names = [n for n, _ in pt.params]
        self.check_unique(names, where, "引数")
        if "result" in names:
            self.d(ERROR, "名前", where, "引数の名前に result は使えません（戻り値の名前です）")
        for n, t in [*pt.params, ("result", pt.ret)]:
            self.check_type(t, f"{where} / {n}", comp_ok=pt.body is not None and n != "result")
            if t.kind not in ("整数", "金額", "割合", "ID", "文字列", "日時") and not (pt.body is not None and t.kind == "合成" and n != "result"):
                self.d(ERROR, "型", where, f"部品の引数と戻り値に使えるのは数・割合・ID・文字列・日時です（{n}）")
        pc = ProcChecker(self, Process(pt.name))
        pc.where = where
        if pt.body is not None:  # convert：式が定義。式の型が戻り値の型と合うことを確かめる（範囲は呼ぶたびに生成コードで確かめる）
            for n, t in pt.params:
                if t.kind in ("整数", "金額", "割合"):
                    pc.var_iv[n] = iv_of(t)
                if t.fills:
                    self.check_fills(where, n, t, pc)
            seen_default = False
            for n, t in pt.params:  # デフォルト値：後ろの引数は、すべてデフォルト値を持つ。値は引数の型と範囲に収まる
                if n not in pt.defaults:
                    if seen_default:
                        self.d(ERROR, "型", where, f"引数「{n}」にもデフォルト値が要ります（デフォルト値のある引数より後ろは、すべて省けるようにします）")
                    continue
                seen_default = True
                dt = pc.ty(pt.defaults[n], {})
                if dt not in (tok_of(t), "?") and not (isinstance(pt.defaults[n], Num) and tok_of(t) in ("Int", "Money")):
                    self.d(ERROR, "型", where, f"引数「{n}」のデフォルト値の型（{dt}）が引数の型（{tok_of(t)}）と違います")
                elif t.kind in ("整数", "金額") and t.range is not None:
                    lo, hi = pc.interval(pt.defaults[n])
                    r = t.range
                    if (r.lo is not None and (lo is None or lo < r.lo)) or (r.hi is not None and (hi is None or hi > r.hi)):
                        self.d(ERROR, "範囲", where, f"引数「{n}」のデフォルト値が、引数の範囲{show_range(r)}に収まりません")
            b = pc.ty(pt.body, {n: tok_of(t) for n, t in pt.params})
            if b not in (tok_of(pt.ret), "?") and not (isinstance(pt.body, Num) and tok_of(pt.ret) in ("Int", "Money")):
                self.d(ERROR, "型", where, f"変換の式の型（{b}）が戻り値の型（{tok_of(pt.ret)}）と違います")
            r = pt.ret.range
            if pt.ret.kind in ("整数", "金額", "割合") and r is not None:  # 範囲は静的に示す。示せなければ、実行時に確かめず検査で止める
                lo, hi = pc.interval(pt.body)
                if (r.lo is not None and (lo is None or lo < r.lo)) or (r.hi is not None and (hi is None or hi > r.hi)):
                    self.d(ERROR, "範囲", where, f"変換の結果が戻り値の範囲{show_range(r)}に収まると示せません"
                           f"（式の範囲 {iv_show((lo, hi))}）。引数の型か式を見直してください")
            elif pt.ret.kind == "文字列" and (pt.ret.range is not None or pt.ret.chars is not None):
                self.d(ERROR, "範囲", where, "長さや文字種が決まっている文字列を返す変換は、まだ静的に示せません")
            self.part_types.update(pc.info.types)
            return
        env = {n: tok_of(t) for n, t in [*pt.params, ("result", pt.ret)]}
        g = pc.ty(pt.guarantee, env)
        if g not in ("Bool", "?"):
            self.d(ERROR, "型", where, f"保証は真偽の式にしてください（{g}）")
        if "result" not in names_in(pt.guarantee):
            self.d(ERROR, "保証", where, "保証が戻り値（result）を使っていません。何も確かめない保証は書けません")
        self.part_types.update(pc.info.types)

    def check_fills(self, where, pname, t: Type, pc):
        """fill：未指定にできる成分に、補う値を示す。補った成分は指定されている扱い（読む前の null の確かめが要らない）"""
        ct = self.comps.get(t.target) if t.kind == "合成" else None
        if ct is None:
            self.d(ERROR, "型", where, f"fill は合成型の引数にだけ書けます（{pname}）")
            return
        for cn, v in t.fills:
            pt = next((x for n, x in ct.parts if n == cn), None)
            if pt is None or not pt.optional:
                self.d(ERROR, "名前", where, f"fill の「{cn}」は、{t.target} の未指定にできる成分ではありません")
                continue
            r = pt.range
            if r is not None and ((r.lo is not None and v < r.lo) or (r.hi is not None and v > r.hi)):
                self.d(ERROR, "範囲", where, f"fill の「{cn}」の値 {v} が、成分の範囲{show_range(r)}に収まりません")
            pc.present.add((("#成分", pname), cn))

    def check_defs(self):
        p = self.p
        # 生成するコードでは、データ・状態・結果・処理・問い合わせ・トリガ・ネクサス・出来事は同じ名前の空間に並ぶ
        tops = [x.name for xs in (p.data, p.states, p.results, p.processes, p.queries, p.triggers, p.flows, p.events, p.parts) for x in xs]
        tops += [c.target for c in p.composites]
        self.check_unique(tops, "全体", "定義の名前")
        self.check_unique([e.name for e in p.effects], "全体", "外部作用")
        self.check_unique([v for s in p.states for v in [s.initial, *s.middle, *s.final] if v], "全体", "状態名")
        for r in p.results:
            self.check_unique([v for v, _ in r.variants], f"結果 {r.name}", "種類")
            for v, ps in r.variants:
                self.check_unique([n for n, _ in ps], f"結果 {r.name} / {v}", "中身の名前")
                for n, t in ps:
                    self.check_type(t, f"結果 {r.name} / {v} / {n}", comp_ok=True)
        for pt in p.parts:
            self.check_part(pt)
        for e in p.effects:
            self.check_unique([n for n, _ in e.params], f"外部作用 {e.name}", "引数")
            for n, t in e.params:
                self.check_type(t, f"外部作用 {e.name} / {n}", comp_ok=True)
        for x in [*p.processes, *p.queries]:
            kind = "処理" if isinstance(x, Process) else "問い合わせ"
            self.check_unique([n for n, _ in [*x.inputs, *x.refs]], f"{kind} {x.name}", "入力・参照の名前")
            for n, t in [*x.inputs, *x.refs]:
                self.check_type(t, f"{kind} {x.name} / {n}", comp_ok=isinstance(x, Process))
        for tg in p.triggers:
            for n, t in tg.params:
                self.check_type(t, f"トリガ {tg.name} / {n}", comp_ok=True)
            self.check_units(f"トリガ {tg.name}", tg.params)
        for r in p.results:
            for v, ps in r.variants:
                self.check_units(f"結果 {r.name} / {v}", ps)
        for e in p.effects:
            self.check_units(f"外部作用 {e.name}", e.params)
        for x in [*p.processes, *p.queries]:
            self.check_units(f"{'処理' if isinstance(x, Process) else '問い合わせ'} {x.name}", [*x.inputs, *x.refs])
        for dt in p.data:
            self.check_units(f"データ {dt.name}", [(f.name, f.type) for f in dt.fields])
        for rp in p.replies:
            self.check_units(f"応答 {rp.name}", rp.params)
        self.check_unique([x.name for x in p.replies], "全体", "応答の宣言")
        for rp in p.replies:
            self.check_unique([n for n, _ in rp.params], f"応答 {rp.name}", "中身の名前")
            for n, t in rp.params:
                self.check_type(t, f"応答 {rp.name} / {n}", comp_ok=True)
        for dt in p.data:
            if self.key_field(dt.name) is None:
                self.d(ERROR, "データ", f"データ {dt.name}", "「ID 一意」の項目（キー）がありません")
            self.check_unique([f.name for f in dt.fields], f"データ {dt.name}", "項目")
            for f in dt.fields:
                self.check_type(f.type, f"{dt.name}.{f.name}", comp_ok=True)
        for s in self.p.states:
            if s.plain:
                continue
            if not s.initial or not (s.final or s.endless):
                self.d(ERROR, "状態", f"状態 {s.name}", "初期と終了の状態は必須です（行き来を繰り返す状態は「終了 なし」と書きます）")
            known = {s.initial, *s.middle, *s.final}
            for a, b, _ in s.rejects:
                for x in (a, b):
                    if x not in known:
                        self.d(ERROR, "名前", f"状態 {s.name}", f"拒否に書かれた「{x}」は状態にありません")

    def check_flows(self):
        in_flow = {}  # 処理 → (最初に書いたフロー, 行き先)
        for fl in self.p.flows:
            for proc, routes in fl.entries:
                where = f"フロー {fl.name} / {proc}"
                if proc not in self.procs:
                    self.d(ERROR, "行き先", where, f"処理「{proc}」が未定義です")
                    continue
                names = [v for v, _ in routes]
                rm = route_map(routes)
                for v, ds in rm.items():
                    if len(ds) > 1 and any(d == "reply" or d.startswith("wait ") for d in ds):
                        self.d(ERROR, "行き先", where, f"結果「{v}」の行き先が複数あります。応答（reply）と待つ（wait）は、par に入れられません")
                    for d in dict.fromkeys(d for d in ds if ds.count(d) > 1):
                        self.d(ERROR, "行き先", where, f"結果「{v}」の行き先「{d}」が2回書かれています")
                # 行き先は処理の結果で決まる。同じ処理を別の行き先で2回書くと、どちらへ進むか決まらない
                if proc in in_flow and in_flow[proc][1] != rm:
                    self.d(ERROR, "行き先", where, f"処理 {proc} は、フロー {in_flow[proc][0]} にも違う行き先で書かれています")
                in_flow.setdefault(proc, (fl.name, rm))
                rdef = self.results.get(self.procs[proc].result)
                if rdef:
                    for v, _ in rdef.variants:
                        if v not in names:
                            self.d(ERROR, "行き先", where, f"結果「{v}」の行き先がありません")
                    for v in names:
                        if v not in [x for x, _ in rdef.variants]:
                            self.d(ERROR, "名前", where, f"「{v}」は結果 {rdef.name} にありません")
                for v, ds in rm.items():  # par の枝の書き込みは、重ならない（または可換な更新だけ）なら許す。実行する順で結果が変わらないため
                    if len(ds) > 1:
                        self.check_par_branches(where, v, [(d, self.infos[d]) for d in ds if d in self.infos])
                for v, dest in routes:
                    base = dest.split(" ", 1)[1] if dest.startswith("wait ") else dest
                    if dest.startswith("wait "):
                        self.check_wait(where, proc, base)
                    elif base != "reply" and base not in self.procs:
                        self.d(ERROR, "行き先", where, f"結果「{v}」の行き先「{base}」が未定義の処理です")
                    elif base != "reply" and rdef:
                        self.check_pass(where, rdef, v, self.procs[base])
        for name in self.procs:
            if name not in in_flow:
                self.d(WARN, "行き先", f"処理 {name}", "どのフローにも入っていません")

    def check_par_branches(self, where, v, branches):
        """par の枝どうしで、書く項目が重ならないこと（実行する順で結果が変わらない）。重なってよいのは、どちらも加算・減算だけの更新。
        ほかの枝が読む項目を書く枝は、読む側が書く前か後かで結果が変わるので止める。データ全体を書く（新規作成）枝は、
        同じデータをほかの枝が触れないようにする"""
        def data_of(x):
            return x.split(".")[0]

        def touched(info):
            return {data_of(x) for x in [*info.field_reads, *info.writes, *info.exist_reads]}

        for i, (a, ia) in enumerate(branches):
            for b, ib in branches[i + 1:]:
                for w in sorted(ia.writes):
                    if "." in w:
                        if w in ib.writes:
                            if not (ia.write_ops.get(w, set()) | ib.write_ops.get(w, set())) <= {"+=", "-="}:
                                self.d(ERROR, "並列", where, f"結果「{v}」の par の枝「{a}」と「{b}」が、同じ項目「{w}」に書いています（加算・減算の更新だけなら重なってよい）")
                        elif w in ib.field_reads or data_of(w) in ib.new_ids:
                            self.d(ERROR, "並列", where, f"結果「{v}」の par の枝「{a}」が書く「{w}」を、枝「{b}」が読んでいます（読む前か後かで結果が変わります）")
                    elif data_of(w) in touched(ib):
                        self.d(ERROR, "並列", where, f"結果「{v}」の par の枝「{a}」が新規作成する「{w}」を、枝「{b}」も触っています（採番と読み書きの順が結果に効きます）")
                for w in sorted(ib.writes):
                    if "." in w and w in ia.field_reads and w not in ia.writes:
                        self.d(ERROR, "並列", where, f"結果「{v}」の par の枝「{b}」が書く「{w}」を、枝「{a}」が読んでいます（読む前か後かで結果が変わります）")
                    elif "." not in w and data_of(w) in touched(ia):
                        self.d(ERROR, "並列", where, f"結果「{v}」の par の枝「{b}」が新規作成する「{w}」を、枝「{a}」も触っています（採番と読み書きの順が結果に効きます）")
                for ta in ia.transitions:
                    for tb in ib.transitions:
                        if ta[0] == tb[0] and ta[1] != "(新規)" and tb[1] != "(新規)":
                            self.d(ERROR, "並列", where, f"結果「{v}」の par の枝「{a}」と「{b}」が、どちらも {ta[0]} の状態を動かします")

    def check_effect_waits(self):
        """外部作用を要求する処理は、結果を待つ経路（フローの「待つ」）を持つか、待たない理由を宣言する。
        信頼できない先（ネットワーク・外部サービス）への永続化が、黙って失敗しっぱなしになるのを防ぐ。
        論理的な失敗の理由（失敗 …）を宣言した外部作用は、待つ先のトリガに理由ごとの応答も要る。"""
        waits = {}
        for fl in self.p.flows:
            for proc, routes in fl.entries:
                for _, d in routes:
                    if d.startswith("wait "):
                        waits.setdefault(proc, set()).add(d.split(" ", 1)[1])
        triggers = {t.name: t for t in self.p.triggers}
        used = {e for info in self.infos.values() for e, _ in info.effects}
        for e in self.p.effects:
            if e.name not in used:
                self.d(WARN, "外部作用", f"外部作用 {e.name}", "どの処理からも要求されていません")
            if e.failures and e.no_wait is not None:
                self.d(ERROR, "失敗", f"外部作用 {e.name}", "失敗の理由を宣言しながら no_wait（待たない）にしています。"
                       "待たないと、理由ごとの応答を受ける先がありません（どちらかにしてください）")
        for proc, info in self.infos.items():
            for eff in sorted({e for e, _ in info.effects}):
                decl = self.effects.get(eff)
                if decl is None or decl.no_wait is not None:
                    continue
                if proc not in waits:
                    self.d(ERROR, "失敗", f"処理 {proc}", f"外部作用 {eff} の失敗の扱いが決まっていません（ネクサスで「wait」へ進めるか、外部作用の宣言に no_wait \"人が決めた理由\" を書いてください）")
                    continue
                for tname in sorted(waits[proc]):
                    tg = triggers.get(tname)
                    answered = {key for k, key, _ in tg.causes if k == "reply"} if tg else set()
                    for reason in decl.failures:
                        if reason not in answered:
                            self.d(ERROR, "失敗", f"処理 {proc}", f"外部作用 {eff} の失敗の理由「{reason}」への応答が、トリガ {tname} にありません")

    def check_queries(self):
        """問い合わせ（読むだけの処理）の検査。使う項目の型、入力との型の一致、宣言した「読む」と実際のずれ。"""
        for q in self.p.queries:
            where = f"問い合わせ {q.name}"
            if q.name in self.procs:
                self.d(ERROR, "名前", where, "同じ名前の処理があります")
            dt = self.data.get(q.target)
            if dt is None:
                self.d(ERROR, "名前", where, f"対象のデータ「{q.target}」が未定義です")
                continue
            fields = {f.name: f for f in dt.fields}
            used, ins = set(), dict(q.inputs)

            def field_of(fn):
                if fn not in fields:
                    self.d(ERROR, "名前", where, f"データ {dt.name} に項目「{fn}」がありません")
                    return None
                used.add(f"{dt.name}.{fn}")
                return fields[fn]

            for group in q.conds:
                for fn, op, st in group:
                    f = field_of(fn)
                    if f is None:
                        continue
                    if st.isdigit():
                        if f.type.kind not in ("整数", "金額", "日時"):
                            self.d(ERROR, "型", where, f"条件「{fn} {op} {st}」: 数との比較は、整数・金額・日時の項目にだけ使えます")
                    elif f.type.kind != "名前" or st not in [x for s in self.p.states if s.name == f.type.target for x in [s.initial, *s.middle, *s.final]]:
                        self.d(ERROR, "型", where, f"条件「{fn} {op} {st}」: 「{st}」は項目 {fn} の状態ではありません")
            for fn, op, arg in q.filters:
                f = field_of(fn)
                if f is None:
                    continue
                if op == "直近":
                    if f.type.kind != "日時":
                        self.d(ERROR, "型", where, f"「{fn} 直近」は日時の項目にだけ使えます")
                    if not any(n == "現在" for n, _ in q.refs):
                        self.d(ERROR, "名前", where, "「直近」を使うには、参照 現在 が必要です")
                    continue
                t = ins.get(arg)
                if t is None:
                    self.d(ERROR, "名前", where, f"絞り込みの入力「{arg}」が、入力にありません")
                    continue
                if op.startswith("含む") and not (f.type.kind == "文字列" and t.kind == "文字列"):
                    self.d(ERROR, "型", where, f"「{fn} 含む {arg}」は文字列どうしにだけ使えます")
                if op == "一致" and not (f.type.kind == t.kind and f.type.kind in ("文字列", "ID", "整数")):
                    self.d(ERROR, "型", where, f"「{fn} equals {arg}」は同じ種類の文字列・ID・整数どうしにだけ使えます")
                if op in ("以上", "以下") and not (f.type.kind == t.kind and f.type.kind in ("日時", "整数", "金額")):
                    self.d(ERROR, "型", where, f"「{fn} {op} {arg}」は同じ種類の数・日時どうしにだけ使えます")
            seen_cols = set()
            for c in q.select:
                if c.name in seen_cols:
                    self.d(ERROR, "名前", where, f"select の列名「{c.name}」が重なっています（as で別の名前にしてください）")
                seen_cols.add(c.name)
                if c.kind == "参照先":
                    rf = field_of(c.via)
                    if rf is None:
                        continue
                    if rf.type.kind != "参照" or rf.type.target not in self.data:
                        self.d(ERROR, "型", where, f"「{c.via}.{c.field}」: {c.via} は他のデータへの参照（ref）ではありません")
                        continue
                    tdt = self.data[rf.type.target]
                    tf = next((x for x in tdt.fields if x.name == c.field), None)
                    if tf is None:
                        self.d(ERROR, "名前", where, f"データ {tdt.name} に項目「{c.field}」がありません")
                        continue
                    used.add(f"{tdt.name}.{c.field}")
                    if tf.type.kind not in SELECT_KINDS:
                        self.d(ERROR, "型", where, f"「{c.via}.{c.field}」は select に書けない型です（{tf.type.kind}）")
                    continue
                f = field_of(c.field)
                if f is None:
                    continue
                if c.kind == "項目" and f.type.kind not in SELECT_KINDS:
                    self.d(ERROR, "型", where, f"「{c.field}」は select に書けない型です（{f.type.kind}）。集まりは sum( ) か count( ) にしてください")
                if c.kind in ("合計", "件数") and f.type.kind != "写像":
                    self.d(ERROR, "型", where, f"「{c.field}」は集計できません（sum と count は、キーつきの集まり 〈データ → 値〉 の項目にだけ使えます）")
                if c.kind == "合計" and f.type.kind == "写像" and f.type.value.kind not in ("整数", "金額"):
                    self.d(ERROR, "型", where, f"sum({c.field}) は、値が整数か金額の集まりにだけ使えます")
            if q.order:
                f = field_of(q.order[0])
                if f is not None and f.type.kind not in ("日時", "整数", "金額"):
                    self.d(ERROR, "型", where, f"並びの項目「{q.order[0]}」は数か日時である必要があります")
            if q.page is not None:
                t = ins.get(q.page)
                if t is None or t.kind != "整数":
                    self.d(ERROR, "型", where, f"1ページの件数「{q.page}」は、整数の入力である必要があります")
            if (q.page is not None or q.page_n is not None) and not q.order:
                self.d(ERROR, "並び", where, "page には order が要ります（並びが無いと、どの1ページかが決まらず、実行のたびに変わりえます）")
            for n, t in q.inputs:
                if not t.optional and not any(n == a for _, _, a in q.filters) and n != q.page:
                    self.d(WARN, "宣言", where, f"入力「{n}」はどこにも使われていません")
            declared, actual = set(q.reads), used
            for x in sorted(actual - declared):
                self.d(ERROR, "宣言", where, f"問い合わせは「{x}」を読むのに、宣言（読む）にありません")
            for x in sorted(declared - actual):
                self.d(ERROR, "宣言", where, f"宣言（読む）に「{x}」がありますが、使っていません")
            self.query_reads[q.name] = actual

    def check_pass(self, where, rdef, v, nxt):
        """結果の中身を、次の処理の入力へ名前と型で渡せるか（過不足も型違いも許さない）"""
        ps = next((ps for x, ps in rdef.variants if x == v), None)
        if ps is None:
            return
        have, want = {n: show_type(t) for n, t in ps}, {n: show_type(t) for n, t in nxt.inputs}
        for n in want:
            if n not in have:
                self.d(ERROR, "行き先", where, f"処理 {nxt.name} の入力「{n}」が、結果「{v}」の中身にありません")
            elif have[n] != want[n]:
                self.d(ERROR, "行き先", where, f"結果「{v}」の中身「{n}」の型（{have[n]}）が、処理 {nxt.name} の入力（{want[n]}）と違います")
        for n in have:
            if n not in want:
                self.d(ERROR, "行き先", where, f"結果「{v}」の中身「{n}」を、処理 {nxt.name} は入力にもっていません")

    def check_wait(self, where, proc, trig):
        tg = self.triggers.get(trig)
        if tg is None:
            self.d(ERROR, "トリガ", where, f"wait 先のトリガ「{trig}」が未定義です")
            return
        have = {n for n, _ in self.procs[proc].inputs}
        for n, _ in tg.params:
            if n not in have:
                self.d(ERROR, "トリガ", where, f"トリガ {trig} の引数「{n}」を、処理 {proc} は入力にもっていません")
        for n, t in self.procs[proc].inputs:
            if t.optional and any(n == x for x, _ in tg.params):
                self.d(ERROR, "トリガ", where, f"トリガ {trig} の引数「{n}」には、required の入力を渡してください（無いかもしれない値では待てません）")
        mine = {n: show_type(t) for n, t in self.procs[proc].inputs}
        for n, t in tg.params:
            if n in mine and mine[n] != show_type(t):
                self.d(ERROR, "トリガ", where, f"トリガ {trig} の引数「{n}」の型が、処理 {proc} の入力と違います")

    def cause_args(self, tg, kind, key, where):
        """要因が成り立ったとき、行き先の処理へ渡るもの：トリガの引数に、起きた応答の中身（all は全部の応答の中身）を足したもの"""
        reps = {x.name: x for x in self.p.replies}
        names = key.split(",") if kind == "all" else [key] if kind == "reply" else []
        args = list(tg.params)
        for n in names:
            for pn, pt in reps[n].params if n in reps else []:
                if any(pn == a for a, _ in args):
                    self.d(ERROR, "トリガ", where, f"応答「{n}」の中身「{pn}」が、トリガの引数か、ほかの応答の中身と同じ名前です（行き先の処理の入力として区別できません）")
                else:
                    args.append((pn, pt))
        return args

    def check_triggers(self):
        used = {x for tg in self.p.triggers for k, key, _ in tg.causes for x in (key.split(",") if k == "all" else [key] if k == "reply" else [])}
        for rp in self.p.replies:
            if rp.name not in used:
                self.d(WARN, "トリガ", f"応答 {rp.name}", "どのトリガの要因にも使われていません")
        waited = set()
        for fl in self.p.flows:
            for _, routes in fl.entries:
                waited |= {d.split(" ", 1)[1] for _, d in routes if d.startswith("wait ")}
        names = [t.name for t in self.p.triggers]
        for tg in self.p.triggers:
            where = f"トリガ {tg.name}"
            if names.count(tg.name) > 1:
                self.d(ERROR, "トリガ", where, "同じ名前のトリガが複数あります")
            deadlines = [key for k, key, _ in tg.causes if k == "deadline"]
            if len(deadlines) > 1:
                self.d(ERROR, "トリガ", where, "期限が複数あります（先に起きた方が勝つので、長い方は起きません）")
            if any(int(x) <= 0 for x in deadlines):
                self.d(ERROR, "トリガ", where, "期限は0より長くしてください")
            if not any(k == "deadline" for k, _, _ in tg.causes):
                self.d(ERROR, "トリガ", where, "期限がありません（待ち続けないよう、期限と期限切れの行き先が必要です）")
            if not any(k in ("reply", "all") for k, _, _ in tg.causes):
                self.d(WARN, "トリガ", where, "解く応答がありません（期限切れでしか終わらない待ちです）")
            singles = {key for k, key, _ in tg.causes if k == "reply"}
            in_all: set = set()
            for k, key, _ in tg.causes:  # 待ち合わせ（all）：2つ以上の応答。ほかの行と同じ応答を使うと、先に来た1つで進むのか全部待つのか決まらない
                if k != "all":
                    continue
                names = key.split(",")
                if len(names) < 2 or len(set(names)) != len(names):
                    self.d(ERROR, "トリガ", where, f"all の応答は、重ならない2つ以上を並べてください（{key}）。1つだけなら、all を付けない reply の行にします")
                for n in names:
                    if n in singles:
                        self.d(ERROR, "トリガ", where, f"応答「{n}」が、all の行と単独の reply の行の両方にあります（先に来た1つで進むのか、全部待つのか、決まりません）")
                    elif n in in_all:
                        self.d(ERROR, "トリガ", where, f"応答「{n}」が、複数の all の行にあります（どの待ち合わせに数えるのか、決まりません）")
                    in_all.add(n)
            seen = set()
            for k, key, dest in tg.causes:
                skey = ",".join(sorted(key.split(","))) if k == "all" else key  # all は、並べる順が違っても同じ要因
                if (k, skey) in seen:
                    self.d(ERROR, "トリガ", where, f"要因「{k} {skey}」が重複しています")
                seen.add((k, skey))
                proc = self.procs.get(dest)
                if proc is None:
                    self.d(ERROR, "トリガ", where, f"要因「{k} {key}」の行き先「{dest}」が未定義の処理です")
                else:
                    want = [(n, show_type(t)) for n, t in self.cause_args(tg, k, key, where)]
                    have = [(n, show_type(t)) for n, t in proc.inputs]
                    if len(want) == len(tg.params):
                        bad = have != want
                    else:  # 応答の中身が加わるときは、並びを問わない
                        bad = sorted(have) != sorted(want)
                    if bad:
                        extra = "" if len(want) == len(tg.params) else "と応答の中身"
                        self.d(ERROR, "トリガ", where, f"行き先の処理 {dest} の入力（名前と型）が、トリガの引数{extra}と合いません")
            if tg.name not in waited:
                self.d(WARN, "トリガ", where, "どのネクサスからも「wait」で作られていません")

    def check_events(self):
        """出来事の検査。「各」は、問い合わせが返すデータ1件ずつに、その識別子だけを入力にもつ処理を起こす。"""
        names = [e.name for e in self.p.events]
        for ev in self.p.events:
            where = f"出来事 {ev.name}"
            if names.count(ev.name) > 1:
                self.d(ERROR, "名前", where, "同じ名前の出来事が複数あります")
            if not ev.steps:
                self.d(WARN, "出来事", where, "起こす処理がありません")
            for kind, qn, pn in ev.steps:
                proc = self.procs.get(pn)
                if proc is None:
                    self.d(ERROR, "出来事", where, f"起こす処理「{pn}」が未定義です")
                    continue
                if kind != "各":
                    continue
                q = self.queries.get(qn)
                if q is None:
                    self.d(ERROR, "出来事", where, f"問い合わせ「{qn}」が未定義です")
                    continue
                if q.select:
                    self.d(ERROR, "出来事", where, f"問い合わせ {qn} は select で列を返すので、出来事の「each」には使えません（IDの列を返す問い合わせにしてください）")
                    continue
                if any(not t.optional for _, t in q.inputs):
                    self.d(ERROR, "出来事", where, f"問い合わせ {qn} は入力が要るので、出来事からは呼べません（任意の入力だけにしてください）")
                dt = self.data.get(q.target)
                if dt is None:
                    continue
                key = self.key_field(dt.name)
                if [(n, show_type(t)) for n, t in proc.inputs] != [(key.name, "ID")]:
                    self.d(ERROR, "出来事", where, f"「each {qn}」は {dt.name} の {key.name} だけを渡すので、処理 {pn} の入力は「{key.name}: ID」だけにしてください")

    def check_states(self):
        for s in self.p.states:
            if s.plain:
                continue
            edges = {}
            for info in self.infos.values():
                for dt, a, b in info.transitions:
                    if dt in self.data and any(f.type.target == s.name for f in self.data[dt].fields):
                        edges.setdefault(a, set()).add(b)
            def reach(starts):
                seen, todo = set(), list(starts)
                while todo:
                    y = todo.pop()
                    if y not in seen:
                        seen.add(y)
                        todo += edges.get(y, set())
                return seen
            got = reach([s.initial, *edges.get("(新規)", set())])
            for x in [*s.middle, *s.final]:
                if x not in got:
                    self.d(WARN, "状態", f"状態 {s.name}", f"「{x}」には、初期からも新規作成からも進む処理が宣言されていません")
            for x in [s.initial, *s.middle]:
                seen, todo = set(), [x]
                while todo:
                    y = todo.pop()
                    if y in seen:
                        continue
                    seen.add(y)
                    todo += edges.get(y, set())
                if not s.endless and not (seen & set(s.final)):
                    self.d(WARN, "状態", f"状態 {s.name}", f"「{x}」から終了の状態へ進む処理が宣言されていません")

    def check_invariants(self):
        """不変条件ごとに、書き込む処理のすべてで示せたかをまとめる。示せない書き込みは、書き込みの時点でエラーにしてある"""
        for i, inv in enumerate(self.p.invariants):
            text = f"{inv.data}: {show(inv.cond)}"
            form = self.inv_forms[i]
            where = f"不変条件 {text}"
            if inv.data not in self.data:
                self.d(ERROR, "名前", where, f"データ「{inv.data}」が未定義です")
                continue
            if form is None:
                self.d(ERROR, "不変条件", where, "この形の不変条件は、まだ検査できません"
                       "（「項目 >= 数」と「状態の項目 == 状態 implies 項目 > 数」だけ）。検査できない約束は書けません")
                continue
            fields = {f.name: f for f in self.data[inv.data].fields}
            names = [form[2]] + ([form[4]] if form[0] == "ならば" else [])
            bad = [n for n in names if n not in fields]
            if bad:
                self.d(ERROR, "名前", where, f"データ {inv.data} に項目「{bad[0]}」がありません")
                continue
            if form[0] == "ならば":
                st = self.states.get(fields[form[2]].type.target)
                if st is None or form[3] not in [st.initial, *st.middle, *st.final]:
                    self.d(ERROR, "名前", where, f"「{form[3]}」は項目 {form[2]} の状態ではありません")
                    continue
            rows = []
            for proc in self.p.processes:
                got = self.inv_rows.get(i, {}).get(proc.name)
                if not got:
                    rows.append((proc.name, "対象外"))
                elif all(ok for ok, _ in got):
                    notes = list(dict.fromkeys(n for _, n in got))
                    rows.append((proc.name, "証明済み（" + " ／ ".join(notes) + "）"))
                else:
                    rows.append((proc.name, "証明できない"))
            self.invariant_status.append((text, rows))


def names_in(e) -> set:
    """式に出てくる変数の名前"""
    if isinstance(e, Name):
        return {e.n}
    if isinstance(e, Attr):
        return names_in(e.e)
    if isinstance(e, Index):
        return names_in(e.e)
    if isinstance(e, Call):
        return set().union(*map(names_in, e.args)) if e.args else set()
    if isinstance(e, Bin):
        return names_in(e.l) | names_in(e.r)
    if isinstance(e, Tuple):
        return set().union(*map(names_in, e.items))
    if isinstance(e, Make):
        return set().union(*(names_in(x) for _, x in e.fields)) if e.fields else set()
    return set()


# ---- 値の範囲（区間）。None は「限りなし」 ----
def iv_add(a, b):
    return (None if None in (a[0], b[0]) else a[0] + b[0], None if None in (a[1], b[1]) else a[1] + b[1])


def iv_neg(a):
    return (None if a[1] is None else -a[1], None if a[0] is None else -a[0])


def iv_mul(a, b):
    if a[0] is not None and b[0] is not None and a[0] >= 0 and b[0] >= 0:
        return (a[0] * b[0], None if None in (a[1], b[1]) else a[1] * b[1])
    if None in (*a, *b):
        return (None, None)
    ps = [x * y for x in a for y in b]
    return (min(ps), max(ps))


def iv_of(t: Type):
    r = t.range if t.kind in ("整数", "金額", "割合") else None
    return (None, None) if r is None else (r.lo, r.hi)


def iv_show(a):
    return f"{'' if a[0] is None else a[0]}..{'' if a[1] is None else a[1]}"


def iv_join(a: dict, b: dict) -> dict:
    """2つの道の合流。両方で分かっていることだけが残る（下限は小さいほう）"""
    return {k: min(a[k], b[k]) for k in a.keys() & b.keys()}

class ProcChecker:
    def __init__(self, ck: Checker, proc: Process):
        self.ck = ck
        self.proc = proc
        self.info = ProcInfo(proc.name)
        self.env: dict[str, str] = {}
        self.where = f"処理 {proc.name}"
        self.exist_sets: dict[str, tuple[str, str]] = {}  # 集合の変数 → (データ, 回した元)
        # 確認はデータ名でなく「どのキーで引いたものか」に効く。別のキーで引いたものには効かない
        self.guarded: set[tuple[str, str]] = set()  # 存在を確かめた (データ, キーの式) か (データ, "∈" + 回した元)
        self.state_guards: set[tuple[str, str, str]] = set()  # (束縛, 項目, 元の状態)
        self.bindings: dict[str, str] = {}  # 変数 → 束縛（同じ名前で引き直すと別の束縛になる）
        self.binding_data: dict[str, str] = {}  # 束縛 → データ名（同じデータの別の束縛は、同じ1件かもしれない）
        # 分かっている下限。(束縛, 項目) → 下限、または ("#不足", データ, 項目, 回した元) → 0（減らす量が足りていると確かめ済み）
        self.facts: dict[tuple, object] = {}
        self.var_iv: dict[str, tuple] = {}  # let の変数 → 値の範囲
        self.var_unit: dict[str, str] = {}  # let・回す変数 → 金額の単位
        self.binder_src: dict[str, str] = {}  # 集まりを回すキーの変数 → 回す元の式（通貨の確認が、その集まり全部に効くか見る）
        self.cur_ok: dict[tuple, str] = {}  # (データ, 通貨の項目, 回す元) → 通貨の単位：その集まりのどの1件も、その通貨と確かめ済み
        self.cur_sets: dict[str, tuple] = {}  # 変数 → 「通貨が違う1件の集まり」の形（確認する前）
        self.loop_deps: set[str] = set()  # for の中で、回すたびに違う値の変数
        self.present: set[tuple[str, str]] = set()  # null でないと確かめた (束縛, 項目)
        self.writing = False  # 書き込む先として見ているとき（null かどうかは問わない）
        self.shortage_sets: dict[str, tuple[str, str, str]] = {}  # { k | (k, v) in 元, D[k].f < v } の変数 → (D, f, 元)
        self.accessed_vars: set[str] = set()
        self.created: set[str] = set()
        self.pos = 0
        self.lookup_data: dict[str, str] = self.info.lookup_vars

    def d(self, level, code, msg, line=0):
        self.ck.d(level, code, self.where + (f"（{line}行目）" if line else ""), msg)

    # ---- 型の推論 ----
    def ty(self, e, env=None) -> str:
        env = self.env if env is None else env
        t = self._ty(e, env)
        self.info.types[id(e)] = t
        return t

    def _ty(self, e, env) -> str:
        ck = self.ck
        if isinstance(e, Num):
            return "Int"
        if isinstance(e, Dur):
            return "Dur"
        if isinstance(e, Null):
            return "Null"
        if isinstance(e, Name):
            if (e.n in env and not self.writing and self.null_input(e, env) and ("#入力", e.n) not in self.present):
                self.d(ERROR, "null", f"入力「{e.n}」は無いことがあります。使う前に確かめてください"
                       f"（let x = {e.n} else {{ return … }}、または if {e.n} is null {{ return … }}。必ず要るなら入力に required）")
            if e.n in env:
                return env[e.n]
            if e.n in ck.variants:
                return f"State:{ck.variants[e.n]}"
            self.d(ERROR, "名前", f"「{e.n}」は定義されていません")
            return "?"
        if isinstance(e, Index):
            if e.d not in ck.data:
                self.d(ERROR, "名前", f"データ「{e.d}」が未定義です")
                return "?"
            if self.ty(e.e, env) != "ID":
                self.d(ERROR, "型", f"{e.d}[ ] の中は ID である必要があります")
            return f"Rec:{e.d}"
        if isinstance(e, Make):
            return self.make(e, env)
        if isinstance(e, Attr):
            base = e.e
            bt = self.ty(base, env)
            if bt.startswith("Comp:"):  # 合成型の成分。入れ子でも、成分は平らに読める
                pt = next((t for n, t in ck.comps[bt[5:]].parts if n == e.n), None)
                if pt is None:
                    self.d(ERROR, "名前", f"合成型 {bt[5:]} に成分「{e.n}」がありません")
                    return "?"
                if pt.optional and not self.writing and (("#成分", show(base)), e.n) not in self.present:
                    self.d(ERROR, "null", f"「{show(e)}」は未指定かもしれない成分です。使う前に確かめてください"
                           f"（if {show(e)} is null {{ return … }}）。未指定にアクセスしうる道があるとエラーです")
                return tok_of(pt)
            if not bt.startswith("Rec:"):
                self.d(ERROR, "型", f"「{show(e)}」: 「.{e.n}」はデータにしか使えません")
                return "?"
            dname = bt[4:]
            fld = next((f for f in ck.data[dname].fields if f.name == e.n), None)
            if fld is None:
                self.d(ERROR, "名前", f"データ {dname} に項目「{e.n}」がありません")
                return "?"
            if fld.type.optional and not self.writing and not (
                    isinstance(base, Name) and base.n in self.bindings and (self.bindings[base.n], e.n) in self.present):
                self.d(ERROR, "null", f"「{show(e)}」は無い（null の）ことがあります。使う前に確かめてください"
                       f"（let x = {show(e)} else {{ return … }}、または if {show(e)} is null {{ return … }}。必ずあるなら項目に required）")
            if isinstance(base, Index) and not self.exists_known(base, env):
                self.d(QUESTION, "存在", f"{show(base)} が存在しないとき、どうしますか"
                       f"（{dname}の存在を確かめて返す処理が、使う前にありません）")
            if isinstance(base, Name):
                self.accessed_vars.add(base.n)
            if not (isinstance(base, Name) and base.n in self.created):
                self.info.field_reads.add(f"{dname}.{e.n}")
            return tok_of(fld.type)
        if isinstance(e, Pred) and e.kind == "null" and isinstance(e.e, Name):
            if not self.null_input(e.e, env):
                self.d(ERROR, "型", f"「is null」は、無いかもしれない入力（required でない入力）にだけ使えます（{e.e.n}）")
            self.writing, saved = True, self.writing
            self.ty(e.e, env)
            self.writing = saved
            return "Bool"
        if (isinstance(e, Pred) and e.kind == "null" and isinstance(e.e, Attr)
                and self.ty_quiet(e.e.e, env).startswith("Comp:")):
            ct = ck.comps[self.ty_quiet(e.e.e, env)[5:]]
            pt = next((t for n, t in ct.parts if n == e.e.n), None)
            if pt is None or not pt.optional:
                self.d(ERROR, "型", f"「is null」は、未指定にできる成分（optional な成分）にだけ使えます（{show(e.e)}）")
            self.writing, saved = True, self.writing
            self.ty(e.e, env)
            self.writing = saved
            return "Bool"
        if isinstance(e, Pred) and e.kind == "null":
            if not (isinstance(e.e, Attr) and self.null_field(e.e, env) is not None):
                self.d(ERROR, "型", f"「is null」は、無いかもしれない項目（required でない項目）にだけ使えます（{show(e.e)}）")
                return "Bool"
            self.writing, saved = True, self.writing
            self.ty(e.e, env)
            self.writing = saved
            return "Bool"
        if isinstance(e, Pred):
            if e.kind in ("ある", "ない"):
                if isinstance(e.e, Index):
                    self.ty(e.e, env)
                    self.info.exist_reads.add(e.e.d)
                else:
                    self.d(ERROR, "型", "「ある／ない」はデータの参照 D[…] にだけ使えます")
            else:
                if self.ty(e.e, env) not in ("Set", "Str"):
                    self.d(ERROR, "型", f"「{show(e.e)}」は集合でも文字列でもありません")
            return "Bool"
        if isinstance(e, Bin):
            return self.binop(e, env)
        if isinstance(e, Call) and e.n in CONVERSIONS:
            # 型を変えてから計算する：money(整数) は金額、int(金額) は整数
            src, dst = CONVERSIONS[e.n]
            if e.n == "money" and len(e.args) == 2:  # money(整数, 単位)：最小単位（円・セント）の個数から金額を作る
                if not (isinstance(e.args[1], Name) and e.args[1].n in CURRENCIES):
                    self.d(ERROR, "型", f"money( ) の単位は {'・'.join(CURRENCIES)} のどれかです")
            elif len(e.args) != 1:
                self.d(ERROR, "型", f"{e.n}( ) の引数は1つです")
                return "?"
            x = self.ty(e.args[0], env)
            if x not in (src, "?"):
                self.d(ERROR, "型", f"{e.n}( ) で変えられるのは{'整数' if src == 'Int' else '金額'}だけです（{x}）")
            return dst
        if isinstance(e, Call) and e.n in ck.parts:
            pt = ck.parts[e.n]
            if len(e.args) < len(pt.params) and all(n in pt.defaults for n, _ in pt.params[len(e.args):]):
                e.args = [*e.args, *(pt.defaults[n] for n, _ in pt.params[len(e.args):])]  # 省いた引数は、デフォルト値で補う
            if len(e.args) != len(pt.params):
                self.d(ERROR, "型", f"部品 {e.n} の引数は {len(pt.params)} 個です" + ("（デフォルト値のある引数だけ省けます）" if pt.defaults else ""))
                return tok_of(pt.ret)
            for a, (n, t) in zip(e.args, pt.params):
                at = self.ty(a, env)
                if at != tok_of(t) and not (isinstance(a, Num) and tok_of(t) in ("Int", "Money")) and at != "?":
                    self.d(ERROR, "型", f"部品 {e.n} の引数「{n}」の型が違います（{at}）。違う型は、変換してから渡してください")
                self.fit_unit(a, t, f"部品 {e.n} の引数「{n}」", e.line if hasattr(e, "line") else 0,
                              siblings={pn: x for (pn, _), x in zip(pt.params, e.args)})
            if pt.external:  # convert は生成コードの中で計算するので、外の部品は要らない
                self.info.parts.add(e.n)
            return tok_of(pt.ret)
        if isinstance(e, Call):
            if e.n != "round" or len(e.args) != 3:
                self.d(ERROR, "名前", f"部品「{e.n}」は呼べません（宣言した部品と、丸め(値, 方式, 単位)、型の変換 money( )・int( ) だけ）")
                return "?"
            x = self.ty(e.args[0], env)
            m, u = e.args[1], e.args[2]
            if x != "Rational":
                self.d(ERROR, "型", "丸めの対象は端数の出る値（金額 × 割合）である必要があります")
            if not (isinstance(m, Name) and m.n in METHODS):
                self.d(ERROR, "型", f"丸めの方式は {'／'.join(METHODS)} のどれかです")
            if not isinstance(u, Name):
                self.d(ERROR, "型", "丸めの単位が必要です")
            else:
                self.info.roundings.append((getattr(m, "n", "?"), u.n))
                au = self.unit_of(e.args[0], env)
                ru = self.cur_name_unit(u.n, env)
                if ru is None:
                    self.d(ERROR, "型", f"丸めの単位「{u.n}」は使えません（{'・'.join(CURRENCIES)}、または通貨の入力・slot の名前）")
                elif au is not None and au != ru:
                    self.d(ERROR, "単位", f"{unit_label(au)}の金額を{unit_label(ru)}の単位で丸めています")
            # 丸めた結果は、端数の元になった数の型（金額 × 割合 なら金額、整数 × 割合 なら整数）
            inner = e.args[0]
            if isinstance(inner, Bin) and "Money" not in (self.info.types.get(id(inner.l)), self.info.types.get(id(inner.r))):
                return "Int"
            return "Money"
        if isinstance(e, (SetB, SumE)):
            env2 = self.bind(e.binder, env)
            if e.cond is not None:
                self.ty(e.cond, env2)
                self.note_exist_set(e)
            et = self.ty(e.elem, env2)
            if isinstance(e, SetB):
                if et != "ID":
                    self.d(ERROR, "型", "集合の要素は ID である必要があります")
                return "Set"
            if et not in ("Int", "Money"):
                self.d(ERROR, "型", "Σ の要素は数である必要があります")
            return et
        if isinstance(e, Implies):
            self.ty(e.l, env)
            self.ty(e.r, env)
            return "Bool"
        raise ValueError(e)

    def ty_quiet(self, e, env) -> str:
        """診断を増やさずに式の型を見る（調べるだけ）"""
        n = len(self.ck.diags)
        saved_types = dict(self.info.types)
        try:
            return self._ty(e, env)
        finally:
            del self.ck.diags[n:]
            self.info.types = saved_types

    def make(self, e: Make, env) -> str:
        """合成型の値を作る式。成分はすべて、過不足なく、型と範囲が合う値で書く"""
        ct = self.ck.comps[e.n]
        given = dict(e.fields)
        for n, _ in e.fields:
            if sum(1 for x, _ in e.fields if x == n) > 1:
                self.d(ERROR, "名前", f"{e.n} の成分「{n}」が2つ以上あります", e.line)
        for pn, pt in ct.parts:
            if pn not in given and not pt.optional:
                self.d(ERROR, "項目", f"{e.n} の成分「{pn}」がありません（未指定にできる成分（optional）以外は、すべて書きます）", e.line)
        for n, x in e.fields:
            pt = next((t for pn, t in ct.parts if pn == n), None)
            if pt is None:
                self.d(ERROR, "名前", f"合成型 {e.n} に成分「{n}」がありません", e.line)
                self.ty(x, env)
                continue
            vt = self.ty(x, env)
            if vt == "Null":
                if not pt.optional:
                    self.d(ERROR, "null", f"{e.n}.{n} は必須の成分なので null は入れられません", e.line)
            elif vt == "Rational":
                self.d(QUESTION, "丸め", "端数が出る値を、丸めずに書いています", e.line)
            elif vt != tok_of(pt) and not (isinstance(x, Num) and tok_of(pt) in ("Int", "Money")) and vt != "?":
                self.d(ERROR, "型", f"{e.n}.{n} に型の違う値（{vt}）を入れています", e.line)
            else:
                self.check_component_fit(x, pt, f"{e.n}.{n}", e.line)
            self.fit_unit(x, pt, f"{e.n}.{n}", e.line, siblings=given)
        return f"Comp:{e.n}"

    def check_component_fit(self, x, pt: Type, where: str, line: int):
        """成分の範囲・長さ・文字に、書く値が収まると示せるか。示せるのは、数の区間と、宣言のある入力・項目・部品の戻り値"""
        if pt.kind in ("整数", "金額") and pt.range:
            iv, r = self.interval(x), pt.range
            if (r.lo is not None and (iv[0] is None or iv[0] < r.lo)) or (r.hi is not None and (iv[1] is None or iv[1] > r.hi)):
                self.d(ERROR, "範囲", f"{where} に書く値の範囲が、成分の範囲に収まると示せません"
                       f"（書く値 {iv_show(iv)} ／ 成分 {show_range(r).strip()}）", line)
        elif pt.kind == "文字列":
            self.check_text_fit(x, pt, where, line)
        elif pt.kind in ("割合", "バイト列") and pt.range:
            src = self.decl_type(x, self.env)
            if src is None or src.kind != pt.kind or src.range is None or not (
                    (pt.range.lo is None or (src.range.lo is not None and src.range.lo >= pt.range.lo))
                    and (pt.range.hi is None or (src.range.hi is not None and src.range.hi <= pt.range.hi))):
                self.d(ERROR, "範囲", f"{where} は範囲が決まっているので、同じ範囲に収まる宣言のある入力・項目を、そのまま書いてください", line)

    def note_exist_set(self, e):
        """{ k | (k, v) in 元, D[k] is absent } の形なら、「元のキーがすべて D にあるか」を表す集合として覚える"""
        c = e.cond
        if (isinstance(c, Pred) and c.kind == "ない" and isinstance(c.e, Index)
                and c.e.e == Name(e.binder.names[0]) and e.elem == Name(e.binder.names[0])):
            self._last_exist = (c.e.d, "∈" + show(e.binder.src))

    def pass_null(self, e, ft: Type) -> bool:
        """無いかもしれない入力を、そのまま null になりうる項目へ入れる（使うのではなく渡すだけなので、確かめは要らない）"""
        return (isinstance(e, Name) and ft.optional and self.null_input(e, self.env)
                and ("#入力", e.n) not in self.present)

    def null_input(self, e: Name, env) -> bool:
        """e が「無いかもしれない」入力そのものか（同じ名前の let や、回す変数ではないか）"""
        if "#範囲:" + e.n in env or e.n in self.var_iv or e.n in self.lookup_data:
            return False
        t = next((t for n, t in self.proc.inputs if n == e.n), None)
        return t is not None and t.optional

    def null_field(self, e: Attr, env):
        """e が無いかもしれない項目なら、その項目。そうでなければ None（型の検査はしない）"""
        data = (self.lookup_data.get(e.e.n) if isinstance(e.e, Name)
                else e.e.d if isinstance(e.e, Index) else None)
        if data not in self.ck.data:
            return None
        f = next((x for x in self.ck.data[data].fields if x.name == e.n), None)
        return f if f is not None and f.type.optional else None

    def set_null(self, data, fname, bind, known_present):
        """data.fname に書いた。同じデータの別の束縛（同じ1件かもしれない）の確認は消し、この1件は書いた値で分かる"""
        self.present = {p for p in self.present if not (self.binding_data.get(p[0]) == data and p[1] == fname)}
        if bind is not None and known_present:
            self.present.add((bind, fname))

    def exists_known(self, ix: Index, env) -> bool:
        """D[k] の存在が確かめ済みか。同じキーの式で確かめたか、確かめた集まりを回している変数か"""
        if (ix.d, show(ix.e)) in self.guarded:
            return True
        src = env.get("#元:" + ix.e.n) if isinstance(ix.e, Name) else None
        return src is not None and (ix.d, "∈" + src) in self.guarded

    def bind_var(self, name: str) -> str:
        self.bind_count = getattr(self, "bind_count", 0) + 1
        self.bindings[name] = f"{name}#{self.bind_count}"
        self.binding_data[self.bindings[name]] = self.lookup_data.get(name)
        return self.bindings[name]

    def cur_name_unit(self, n: str, env):
        """通貨コードを表す名前 n（JPY など、または通貨の入力・slot・let）の単位。通貨でなければ None"""
        if n in CURRENCIES and n not in env:
            return n
        if env.get(n) == "State:Currency":
            return "$" + n
        return None

    def cur_expr_unit(self, e, env=None):
        """通貨コードを表す式（名前、または x.currency）の単位の記号"""
        env = self.env if env is None else env
        if isinstance(e, Name):
            return self.cur_name_unit(e.n, env)
        if isinstance(e, Attr):
            return self.cur_sym(e.e, e.n, env)
        return None

    def cur_sym(self, base, sib: str, env=None):
        """base（引いた1件など）の通貨の項目 sib が表す単位。「if x.通貨 != 通貨 { return … }」で同じと確かめてあれば、その通貨の単位。
        確かめていなければ「@base.sib」という、ほかの何とも一致しない記号"""
        env = self.env if env is None else env
        if isinstance(base, Name) and base.n in self.bindings:
            for (b, f, x) in self.state_guards:
                if b == self.bindings[base.n] and f == sib:
                    u = self.cur_name_unit(x, env)
                    if u is not None:
                        return u
        if isinstance(base, Index) and isinstance(base.e, Name):
            src = self.binder_src.get(base.e.n)
            if src is not None and (base.d, sib, src) in self.cur_ok:
                return self.cur_ok[(base.d, sib, src)]
        return f"@{show(base)}.{sib}"

    def unit_of(self, e, env=None):
        """金額の式の単位。分からない（数字そのもの・金額でない）ときは None。型の推論とは別に、式の形だけから追う。
        単位は円・ドルなどの通貨コードか、同じ入力・slot の通貨（$名前）か、保存した1件の通貨の項目（@式.項目）"""
        env = self.env if env is None else env
        if isinstance(e, Name):
            if e.n in self.var_unit:
                return self.var_unit[e.n]
            t = self.decl_type(e, env)
            return t.unit if t is not None and t.kind == "金額" else None
        if isinstance(e, Attr):
            t = self.decl_type(e, env)
            if t is None or t.kind != "金額":
                return None
            if t.unit.startswith("$"):
                return self.cur_sym(e.e, t.unit[1:], env)
            return t.unit
        if isinstance(e, Bin) and e.op in ("+", "-", "*"):
            return self.unit_of(e.l, env) or self.unit_of(e.r, env)
        if isinstance(e, Call):
            if e.n == "money":
                u = e.args[1] if len(e.args) == 2 else None
                return (self.cur_name_unit(u.n, env) or u.n) if isinstance(u, Name) else "JPY"
            if e.n == "round" and e.args:
                return self.unit_of(e.args[0], env)
            pt = self.ck.parts.get(e.n)
            return pt.ret.unit if pt is not None and pt.ret.kind == "金額" else None
        if isinstance(e, SumE):
            saved, saved_src = dict(self.var_unit), dict(self.binder_src)
            self.note_binder_units(e.binder, env)
            u = self.unit_of(e.elem, env)
            self.var_unit, self.binder_src = saved, saved_src
            return u
        return None

    def note_binder_units(self, b: Binder, env):
        """(k, v) in 写像 で回す v の単位と、k がどの集まりのキーか"""
        t = self.decl_type(b.src, env) if isinstance(b.src, (Name, Attr)) else None
        if t is not None and t.kind == "写像" and len(b.names) == 2 and t.value.kind == "金額":
            self.var_unit[b.names[1]] = t.value.unit
        else:
            for n in b.names:
                self.var_unit.pop(n, None)
        for n in b.names:
            self.binder_src.pop(n, None)
        if b.names:
            self.binder_src[b.names[0]] = show(b.src)

    def fit_unit(self, e, t: Type, what: str, line=0, siblings=None, base=None):
        """金額を入れる先（項目・引数・成分）の単位と、入れる式の単位が合っているか。
        先の単位が「同じ並びのほかの通貨」（$名前）のときは、siblings（名前 → 入れる式）か、base（項目を持つ1件）から決まる"""
        if t.kind != "金額":
            return
        want = t.unit
        if want.startswith("$"):
            sib = want[1:]
            if siblings is not None and sib in siblings:
                want = self.cur_expr_unit(siblings[sib]) or want
            elif base is not None:
                want = self.cur_sym(base, sib)

        label = unit_label
        u = self.unit_of(e)
        if u is not None and u != want:
            self.d(ERROR, "単位", f"{what} は{label(want)}の金額ですが、{label(u)}の金額（{show(e)}）を入れています。換算の部品を通すか、"
                   "通貨が同じと確かめてから（if … != 通貨 { return … }）使ってください", line)
        elif isinstance(e, Num) and e.v != 0 and want != "JPY":
            self.d(ERROR, "単位", f"{what} は円以外の金額なので、0 以外の数字は直接書けません（money(整数, 通貨) で最小単位の個数から作ります）", line)

    def decl_type(self, e, env):
        """入力・参照・項目として宣言された型（範囲つき）。分からなければ None"""
        if isinstance(e, Name):
            if e.n in env and e.n in self.var_iv:
                return None
            return next((t for n, t in [*self.proc.inputs, *self.proc.refs] if n == e.n), None)
        if isinstance(e, Attr):
            data = (self.lookup_data.get(e.e.n) if isinstance(e.e, Name)
                    else e.e.d if isinstance(e.e, Index) else None)
            if data in self.ck.data:
                return next((f.type for f in self.ck.data[data].fields if f.name == e.n), None)
            bt = self.decl_type(e.e, env) if isinstance(e.e, (Name, Attr)) else None
            if bt is not None and bt.kind == "合成":
                return next((t for n, t in bt.parts if n == e.n), None)
        return None

    def range_env(self, b: Binder, env):
        """(k, v) in 写像 の v に、写像の値の範囲を持たせる"""
        env2 = dict(env)
        for n in b.names:  # 外の let と同じ名前でも、回す変数の範囲は外のものではない
            env2["#範囲:" + n] = (None, None)
        t = self.decl_type(b.src, env)
        if t is not None and t.kind == "写像" and len(b.names) == 2:
            env2["#範囲:" + b.names[1]] = iv_of(t.value)
        return env2

    def interval(self, e, env=None) -> tuple:
        """式の値の範囲。分かる形だけを見て、分からなければ限りなし"""
        env = self.env if env is None else env
        if isinstance(e, Num):
            return (e.v, e.v)
        if isinstance(e, Name):
            if "#範囲:" + e.n in env:
                return env["#範囲:" + e.n]
            if e.n in self.var_iv:
                return self.var_iv[e.n]
            t = self.decl_type(e, env)
            return (None, None) if t is None else iv_of(t)
        if isinstance(e, Attr):
            data = (self.lookup_data.get(e.e.n) if isinstance(e.e, Name)
                    else e.e.d if isinstance(e.e, Index) else None)
            if data not in self.ck.data:  # 合成型の成分：宣言の範囲
                t = self.decl_type(e, env)
                return (None, None) if t is None else iv_of(t)
            lo, hi = self.field_iv(data, e.n)
            if isinstance(e.e, Name) and e.e.n in self.bindings:
                known = self.facts.get((self.bindings[e.e.n], e.n))
                if known is not None and (lo is None or known > lo):
                    lo = known
            return (lo, hi)
        if isinstance(e, Bin) and e.op == "%":  # 負でも 0 以上 n 未満（rem_euclid）
            return (0, e.r.v - 1)
        if isinstance(e, Bin) and e.op == "/":  # 正の定数で割る切り捨て：区間の端をそのまま割る
            lo, hi = self.interval(e.l, env)
            return (None if lo is None else lo // e.r.v, None if hi is None else hi // e.r.v)
        if isinstance(e, Bin) and e.op in ("+", "-", "*"):
            a, b = self.interval(e.l, env), self.interval(e.r, env)
            return iv_add(a, b) if e.op == "+" else iv_add(a, iv_neg(b)) if e.op == "-" else iv_mul(a, b)
        if isinstance(e, Call) and e.n in CONVERSIONS and len(e.args) == 1:
            return self.interval(e.args[0], env)
        if isinstance(e, Call) and e.n in self.ck.parts:  # 戻り値の範囲は、呼ぶたびに確かめる（生成コード）ので、宣言をそのまま信じてよい
            return iv_of(self.ck.parts[e.n].ret)
        if isinstance(e, Call) and e.n == "round" and len(e.args) == 3:
            lo, hi = self.interval(e.args[0], env)
            how = getattr(e.args[1], "n", "")
            if getattr(e.args[2], "n", "") == "JPY":  # 1円単位：整数への丸め
                return (None if lo is None else Fraction(floor(lo) if how != "ceil" else ceil(lo)),
                        None if hi is None else Fraction(ceil(hi) if how != "floor" else floor(hi)))
            # 単位の大きさが分からないとき：0 以上の値は、どう丸めても 0 以上
            return (0 if lo is not None and lo >= 0 else None, None)
        if isinstance(e, SumE):
            lo, _ = self.interval(e.elem, self.range_env(e.binder, env))
            return (0 if lo is not None and lo >= 0 else None, None)
        return (None, None)

    def bind(self, b: Binder, env=None):
        env = self.env if env is None else env
        st = self.ty(b.src, env)
        self.note_binder_units(b, env)
        env2 = self.range_env(b, env)
        if st.startswith("Map:") and len(b.names) == 2:
            _, _, vt = st.split(":", 2)
            env2[b.names[0]] = "ID"
            env2[b.names[1]] = vt
            env2["#元:" + b.names[0]] = show(b.src)
        elif st == "Set" and len(b.names) == 1:
            env2[b.names[0]] = "ID"
            env2["#元:" + b.names[0]] = show(b.src)
        else:
            self.d(ERROR, "型", f"「{show(b.src)}」を (k, v) in … で回せません")
            for n in b.names:
                env2[n] = "?"
        return env2

    def binop(self, e: Bin, env) -> str:
        a, b = self.ty(e.l, env), self.ty(e.r, env)
        if "?" in (a, b):
            return "?"
        num = ("Int", "Money")
        # 整数と金額は違う型で、そのままでは混ぜられない。数字をそのまま書いたもの（0 など）だけは、どちらとしても使える
        if a in num and b in num and a != b and (isinstance(e.l, Num) or isinstance(e.r, Num)):
            a = b = "Money"
        mixed = a in num and b in num and a != b
        if e.op in ("+", "-", "<", ">", "<=", ">=", "==", "!=") and "Money" in (a, b):
            ul, ur = self.unit_of(e.l), self.unit_of(e.r)
            if ul and ur and ul != ur:
                self.d(ERROR, "単位", f"「{show(e)}」: {unit_label(ul)}の金額と{unit_label(ur)}の金額は、そのままでは足し引きも比較もできません。換算の部品を通すか、通貨が同じと確かめてから使ってください")
            else:
                num_side = e.l if isinstance(e.l, Num) else e.r if isinstance(e.r, Num) else None
                u = ul or ur
                if num_side is not None and num_side.v != 0 and u not in (None, "JPY"):
                    self.d(ERROR, "単位", f"「{show(e)}」: {unit_label(u)}の金額と比べたり足したりする 0 以外の数字は直接書けません（money(整数, 通貨) で作ります）")
        if e.op == "かつ":
            if a != "Bool" or b != "Bool":
                self.d(ERROR, "型", f"「{show(e)}」: かつ の両側は条件である必要があります")
            return "Bool"
        if e.op in ("<", ">", "<=", ">="):
            if not ((a in num and b in num and not mixed) or (a == "Time" and b == "Time")):
                self.d(ERROR, "型", f"「{show(e)}」: 同じ種類の数どうし（または日時どうし）で比べてください"
                       + ("（整数と金額は比べられません）" if mixed else ""))
            return "Bool"
        if e.op in ("==", "!="):
            if a != b:
                self.d(ERROR, "型", f"「{show(e)}」: 違う型を比べています")
            return "Bool"
        if e.op in ("+", "-") and "Time" in (a, b) and not (e.op == "-" and a == "Dur"):
            if (a, b) == ("Time", "Dur") or (e.op == "+" and (a, b) == ("Dur", "Time")):
                return "Time"  # 時刻 ± 期間 は時刻
            if (a, b) == ("Time", "Time") and e.op == "-":
                return "Int"  # 時刻どうしの差は、秒数
            self.d(ERROR, "型", f"「{show(e)}」: 時刻に足し引きできるのは期間（30 min など）だけです。時刻どうしは引き算（差は秒数）だけできます")
            return "?"
        if "Dur" in (a, b):
            self.d(ERROR, "型", f"「{show(e)}」: 期間は、時刻に足し引きするときだけ使えます")
            return "?"
        if e.op in ("+", "-"):
            if "Rational" in (a, b):
                self.d(QUESTION, "丸め", f"「{show(e)}」: 端数が出る計算の丸め（切り捨て・切り上げ・四捨五入）と単位が"
                       "指定されていません。端数をどう丸めますか")
                return "Money"
            if a in num and b in num and not mixed:
                return a
            self.d(ERROR, "型", f"「{show(e)}」: 同じ種類の数どうしで計算してください"
                   + ("（整数と金額は、そのままでは足し引きできません）" if mixed else ""))
            return "?"
        if e.op in ("/", "%"):
            if a != "Int":
                self.d(ERROR, "型", f"「{show(e)}」: 割り算できるのは整数だけです（{a}）")
                return "?"
            return "Int"
        if e.op == "*":
            if a == "Money" and b == "Money":
                self.d(ERROR, "型", f"「{show(e)}」: 金額どうしは掛けられません（金額 × 整数 か 金額 × 割合 にしてください）")
                return "?"
            if a in num and b in num:  # 金額 × 整数（数量ぶん）は金額
                return "Money" if "Money" in (a, b) else "Int"
            if {a, b} <= {"Ratio", "Int", "Money"} and "Ratio" in (a, b) and "Ratio" not in (a if b == "Ratio" else b):
                return "Rational"
            self.d(ERROR, "型", f"「{show(e)}」: 掛けられない組み合わせです")
            return "?"
        raise ValueError(e.op)

    # ---- 文 ----
    def check_return(self, r: Return):
        rdef = self.ck.results.get(self.proc.result)
        if rdef is None:
            return
        v = next((x for x in rdef.variants if x[0] == r.variant), None)
        if v is None:
            self.d(ERROR, "結果", f"「{r.variant}」は結果 {rdef.name} にありません", r.line)
            return
        self.info.returns.add(r.variant)
        if len(v[1]) != len(r.args):
            self.d(ERROR, "結果", f"結果「{r.variant}」の引数は {len(v[1])} 個です", r.line)
            return
        for (_, t), a in zip(v[1], r.args):
            at = self.ty(a)
            if at != tok_of(t) and at != "?" and not (isinstance(a, Num) and tok_of(t) in ("Int", "Money")):
                self.d(ERROR, "型", f"結果「{r.variant}」の引数の型が違います（{at}）", r.line)
            self.fit_unit(a, t, f"結果「{r.variant}」の引数", r.line, siblings={pn: x for (pn, _), x in zip(v[1], r.args)})

    def run(self) -> ProcInfo:
        ck, proc, info = self.ck, self.proc, self.info
        if proc.result not in ck.results:
            self.d(ERROR, "名前", f"結果「{proc.result}」が未定義です")
        for n, t in proc.inputs:
            self.env[n] = tok_of(t)
        for n, t in proc.refs:
            self.env[n] = tok_of(t)
        self.exist_sets = {}
        for pos, st in enumerate(proc.body):
            self.stmt(st, pos)
        if not (proc.body and isinstance(proc.body[-1], Return)):
            self.d(ERROR, "結果", "処理の最後は return で終えてください（結果を返さずに終わる道があります）")
        self.finish()
        return info

    def stmt(self, st, pos):
        info = self.info
        self.pos = pos
        if isinstance(st, Return):
            self.check_return(st)
        elif isinstance(st, If):
            if self.ty(st.cond) not in ("Bool", "?"):
                self.d(ERROR, "型", f"if の条件「{show(st.cond)}」は、成り立つか成り立たないかの条件である必要があります", st.line)
            c = st.cond
            state_cmp = (isinstance(c, Bin) and c.op in ("==", "!=") and isinstance(c.l, Attr) and isinstance(c.l.e, Name)
                         and c.l.e.n in self.bindings and isinstance(c.r, Name))
            fact = (self.bindings[c.l.e.n], c.l.n, c.r.n) if state_cmp else None
            if not st.body:
                self.d(ERROR, "型", "if の中が空です", st.line)
            # 「x.項目 == 状態」や「x.項目 > 数」が成り立つ分岐の中で分かることは、分岐の外へは持ち出さない
            outer_sg, outer_facts, outer_guarded, outer_present = self.state_guards, self.facts, self.guarded, self.present
            self.present = set(self.present)
            self.state_guards = self.state_guards | ({fact} if state_cmp and c.op == "==" else set())
            self.facts = self.add_facts(dict(self.facts), self.cond_facts(c, True))
            self.guarded = set(self.guarded)
            for inner in st.body:
                if not isinstance(inner, (Return, If, SetField, Update, Request)):
                    self.d(ERROR, "型", "if の中に書けるのは、return・項目の更新・update・request・if だけです（let は外に書いてください）", st.line)
                    continue
                self.stmt(inner, pos)
            skip_facts = self.add_facts(dict(outer_facts), self.cond_facts(c, False))
            if st.body and isinstance(st.body[-1], Return):
                # 分岐がかならず return で終わるなら、成り立たなかったときだけ先へ進む。その確認は、あとの文へ引き継ぐ
                self.state_guards, self.facts, self.guarded, self.present = outer_sg, skip_facts, outer_guarded, outer_present
                if (isinstance(c, Pred) and c.kind == "null" and isinstance(c.e, Attr) and isinstance(c.e.e, Name)
                        and c.e.e.n in self.bindings):
                    self.present.add((self.bindings[c.e.e.n], c.e.n))
                if isinstance(c, Pred) and c.kind == "null" and isinstance(c.e, Name):
                    self.present.add(("#入力", c.e.n))
                if isinstance(c, Pred) and c.kind == "null" and isinstance(c.e, Attr) and self.ty_quiet(c.e.e, self.env).startswith("Comp:"):
                    self.present.add((("#成分", show(c.e.e)), c.e.n))
                if isinstance(c, Pred) and c.kind == "空でない" and isinstance(c.e, Name) and c.e.n in self.exist_sets:
                    self.guarded.add(self.exist_sets[c.e.n])
                if isinstance(c, Pred) and c.kind == "空でない" and isinstance(c.e, Name) and c.e.n in self.cur_sets:
                    d, sib, src, unit = self.cur_sets[c.e.n]
                    self.cur_ok[(d, sib, src)] = unit
                if isinstance(c, Pred) and c.kind == "空でない" and isinstance(c.e, Name) and c.e.n in self.shortage_sets:
                    self.facts[("#不足", *self.shortage_sets[c.e.n])] = 0
                if isinstance(c, Pred) and c.kind == "ない" and isinstance(c.e, Index):
                    self.guarded.add((c.e.d, show(c.e.e)))
                if state_cmp and c.op == "!=":
                    self.state_guards.add(fact)
            else:
                # 分岐を通った道と通らなかった道の合流。両方で分かっていることだけが残る（分岐の中の書き込みで崩れた確認は消える）
                self.state_guards = outer_sg & self.state_guards
                self.facts = iv_join(skip_facts, self.facts)
                self.guarded = outer_guarded
                self.present = outer_present & self.present
        elif isinstance(st, Assign):
            u = self.unit_of(st.e)
            if u is not None:
                self.var_unit[st.name] = u
            else:
                self.var_unit.pop(st.name, None)
            if isinstance(st.e, Index):
                t = self.ty(st.e)
                self.env[st.name] = t
                self.lookup_data[st.name] = st.e.d
                self.bind_var(st.name)
                self.var_iv.pop(st.name, None)
                self.shortage_sets.pop(st.name, None)
                if names_in(st.e.e) & self.loop_deps:
                    self.loop_deps.add(st.name)
                info.exist_reads.add(st.e.d)
                if st.absent is None:
                    self.d(QUESTION, "存在", f"{show(st.e)} が存在しないとき、どうしますか（else {{ return … }} が必要です）", st.line)
                else:
                    self.check_return(st.absent)
                    self.guarded.add((st.e.d, show(st.e.e)))
            elif st.absent is not None and isinstance(st.e, Name) and self.null_input(st.e, self.env):
                # let x = 入力 else { return … }：入力が無ければ返す
                self.writing = True
                t = self.ty(st.e)
                self.writing = False
                self.check_return(st.absent)
                self.present.add(("#入力", st.e.n))
                self.env[st.name] = t
                self.lookup_data.pop(st.name, None)
                self.bindings.pop(st.name, None)
                self.var_iv[st.name] = self.interval(st.e)
                self.shortage_sets.pop(st.name, None)
            elif st.absent is not None and isinstance(st.e, Attr) and self.null_field(st.e, self.env) is not None:
                # let x = y.項目 else { return … }：null なら返し、そうでなければ値を x に
                self.writing = True
                t = self.ty(st.e)
                self.writing = False
                self.check_return(st.absent)
                self.env[st.name] = t
                self.lookup_data.pop(st.name, None)
                self.bindings.pop(st.name, None)
                self.var_iv[st.name] = self.interval(st.e)
                self.shortage_sets.pop(st.name, None)
                if isinstance(st.e.e, Name) and st.e.e.n in self.bindings:
                    self.present.add((self.bindings[st.e.e.n], st.e.n))
            else:
                if st.absent is not None:
                    self.d(ERROR, "型", f"let … else は、1件を引くとき（D[k]）か、無いかもしれない項目・入力を取り出すときだけ書けます（{show(st.e)}）", st.line)
                self._last_exist = None
                t = self.ty(st.e)
                self.env[st.name] = t
                self.lookup_data.pop(st.name, None)
                self.bindings.pop(st.name, None)
                self.var_iv[st.name] = self.interval(st.e)
                self.shortage_sets.pop(st.name, None)
                if isinstance(st.e, SetB) and self._last_exist:
                    self.exist_sets[st.name] = self._last_exist
                if isinstance(st.e, SetB):
                    self.note_shortage_set(st.name, st.e)
                    self.note_currency_set(st.name, st.e)
        elif isinstance(st, Update):
            env2 = self.bind(st.binder)
            vt = self.ty(st.value, env2)
            if st.cond is not None:
                self.ty(st.cond, env2)
            tgt = st.target
            if not (isinstance(tgt, Attr) and isinstance(tgt.e, Index)):
                self.d(ERROR, "型", "更新の対象は D[k].項目 の形です", st.line)
                return
            # = で書くだけなら、対象の項目は読まない（ほかの所で読んでいれば、読んだまま）
            was_read = f"{tgt.e.d}.{tgt.n}" in info.field_reads
            self.writing = st.op == "="  # = で書くだけなら、null かどうかは問わない
            self.ty(tgt, env2)
            self.writing = False
            if st.op == "=" and not was_read:
                info.field_reads.discard(f"{tgt.e.d}.{tgt.n}")
            tf = next((f for f in self.ck.data[tgt.e.d].fields if f.name == tgt.n), None) if tgt.e.d in self.ck.data else None
            if tf is not None and tf.type.kind == "名前":
                self.d(ERROR, "遷移", f"update で状態の項目「{tgt.e.d}.{tgt.n}」は変えられません（遷移の確認ができないため。"
                       "1件ずつ let で引き、元の状態を確かめてから変えてください）", st.line)
            if vt == "Null":
                if tf is not None and not (tf.type.optional and st.op == "="):
                    self.d(ERROR, "null", f"{tgt.e.d}.{tgt.n} に null は入れられません（必須の項目か、= 以外の更新です）", st.line)
            elif vt == "Rational":
                self.d(QUESTION, "丸め", "端数が出る値を、丸めずに書いています", st.line)
            elif (tf is not None and tf.type.kind != "名前" and vt not in (tok_of(tf.type), "?")
                  and not (isinstance(st.value, Num) and tok_of(tf.type) in ("Int", "Money"))):
                self.d(ERROR, "型", f"{tgt.e.d}.{tgt.n} に型の違う値（{vt}）を入れています", st.line)
            elif tf is not None and st.op != "=" and tf.type.kind not in ("整数", "金額", "割合"):
                self.d(ERROR, "型", f"{tgt.e.d}.{tgt.n} は数ではないので {st.op} できません", st.line)
            if tf is not None and st.op in ("=", "+=", "-="):
                self.fit_unit(st.value, tf.type, f"{tgt.e.d}.{tgt.n}", st.line, base=tgt.e)
            info.writes.add(f"{tgt.e.d}.{tgt.n}")
            info.write_list.append(f"{tgt.e.d}.{tgt.n}")
            info.write_ops.setdefault(f"{tgt.e.d}.{tgt.n}", set()).add(st.op)
            if tf is not None and tf.type.kind in ("整数", "金額", "割合") and vt != "Null":
                self.check_update(st, env2)
            self.forget(tgt.e.d, tgt.n)
            self.set_null(tgt.e.d, tgt.n, None, False)
        elif isinstance(st, SetField):
            data = self.lookup_data.get(st.var)
            if data is None:
                self.d(ERROR, "型", f"「{st.var}」は、データを引いた変数ではありません", st.line)
                return
            fld = next((f for f in self.ck.data[data].fields if f.name == st.f), None)
            if fld is None:
                self.d(ERROR, "名前", f"データ {data} に項目「{st.f}」がありません", st.line)
                return
            passing = self.pass_null(st.e, fld.type)
            self.writing, saved = passing, self.writing
            vt = self.ty(st.e)
            self.writing = saved
            b = self.bindings.get(st.var, st.var)
            if vt == "Null":
                if not fld.type.optional:
                    self.d(ERROR, "null", f"{data}.{st.f} は必須（required）なので null は入れられません", st.line)
                    return
                if fld.type.kind in ("整数", "金額", "割合"):
                    self.check_write(data, st.f, (None, None), st.line, bind=b, null=True)
                self.forget(data, st.f)
                self.set_null(data, st.f, b, False)
                info.writes.add(f"{data}.{st.f}")
                info.write_list.append(f"{data}.{st.f}")
                info.write_ops.setdefault(f"{data}.{st.f}", set()).add("=")
                return
            if vt == "Rational":
                self.d(QUESTION, "丸め", "端数が出る値を、丸めずに書いています", st.line)
            elif (vt != tok_of(fld.type) and vt != "?" and fld.type.kind != "名前"
                  and not (isinstance(st.e, Num) and tok_of(fld.type) in ("Int", "Money"))):
                self.d(ERROR, "型", f"{data}.{st.f} に型の違う値（{vt}）を入れています", st.line)
                return
            self.fit_unit(st.e, fld.type, f"{data}.{st.f}", st.line, base=Name(st.var))
            self.check_range_fit(st.e, fld.type, f"{data}.{st.f}", st.line)
            self.check_text_fit(st.e, fld.type, f"{data}.{st.f}", st.line)
            if fld.type.kind == "合成":  # 書き換えたので、その成分について確かめたことは消える
                path = f"{st.var}.{st.f}"
                self.present = {k for k in self.present if not (isinstance(k[0], tuple) and k[0][0] == "#成分" and k[0][1].startswith(path))}
            if passing and fld.type.kind in ("整数", "金額", "割合"):  # 入力が無ければ null になる
                self.check_write(data, st.f, (None, None), st.line, bind=b, null=True)
            self.set_null(data, st.f, b, not passing)
            if fld.type.kind in ("整数", "金額", "割合"):
                iv = self.interval(st.e)
                self.check_write(data, st.f, iv, st.line, bind=b)
                self.forget(data, st.f)
                if iv[0] is not None:
                    self.facts[(b, st.f)] = iv[0]
            info.writes.add(f"{data}.{st.f}")
            info.write_list.append(f"{data}.{st.f}")
            info.write_ops.setdefault(f"{data}.{st.f}", set()).add("=")
            if fld.type.kind == "名前" and not self.ck.states[fld.type.target].plain:
                if not (isinstance(st.e, Name) and st.e.n in self.ck.variants and st.e.n not in self.env):
                    self.d(ERROR, "遷移", f"{data}.{st.f} は状態名で書いてください（「{show(st.e)}」では、どの遷移か確かめられません）", st.line)
                    return
                self.check_transition(data, st.f, st.e.n, st.line, st.var)
                self.check_state_write(data, st.f, st.e.n, st.var, st.line)
                # 状態が変わった。同じデータの別の束縛（同じ1件かもしれない）の確認も消し、この1件は新しい状態だと分かる
                self.state_guards = {g for g in self.state_guards if not (self.binding_data.get(g[0]) == data and g[1] == st.f)}
                self.state_guards.add((b, st.f, st.e.n))
        elif isinstance(st, Create):
            self.create(st)
        elif isinstance(st, Request):
            self.request(st)
        elif isinstance(st, ForEach):
            env2 = self.bind(st.binder)
            saved = self.env
            self.env = env2
            # 要求ごとに違う値：回すキー（写像なら k）と、それで引いたもの
            self.loop_deps = {st.binder.names[0]}
            for inner in st.body:
                if isinstance(inner, Request) or (isinstance(inner, Assign) and isinstance(inner.e, Index)):
                    self.stmt(inner, pos)
                else:
                    self.d(ERROR, "型", "for … { } の中に書けるのは、データを引く文と、要求だけです", st.line)
            self.env = saved
            self.loop_deps = set()

    def check_transition(self, data, fname, dst, line, var):
        sname = next(f.type.target for f in self.ck.data[data].fields if f.name == fname)
        decl = [(a, b) for (d, a, b) in self.proc.transitions if d == data and b == dst]
        if not decl:
            self.d(ERROR, "遷移", f"{data} を「{dst}」にする遷移が、処理の宣言（遷移）にありません", line)
            return
        for a, b in decl:
            self.info.transitions.add((data, a, b))
            if a != "(新規)" and (self.bindings.get(var, var), fname, a) not in self.state_guards:
                self.d(ERROR, "遷移", f"{data} が「{a}」であることの確認（if … != {a} {{ return … }}、または if … == {a} {{ … }} の中）が、遷移の前にありません", line)
        for x, y, why in self.ck.states[sname].rejects:
            if y == dst and any(a == x for a, _ in decl):
                self.d(ERROR, "遷移", f"拒否された遷移 {x} → {y}（{why}）を行っています", line)

    def check_range_fit(self, e, dst: Type, where: str, line: int):
        """入力をそのまま項目へ入れるとき、入力の範囲が項目の範囲に収まるか。"""
        src = next((t for n, t in self.proc.inputs if isinstance(e, Name) and n == e.n), None)
        if src is None:
            return
        if not (src.kind == "写像" and dst.kind == "写像"):
            return  # 数の項目は、書き込みの検査（値の範囲）で見る
        src, dst = src.value, dst.value
        if src.kind not in ("整数", "金額", "割合") or src.kind != dst.kind:
            return
        s, d = src.range or Range(), dst.range or Range()
        lo_ng = d.lo is not None and (s.lo is None or s.lo < d.lo)
        hi_ng = d.hi is not None and (s.hi is None or s.hi > d.hi)
        if lo_ng or hi_ng:
            self.d(ERROR, "範囲", f"{where} に、範囲の広い入力「{e.n}」を入れています"
                   f"（入力{show_range(src.range).strip() or '制限なし'} ／ 項目{show_range(dst.range).strip()}）。収まるか確認が要ります", line)

    def check_text_fit(self, e, dst: Type, where: str, line: int):
        """長さか使える文字が決まっている文字列の項目へ書くとき、書く値の長さと文字が収まると示せるか。
        示せるのは、宣言のある入力・項目・slot と、部品の戻り値を、そのまま書く場合だけ（外から来る値は、境界で確かめてある）"""
        if dst.kind != "文字列" or (dst.range is None and dst.chars is None) or isinstance(e, Null):
            return
        if isinstance(e, Call) and e.n in self.ck.parts:
            src = self.ck.parts[e.n].ret
        else:
            src = self.decl_type(e, self.env)
        if src is None or src.kind != "文字列":
            if src is None:
                self.d(ERROR, "範囲", f"{where} は長さか使える文字が決まっているので、宣言のある入力・項目・slot か部品の戻り値を、そのまま書いてください（「{show(e)}」では収まると示せません）", line)
            return
        s, d = src.range or Range(), dst.range or Range()
        if (d.lo is not None and d.lo > 0 and (s.lo is None or s.lo < d.lo)) or (d.hi is not None and (s.hi is None or s.hi > d.hi)):
            self.d(ERROR, "範囲", f"{where} に書く文字列の長さが、項目の長さの範囲に収まると示せません"
                   f"（書く値{show_range(src.range).strip() or '長さの制限なし'} ／ 項目{show_range(dst.range).strip() or '長さの制限なし'}）", line)
        if dst.chars is not None and (src.chars is None or not all(any(c <= a and b <= e_ for c, e_ in dst.chars) for a, b in src.chars)):
            self.d(ERROR, "文字種", f"{where} に書く文字列の文字が、項目の使える文字に収まると示せません"
                   f"（書く値{'の文字は制限なし' if src.chars is None else ' [' + show_chars(src.chars) + ']'} ／ 項目 [{show_chars(dst.chars)}]）", line)

    def create(self, st: Create):
        ck, info = self.ck, self.info
        if st.data not in ck.data:
            self.d(ERROR, "名前", f"データ「{st.data}」が未定義です", st.line)
            return
        dt = ck.data[st.data]
        key = ck.key_field(st.data)
        given = dict(st.fields)
        for f in dt.fields:
            if f is key:
                continue
            if f.name not in given and not f.type.optional:
                self.d(ERROR, "項目", f"{st.data} の新規作成に必須の項目「{f.name}」がありません", st.line)
        for n, e in st.fields:
            f = next((x for x in dt.fields if x.name == n), None)
            if f is None:
                self.d(ERROR, "名前", f"データ {st.data} に項目「{n}」がありません", st.line)
                continue
            self.writing, saved = self.pass_null(e, f.type), self.writing
            vt = self.ty(e)
            self.writing = saved
            if vt == "Null":
                if not f.type.optional:
                    self.d(ERROR, "null", f"{st.data}.{n} は必須（required）なので null は入れられません", st.line)
            elif vt == "Rational":
                self.d(QUESTION, "丸め", "端数が出る値を、丸めずに書いています", st.line)
            elif vt != tok_of(f.type) and not (isinstance(e, Num) and tok_of(f.type) in ("Int", "Money")) and vt != "?":
                self.d(ERROR, "型", f"{st.data}.{n} に型の違う値（{vt}）を入れています", st.line)
            self.fit_unit(e, f.type, f"{st.data}.{n}", st.line, siblings=given)
            self.check_range_fit(e, f.type, f"{st.data}.{n}", st.line)
            self.check_text_fit(e, f.type, f"{st.data}.{n}", st.line)
            if f.type.kind == "名前" and not self.ck.states[f.type.target].plain:
                if isinstance(e, Name) and e.n in self.ck.variants and e.n not in self.env:
                    self.check_transition_new(st.data, n, e.n, st.line)
                else:
                    self.d(ERROR, "遷移", f"{st.data} の新規作成の「{n}」は状態名で書いてください（「{show(e)}」では、どの遷移か確かめられません）", st.line)
        info.writes.add(st.data)
        info.new_ids.append(st.data)
        states = {f.name: given[f.name].n for f in dt.fields
                  if f.type.kind == "名前" and not self.ck.states[f.type.target].plain and isinstance(given.get(f.name), Name)}
        lows = {}
        for f in dt.fields:
            e = given.get(f.name, Null())  # 書かなかった項目は null
            if f.type.kind in ("整数", "金額", "割合") and (f.name in given or f.type.optional):
                if isinstance(e, Null):
                    self.check_write(st.data, f.name, (None, None), st.line, new_state=states, null=True)
                    continue
                if self.pass_null(e, f.type):  # 入力が無ければ null になる
                    self.check_write(st.data, f.name, (None, None), st.line, new_state=states, null=True)
                iv = self.interval(e)
                self.check_write(st.data, f.name, iv, st.line, new_state=states)
                lows[f.name] = iv[0]
        self.created.add(st.name)
        self.env[st.name] = f"Rec:{st.data}"
        self.lookup_data[st.name] = st.data
        b = self.bind_var(st.name)
        # 作ったばかりの1件は、状態も値も分かっている
        self.state_guards |= {(b, n, v) for n, v in states.items()}
        self.present |= {(b, n) for n, e in st.fields if not isinstance(e, Null)}
        self.facts.update({(b, n): lo for n, lo in lows.items() if lo is not None})

    # ---- 書き込みの証明（項目の範囲と不変条件）。書き込むその場で、値の範囲から示す ----
    def field_iv(self, data, fname):
        """項目の値の範囲。型の範囲に、下限の不変条件を重ねる（どの書き込みも示してあるので、いつでも成り立つ）"""
        if data not in self.ck.data:
            return (None, None)
        fld = next((f for f in self.ck.data[data].fields if f.name == fname), None)
        if fld is None:
            return (None, None)
        lo, hi = iv_of(fld.type)
        for form in self.ck.inv_forms:
            if form and form[0] == "下限" and form[1] == data and form[2] == fname and (lo is None or form[3] > lo):
                lo = form[3]
        return (lo, hi)

    def add_facts(self, facts, new):
        for k, lo in new.items():
            if k not in facts or lo > facts[k]:
                facts[k] = lo
        return facts

    def cond_facts(self, c, holds: bool) -> dict:
        """条件 c が成り立つ（holds）／成り立たないときに分かる下限。「x.項目 と 数」の比較だけを見る（整数と金額）"""
        if not (isinstance(c, Bin) and c.op in ("<", "<=", ">", ">=", "==") and isinstance(c.l, Attr)
                and isinstance(c.l.e, Name) and c.l.e.n in self.bindings and isinstance(c.r, Num)):
            return {}
        t = self.decl_type(c.l, self.env)
        if t is None or t.kind not in ("整数", "金額"):
            return {}
        n = c.r.v
        op = c.op if holds else {"<": ">=", "<=": ">", ">": "<=", ">=": "<", "==": "!="}[c.op]
        lo = {">": n + 1, ">=": n, "==": n}.get(op)
        if op == "!=" and self.interval(c.l)[0] == n:
            lo = n + 1
        return {} if lo is None else {(self.bindings[c.l.e.n], c.l.n): lo}

    def note_currency_set(self, name, e: SetB):
        """{ k | (k, v) in 元, D[k].通貨 != 通貨 } の形なら、「通貨が違う k の集まり」として覚える（空と確かめると、元のどの1件も同じ通貨）"""
        c, b = e.cond, e.binder
        self.cur_sets.pop(name, None)
        if (b.names and isinstance(c, Bin) and c.op == "!=" and isinstance(c.l, Attr) and isinstance(c.l.e, Index)
                and c.l.e.e == Name(b.names[0]) and isinstance(c.r, Name) and e.elem == Name(b.names[0])
                and self.cur_name_unit(c.r.n, self.env) is not None):
            dt = self.ck.data.get(c.l.e.d)
            f = next((x for x in dt.fields if x.name == c.l.n), None) if dt else None
            if f is not None and f.type.kind == "名前" and f.type.target == "Currency":
                self.cur_sets[name] = (c.l.e.d, c.l.n, show(b.src), self.cur_name_unit(c.r.n, self.env))

    def note_shortage_set(self, name, e: SetB):
        """{ k | (k, v) in 元, D[k].f < v } の形なら、「減らす量が足りない k の集まり」として覚える"""
        c, b = e.cond, e.binder
        if (len(b.names) == 2 and isinstance(c, Bin) and c.op == "<" and isinstance(c.l, Attr) and isinstance(c.l.e, Index)
                and c.l.e.e == Name(b.names[0]) and c.r == Name(b.names[1]) and e.elem == Name(b.names[0])):
            self.shortage_sets[name] = (c.l.e.d, c.l.n, show(b.src))

    def forget(self, data, fname):
        """data.fname が書き換わった。同じデータのどの束縛（同じ1件かもしれない）についても、分かっていた下限を消す"""
        self.facts = {k: v for k, v in self.facts.items()
                      if not ((k[0] == "#不足" and k[1:3] == (data, fname))
                              or (k[0] != "#不足" and self.binding_data.get(k[0]) == data and k[1] == fname))}
        # 通貨の項目が書き換わったら、「同じ通貨と確かめた」ことも消える
        self.state_guards = {g for g in self.state_guards if not (self.binding_data.get(g[0]) == data and g[1] == fname
                                                                   and self.ck.currency_field(data, fname))}
        self.cur_ok = {k: v for k, v in self.cur_ok.items() if k[:2] != (data, fname)}
        self.cur_sets = {n: v for n, v in self.cur_sets.items() if v[:2] != (data, fname)}
        # 書き換わる前に作った「足りない k の集まり」は、もう今の値を表さない
        self.shortage_sets = {n: v for n, v in self.shortage_sets.items() if v[:2] != (data, fname)}

    def inv_row(self, i, ok, note):
        self.ck.inv_rows.setdefault(i, {}).setdefault(self.proc.name, []).append((ok, note))

    def check_write(self, data, fname, iv, line, bind=None, note=None, grow=False, new_state=None, null=False):
        """項目へ範囲 iv の値を書くとき、項目の範囲と、その項目にかかる不変条件が守れるか。
        bind は書く1件の束縛（update では None）。note は値の範囲を示せた理由。grow は「元の値から減らさない」こと。
        new_state は新規作成のときの状態（状態の項目 → 状態名）"""
        fld = next(f for f in self.ck.data[data].fields if f.name == fname)
        lo, hi = iv_of(fld.type)
        if not null and ((lo is not None and (iv[0] is None or iv[0] < lo)) or (hi is not None and (iv[1] is None or iv[1] > hi))):
            self.d(ERROR, "範囲", f"{data}.{fname} に書く値が、項目の範囲 {iv_show((lo, hi))} に収まると示せません"
                   f"（書く値の範囲 {iv_show(iv)}。はみ出す例（反例）があります）", line)
        for i, form in enumerate(self.ck.inv_forms):
            if form is None or form[1] != data:
                continue
            if form[0] == "下限" and form[2] == fname:
                n = form[3]
                ok = iv[0] is not None and iv[0] >= n
                self.inv_row(i, ok, note or f"書く値は {n} 以上")
                if not ok:
                    self.d(ERROR, "不変条件", f"不変条件 {data}: {fname} >= {n} を示せません"
                           + ("（null は満たしません。必ずあるなら項目に required）" if null else
                              f"（反例：書く値が {'いくらでも小さく' if iv[0] is None else iv[0]} になりえます）"), line)
            elif form[0] == "ならば" and form[4] == fname:
                _, _, sf, s, g, n = form
                big = iv[0] is not None and iv[0] > n
                if new_state is not None and new_state.get(sf) != s:
                    self.inv_row(i, True, f"新規作成は{s}以外")
                elif big:
                    self.inv_row(i, True, f"{g} は {n} より大きい")
                elif grow and new_state is None:
                    self.inv_row(i, True, "元の値から減らさない")
                elif bind is not None and any(x[0] == bind and x[1] == sf and x[2] != s for x in self.state_guards):
                    self.inv_row(i, True, f"{s}でないことを確認している")
                else:
                    self.inv_row(i, False, "")
                    self.d(ERROR, "不変条件", f"{data}.{g} を書くと、{sf} が {s} のとき {g} > {n} が破れるおそれがあります"
                           f"（書く値の範囲 {iv_show(iv)}。{s}でないことか、{g} > {n} を確かめてください）", line)

    def check_update(self, st: Update, env2):
        """update { D[k].f op 値 | … } の書き込み。束縛が無いので、値の範囲だけで示す"""
        tgt, b = st.target, st.binder
        data, fname = tgt.e.d, tgt.n
        old, v = self.field_iv(data, fname), self.interval(st.value, env2)
        note, grow = None, False
        if st.op == "=":
            iv = v
        elif st.op == "+=":
            iv, grow = iv_add(old, v), v[0] is not None and v[0] >= 0
        elif (len(b.names) == 2 and tgt.e.e == Name(b.names[0]) and st.value == Name(b.names[1])
              and ("#不足", data, fname, show(b.src)) in self.facts):
            # 足りないものが無いと確かめてから、その量だけ減らす：減らした後も 0 以上
            iv, note = (0, iv_add(old, iv_neg(v))[1]), "減算の前に不足を確認している"
        else:
            iv = iv_add(old, iv_neg(v))
        self.check_write(data, fname, iv, st.line, note=note, grow=grow)

    def check_state_write(self, data, sf, dst, var, line):
        """x.状態 = 状態名 のとき、「状態 == S implies g > n」が守れるか"""
        for i, form in enumerate(self.ck.inv_forms):
            if not (form and form[0] == "ならば" and form[1] == data and form[2] == sf):
                continue
            _, _, _, s, g, n = form
            if dst != s:
                self.inv_row(i, True, f"{s}以外にする")
                continue
            lo = self.interval(Attr(Name(var), g))[0]
            fg = next((f for f in self.ck.data[data].fields if f.name == g), None)
            if fg is not None and fg.type.optional and (self.bindings.get(var, var), g) not in self.present:
                lo = None  # null かもしれない値は「g > n」を満たさない
            ok = lo is not None and lo > n
            self.inv_row(i, ok, f"{s}にする前に {g} > {n} を確認している")
            if not ok:
                self.d(ERROR, "不変条件", f"{data}.{sf} を {s} にする前に、{g} > {n} の確認がありません"
                       f"（不変条件 {data}: {sf} == {s} implies {g} > {n}）", line)

    def check_transition_new(self, data, fname, dst, line):
        decl = [x for x in self.proc.transitions if x[0] == data and x[1] == "(新規)" and x[2] == dst]
        if not decl:
            self.d(ERROR, "遷移", f"{data} を新規に「{dst}」で作る遷移が、処理の宣言（遷移 … (新規) → {dst}）にありません", line)
        else:
            self.info.transitions.add(decl[0])

    def request(self, st: Request):
        ck, info = self.ck, self.info
        eff = ck.effects.get(st.effect)
        if eff is None:
            self.d(ERROR, "名前", f"外部作用「{st.effect}」が宣言されていません", st.line)
            return
        if len(st.args) != len(eff.params):
            self.d(ERROR, "型", f"外部作用 {st.effect} の引数は {len(eff.params)} 個です", st.line)
        for a, (_, t) in zip(st.args, eff.params):
            at = self.ty(a)
            if at != tok_of(t) and not (isinstance(a, Num) and tok_of(t) in ("Int", "Money")) and at != "?":
                self.d(ERROR, "型", f"外部作用 {st.effect} の引数の型が違います（{at}）", st.line)
            self.fit_unit(a, t, f"外部作用 {st.effect} の引数", st.line, siblings={pn: x for (pn, _), x in zip(eff.params, st.args)})
        if st.idem is None:
            self.d(ERROR, "冪等", f"外部作用 {st.effect} に冪等キーがありません（idempotency_key = … が必須です）", st.line)
        else:
            parts = st.idem.items if isinstance(st.idem, Tuple) else [st.idem]
            for x in parts:
                kt = self.ty(x)
                if kt not in ("ID", "Str", "?"):
                    self.d(ERROR, "冪等", f"外部作用 {st.effect} の冪等キーは ID か文字列（の組）にしてください（{show(x)}: {kt}）", st.line)
            if self.loop_deps:
                # for の中では、回すたびに違う値と、for の外で決まる値の両方が要る。
                # 回す値だけだと、別の処理（別の注文など）で同じ値を回したときにキーがぶつかり、外で重複として捨てられる
                inner = any(names_in(x) & self.loop_deps for x in parts)
                outer = any(names_in(x) and not (names_in(x) & self.loop_deps) for x in parts)
                if not inner:
                    self.d(ERROR, "冪等", f"外部作用 {st.effect} の冪等キー「{show(st.idem)}」が、for で回す値によりません"
                           "（どの要求も同じキーになり、2件目からが重複として捨てられます）", st.line)
                elif not outer:
                    self.d(ERROR, "冪等", f"外部作用 {st.effect} の冪等キー「{show(st.idem)}」が、for で回す値だけです。"
                           "別の処理で同じ値を回すとキーがぶつかるので、for の外で決まる値との組にしてください（idempotency_key = (注文ID, k) など）", st.line)
        info.effects.append((st.effect, show(st.idem) if st.idem is not None else ""))

    def finish(self):
        info, proc = self.info, self.proc
        for dname in list(info.exist_reads):
            pass
        # 宣言と本体の突き合わせ
        def cmp(label, declared, actual):
            declared, actual = set(declared), set(actual)
            for x in sorted(actual - declared):
                self.d(ERROR, "宣言", f"本体は「{x}」を{label}のに、宣言（{label}）にありません")
            for x in sorted(declared - actual):
                self.d(ERROR, "宣言", f"宣言（{label}）に「{x}」がありますが、本体では{label}ていません")

        cmp("読む", proc.reads, info.reads)
        cmp("書く", proc.writes, info.writes)
        cmp("外部", proc.effects, {e for e, _ in info.effects})
        declared_tr = set(proc.transitions)
        for tr in sorted(declared_tr - info.transitions):
            self.d(ERROR, "宣言", f"宣言した遷移 {tr[0]}: {tr[1]} → {tr[2]} が、本体にありません")
        dup = {w for w in info.write_list if info.write_list.count(w) > 1}
        for w in sorted(dup):
            self.d(ERROR, "書込", f"1つの処理で「{w}」に2回書いています（要素が重なると結果が順序で変わります）")
        rdef = self.ck.results.get(proc.result)
        if rdef:
            for v, _ in rdef.variants:
                if v not in info.returns:
                    self.d(WARN, "結果", f"結果「{v}」を返す箇所がありません")


def check(prog: Program, strict_names: bool = False):
    ck = Checker(prog, strict_names)
    ck.run()
    return ck
