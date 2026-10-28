# SPDX-License-Identifier: GPL-3.0-or-later
"""構文解析（再帰下降）。文は1行1つ。ブロックは「{ で行が終わり、} だけの行で閉じる」。
キーワードは英語。構文木の中身（型の種類や結果の行き先など）は、仕様書の言葉に合わせた日本語のまま持つ。"""
import copy

from .ast import *  # noqa: F401,F403
from .lexer import Tok, lex
from .modules import ModuleError, expand, statement_starts


def parse_labels(doc: str) -> dict:
    """説明コメントの表示名。`@ja 注文` のように言語ごとに書く。タグが無ければ、全体を既定（空の言語名）の表示名とする"""
    out: dict[str, str] = {}
    plain: list[str] = []
    for ln in doc.split("\n"):
        if ln.startswith("@"):
            tag, _, text = ln[1:].partition(" ")
            out[tag] = text.strip()
        elif ln:
            plain.append(ln)
    return ({"": " ".join(plain)} if plain else {}) | out


# 論理型の名前に使えない、組み込みの型の名前
BUILTIN_TYPE_NAMES = {"id", "set", "text", "time", "bytes", "int", "ratio", "money", "ref", "vtime"}


def virtual_datetime() -> Type:
    """組み込みの仮想日時：年・月・日・時・分・秒の成分を持つ合成型で、成分は未指定にできる（実在する日時ではない）。
    成分の範囲は構造の上限だけ（月 1..12 など）で、暦として正しいか（2月30日など）や意味は、アプリの規則で決める"""
    comp = lambda lo, hi: Type("整数", range=Range(lo, hi), optional=True)
    return Type("合成", target="vtime", parts=(
        ("year", comp(1, 9999)), ("month", comp(1, 12)), ("day", comp(1, 31)), ("hour", comp(0, 23)), ("minute", comp(0, 59)), ("second", comp(0, 59))))


# 合成型の成分に使える型の種類（値の型）
COMPONENT_KINDS = ("ID", "整数", "金額", "割合", "文字列", "日時", "バイト列", "合成")


class ParseError(Exception):
    def __init__(self, msg: str, line: int):
        super().__init__(f"{line}行目: {msg}")
        self.line = line


CMP = ("<", ">", "<=", ">=", "==", "!=")
UNITS = {"sec": 1, "min": 60, "hour": 3600}  # day・week・month・year は暦で数え方が変わる（人の「1日後」は86400秒後と限らない）ので、論理日時を設計するまで入れない
CAL_MSG = "「{}」は暦で数え方が変わる（夏時間・月の長さ・タイムゾーン）ので、まだ使えません。ちょうど何時間か何秒かなら hour か sec で書いてください（24 hour など）"
CALENDAR_UNITS = ("day", "week", "month", "year")
MAX_DUR = 10**10  # 期間の上限（約317年）。時刻の足し引きがあふれないようにする
UNIT_WORDS = "sec・min・hour"
SIMPLE_TYPES = {"text": "文字列", "time": "日時", "bytes": "バイト列", "int": "整数", "ratio": "割合", "money": "金額"}
PRED = {"empty": "空", "nonempty": "空でない", "present": "ある", "absent": "ない", "null": "null"}


