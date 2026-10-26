"""構文解析（再帰下降）。文は1行1つ。ブロックは「{ で行が終わり、} だけの行で閉じる」。
キーワードは英語。構文木の中身（型の種類や結果の行き先など）は、仕様書の言葉に合わせた日本語のまま持つ。"""
from .ast import *  # noqa: F401,F403
from .lexer import Tok, lex


class ParseError(Exception):
    def __init__(self, msg: str, line: int):
        super().__init__(f"{line}行目: {msg}")
        self.line = line


CMP = ("<", ">", "<=", ">=", "==", "!=")
UNITS = {"sec": 1, "min": 60, "hour": 3600, "day": 86400}
UNIT_WORDS = "sec・min・hour・day"
SIMPLE_TYPES = {"text": "文字列", "time": "日時", "bytes": "バイト列", "int": "整数", "ratio": "割合", "money": "金額"}
PRED = {"empty": "空", "nonempty": "空でない", "present": "ある", "absent": "ない"}


class Parser:
    def __init__(self, toks: list[Tok]):
        self.t = toks
        self.i = 0

    # ---- 部品 ----
    def peek(self, k=0) -> Tok:
        return self.t[min(self.i + k, len(self.t) - 1)]

    def next(self) -> Tok:
        tok = self.t[self.i]
        if tok.kind != "EOF":
            self.i += 1
        return tok

    def is_(self, val, k=0) -> bool:
        t = self.peek(k)
        return t.kind in ("NAME", "OP") and t.val == val

    def accept(self, val) -> bool:
        if self.is_(val):
            self.i += 1
            return True
        return False

    def err(self, msg):
        return ParseError(msg, self.peek().line)

    def expect(self, val):
        if not self.accept(val):
            got = self.peek().val or "行末"
            raise self.err(f"「{val}」が必要です（「{got}」があります）")

    def name(self) -> str:
        t = self.peek()
        if t.kind != "NAME":
            raise self.err(f"名前が必要です（「{t.val or '行末'}」があります）")
        self.i += 1
        return t.val

    def num(self) -> int:
        t = self.peek()
        if t.kind != "NUM":
            raise self.err("数が必要です")
        self.i += 1
        return int(t.val)

    def skip_nl(self):
        while self.peek().kind == "NL":
            self.i += 1

    def end_line(self):
        if self.peek().kind == "NL":
            self.skip_nl()
        elif self.peek().kind != "EOF":
            raise self.err(f"行末のはずが「{self.peek().val}」があります")

    def comma_list(self, item):
        out = [item()]
        while self.accept(","):
            out.append(item())
        return out

    # ---- 全体 ----
    def program(self) -> Program:
        p = Program()
        self.skip_nl()
        while self.peek().kind != "EOF":
            line = self.peek().line
            kw = self.name()
            if kw == "data":
                p.data.append(self.data(line))
            elif kw == "state":
                p.states.append(self.state(line))
            elif kw == "result":
                p.results.append(self.result_def(line))
            elif kw == "effect":
                p.effects.append(self.effect_decl(line))
            elif kw == "part":
                p.parts.append(self.part(line))
            elif kw == "invariant":
                p.invariants.append(self.invariant(line))
            elif kw == "deed":
                p.processes.append(self.process(line))
            elif kw in ("nexus", "flow"):  # flow は nexus の別名（糖衣構文）
                p.flows.append(self.flow(line))
            elif kw == "trigger":
                p.triggers.append(self.trigger(line))
            elif kw == "query":
                p.queries.append(self.query(line))
            elif kw == "event":
                p.events.append(self.event(line))
            else:
                raise ParseError(f"未知の定義「{kw}」です", line)
            self.skip_nl()
        self.add_sync_state(p)
        return p

    def add_sync_state(self, p: Program):
        """保存先（外部）を持つデータには、未同期かどうかの状態「記録」を言語が足す（J63）。人は宣言しない"""
        ext = [d for d in p.data if d.store]
        if not ext:
            return
        for d in ext:
            if any(f.name == "記録" for f in d.fields):
                raise ParseError(f"データ {d.name} の「記録」は、保存先の宣言から言語が足します。書かないでください", d.line)
            d.fields.append(Field("記録", Type("名前", target="記録状態")))
        if any(s.name == "記録状態" for s in p.states):
            raise ParseError("状態「記録状態」は、保存先の宣言から言語が足します。書かないでください", ext[0].line)
        p.states.append(State("記録状態", initial="端末保存済", middle=["記録済"], endless=True, line=ext[0].line))

    def block_open(self):
        self.expect("{")
        self.end_line()

    # ---- データ ----
    def data(self, line) -> Data:
        name = self.name()
        store = self.name() if self.accept("store") else None
        self.block_open()
        fields = []
        while not self.accept("}"):
            fname = self.name()
            self.expect(":")
            t = self.type()
            f = Field(fname, t)
            while self.peek().kind == "NAME" and self.peek().val in ("unique", "optional", "required"):
                m = self.name()
                if m == "unique":
                    f.unique = True
                elif m == "optional":
                    f.optional = True
            fields.append(f)
            self.end_line()
        return Data(name, fields, line, store)

    def range_opt(self):
        if self.peek().kind == "NUM":
            lo = self.num()
            self.expect("..")
            hi = self.num() if self.peek().kind == "NUM" else None
            return Range(lo, hi)
        if self.is_(".."):
            self.next()
            return Range(None, self.num())
        return None

    def type(self) -> Type:
        tok = self.name()
        if tok == "id":
            return Type("ID")
        if tok == "set":  # set of id
            self.expect("of")
            self.expect("id")
            return Type("集合", value=Type("ID"))
        if tok in ("text", "time"):
            return Type(SIMPLE_TYPES[tok])
        if tok == "bytes":  # 範囲は長さ（バイト数）
            return Type("バイト列", range=self.range_opt())
        if tok in ("int", "ratio"):
            return Type(SIMPLE_TYPES[tok], range=self.range_opt())
        if tok == "money":
            unit = "円"
            if self.accept("("):
                unit = self.name()
                self.expect(")")
            return Type("金額", range=self.range_opt(), unit=unit)
        if tok == "ref":  # ref Data
            return Type("参照", target=self.name())
        if self.accept("→"):
            return Type("写像", key=tok, value=self.type())
        return Type("名前", target=tok)

    # ---- 状態 ----
    def state(self, line) -> State:
        s = State(self.name(), line=line)
        self.block_open()
        while not self.accept("}"):
            kw = self.name()
            if kw == "initial":
                s.initial = self.name()
            elif kw == "middle":
                s.middle += self.comma_list(self.name)
            elif kw == "final":
                s.final += self.comma_list(self.name)
            elif kw == "endless":
                s.endless = True
            elif kw == "reject":
                a = self.name()
                self.expect("→")
                b = self.name()
                self.expect("reason")
                t = self.next()
                s.rejects.append((a, b, t.val))
            else:
                raise self.err(f"state の中に「{kw}」は書けません")
            self.end_line()
        return s

    # ---- 結果 ----
    def result_def(self, line) -> ResultDef:
        name = self.name()
        self.expect("{")
        variants = []
        self.skip_nl()
        while not self.accept("}"):
            v = self.name()
            payload = []
            if self.accept("("):
                payload = self.comma_list(self.type)
                self.expect(")")
            variants.append((v, payload))
            self.accept(",")
            self.skip_nl()
        self.end_line()
        return ResultDef(name, variants, line)

    # ---- 外部作用・部品・不変条件 ----
    def dotted(self) -> str:
        a = self.name()
        if self.accept("."):
            a += "." + self.name()
        return a

    def effect_decl(self, line) -> EffectDecl:
        name = self.dotted()
        self.expect("(")
        params = self.comma_list(lambda: (self.name(), (self.expect(":"), self.type())[1]))
        self.expect(")")
        failures = []
        if self.accept("failures"):
            failures = self.comma_list(self.name)
        no_wait = None
        if self.accept("no_wait"):
            t = self.next()
            if t.kind != "STR":
                raise self.err("no_wait の後ろには、理由を \"…\" で書いてください")
            no_wait = t.val
        self.end_line()
        return EffectDecl(name, params, line, failures, no_wait)

    def part(self, line) -> Part:
        name = self.name()
        raw = []
        while self.peek().kind not in ("NL", "EOF"):
            raw.append(self.next().val)
        self.end_line()
        return Part(name, " ".join(raw), line)

    def invariant(self, line) -> Invariant:
        d = self.name()
        self.expect(":")
        c = self.expr()
        if self.accept("implies"):
            c = Implies(c, self.expr())
        self.end_line()
        return Invariant(d, c, line)

    # ---- 処理（deed） ----
    def access(self) -> str:
        return self.dotted()

    DECLS = ("input", "slot", "reads", "writes", "transition", "result", "effects")

    def process(self, line) -> Process:
        p = Process(self.name(), line=line)
        self.block_open()
        while not self.accept("}"):
            if self.peek().kind == "NAME" and self.peek().val in self.DECLS:
                self.decl(p)
            else:
                p.body.append(self.stmt())  # 宣言でなければ本体の文（body ブロックは無い）
        return p

    def decl(self, p: Process):
        kw = self.name()
        if kw in ("input", "slot"):
            items = self.comma_list(lambda: (self.name(), (self.expect(":"), self.type())[1]))
            (p.inputs if kw == "input" else p.refs).extend(items)
        elif kw == "reads":
            p.reads += self.comma_list(self.access)
        elif kw == "writes":
            p.writes += self.comma_list(self.access)
        elif kw == "transition":
            d = self.name()
            self.expect(":")
            if self.accept("("):
                self.name()
                self.expect(")")
                src = "(新規)"
            else:
                src = self.name()
            self.expect("→")
            p.transitions.append((d, src, self.name()))
        elif kw == "result":
            p.result = self.name()
        else:  # effects
            if not self.accept("none"):
                p.effects += self.comma_list(self.dotted)
        self.end_line()

    # ---- 文 ----
    def ret(self) -> Return:
        line = self.peek().line
        self.expect("return")
        v = self.name()
        args = []
        if self.accept("("):
            args = self.comma_list(self.expr)
            self.expect(")")
        return Return(v, args, line)

    def ret_block(self) -> Return:
        """「{ return X }」。1行でも、改行して書いてもよい"""
        self.expect("{")
        self.skip_nl()
        r = self.ret()
        self.skip_nl()
        self.expect("}")
        return r

    def end_stmt(self):
        """文の終わり。1行に書いた「{ … }」の中では、} の手前で終わってよい"""
        if not self.is_("}"):
            self.end_line()

    def if_block(self) -> list:
        """「{ 文… }」。1行でも、改行して書いてもよい"""
        self.expect("{")
        self.skip_nl()
        body = []
        while not self.accept("}"):
            body.append(self.stmt())
            self.skip_nl()
        return body

    def binder(self) -> Binder:
        if self.accept("("):
            names = self.comma_list(self.name)
            self.expect(")")
        else:
            names = [self.name()]
        self.expect("in")
        return Binder(names, self.postfix())

    def stmt(self):
        line = self.peek().line
        if self.is_("return"):
            s = self.ret()
        elif self.accept("if"):
            cond = self.expr()
            s = If(cond, self.if_block(), line)
        elif self.accept("update"):
            self.expect("{")
            target = self.postfix()
            if self.peek().val not in ("-=", "+=", "="):
                raise self.err("update の演算子（-= += =）が必要です")
            op = self.next().val
            value = self.expr()
            self.expect("|")
            b = self.binder()
            cond = self.expr() if self.accept(",") else None
            self.expect("}")
            s = Update(target, op, value, b, cond, line)
        elif self.accept("for"):
            b = self.binder()
            self.block_open()
            body = []
            while not self.accept("}"):
                body.append(self.stmt())
            s = ForEach(b, body, line)
        elif self.accept("request"):
            eff = self.dotted()
            self.expect("(")
            args, idem = [], None
            while True:
                if self.is_("idempotency_key") and self.is_("=", 1):
                    self.next()
                    self.next()
                    idem = self.expr()
                else:
                    args.append(self.expr())
                if not self.accept(","):
                    break
            self.expect(")")
            s = Request(eff, args, idem, line)
        elif self.accept("let"):
            n = self.name()
            self.expect("=")
            if self.accept("create"):
                d = self.name()
                self.expect("{")
                fields = self.comma_list(lambda: (self.name(), (self.expect(":"), self.expr())[1]))
                self.expect("}")
                s = Create(n, d, fields, line)
            else:
                e = self.expr()
                absent = None
                if self.accept("else"):
                    absent = self.ret_block()
                s = Assign(n, e, absent, line)
        else:
            n = self.name()
            if not self.accept("."):
                raise self.err(f"文の始まりが分かりません（「{n}」。変数は let で作り、項目の更新は 名前.項目 = 値 と書きます）")
            f = self.name()
            self.expect("=")
            s = SetField(n, f, self.expr(), line)
        self.end_stmt()
        return s

    # ---- 式 ----
    def expr(self):
        l = self.cmp_expr()
        while self.accept("and"):
            l = Bin("かつ", l, self.cmp_expr())
        return l

    def cmp_expr(self):
        l = self.add()
        if self.peek().kind == "OP" and self.peek().val in CMP:
            op = self.next().val
            return Bin(op, l, self.add())
        if self.accept("is"):
            kw = self.name()
            if kw not in PRED:
                raise self.err(f"「is」の後ろに「{kw}」は書けません（{'・'.join(PRED)}）")
            return Pred(l, PRED[kw])
        return l

    def add(self):
        l = self.mul()
        while self.peek().kind == "OP" and self.peek().val in ("+", "-"):
            op = self.next().val
            l = Bin(op, l, self.mul())
        return l

    def mul(self):
        l = self.postfix()
        while self.accept("*"):
            l = Bin("*", l, self.postfix())
        return l

    def postfix(self):
        e = self.primary()
        while True:
            if self.accept("."):
                e = Attr(e, self.name())
            elif self.is_("["):
                if not isinstance(e, Name):
                    raise self.err("[ ] の前にはデータの名前が必要です")
                self.next()
                k = self.expr()
                self.expect("]")
                e = Index(e.n, k)
            else:
                return e

    def builder(self):
        self.expect("{")
        elem = self.expr()
        self.expect("|")
        b = self.binder()
        cond = self.expr() if self.accept(",") else None
        self.expect("}")
        return elem, b, cond

    def primary(self):
        t = self.peek()
        if t.kind == "NUM":
            self.next()
            return Num(int(t.val))
        if self.accept("("):
            e = self.expr()
            self.expect(")")
            return e
        if self.is_("sum") and self.is_("{", 1):
            self.next()
            return SumE(*self.builder())
        if self.is_("{"):
            return SetB(*self.builder())
        if t.kind == "NAME":
            self.next()
            if self.accept("("):
                args = self.comma_list(self.expr)
                self.expect(")")
                return Call(t.val, args)
            return Name(t.val)
        raise self.err(f"式のはずが「{t.val or '行末'}」があります")

    # ---- ネクサス（nexus。flow は別名） ----
    def flow(self, line) -> Flow:
        name = self.name()
        self.block_open()
        entries = []
        while not self.accept("}"):
            proc = self.name()
            self.expect(":")
            routes = []
            while True:
                v = self.name()
                self.expect("→")
                d = self.name()
                if d == "reply":
                    d = "応答"
                elif d == "wait":
                    d = "待つ " + self.name()
                routes.append((v, d))
                if not self.accept(","):
                    break
                self.skip_nl()
            entries.append((proc, routes))
            self.end_line()
        return Flow(name, entries, line)

    def unit(self) -> str:
        unit = self.name()
        if unit not in UNITS:
            raise self.err(f"時間の単位「{unit}」は使えません（{UNIT_WORDS}）")
        return unit

    # ---- 問い合わせ（読むだけの処理） ----
    FILTER_OPS = {"at_least": "以上", "at_most": "以下", "contains": "含む"}
    ORDERS = {"newest": "新しい順", "oldest": "古い順"}

    def query(self, line) -> Query:
        q = Query(self.name(), line=line)
        self.block_open()

        def typed():
            n = self.name()
            self.expect(":")
            t = self.type()
            if self.accept("optional"):
                t.optional = True
            return n, t

        while not self.accept("}"):
            kw = self.name()
            if kw == "input":
                q.inputs += self.comma_list(typed)
            elif kw == "slot":
                q.refs += self.comma_list(typed)
            elif kw == "from":
                q.target = self.name()
            elif kw == "where":
                group = []
                while True:
                    f = self.name()
                    op = self.next().val
                    if op not in CMP:
                        raise self.err("where は「項目 == 状態」「項目 != 状態」「項目 >= 数」の形です（or でつなげます）")
                    if self.peek().kind == "NUM":
                        group.append((f, op, str(self.num())))
                    elif op in ("==", "!="):
                        group.append((f, op, self.name()))
                    else:
                        raise self.err(f"「{op}」は数との比較にだけ使えます")
                    if not self.accept("or"):
                        break
                q.conds.append(group)
            elif kw == "filter":
                f = self.name()
                op = self.name()
                if op == "within":
                    n = self.num()
                    q.filters.append((f, "直近", str(n * UNITS[self.unit()])))
                elif op in self.FILTER_OPS:
                    q.filters.append((f, self.FILTER_OPS[op], self.name()))
                else:
                    raise self.err(f"filter の種類「{op}」は使えません（at_least・at_most・contains・within）")
            elif kw == "order":
                f = self.name()
                d = self.name()
                if d not in self.ORDERS:
                    raise self.err("order は「newest」か「oldest」です")
                q.order = (f, self.ORDERS[d])
            elif kw == "limit":
                q.limit = self.name()
            elif kw == "reads":
                q.reads += self.comma_list(self.access)
            else:
                raise self.err(f"query の中に「{kw}」は書けません")
            self.end_line()
        return q

    # ---- 出来事（外の出来事をきっかけに処理を起こす） ----
    def event(self, line) -> Event:
        ev = Event(self.name(), line=line)
        self.block_open()
        while not self.accept("}"):
            kw = self.name()
            if kw == "each":
                q = self.name()
                self.expect("→")
                ev.steps.append(("各", q, self.name()))
            elif kw == "external":
                self.expect("→")
                ev.steps.append(("外から", "", self.name()))
            else:
                raise self.err("event の中は「each 問い合わせ → 処理」か「external → 処理」です")
            self.end_line()
        return ev

    # ---- トリガ ----
    def trigger(self, line) -> Trigger:
        name = self.name()
        self.expect("(")
        params = self.comma_list(lambda: (self.name(), (self.expect(":"), self.type())[1]))
        self.expect(")")
        self.block_open()
        causes = []
        while not self.accept("}"):
            kind = self.name()
            if kind == "reply":
                kind, key = "応答", self.name()
            elif kind == "deadline":
                kind = "期限"
                if self.accept("now"):  # 「now + 30 min」。now は動的具体化の【参照】
                    self.expect("+")
                n = self.num()
                key = str(n * UNITS[self.unit()])
            else:
                raise self.err(f"トリガの要因は「reply」か「deadline」です（「{kind}」は使えません）")
            self.expect("→")
            causes.append((kind, key, self.name()))
            self.end_line()
        return Trigger(name, params, causes, line)


def parse(src: str) -> Program:
    return Parser(lex(src)).program()
