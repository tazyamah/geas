"""検査器。型の推論つき。宣言（読む・書く・遷移・外部・結果）と本体のずれ、
行き先の漏れ、丸めの指定漏れ、冪等キーの漏れなどを見つける。"""
from dataclasses import dataclass, field

from .ast import *  # noqa: F401,F403

ERROR, WARN, QUESTION = "エラー", "警告", "問い"
METHODS = ("切り捨て", "切り上げ", "四捨五入")


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
    transitions: set = field(default_factory=set)  # (データ, 元, 先)
    effects: list = field(default_factory=list)  # (名前, 冪等キー)
    returns: set = field(default_factory=set)
    roundings: list = field(default_factory=list)  # (方式, 単位)
    new_ids: list = field(default_factory=list)  # 採番するデータ名
    decrements: list = field(default_factory=list)  # (D, f, Update, 本体の位置)
    types: dict = field(default_factory=dict)  # id(式) → 型トークン
    lookup_vars: dict = field(default_factory=dict)  # 変数 → データ名
    setfields: list = field(default_factory=list)  # (データ, 項目, 値, 位置, 変数)
    creates: list = field(default_factory=list)  # (データ, {項目: 値の名前})
    value_guards: list = field(default_factory=list)  # (データ, 項目, 演算子, 数, 位置)

    @property
    def reads(self) -> set:
        data_with_fields = {r.split(".")[0] for r in self.field_reads}
        return set(self.field_reads) | {d for d in self.exist_reads if d not in data_with_fields}


