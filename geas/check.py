"""検査器。型の推論つき。宣言（読む・書く・遷移・外部・結果）と本体のずれ、
行き先の漏れ、丸めの指定漏れ、冪等キーの漏れなどを見つける。"""
from dataclasses import dataclass, field
from fractions import Fraction
from math import ceil, floor

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
        return next(x.type for x in tdt.fields if x.name == c.field)
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
        self.inv_forms = [inv_form(inv) for inv in prog.invariants]
        self.inv_rows: dict[int, dict[str, list[tuple[bool, str]]]] = {}  # 不変条件 → 処理 → (示せたか, 理由)

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
                    self.d(ERROR, "失敗", f"処理 {proc}", f"外部作用 {eff} の失敗の扱いが決まっていません（ネクサスで「wait」へ進めるか、外部作用の宣言に no_wait \"人が決めた理由\" を書いてください）")
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

            for group in q.conds:
                for fn, op, st in group:
                    f = field_of(fn)
                    if f is None:
                        continue
                    if st.isdigit():
                        if f.type.kind not in ("整数", "金額", "日時", "割合"):
                            self.d(ERROR, "型", where, f"条件「{fn} {op} {st}」: 数との比較は、数か日時の項目にだけ使えます")
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
            self.d(ERROR, "トリガ", where, f"wait 先のトリガ「{trig}」が未定義です")
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
            if isinstance(base, Index) and not self.exists_known(base, env):
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
        """{ k | (k, v) in 元, D[k] is absent } の形なら、「元のキーがすべて D にあるか」を表す集合として覚える"""
        c = e.cond
        if (isinstance(c, Pred) and c.kind == "ない" and isinstance(c.e, Index)
                and c.e.e == Name(e.binder.names[0]) and e.elem == Name(e.binder.names[0])):
            self._last_exist = (c.e.d, "∈" + show(e.binder.src))

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
            lo, hi = self.field_iv(data, e.n)
            if isinstance(e.e, Name) and e.e.n in self.bindings:
                known = self.facts.get((self.bindings[e.e.n], e.n))
                if known is not None and (lo is None or known > lo):
                    lo = known
            return (lo, hi)
        if isinstance(e, Bin) and e.op in ("+", "-", "*"):
            a, b = self.interval(e.l, env), self.interval(e.r, env)
            return iv_add(a, b) if e.op == "+" else iv_add(a, iv_neg(b)) if e.op == "-" else iv_mul(a, b)
        if isinstance(e, Call) and e.n == "丸め" and len(e.args) == 3:
            lo, hi = self.interval(e.args[0], env)
            how = getattr(e.args[1], "n", "")
            if getattr(e.args[2], "n", "") == "円":  # 1円単位：整数への丸め
                return (None if lo is None else Fraction(floor(lo) if how != "切り上げ" else ceil(lo)),
                        None if hi is None else Fraction(ceil(hi) if how != "切り捨て" else floor(hi)))
            # 単位の大きさが分からないとき：0 以上の値は、どう丸めても 0 以上
            return (0 if lo is not None and lo >= 0 else None, None)
        if isinstance(e, SumE):
            lo, _ = self.interval(e.elem, self.range_env(e.binder, env))
            return (0 if lo is not None and lo >= 0 else None, None)
        return (None, None)

    def bind(self, b: Binder, env=None):
        env = self.env if env is None else env
        st = self.ty(b.src, env)
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
            c = st.cond
            state_cmp = (isinstance(c, Bin) and c.op in ("==", "!=") and isinstance(c.l, Attr) and isinstance(c.l.e, Name)
                         and c.l.e.n in self.bindings and isinstance(c.r, Name))
            fact = (self.bindings[c.l.e.n], c.l.n, c.r.n) if state_cmp else None
            if not st.body:
                self.d(ERROR, "型", "if の中が空です", st.line)
            # 「x.項目 == 状態」や「x.項目 > 数」が成り立つ分岐の中で分かることは、分岐の外へは持ち出さない
            outer_sg, outer_facts, outer_guarded = self.state_guards, self.facts, self.guarded
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
                self.state_guards, self.facts, self.guarded = outer_sg, skip_facts, outer_guarded
                if isinstance(c, Pred) and c.kind == "空でない" and isinstance(c.e, Name) and c.e.n in self.exist_sets:
                    self.guarded.add(self.exist_sets[c.e.n])
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
        elif isinstance(st, Assign):
            if isinstance(st.e, Index):
                t = self.ty(st.e)
                self.env[st.name] = t
                self.lookup_data[st.name] = st.e.d
                self.bind_var(st.name)
                self.var_iv.pop(st.name, None)
                self.shortage_sets.pop(st.name, None)
                info.exist_reads.add(st.e.d)
                if st.absent is None:
                    self.d(QUESTION, "存在", f"{show(st.e)} が存在しないとき、どうしますか（else {{ return … }} が必要です）", st.line)
                else:
                    self.check_return(st.absent)
                    self.guarded.add((st.e.d, show(st.e.e)))
            else:
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
            tf = next((f for f in self.ck.data[tgt.e.d].fields if f.name == tgt.n), None) if tgt.e.d in self.ck.data else None
            if tf is not None and tf.type.kind == "名前":
                self.d(ERROR, "遷移", f"update で状態の項目「{tgt.e.d}.{tgt.n}」は変えられません（遷移の確認ができないため。"
                       "1件ずつ let で引き、元の状態を確かめてから変えてください）", st.line)
            if vt == "Rational":
                self.d(QUESTION, "丸め", "端数が出る値を、丸めずに書いています", st.line)
            info.writes.add(f"{tgt.e.d}.{tgt.n}")
            info.write_list.append(f"{tgt.e.d}.{tgt.n}")
            if tf is not None and tf.type.kind in ("整数", "金額", "割合"):
                self.check_update(st, env2)
            self.forget(tgt.e.d, tgt.n)
        elif isinstance(st, SetField):
            data = self.lookup_data.get(st.var)
            if data is None:
                self.d(ERROR, "型", f"「{st.var}」は、データを引いた変数ではありません", st.line)
                return
            fld = next((f for f in self.ck.data[data].fields if f.name == st.f), None)
            if fld is None:
                self.d(ERROR, "名前", f"データ {data} に項目「{st.f}」がありません", st.line)
                return
            vt = self.ty(st.e)
            if vt == "Rational":
                self.d(QUESTION, "丸め", "端数が出る値を、丸めずに書いています", st.line)
            elif (vt != tok_of(fld.type) and vt != "?" and fld.type.kind != "名前"
                  and not (vt in ("Int", "Money") and tok_of(fld.type) in ("Int", "Money"))):
                self.d(ERROR, "型", f"{data}.{st.f} に型の違う値（{vt}）を入れています", st.line)
                return
            self.check_range_fit(st.e, fld.type, f"{data}.{st.f}", st.line)
            b = self.bindings.get(st.var, st.var)
            if fld.type.kind in ("整数", "金額", "割合"):
                iv = self.interval(st.e)
                self.check_write(data, st.f, iv, st.line, bind=b)
                self.forget(data, st.f)
                if iv[0] is not None:
                    self.facts[(b, st.f)] = iv[0]
            info.writes.add(f"{data}.{st.f}")
            info.write_list.append(f"{data}.{st.f}")
            if fld.type.kind == "名前":
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
            for inner in st.body:
                if isinstance(inner, Request) or (isinstance(inner, Assign) and isinstance(inner.e, Index)):
                    self.stmt(inner, pos)
                else:
                    self.d(ERROR, "型", "for … { } の中に書けるのは、データを引く文と、要求だけです", st.line)
            self.env = saved

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
            if f.type.kind == "名前":
                if isinstance(e, Name) and e.n in self.ck.variants and e.n not in self.env:
                    self.check_transition_new(st.data, n, e.n, st.line)
                else:
                    self.d(ERROR, "遷移", f"{st.data} の新規作成の「{n}」は状態名で書いてください（「{show(e)}」では、どの遷移か確かめられません）", st.line)
        info.writes.add(st.data)
        info.new_ids.append(st.data)
        states = {f.name: given[f.name].n for f in dt.fields
                  if f.type.kind == "名前" and isinstance(given.get(f.name), Name)}
        lows = {}
        for n, e in st.fields:
            f = next((x for x in dt.fields if x.name == n), None)
            if f is not None and f.type.kind in ("整数", "金額", "割合"):
                iv = self.interval(e)
                self.check_write(st.data, n, iv, st.line, new_state=states)
                lows[n] = iv[0]
        self.created.add(st.name)
        self.env[st.name] = f"Rec:{st.data}"
        self.lookup_data[st.name] = st.data
        b = self.bind_var(st.name)
        # 作ったばかりの1件は、状態も値も分かっている
        self.state_guards |= {(b, n, v) for n, v in states.items()}
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
        # 書き換わる前に作った「足りない k の集まり」は、もう今の値を表さない
        self.shortage_sets = {n: v for n, v in self.shortage_sets.items() if v[:2] != (data, fname)}

    def inv_row(self, i, ok, note):
        self.ck.inv_rows.setdefault(i, {}).setdefault(self.proc.name, []).append((ok, note))

    def check_write(self, data, fname, iv, line, bind=None, note=None, grow=False, new_state=None):
        """項目へ範囲 iv の値を書くとき、項目の範囲と、その項目にかかる不変条件が守れるか。
        bind は書く1件の束縛（update では None）。note は値の範囲を示せた理由。grow は「元の値から減らさない」こと。
        new_state は新規作成のときの状態（状態の項目 → 状態名）"""
        fld = next(f for f in self.ck.data[data].fields if f.name == fname)
        lo, hi = iv_of(fld.type)
        if (lo is not None and (iv[0] is None or iv[0] < lo)) or (hi is not None and (iv[1] is None or iv[1] > hi)):
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
                           f"（反例：書く値が {'いくらでも小さく' if iv[0] is None else iv[0]} になりえます）", line)
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
            if at != tok_of(t) and not (at in ("Int", "Money") and tok_of(t) in ("Int", "Money")) and at != "?":
                self.d(ERROR, "型", f"外部作用 {st.effect} の引数の型が違います（{at}）", st.line)
        if st.idem is None:
            self.d(ERROR, "冪等", f"外部作用 {st.effect} に冪等キーがありません（idempotency_key = … が必須です）", st.line)
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