class Parser:
    def __init__(self, toks: list[Tok]):
        self.t = toks
        self.i = 0
        self.ltypes: dict[str, Type] = {}
        self.in_ltype = False  # ltype の右辺の中だけ、範囲や組み合わせを直に書ける
        self.composites: set[str] = {"vtime"}  # 合成型の名前（Make の見分けに使う）
        self.used_builtin: set[str] = set()  # 使われた組み込みの合成型

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
        stores: list[tuple[str, str, int]] = []
        self.skip_nl()
        while self.peek().kind != "EOF":
            line = self.peek().line
            kw = self.name()
            if kw == "entity":
                p.data.append(self.data(line))
            elif kw == "data":
                raise ParseError("data は entity に改名しました（entity 名 { … } と書きます）", line)
            elif kw == "ltype":
                self.ltype(line)
            elif kw == "store":
                stores.append(self.store_decl(line))
            elif kw == "state":
                p.states.append(self.state(line))
            elif kw == "result":
                p.results.append(self.result_def(line))
            elif kw == "effect":
                p.effects.append(self.effect_decl(line))
            elif kw == "part":
                p.parts.append(self.part(line))
            elif kw == "convert":
                p.parts.append(self.convert(line))
            elif kw == "invariant":
                p.invariants.append(self.invariant(line))
            elif kw == "deed":
                p.processes.append(self.process(line))
            elif kw in ("nexus", "flow"):  # flow は nexus の別名（糖衣構文）
                p.flows.append(self.flow(line))
            elif kw == "reply":  # reply 名前(中身の名前: 型 [required], …)：応答が運ぶ中身の宣言
                p.replies.append(self.reply_def(line))
            elif kw == "trigger":
                p.triggers.append(self.trigger(line))
            elif kw == "query":
                p.queries.append(self.query(line))
            elif kw == "event":
                p.events.append(self.event(line))
            elif kw == "screen":
                p.screens.append(self.screen(line))
            elif kw == "resource":
                p.resources.append(self.resource(line))
            elif kw == "app":  # app "名前"：アプリの名前（画面の見出しとインストール名）
                t = self.peek()
                if t.kind != "STR" or not t.val:
                    raise ParseError('app は `app "アプリの名前"` と書きます', line)
                self.i += 1
                p.title = t.val
                self.end_line()
            else:
                raise ParseError(f"未知の定義「{kw}」です", line)
            self.skip_nl()
        p.ltype_names = list(self.ltypes)
        p.composites = [t for t in self.ltypes.values() if t.kind == "合成" and t.target in self.composites]
        if "vtime" in self.used_builtin:
            p.composites.insert(0, virtual_datetime())
        if "currency" in self.used_builtin:
            p.states.append(State("Currency", initial="JPY", middle=["USD", "EUR"], plain=True))
        self.apply_stores(p, stores)
        self.add_sync_state(p)
        return p

    def store_decl(self, line):
        """store データ名 in 保存先：データの保存先。データの宣言とは別に、要件に応じて書く（書かなければ保存しない）"""
        data = self.name()
        self.expect("in")
        where = self.name()
        self.end_line()
        return data, where, line

    def apply_stores(self, p: Program, stores):
        by_name = {d.name: d for d in p.data}
        for data, where, line in stores:
            if data not in by_name:
                raise ParseError(f"store の対象「{data}」という entity がありません", line)
            if by_name[data].store:
                raise ParseError(f"entity「{data}」の store が2つ以上あります", line)
            by_name[data].store = where

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

    # ---- 論理型 ----
    def ltype(self, line):
        """ltype 名前 = 型：範囲や組み合わせに名前を付ける。使う場所より前に書く（使った場所には定義が写される）"""
        name = self.name()
        if name in BUILTIN_TYPE_NAMES:
            raise ParseError(f"「{name}」は組み込みの型の名前なので、論理型の名前には使えません", line)
        if name in self.ltypes:
            raise ParseError(f"論理型「{name}」が2つ以上あります", line)
        if self.is_("{"):
            self.ltypes[name] = self.composite(name, line)
            self.composites.add(name)
            return
        self.expect("=")
        self.in_ltype = True
        t = self.type()
        self.in_ltype = False
        self.end_line()
        self.ltypes[name] = t

    def composite(self, name, line) -> Type:
        """ltype 名前 { 成分: 型 … }：成分は、名前をつけた論理型か約束のない組み込みの型。成分はすべて必須"""
        self.expect("{")
        parts = []

        def one():
            cl = self.peek().line
            pn = self.name()
            self.expect(":")
            pt = self.type()
            if self.accept("optional"):  # 未指定にできる成分。書かなければ必須
                pt.optional = True
            if pt.kind not in COMPONENT_KINDS:
                raise ParseError(f"合成型「{name}」の成分「{pn}」に、この型は使えません（使えるのは値の型：{'・'.join(COMPONENT_KINDS)}）", cl)
            if any(pn == x for x, _ in parts):
                raise ParseError(f"合成型「{name}」に成分「{pn}」が2つ以上あります", cl)
            parts.append((pn, pt))

        if self.peek().kind == "NL":  # 1行ずつ書く形
            self.end_line()
            while not self.accept("}"):
                one()
                self.end_line()
        else:  # { 成分: 型, 成分: 型 } と1行で書く形
            self.comma_list(one)
            self.expect("}")
            self.end_line()
        if not parts:
            raise ParseError(f"合成型「{name}」に成分がありません", line)
        return Type("合成", target=name, parts=tuple(parts))

    # ---- データ ----
    def data(self, line) -> Data:
        name = self.name()
        if self.is_("store"):
            raise self.err(f"保存先は entity の中ではなく、別の行で store {name} in 保存先 と書きます")
        self.block_open()
        fields = []
        keep, removable, on_delete = False, [], []
        while not self.accept("}"):
            if self.is_("keep") and not self.is_(":", 1):  # 削除させない
                self.next()
                keep = True
                self.end_line()
                continue
            if self.is_("removable") and not self.is_(":", 1):  # removable in 状態, …：消してよい状態
                self.next()
                self.expect("in")
                removable += self.comma_list(self.name)
                self.end_line()
                continue
            if self.is_("on") and self.is_("delete", 1) and not self.is_(":", 1):  # on delete 参照元.項目 → detach｜cascade｜leave
                self.next()
                self.next()
                src = self.name()
                self.expect(".")
                fld = self.name()
                self.expect("→")
                pol = self.name()
                if pol not in ("detach", "cascade", "leave"):
                    raise self.err("on delete の扱いは「detach」（参照を空にする）「cascade」（参照元も消す）「leave」（参照元はそのまま残す）です")
                on_delete.append((src, fld, pol))
                self.end_line()
                continue
            fname = self.name()
            self.expect(":")
            t = self.type()
            f = Field(fname, t)
            while self.peek().kind == "NAME" and self.peek().val in ("unique", "optional", "required"):
                m = self.name()
                if m == "unique":
                    f.unique = True
                elif m == "required":
                    f.required = True
                else:
                    raise self.err("項目は、書かなければ「無いかもしれない」（null になりうる）扱いです。optional は要りません（必須なら required と書きます）")
            fields.append(f)
            self.end_line()
        d = Data(name, fields, line)
        d.keep, d.removable, d.on_delete = keep, removable, on_delete
        return d

    def signed_num(self):
        """範囲の端：負の数も書ける（UTC からのずれなど）"""
        if self.accept("-"):
            return -self.num()
        return self.num()

    def range_opt(self):
        if self.peek().kind == "NUM" or self.is_("-"):
            lo = self.signed_num()
            self.expect("..")
            hi = self.signed_num() if self.peek().kind == "NUM" or self.is_("-") else None
            return Range(lo, hi)
        if self.is_(".."):
            self.next()
            return Range(None, self.signed_num())
        return None

    def chars_opt(self):
        """of "a-z0-9_"：- でつないだ範囲と、\\ で逃がした文字。小さい順につないで返す"""
        if not self.accept("of"):
            return None
        t = self.next()
        if t.kind != "STR" or not t.val:
            raise self.err('of のあとには、使える文字を "a-z0-9_" の形で書きます')
        s, out, i = t.val, [], 0
        def one():
            nonlocal i
            if s[i] == "\\":
                i += 1
                if i >= len(s):
                    raise self.err("文字種の末尾の「\\」のあとに文字がありません")
            c = ord(s[i])
            i += 1
            if 0xD800 <= c <= 0xDFFF:
                raise self.err("文字種に使えない文字です")
            return c
        while i < len(s):
            a = one()
            b = a
            if i + 1 < len(s) and s[i] == "-":
                i += 1
                b = one()
                if b < a:
                    raise self.err(f"文字種の範囲の向きが逆です（{chr(a)}-{chr(b)}）")
            out.append((a, b))
        out.sort()
        merged = [out[0]]
        for a, b in out[1:]:
            if a <= merged[-1][1] + 1:
                merged[-1] = (merged[-1][0], max(merged[-1][1], b))
            else:
                merged.append((a, b))
        return tuple(merged)

    def check_opt(self, rng, chars):
        """check gtin：JAN・EAN・UPC の最後の1桁（チェックディジット）が合っている文字列。長さは8・12・13・14桁、文字は数字だけ"""
        if not self.accept("check"):
            return None
        kind = self.name()
        if kind != "gtin":
            raise self.err(f"check に書けるのは gtin だけです（「{kind}」は使えません）")
        if rng is None or rng.lo != rng.hi or rng.lo not in (8, 12, 13, 14):
            raise self.err("check gtin の文字列は、長さを 8・12・13・14 のどれか1つに決めてください（例：text 13..13 of \"0-9\" check gtin）")
        if chars != ((48, 57),):
            raise self.err('check gtin の文字列は、文字種を of "0-9" にしてください')
        return kind

    def type(self) -> Type:
        line = self.peek().line
        named = self.peek().val in self.ltypes and not self.is_("→", 1)
        t = self._type()
        if not (self.in_ltype or named) and (t.kind in ("写像", "集合") or t.range is not None or t.chars is not None or t.check is not None):
            raise ParseError("範囲や組み合わせは、ltype で名前を付けてから使ってください（例：ltype 数量 = int 1..99）", line)
        return t

    def _type(self) -> Type:
        tok = self.name()
        if tok == "id":
            if self.accept("→"):  # id → 値：キーが entity のキーに限られない、読み取り専用の値の入れ物
                return Type("写像", key="id", value=self.type())
            return Type("ID")
        if tok == "set":  # set of id
            self.expect("of")
            self.expect("id")
            return Type("集合", value=Type("ID"))
        if tok == "text":  # 範囲は長さ（文字数）。of "a-z0-9_" は使える文字
            rng, chars = self.range_opt(), self.chars_opt()
            return Type("文字列", range=rng, chars=chars, check=self.check_opt(rng, chars))
        if tok == "time":
            return Type(SIMPLE_TYPES[tok])
        if tok == "bytes":  # 範囲は長さ（バイト数）
            return Type("バイト列", range=self.range_opt())
        if tok in ("int", "ratio"):
            return Type(SIMPLE_TYPES[tok], range=self.range_opt())
        if tok == "money":
            unit = "JPY"
            if self.accept("("):
                unit = self.name()
                if unit[0].islower():  # money(currency)：同じ入力・slot・項目・引数にある通貨コードの名前
                    unit = "$" + unit
                self.expect(")")
            return Type("金額", range=self.range_opt(), unit=unit)
        if tok == "currency":  # 通貨コード（JPY・USD・EUR）。遷移の無い列挙で、slot や項目に持てる
            self.used_builtin.add(tok)
            return Type("名前", target="Currency")
        if tok == "ref":  # ref Data
            return Type("参照", target=self.name())
        if self.accept("→"):
            return Type("写像", key=tok, value=self.type())
        if tok == "vtime":
            self.used_builtin.add(tok)
            return virtual_datetime()
        if tok in self.ltypes:
            return copy.deepcopy(self.ltypes[tok])
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

    def named_type(self):
        """名前: 型。結果の中身は名前つき（次の処理の入力と、名前と型で突き合わせる）"""
        n = self.name()
        if not self.accept(":"):
            raise self.err(f"結果の中身には名前を付けてください（「{n}」ではなく「名前: {n}」の形）")
        return n, self.type()

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
                payload = self.comma_list(self.named_type)
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
        joins = None
        if self.accept("joins"):  # joins トリガ名：この外部作用の返事は、そのトリガに合流する（トリガの引数は、要求する処理の入力から決まる）
            joins = self.name()
        no_wait = None
        if self.accept("no_wait"):
            t = self.next()
            if t.kind != "STR":
                raise self.err("no_wait の後ろには、理由を \"…\" で書いてください")
            no_wait = t.val
        self.end_line()
        return EffectDecl(name, params, line, failures, no_wait, joins)

    def part(self, line) -> Part:
        name = self.name()
        self.expect("(")
        params = self.comma_list(lambda: (self.name(), (self.expect(":"), self.type())[1]))
        self.expect(")")
        self.expect("→")
        ret = self.type()
        if not self.accept("guarantee"):
            raise self.err(f"部品 {name} に保証がありません（guarantee 式 を書きます。例：guarantee result >= x）")
        g = self.expr()
        self.end_line()
        return Part(name, params, ret, g, line)

    def convert(self, line) -> Part:
        """convert 名前(引数: 型) -> 型 = 式：論理型どうしの一方通行の変換。定義が geas の式なので、外の実装も保証も要らない"""
        name = self.name()
        self.expect("(")

        defaults = {}

        def param():
            n = self.name()
            self.expect(":")
            t = self.type()
            if self.accept("fill"):  # fill { 成分: 数, … }：未指定の成分を補うルール。補った成分は指定されている扱い
                self.expect("{")
                fs = self.comma_list(lambda: (self.name(), (self.expect(":"), self.fill_value())[1]))
                self.expect("}")
                t.fills = tuple(fs)
            if self.accept("="):  # = 式：デフォルト値。呼ぶとき省ける（標準のタイムゾーン tz_offset など）
                defaults[n] = self.expr()
            return n, t

        params = self.comma_list(param)
        self.expect(")")
        self.expect("→")
        ret = self.type()
        self.expect("=")
        body = self.expr()
        self.end_line()
        return Part(name, params, ret, None, line, body, defaults)

    def fill_value(self) -> int:
        t = self.next()
        if t.kind != "NUM":
            raise self.err("fill の値は整数です")
        return int(t.val)

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

    def input_item(self):
        """入力 名前: 型 [required]。入力も原則「無いかもしれない」（キーが無いのも null も同じ「無い」）。
        必ず要るものは required で、無ければ処理の手前（境界）で入力不正になる"""
        n = self.name()
        self.expect(":")
        t = self.type()
        if self.is_("optional"):
            raise self.err("入力は、書かなければ「無いかもしれない」扱いです。optional は要りません（必ず要るなら required と書きます）")
        t.optional = not self.accept("required")
        return n, t

    def decl(self, p: Process):
        kw = self.name()
        if kw == "input":
            p.inputs += self.comma_list(self.input_item)
        elif kw == "slot":
            p.refs += self.comma_list(lambda: (self.name(), (self.expect(":"), self.type())[1]))
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
                    idem = self.idem_key()
                else:
                    args.append(self.expr())
                if not self.accept(","):
                    break
            self.expect(")")
            s = Request(eff, args, idem, line)
        elif self.is_("delete") and self.peek(1).kind == "NAME" and not self.is_(".", 1):
            self.next()
            s = Delete(self.name(), line)
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
            if self.peek().val in ("+=", "-="):  # x.f += e は x.f = x.f + e（読む項目にも数える）。並列の枝で重なってよい可換な更新の印を残す
                op = self.next().val
                s = SetField(n, f, Bin(op[0], Attr(Name(n), f), self.expr()), line, op)
            else:
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
        while True:
            if self.accept("*"):
                l = Bin("*", l, self.postfix())
            elif self.is_("/") or self.is_("%"):  # 整数の割り算（切り捨て）と余り。割る数は正の整数の定数だけ（0で割る道を作らない）
                op = self.next().val
                t = self.peek()
                if t.kind != "NUM" or int(t.val) <= 0:
                    raise self.err(f"「{op}」の右は、正の整数の定数だけです（割る数が0になる道を作らないため）")
                self.next()
                l = Bin(op, l, Num(int(t.val)))
            else:
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

    def idem_key(self):
        """冪等キー。1つの値か、値の組 (a, b, …)"""
        if not self.is_("("):
            return self.expr()
        self.next()
        items = [self.expr()]
        while self.accept(","):
            items.append(self.expr())
        self.expect(")")
        return items[0] if len(items) == 1 else Tuple(items)

    def primary(self):
        t = self.peek()
        if t.kind == "NUM":
            self.next()
            nxt = self.peek()
            if nxt.kind == "NAME" and nxt.val in CALENDAR_UNITS:
                raise self.err(CAL_MSG.format(nxt.val))
            if nxt.kind == "NAME" and nxt.val in UNITS:  # 30 min：期間（時刻に足し引きする）
                self.next()
                secs = int(t.val) * UNITS[nxt.val]
                if secs > MAX_DUR:
                    raise self.err(f"期間が長すぎます（{t.val} {nxt.val}。{MAX_DUR} 秒まで）")
                return Dur(secs, f"{t.val} {nxt.val}")
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
        if t.kind == "NAME" and t.val == "null":
            self.next()
            return Null()
        if t.kind == "NAME" and t.val in self.composites and self.is_("{", 1) and self.peek(2).kind == "NAME" and self.is_(":", 3):
            self.next()
            self.next()
            fields = self.comma_list(lambda: (self.name(), (self.expect(":"), self.expr())[1]))
            self.expect("}")
            return Make(t.val, fields, t.line)
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
                if any(v == x for x, _ in routes):
                    raise self.err(f"結果「{v}」の行き先が2つ以上あります。並列に起こすなら par {{ 処理, 処理 }} と書きます")
                d = self.name()
                if d == "par":  # 並列に起こす（実行は書いた順）。仕様書にも並列として出る
                    self.expect("{")
                    ds = self.comma_list(self.name)
                    self.expect("}")
                    if len(ds) < 2:
                        raise self.err("par には2つ以上の処理を書きます")
                    if "wait" in ds:
                        raise self.err("wait は par に入れられません（待つ行き先は、ほかの行き先と並べられません）")
                    ds = ["reply" if x == "reply" else x for x in ds]
                    routes.extend((v, x) for x in ds)
                    if not self.accept(","):
                        break
                    self.skip_nl()
                    continue
                if d == "reply":
                    d = "reply"
                elif d == "wait":
                    d = "wait " + self.name()
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
            if unit in CALENDAR_UNITS:
                raise self.err(CAL_MSG.format(unit))
            raise self.err(f"時間の単位「{unit}」は使えません（{UNIT_WORDS}）")
        return unit

    # ---- 問い合わせ（読むだけの処理） ----
    FILTER_OPS = {"at_least": "以上", "at_most": "以下", "equals": "一致"}
    FOLDS = {"ignore_case": "大小無視", "ignore_kana": "かな無視", "ignore_width": "幅無視"}
    ORDERS = {"newest": "新しい順", "oldest": "古い順"}

    def query(self, line) -> Query:
        q = Query(self.name(), line=line)
        self.block_open()

        while not self.accept("}"):
            kw = self.name()
            if kw == "input":
                q.inputs += self.comma_list(self.input_item)
            elif kw == "slot":
                q.refs += self.comma_list(lambda: (self.name(), (self.expect(":"), self.type())[1]))
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
                elif op == "contains":  # 既定は完全一致の部分文字列。同一視する軸は、書いたものだけ
                    arg = self.name()
                    folds = []
                    while self.peek().kind == "NAME" and self.peek().val in self.FOLDS:
                        folds.append(self.FOLDS[self.name()])
                    q.filters.append((f, "含む" + (f"({','.join(folds)})" if folds else ""), arg))
                elif op in self.FILTER_OPS:
                    q.filters.append((f, self.FILTER_OPS[op], self.name()))
                else:
                    raise self.err(f"filter の種類「{op}」は使えません（equals・at_least・at_most・contains・within）")
            elif kw == "order":
                f = self.name()
                d = self.name()
                if d not in self.ORDERS:
                    raise self.err("order は「newest」か「oldest」です")
                q.order = (f, self.ORDERS[d])
            elif kw == "page":  # 1ページ分（最大 N 件）。N は数か、整数の入力の名前
                if self.peek().kind == "NUM":
                    q.page_n = self.num()
                    if q.page_n < 1:
                        raise self.err("page の件数は 1 以上にしてください")
                else:
                    q.page = self.name()
            elif kw == "limit":
                raise self.err("limit は page に替わりました（page 20 か page 入力の名前。order が要ります）")
            elif kw == "select":
                q.select += self.comma_list(self.column)
            elif kw == "reads":
                q.reads += self.comma_list(self.access)
            else:
                raise self.err(f"query の中に「{kw}」は書けません")
            self.end_line()
        return q

    def column(self) -> Column:
        """select の1列：項目／参照の項目.先の項目／sum(写像の項目)／count(写像の項目)（as 名前 で列名を決める）"""
        first = self.name()
        if first in ("sum", "count") and self.is_("("):
            self.next()
            f = self.name()
            self.expect(")")
            col = Column(f + ("合計" if first == "sum" else "件数"), "合計" if first == "sum" else "件数", f)
        elif self.accept("."):
            f = self.name()
            col = Column(f"{first}_{f}", "参照先", f, first)
        else:
            col = Column(first, "項目", first)
        if self.accept("as"):
            col.name = self.name()
        return col

    # ---- 出来事（外の出来事をきっかけに処理を起こす） ----
    def event(self, line) -> Event:
        ev = Event(self.name(), line=line)
        if self.accept("on"):
            ev.on = self.name()
            if ev.on not in ("online", "opened"):
                raise self.err("event の「on」は、online（つながりが戻った）か opened（アプリを開いた）です")
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

    # ---- 画面 ----
    def screen(self, line) -> Screen:
        """screen 名前(受け取る値) [entry] { show 問い合わせ / open 画面 / field 入力 / action 処理 { 結果 → 行き先 } / page "URLのパターン", … / design { 指定 } }"""
        sc = Screen(self.name(), line=line)
        if self.accept("("):
            sc.params = self.comma_list(self.input_item)
            self.expect(")")
        sc.entry = self.accept("entry")
        self.block_open()
        while not self.accept("}"):
            kw = self.name()
            if kw == "show":
                sc.shows.append(self.name())
            elif kw == "open":
                sc.opens.append(self.name())
            elif kw == "field":
                def item():
                    f = self.name()
                    sc.fields.append(f)
                    if self.accept("from"):
                        sc.field_from[f] = self.name()
                self.comma_list(item)
            elif kw == "slot":
                sc.slots += self.comma_list(self.name)
            elif kw == "action":
                deed = self.name()
                self.block_open()
                routes = []
                while not self.accept("}"):
                    v = self.name()
                    self.expect("→")
                    routes.append((v, self.name()))
                    self.end_line()
                sc.actions.append((deed, routes))
            elif kw == "page":
                def pat():
                    t = self.next()
                    if t.kind != "STR":
                        raise ParseError("page には、ページの URL のパターン（\"*://www.example.com/*\" のような文字列）を書いてください", t.line)
                    sc.pages.append(t.val)
                self.comma_list(pat)
            elif kw == "design":
                self.block_open()
                while not self.accept("}"):
                    k = self.name()
                    t = self.next()
                    if t.kind not in ("NAME", "STR", "NUM"):
                        raise ParseError(f"design の「{k}」の値が必要です", t.line)
                    sc.design[k] = t.val
                    self.end_line()
            else:
                raise self.err("screen の中は「show」「open」「field」「slot」「action」「page」「design」です")
            self.end_line()
        return sc

    def resource(self, line) -> Resource:
        """resource 名前 [under 親] [key 名前] { index 問い合わせ / show 問い合わせ / create 処理 { 結果 → 状態 } / update 処理 {…} / destroy 処理 {…} }"""
        rs = Resource(self.name(), line=line)
        while self.peek().val != "{" and self.peek().kind not in ("NL", "EOF"):
            kw = self.name()
            if kw == "under":
                rs.under = self.name()
            elif kw == "key":
                rs.key = self.name()
            else:
                raise self.err("resource の見出しは「under 親」「key 名前」です")
        self.block_open()
        while not self.accept("}"):
            l2 = self.peek().line
            kw = self.name()
            if kw in ("index", "show"):
                rs.actions.append(ResAction(kw, self.name(), line=l2))
            elif kw in ("create", "update", "destroy"):
                act = ResAction(kw, self.name(), line=l2)
                if self.peek().val == "{":  # 出来事を呼ぶ create は、結果の種類を持たないので省ける
                    self.block_open()
                    while not self.accept("}"):
                        v = self.name()
                        self.expect("→")
                        act.statuses.append((v, self.num()))
                        self.end_line()
                rs.actions.append(act)
            else:
                raise self.err("resource の中は「index」「show」「create」「update」「destroy」です")
            self.end_line()
        return rs

    # ---- トリガ ----
    def reply_def(self, line) -> ReplyDef:
        name = self.name()
        params = []
        if self.accept("("):
            params = self.comma_list(self.input_item)
            self.expect(")")
        self.end_line()
        return ReplyDef(name, params, line)

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
                kind, key = "reply", self.name()
            elif kind == "all":  # all reply A, B → 処理：全部の応答がそろったら進む（待ち合わせ）。reply だけの行は、先に来た1つで進む
                if self.name() != "reply":
                    raise self.err("all の後ろには、reply と応答の名前を並べます（例：all reply 決済成功, 在庫確保 → 出荷）")
                key = ",".join(self.comma_list(self.name))
            elif kind == "deadline":
                kind = "deadline"
                if self.accept("now"):  # 「now + 30 min」。now は動的具体化の【参照】
                    self.expect("+")
                n = self.num()
                key = str(n * UNITS[self.unit()])
            else:
                raise self.err(f"トリガの要因は「reply」「all reply」「deadline」です（「{kind}」は使えません）")
            self.expect("→")
            causes.append((kind, key, self.name()))
            self.end_line()
        return Trigger(name, params, causes, line)