class Checker:
    def __init__(self, prog: Program):
        self.p = prog
        self.diags: list[Diag] = []
        self.data = {d.name: d for d in prog.data}
        self.states = {s.name: s for s in prog.states}
        self.results = {r.name: r for r in prog.results}
        self.effects = {e.name: e for e in prog.effects}
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

    def d(self, level, code, where, msg):
        self.diags.append(Diag(level, code, where, msg))

    def key_field(self, dname):
        for f in self.data[dname].fields:
            if f.type.kind == "ID" and f.unique:
                return f
        return None

    # ---- 全体 ----
    def run(self):
        self.check_defs()
        for proc in self.p.processes:
            self.infos[proc.name] = ProcChecker(self, proc).run()
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

    def check_defs(self):
        for dt in self.p.data:
            if self.key_field(dt.name) is None:
                self.d(ERROR, "データ", f"データ {dt.name}", "「ID 一意」の項目（キー）がありません")
            for f in dt.fields:
                t = f.type
                if t.kind == "参照" and t.target not in self.data:
                    self.d(ERROR, "名前", f"{dt.name}.{f.name}", f"参照先のデータ「{t.target}」が未定義です")
                if t.kind == "名前" and t.target not in self.states:
                    self.d(ERROR, "名前", f"{dt.name}.{f.name}", f"型「{t.target}」が未定義です")
                if t.kind == "写像" and t.key not in self.data:
                    self.d(ERROR, "名前", f"{dt.name}.{f.name}", f"キーのデータ「{t.key}」が未定義です")
        for s in self.p.states:
            if not s.initial or not (s.final or s.endless):
                self.d(ERROR, "状態", f"状態 {s.name}", "初期と終了の状態は必須です（行き来を繰り返す状態は「終了 なし」と書きます）")
            known = {s.initial, *s.middle, *s.final}
            for a, b, _ in s.rejects:
                for x in (a, b):
                    if x not in known:
                        self.d(ERROR, "名前", f"状態 {s.name}", f"拒否に書かれた「{x}」は状態にありません")

    def check_flows(self):
        in_flow = set()
        for fl in self.p.flows:
            for proc, routes in fl.entries:
                where = f"フロー {fl.name} / {proc}"
                if proc not in self.procs:
                    self.d(ERROR, "行き先", where, f"処理「{proc}」が未定義です")
                    continue
                in_flow.add(proc)
                rdef = self.results.get(self.procs[proc].result)
                names = [v for v, _ in routes]
                if rdef:
                    for v, _ in rdef.variants:
                        if v not in names:
                            self.d(ERROR, "行き先", where, f"結果「{v}」の行き先がありません")
                    for v in names:
                        if v not in [x for x, _ in rdef.variants]:
                            self.d(ERROR, "名前", where, f"「{v}」は結果 {rdef.name} にありません")
                for v, dest in routes:
                    base = dest.split(" ", 1)[1] if dest.startswith("待つ ") else dest
                    if dest.startswith("待つ "):
                        self.check_wait(where, proc, base)
                    elif base != "応答" and base not in self.procs:
                        self.d(ERROR, "行き先", where, f"結果「{v}」の行き先「{base}」が未定義の処理です")
        for name in self.procs:
            if name not in in_flow:
                self.d(WARN, "行き先", f"処理 {name}", "どのフローにも入っていません")

    def check_effect_waits(self):
        """外部作用を要求する処理は、結果を待つ経路（フローの「待つ」）を持つか、待たない理由を宣言する。
        信頼できない先（ネットワーク・外部サービス）への永続化が、黙って失敗しっぱなしになるのを防ぐ。
        論理的な失敗の理由（失敗 …）を宣言した外部作用は、待つ先のトリガに理由ごとの応答も要る。"""
        waits = {}
        for fl in self.p.flows:
            for proc, routes in fl.entries:
                for _, d in routes:
                    if d.startswith("待つ "):
                        waits.setdefault(proc, set()).add(d.split(" ", 1)[1])
        triggers = {t.name: t for t in self.p.triggers}
        for proc, info in self.infos.items():
            for eff in sorted({e for e, _ in info.effects}):
                decl = self.effects.get(eff)
                if decl is None or decl.no_wait is not None:
                    continue
                if proc not in waits:
                    self.d(ERROR, "失敗", f"処理 {proc}", f"外部作用 {eff} の失敗の扱いが決まっていません（フローで「待つ」へ進めるか、外部作用の宣言に 待たない \"人が決めた理由\" を書いてください）")
                    continue
                for tname in sorted(waits[proc]):
                    tg = triggers.get(tname)
                    answered = {key for k, key, _ in tg.causes if k == "応答"} if tg else set()
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

            for fn, op, st in q.conds:
                f = field_of(fn)
                if f is not None and (f.type.kind != "名前" or st not in [x for s in self.p.states if s.name == f.type.target for x in [s.initial, *s.middle, *s.final]]):
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
                if op == "含む" and not (f.type.kind == "文字列" and t.kind == "文字列"):
                    self.d(ERROR, "型", where, f"「{fn} 含む {arg}」は文字列どうしにだけ使えます")
                if op in ("以上", "以下") and not (f.type.kind == t.kind and f.type.kind in ("日時", "整数", "金額")):
                    self.d(ERROR, "型", where, f"「{fn} {op} {arg}」は同じ種類の数・日時どうしにだけ使えます")
            if q.order:
                f = field_of(q.order[0])
                if f is not None and f.type.kind not in ("日時", "整数", "金額"):
                    self.d(ERROR, "型", where, f"並びの項目「{q.order[0]}」は数か日時である必要があります")
            if q.limit is not None:
                t = ins.get(q.limit)
                if t is None or t.kind != "整数":
                    self.d(ERROR, "型", where, f"先頭の件数「{q.limit}」は、整数の入力である必要があります")
            for n, t in q.inputs:
                if not t.optional and not any(n == a for _, _, a in q.filters) and n != q.limit:
                    self.d(WARN, "宣言", where, f"入力「{n}」はどこにも使われていません")
            declared, actual = set(q.reads), used
            for x in sorted(actual - declared):
                self.d(ERROR, "宣言", where, f"問い合わせは「{x}」を読むのに、宣言（読む）にありません")
            for x in sorted(declared - actual):
                self.d(ERROR, "宣言", where, f"宣言（読む）に「{x}」がありますが、使っていません")
            self.query_reads[q.name] = actual

    def check_wait(self, where, proc, trig):
        tg = self.triggers.get(trig)
        if tg is None:
            self.d(ERROR, "トリガ", where, f"待つ 先のトリガ「{trig}」が未定義です")
            return
        have = {n for n, _ in self.procs[proc].inputs}
        for n, _ in tg.params:
            if n not in have:
                self.d(ERROR, "トリガ", where, f"トリガ {trig} の引数「{n}」を、処理 {proc} は入力にもっていません")
        mine = {n: show_type(t) for n, t in self.procs[proc].inputs}
        for n, t in tg.params:
            if n in mine and mine[n] != show_type(t):
                self.d(ERROR, "トリガ", where, f"トリガ {trig} の引数「{n}」の型が、処理 {proc} の入力と違います")

    def check_triggers(self):
        waited = set()
        for fl in self.p.flows:
            for _, routes in fl.entries:
                waited |= {d.split(" ", 1)[1] for _, d in routes if d.startswith("待つ ")}
        names = [t.name for t in self.p.triggers]
        for tg in self.p.triggers:
            where = f"トリガ {tg.name}"
            if names.count(tg.name) > 1:
                self.d(ERROR, "トリガ", where, "同じ名前のトリガが複数あります")
            deadlines = [key for k, key, _ in tg.causes if k == "期限"]
            if len(deadlines) > 1:
                self.d(ERROR, "トリガ", where, "期限が複数あります（先に起きた方が勝つので、長い方は起きません）")
            if any(int(x) <= 0 for x in deadlines):
                self.d(ERROR, "トリガ", where, "期限は0より長くしてください")
            if not any(k == "期限" for k, _, _ in tg.causes):
                self.d(ERROR, "トリガ", where, "期限がありません（待ち続けないよう、期限と期限切れの行き先が必要です）")
            if not any(k == "応答" for k, _, _ in tg.causes):
                self.d(WARN, "トリガ", where, "解く応答がありません（期限切れでしか終わらない待ちです）")
            seen = set()
            for k, key, dest in tg.causes:
                if (k, key) in seen:
                    self.d(ERROR, "トリガ", where, f"要因「{k} {key}」が重複しています")
                seen.add((k, key))
                proc = self.procs.get(dest)
                if proc is None:
                    self.d(ERROR, "トリガ", where, f"要因「{k} {key}」の行き先「{dest}」が未定義の処理です")
                elif [(n, show_type(t)) for n, t in proc.inputs] != [(n, show_type(t)) for n, t in tg.params]:
                    self.d(ERROR, "トリガ", where, f"行き先の処理 {dest} の入力（名前と型）が、トリガの引数と合いません")
            if tg.name not in waited:
                self.d(WARN, "トリガ", where, "どのフローからも「待つ」で作られていません")

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
                if any(not t.optional for _, t in q.inputs):
                    self.d(ERROR, "出来事", where, f"問い合わせ {qn} は入力が要るので、出来事からは呼べません（任意の入力だけにしてください）")
                dt = self.data.get(q.target)
                if dt is None:
                    continue
                key = self.key_field(dt.name)
                if [(n, show_type(t)) for n, t in proc.inputs] != [(key.name, "ID")]:
                    self.d(ERROR, "出来事", where, f"「各 {qn}」は {dt.name} の {key.name} だけを渡すので、処理 {pn} の入力は「{key.name}: ID」だけにしてください")

    def check_states(self):
        for s in self.p.states:
            edges = {}
            for info in self.infos.values():
                for dt, a, b in info.transitions:
                    if dt in self.data and any(f.type.target == s.name for f in self.data[dt].fields):
                        edges.setdefault(a, set()).add(b)
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
        for inv in self.p.invariants:
            text = f"{inv.data}: {show(inv.cond)}"
            rows = []
            c = inv.cond
            simple = isinstance(c, Bin) and c.op == ">=" and isinstance(c.l, Name) and c.r == Num(0)
            for info in self.infos.values():
                proc = self.procs[info.name]
                touched = [w for w in info.writes if w == inv.data or w.startswith(inv.data + ".")]
                if not touched:
                    rows.append((info.name, "対象外"))
                    continue
                if isinstance(c, Implies):
                    rows.append((info.name, self.check_implies(inv, c, info, text)))
                    continue
                if not simple:
                    rows.append((info.name, "未検査（試作は「項目 >= 0」と「状態 == X なら 項目 > n」の形だけ検査する）"))
                    continue
                f = c.l.n
                bad = False
                for (dname, fname, up, pos) in info.decrements:
                    if dname != inv.data or fname != f:
                        continue
                    if up.op == "-=" and self.guarded_decrement(proc, up, pos, info):
                        continue
                    bad = True
                    self.d(ERROR, "不変条件", f"処理 {info.name}", f"不変条件「{text}」が成り立つ保証がありません"
                           "（減算の前に不足の確認がありません。反例: 数量 > 在庫 の入力）")
                rows.append((info.name, "証明できない" if bad else "証明済み（減算の前に不足を確認している）"))
            self.invariant_status.append((text, rows))

    def check_implies(self, inv, c, info, text) -> str:
        l, r = c.l, c.r
        if not (isinstance(l, Bin) and l.op == "==" and isinstance(l.l, Name) and isinstance(l.r, Name)
                and isinstance(r, Bin) and r.op == ">" and isinstance(r.l, Name) and isinstance(r.r, Num)):
            return "未検査（形が未対応）"
        f, state, g, n = l.l.n, l.r.n, r.l.n, r.r.v
        status = []
        for (dname, ff, val, pos, var) in info.setfields:
            if dname != inv.data or ff != f or val != state:
                continue
            ok = any(gd == (dname, g) and op == "<=" and k == n and gp < pos
                     for (gd0, gd1, op, k, gp) in info.value_guards for gd in [(gd0, gd1)])
            if not ok:
                self.d(ERROR, "不変条件", f"処理 {info.name}", f"不変条件「{text}」が成り立つ保証がありません"
                       f"（{dname}を「{state}」にする前に「{g} > {n}」の確認がありません。反例: {g} が {n} 以下の{dname}）")
                return "証明できない"
            status.append(f"{state}にする前に {g} > {n} を確認している")
        for (dname, fields) in info.creates:
            if dname != inv.data:
                continue
            if fields.get(f) == state:
                return "未検査（「%s」で新規作成している）" % state
            if f in fields and fields[f] != state:
                status.append(f"新規作成は{state}以外")
        if status:
            return "証明済み（" + " ／ ".join(status) + "）"
        return "対象外"

    def guarded_decrement(self, proc, up, pos, info) -> bool:
        defs = {}
        guards = set()
        for i, st in enumerate(proc.body[:pos]):
            if isinstance(st, Assign):
                defs[st.name] = st.e
            if isinstance(st, If) and isinstance(st.cond, Pred) and st.cond.kind == "空でない" and isinstance(st.cond.e, Name):
                guards.add(st.cond.e.n)
        t = up.target  # Attr(Index(D, k), f)
        for var in guards:
            sb = defs.get(var)
            if not (isinstance(sb, SetB) and isinstance(sb.cond, Bin) and sb.cond.op == "<"):
                continue
            if show(sb.binder.src) != show(up.binder.src) or len(sb.binder.names) != 2:
                continue
            k2, v2 = sb.binder.names
            lhs = sb.cond.l
            if (isinstance(lhs, Attr) and lhs.n == t.n and isinstance(lhs.e, Index) and lhs.e.d == t.e.d
                    and lhs.e.e == Name(k2) and sb.cond.r == Name(v2) and sb.elem == Name(k2)
                    and isinstance(up.value, Name) and up.value.n == up.binder.names[1]):
                return True
        return False


