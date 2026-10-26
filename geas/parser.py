"""構文解析（再帰下降）。文は1行1つ。ブロックは「{ で行が終わり、} だけの行で閉じる」。"""
from .ast import *  # noqa: F401,F403
from .lexer import Tok, lex


class ParseError(Exception):
    def __init__(self, msg: str, line: int):
        super().__init__(f"{line}行目: {msg}")
        self.line = line


CMP = ("<", ">", "<=", ">=", "==", "!=")


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
            if kw == "データ":
                p.data.append(self.data(line))
            elif kw == "状態":
                p.states.append(self.state(line))
            elif kw == "結果":
                p.results.append(self.result_def(line))
            elif kw == "外部作用":
                p.effects.append(self.effect_decl(line))
            elif kw == "部品":
                p.parts.append(self.part(line))
            elif kw == "不変条件":
                p.invariants.append(self.invariant(line))
            elif kw == "処理定義":
                p.processes.append(self.process(line))
            elif kw == "フロー":
                p.flows.append(self.flow(line))
            elif kw == "トリガ":
                p.triggers.append(self.trigger(line))
            elif kw == "問い合わせ":
                p.queries.append(self.query(line))
            elif kw == "出来事":
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
        store = self.name() if self.accept("保存先") else None
        self.block_open()
        fields = []
        while not self.accept("}"):
            fname = self.name()
            self.expect(":")
            t = self.type()
            f = Field(fname, t)
            while self.peek().kind == "NAME" and self.peek().val in ("一意", "任意", "必須"):
                m = self.name()
                if m == "一意":
                    f.unique = True
                elif m == "任意":
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
        if tok == "ID":
            if self.accept("の集合"):
                return Type("集合", value=Type("ID"))
            return Type("ID")
        if tok in ("文字列", "日時"):
            return Type(tok)
        if tok == "バイト列":  # 範囲は長さ（バイト数）
            return Type(tok, range=self.range_opt())
        if tok in ("整数", "割合"):
            return Type(tok, range=self.range_opt())
        if tok == "金額":
            unit = "円"
            if self.accept("("):
                unit = self.name()
                self.expect(")")
            return Type("金額", range=self.range_opt(), unit=unit)
        if self.accept("を参照"):
            return Type("参照", target=tok)
        if self.accept("→"):
            return Type("写像", key=tok, value=self.type())
        return Type("名前", target=tok)

    # ---- 状態 ----
    def state(self, line) -> State:
        s = State(self.name(), line=line)
        self.block_open()
        while not self.accept("}"):
            kw = self.name()
            if kw == "初期":
                s.initial = self.name()
            elif kw == "中間":
                s.middle += self.comma_list(self.name)
            elif kw == "終了":
                names = self.comma_list(self.name)
                if names == ["なし"]:
                    s.endless = True
                else:
                    s.final += names
            elif kw == "拒否":
                a = self.name()
                self.expect("→")
                b = self.name()
                self.expect("理由")
                t = self.next()
                s.rejects.append((a, b, t.val))
            else:
                raise self.err(f"状態の中に「{kw}」は書けません")
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
        if self.accept("失敗"):
            failures = self.comma_list(self.name)
        no_wait = None
        if self.accept("待たない"):
            t = self.next()
            if t.kind != "STR":
                raise self.err("待たない の後ろには、理由を \"…\" で書いてください")
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
        if self.accept("なら"):
            c = Implies(c, self.expr())
        self.end_line()
        return Invariant(d, c, line)

    # ---- 処理定義 ----
    def access(self) -> str:
        return self.dotted()

    def process(self, line) -> Process:
        p = Process(self.name(), line=line)
        self.block_open()
        while not self.accept("}"):
            kw = self.name()
            if kw in ("入力", "参照"):
                items = self.comma_list(lambda: (self.name(), (self.expect(":"), self.type())[1]))
                (p.inputs if kw == "入力" else p.refs).extend(items)
            elif kw == "読む":
                p.reads += self.comma_list(self.access)
            elif kw == "書く":
                p.writes += self.comma_list(self.access)
            elif kw == "遷移":
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
            elif kw == "結果":
                p.result = self.name()
            elif kw == "外部":
                if not self.accept("なし"):
                    p.effects += self.comma_list(self.dotted)
            elif kw == "本体":
                self.block_open()
                while not self.accept("}"):
                    p.body.append(self.stmt())
            else:
                raise self.err(f"処理定義の中に「{kw}」は書けません")
            self.end_line()
        return p

    # ---- 文 ----
    def ret(self) -> Return:
        line = self.peek().line
        self.expect("返す")
        v = self.name()
        args = []
        if self.accept("("):
            args = self.comma_list(self.expr)
            self.expect(")")
        return Return(v, args, line)

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
        if self.is_("返す"):
            s = self.ret()
        elif self.accept("もし"):
            cond = self.expr()
            self.expect("なら")
            s = If(cond, self.ret(), line)
        elif self.accept("更新"):
            self.expect("{")
            target = self.postfix()
            if self.peek().val not in ("-=", "+=", "="):
                raise self.err("更新の演算子（-= += =）が必要です")
            op = self.next().val
            value = self.expr()
            self.expect("|")
            b = self.binder()
            cond = self.expr() if self.accept(",") else None
            self.expect("}")
            s = Update(target, op, value, b, cond, line)
        elif self.accept("各"):
            b = self.binder()
            self.block_open()
            body = []
            while not self.accept("}"):
                body.append(self.stmt())
            s = ForEach(b, body, line)
        elif self.accept("要求"):
            eff = self.dotted()
            self.expect("(")
            args, idem = [], None
            while True:
                if self.is_("冪等キー") and self.is_("=", 1):
                    self.next()
                    self.next()
                    idem = self.expr()
                else:
                    args.append(self.expr())
                if not self.accept(","):
                    break
            self.expect(")")
            s = Request(eff, args, idem, line)
        else:
            n = self.name()
            if self.accept("."):
                f = self.name()
                self.expect("=")
                s = SetField(n, f, self.expr(), line)
            else:
                self.expect("=")
                if self.accept("新規作成"):
                    d = self.name()
                    self.expect("{")
                    fields = self.comma_list(lambda: (self.name(), (self.expect(":"), self.expr())[1]))
                    self.expect("}")
                    s = Create(n, d, fields, line)
                else:
                    e = self.expr()
                    absent = None
                    if self.accept("なければ"):
                        absent = self.ret()
                    s = Assign(n, e, absent, line)
        self.end_line()
        return s

    # ---- 式 ----
    def expr(self):
        l = self.cmp_expr()
        while self.accept("かつ"):
            l = Bin("かつ", l, self.cmp_expr())
        return l

    def cmp_expr(self):
        l = self.add()
        if self.peek().kind == "OP" and self.peek().val in CMP:
            op = self.next().val
            return Bin(op, l, self.add())
        if self.accept("が"):
            kw = self.name()
            if kw not in ("空", "空でない", "ある", "ない"):
                raise self.err(f"「が」の後ろに「{kw}」は書けません")
            return Pred(l, kw)
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
        if self.accept("Σ"):
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

    # ---- フロー ----
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
                if d == "待つ":
                    d = "待つ " + self.name()
                routes.append((v, d))
                if not self.accept(","):
                    break
                self.skip_nl()
            entries.append((proc, routes))
            self.end_line()
        return Flow(name, entries, line)

    # ---- 問い合わせ（読むだけの処理） ----
    def query(self, line) -> Query:
        q = Query(self.name(), line=line)
        self.block_open()

        def typed():
            n = self.name()
            self.expect(":")
            t = self.type()
            if self.accept("任意"):
                t.optional = True
            return n, t

        while not self.accept("}"):
            kw = self.name()
            if kw == "入力":
                q.inputs += self.comma_list(typed)
            elif kw == "参照":
                q.refs += self.comma_list(typed)
            elif kw == "対象":
                q.target = self.name()
            elif kw == "条件":
                f = self.name()
                op = self.next().val
                if op not in ("==", "!="):
                    raise self.err("問い合わせの条件は「項目 == 状態」か「項目 != 状態」の形です")
                q.conds.append((f, op, self.name()))
            elif kw == "絞り込み":
                f = self.name()
                op = self.name()
                if op == "直近":
                    n = self.num()
                    unit = self.name()
                    if unit not in self.UNITS:
                        raise self.err(f"時間の単位「{unit}」は使えません（秒・分・時間・日）")
                    q.filters.append((f, op, str(n * self.UNITS[unit])))
                elif op in ("以上", "以下", "含む"):
                    q.filters.append((f, op, self.name()))
                else:
                    raise self.err(f"絞り込みの種類「{op}」は使えません（以上・以下・含む・直近）")
            elif kw == "並び":
                f = self.name()
                d = self.name()
                if d not in ("新しい順", "古い順"):
                    raise self.err("並びは「新しい順」か「古い順」です")
                q.order = (f, d)
            elif kw == "先頭":
                q.limit = self.name()
            elif kw == "読む":
                q.reads += self.comma_list(self.access)
            else:
                raise self.err(f"問い合わせの中に「{kw}」は書けません")
            self.end_line()
        return q

    # ---- 出来事（外の出来事をきっかけに処理を起こす） ----
    def event(self, line) -> Event:
        ev = Event(self.name(), line=line)
        self.block_open()
        while not self.accept("}"):
            kw = self.name()
            if kw == "各":
                q = self.name()
                self.expect("→")
                ev.steps.append(("各", q, self.name()))
            elif kw == "外から":
                ev.steps.append(("外から", "", self.name()))
            else:
                raise self.err("出来事の中は「各 問い合わせ → 処理」か「外から 処理」です")
            self.end_line()
        return ev

    # ---- トリガ ----
    UNITS = {"秒": 1, "分": 60, "時間": 3600, "日": 86400}

    def trigger(self, line) -> Trigger:
        name = self.name()
        self.expect("(")
        params = self.comma_list(lambda: (self.name(), (self.expect(":"), self.type())[1]))
        self.expect(")")
        self.block_open()
        causes = []
        while not self.accept("}"):
            kind = self.name()
            if kind == "応答":
                key = self.name()
            elif kind == "期限":
                if self.accept("現在"):  # 「現在 + 30 分」。現在は動的具体化の【参照】
                    self.expect("+")
                n = self.num()
                unit = self.name()
                if unit not in self.UNITS:
                    raise self.err(f"時間の単位「{unit}」は使えません（秒・分・時間・日）")
                key = str(n * self.UNITS[unit])
            else:
                raise self.err(f"トリガの要因は「応答」か「期限」です（「{kind}」は使えません）")
            self.expect("→")
            causes.append((kind, key, self.name()))
            self.end_line()
        return Trigger(name, params, causes, line)


def parse(src: str) -> Program:
    return Parser(lex(src)).program()