# 宣言を始めるキーワード。説明コメントは、このキーワードの次の名前に付く
DECL_KEYWORDS = {"entity", "ltype", "store", "state", "result", "effect", "part", "convert", "invariant", "deed", "nexus", "flow",
                 "trigger", "query", "event", "screen", "resource", "let", "initial", "middle", "final", "reply", "deadline", "slot", "input"}


def collect_labels(toks: list[Tok]) -> dict:
    """説明コメント（///）の付いた名前の表示名。宣言のキーワードに付いたものは次の名前に、それ以外は付いた名前そのものに付く。
    同じ名前に別の表示名を付けると、仕様書でどちらか分からなくなるので、エラーにする"""
    labels: dict[str, dict] = {}
    for i, t in enumerate(toks):
        if not t.doc:
            continue
        target = toks[i + 1] if t.kind == "NAME" and t.val in DECL_KEYWORDS and i + 1 < len(toks) else t
        if t.kind == "NAME" and t.val == "all" and i + 2 < len(toks) and toks[i + 1].val == "reply":
            target = toks[i + 2]  # all reply A, B：最初の応答の名前に付く
        if target.kind != "NAME":
            raise ParseError("説明コメント（///）は名前の前に書きます", t.line)
        new = parse_labels(t.doc)
        old = labels.setdefault(target.val, new)
        if old != new:
            raise ParseError(f"名前「{target.val}」に別の表示名が付いています（同じ名前の表示名は1つにします）", t.line)
    return labels