class ProcChecker:
    def __init__(self, ck: Checker, proc: Process):
        self.ck = ck
        self.proc = proc
        self.info = ProcInfo(proc.name)
        self.env: dict[str, str] = {}
        self.where = f"処理 {proc.name}"
        self.exist_sets: dict[str, str] = {}
        self.guarded: set[str] = set()
        self.state_guards: set[tuple[str, str, str]] = set()  # (データ, 項目, 元)
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
        if isinstance(e, Name):
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
        if isinstance(e, Attr):
            base = e.e
            bt = self.ty(base, env)
            if not bt.startswith("Rec:"):
                self.d(ERROR, "型", f"「{show(e)}」: 「.{e.n}」はデータにしか使えません")
                return "?"
            dname = bt[4:]
            fld = next((f for f in ck.data[dname].fields if f.name == e.n), None)
            if fld is None:
                self.d(ERROR, "名前", f"データ {dname} に項目「{e.n}」がありません")
                return "?"
            if isinstance(base, Index) and dname not in self.guarded:
                self.d(QUESTION, "存在", f"{show(base)} が存在しないとき、どうしますか"
                       f"（{dname}の存在を確かめて返す処理が、使う前にありません）")
            if isinstance(base, Name):
                self.accessed_vars.add(base.n)
            if not (isinstance(base, Name) and base.n in self.created):
                self.info.field_reads.add(f"{dname}.{e.n}")
            return tok_of(fld.type)
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
        if isinstance(e, Call):
            if e.n != "丸め" or len(e.args) != 3:
                self.d(ERROR, "名前", f"部品「{e.n}」は呼べません（丸め(値, 方式, 単位) だけ）")
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

    def note_exist_set(self, e):
        c = e.cond
        if isinstance(c, Pred) and c.kind == "ない" and isinstance(c.e, Index):
            self._last_exist = c.e.d

    def bind(self, b: Binder, env=None):
        env = self.env if env is None else env
        st = self.ty(b.src, env)
        env2 = dict(env)
        if st.startswith("Map:") and len(b.names) == 2:
            _, _, vt = st.split(":", 2)
            env2[b.names[0]] = "ID"
            env2[b.names[1]] = vt
        elif st == "Set" and len(b.names) == 1:
            env2[b.names[0]] = "ID"
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
        if e.op == "かつ":
            if a != "Bool" or b != "Bool":
                self.d(ERROR, "型", f"「{show(e)}」: かつ の両側は条件である必要があります")
            return "Bool"
        if e.op in ("<", ">", "<=", ">="):
            if not ((a in num and b in num) or (a == "Time" and b == "Time")):
                self.d(ERROR, "型", f"「{show(e)}」: 数どうし（または日時どうし）で比べてください")
            return "Bool"
        if e.op in ("==", "!="):
            if a != b and not (a in num and b in num):
                self.d(ERROR, "型", f"「{show(e)}」: 違う型を比べています")
            return "Bool"
        if e.op in ("+", "-"):
            if "Rational" in (a, b):
                self.d(QUESTION, "丸め", f"「{show(e)}」: 端数が出る計算の丸め（切り捨て・切り上げ・四捨五入）と単位が"
                       "指定されていません。端数をどう丸めますか")
                return "Money"
            if a in num and b in num:
                return "Money" if "Money" in (a, b) else "Int"
            self.d(ERROR, "型", f"「{show(e)}」: 数どうしで計算してください")
            return "?"
        if e.op == "*":
            if a in num and b in num:
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
        for t, a in zip(v[1], r.args):
            at = self.ty(a)
            if at != tok_of(t) and at != "?":
                self.d(ERROR, "型", f"結果「{r.variant}」の引数の型が違います（{at}）", r.line)

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
        self.finish()
        return info

    def stmt(self, st, pos):
        info = self.info
        self.pos = pos
        if isinstance(st, Return):
            self.check_return(st)
        elif isinstance(st, If):
            self.ty(st.cond)
            self.check_return(st.ret)
            c = st.cond
            if isinstance(c, Pred) and c.kind == "空でない" and isinstance(c.e, Name) and c.e.n in self.exist_sets:
                self.guarded.add(self.exist_sets[c.e.n])
            if (isinstance(c, Bin) and c.op == "!=" and isinstance(c.l, Attr) and isinstance(c.l.e, Name)
                    and c.l.e.n in self.lookup_data and isinstance(c.r, Name)):
                self.state_guards.add((self.lookup_data[c.l.e.n], c.l.n, c.r.n))
            if (isinstance(c, Bin) and c.op in ("<=", "<", "==") and isinstance(c.l, Attr) and isinstance(c.l.e, Name)
                    and c.l.e.n in self.lookup_data and isinstance(c.r, Num)):
                info.value_guards.append((self.lookup_data[c.l.e.n], c.l.n, c.op, c.r.v, pos))
        elif isinstance(st, Assign):
            if isinstance(st.e, Index):
                t = self.ty(st.e)
                self.env[st.name] = t
                self.lookup_data[st.name] = st.e.d
                info.exist_reads.add(st.e.d)
                if st.absent is None:
                    self.d(QUESTION, "存在", f"{show(st.e)} が存在しないとき、どうしますか（なければ 返す …が必要です）", st.line)
                else:
                    self.check_return(st.absent)
                    self.guarded.add(st.e.d)
            else:
                self._last_exist = None
                t = self.ty(st.e)
                self.env[st.name] = t
                if isinstance(st.e, SetB) and self._last_exist:
                    self.exist_sets[st.name] = self._last_exist
        elif isinstance(st, Update):
            env2 = self.bind(st.binder)
            vt = self.ty(st.value, env2)
            if st.cond is not None:
                self.ty(st.cond, env2)
            tgt = st.target
            if not (isinstance(tgt, Attr) and isinstance(tgt.e, Index)):
                self.d(ERROR, "型", "更新の対象は D[k].項目 の形です", st.line)
                return
            self.ty(tgt, env2)
            info.field_reads.discard(f"{tgt.e.d}.{tgt.n}") if st.op == "=" else None
            if vt == "Rational":
                self.d(QUESTION, "丸め", "端数が出る値を、丸めずに書いています", st.line)
            info.writes.add(f"{tgt.e.d}.{tgt.n}")
            info.write_list.append(f"{tgt.e.d}.{tgt.n}")
            info.decrements.append((tgt.e.d, tgt.n, st, pos))
        elif isinstance(st, SetField):
            data = self.lookup_data.get(st.var)
            if data is None:
                self.d(ERROR, "型", f"「{st.var}」は、データを引いた変数ではありません", st.line)
                return
            fld = next((f for f in self.ck.data[data].fields if f.name == st.f), None)
            if fld is None:
                self.d(ERROR, "名前", f"データ {data} に項目「{st.f}」がありません", st.line)
                return
            self.ty(st.e)
            self.check_range_fit(st.e, fld.type, f"{data}.{st.f}", st.line)
            info.writes.add(f"{data}.{st.f}")
            info.write_list.append(f"{data}.{st.f}")
            if fld.type.kind == "名前" and isinstance(st.e, Name):
                self.check_transition(data, st.f, st.e.n, st.line)
                info.setfields.append((data, st.f, st.e.n, pos, st.var))
        elif isinstance(st, Create):
            self.create(st)
        elif isinstance(st, Request):
            self.request(st)
        elif isinstance(st, ForEach):
            env2 = self.bind(st.binder)
            saved = self.env
            self.env = env2
            for inner in st.body:
                if isinstance(inner, Request) or (isinstance(inner, Assign) and isinstance(inner.e, Index)):
                    self.stmt(inner, pos)
                else:
                    self.d(ERROR, "型", "各 … { } の中に書けるのは、データを引く文と、要求だけです", st.line)
            self.env = saved

    def check_transition(self, data, fname, dst, line):
        sname = next(f.type.target for f in self.ck.data[data].fields if f.name == fname)
        decl = [(a, b) for (d, a, b) in self.proc.transitions if d == data and b == dst]
        if not decl:
            self.d(ERROR, "遷移", f"{data} を「{dst}」にする遷移が、処理の宣言（遷移）にありません", line)
            return
        for a, b in decl:
            self.info.transitions.add((data, a, b))
            if a != "(新規)" and (data, fname, a) not in self.state_guards:
                self.d(ERROR, "遷移", f"{data} が「{a}」であることの確認（もし … != {a} なら 返す）が、遷移の前にありません", line)
        for x, y, why in self.ck.states[sname].rejects:
            if y == dst and any(a == x for a, _ in decl):
                self.d(ERROR, "遷移", f"拒否された遷移 {x} → {y}（{why}）を行っています", line)

    def check_range_fit(self, e, dst: Type, where: str, line: int):
        """入力をそのまま項目へ入れるとき、入力の範囲が項目の範囲に収まるか。"""
        src = next((t for n, t in self.proc.inputs if isinstance(e, Name) and n == e.n), None)
        if src is None:
            return
        if src.kind == "写像" and dst.kind == "写像":
            src, dst = src.value, dst.value
        if src.kind not in ("整数", "金額", "割合") or src.kind != dst.kind:
            return
        s, d = src.range or Range(), dst.range or Range()
        lo_ng = d.lo is not None and (s.lo is None or s.lo < d.lo)
        hi_ng = d.hi is not None and (s.hi is None or s.hi > d.hi)
        if lo_ng or hi_ng:
            self.d(ERROR, "範囲", f"{where} に、範囲の広い入力「{e.n}」を入れています"
                   f"（入力{show_range(src.range).strip() or '制限なし'} ／ 項目{show_range(dst.range).strip()}）。収まるか確認が要ります", line)

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
            if f.name not in given:
                self.d(ERROR, "項目", f"{st.data} の新規作成に項目「{f.name}」がありません", st.line)
        for n, e in st.fields:
            f = next((x for x in dt.fields if x.name == n), None)
            if f is None:
                self.d(ERROR, "名前", f"データ {st.data} に項目「{n}」がありません", st.line)
                continue
            vt = self.ty(e)
            if vt == "Rational":
                self.d(QUESTION, "丸め", "端数が出る値を、丸めずに書いています", st.line)
            elif vt != tok_of(f.type) and not (vt in ("Int", "Money") and tok_of(f.type) in ("Int", "Money")) and vt != "?":
                self.d(ERROR, "型", f"{st.data}.{n} に型の違う値（{vt}）を入れています", st.line)
            self.check_range_fit(e, f.type, f"{st.data}.{n}", st.line)
            if f.type.kind == "名前" and isinstance(e, Name):
                self.check_transition_new(st.data, n, e.n, st.line)
        info.writes.add(st.data)
        info.new_ids.append(st.data)
        info.creates.append((st.data, {n: e.n for n, e in st.fields if isinstance(e, Name)}))
        self.created.add(st.name)
        self.env[st.name] = f"Rec:{st.data}"
        self.lookup_data[st.name] = st.data
        self.guarded.add(st.data)

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
            if at != tok_of(t) and not (at in ("Int", "Money") and tok_of(t) in ("Int", "Money")) and at != "?":
                self.d(ERROR, "型", f"外部作用 {st.effect} の引数の型が違います（{at}）", st.line)
        if st.idem is None:
            self.d(ERROR, "冪等", f"外部作用 {st.effect} に冪等キーがありません（冪等キー = … が必須です）", st.line)
        else:
            self.ty(st.idem)
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


def check(prog: Program):
    ck = Checker(prog)
    ck.run()
    return ck