MAX_TZ = 14 * 3600  # UTC からのずれは ±14:00 まで


def split_timezone(toks):
    """`timezone +09:00`（アプリに1つ。書かなければ UTC）を取り除き、組み込みの `tz_offset`（UTC からのずれ、秒）をその値に置き換える。
    (字句の列, ずれ（秒）) を返す。tz_offset は値そのものに置き換わるので、範囲は検査器がそのまま知っている"""
    starts, keep, found, i = set(statement_starts(toks)), [], None, 0
    while i < len(toks):
        t = toks[i]
        if i in starts and t.kind == "NAME" and t.val == "timezone":
            if found is not None:
                raise ParseError("timezone は1つだけ宣言できます", t.line)
            w = toks[i + 1:i + 5]
            if len(w) < 4 or w[0].kind != "OP" or w[0].val not in ("+", "-") or w[1].kind != "NUM" or w[2].val != ":" or w[3].kind != "NUM":
                raise ParseError("timezone は `timezone +09:00` のように、UTC からのずれを時:分で書きます", t.line)
            secs = int(w[1].val) * 3600 + int(w[3].val) * 60
            if int(w[3].val) > 59 or secs > MAX_TZ:
                raise ParseError("timezone のずれは、分が 0〜59、全体で ±14:00 までです", t.line)
            found = -secs if w[0].val == "-" else secs
            i += 5
            continue
        keep.append(t)
        i += 1
    off = found or 0
    out = []
    for t in keep:
        if t.kind == "NAME" and t.val == "tz_offset":
            out += [Tok("NUM", str(off), t.line)] if off >= 0 else [Tok("OP", "(", t.line), Tok("NUM", "0", t.line), Tok("OP", "-", t.line), Tok("NUM", str(-off), t.line), Tok("OP", ")", t.line)]
        else:
            out.append(t)
    return out, off


def parse_toks(toks, origin) -> Program:
    toks, tz = split_timezone(toks)
    p = Parser(toks).program()
    p.tz_offset = tz
    p.labels = collect_labels(toks)
    p.origin = origin
    return p


def parse(src: str, path=None) -> Program:
    """path は、use でモジュールを取り込むときの基準になるファイル（文字列から読むときは無し）"""
    toks, origin = expand(lex(src), path)
    return parse_toks(toks, origin)
