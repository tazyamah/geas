# SPDX-License-Identifier: GPL-3.0-or-later
"""構文木。"""
from dataclasses import dataclass, field
from typing import Optional


@dataclass
class Range:
    lo: Optional[int] = None
    hi: Optional[int] = None


@dataclass
class Type:
    kind: str  # ID 整数 金額 割合 参照 名前 写像 集合
    range: Optional[Range] = None
    target: Optional[str] = None  # 参照先／状態の名前
    key: Optional[str] = None  # 写像のキーのデータ名
    value: Optional["Type"] = None
    unit: Optional[str] = None
    optional: bool = False  # 無いかもしれない（Rust では Option）。問い合わせの入力の「任意」と、null になりうる項目
    chars: Optional[tuple] = None  # 文字列の使える文字。(下限, 上限) のコードポイントの組を、小さい順につないだもの
    fills: Optional[tuple] = None  # 変換の引数で、未指定の成分を補うルール。((成分名, 値), …)。補った成分は「指定されている」扱い
    parts: Optional[tuple] = None  # 合成型の成分。((名前, 型), …)。target は論理型の名前


@dataclass
class Field:
    """データの項目。すべては原則「無いかもしれない」（null になりうる）。必須の約束は required で書く。
    キーと状態の項目は必須（状態は作るときに初期の状態が入る）"""
    name: str
    type: Type
    unique: bool = False
    required: bool = False


@dataclass
class Data:
    name: str
    fields: list[Field]
    line: int = 0
    store: Optional[str] = None  # 保存先（外部）。あれば、未同期かどうかの状態「記録」を言語が足す（J63）


@dataclass
class State:
    name: str
    initial: str = ""
    middle: list[str] = field(default_factory=list)
    final: list[str] = field(default_factory=list)
    rejects: list[tuple[str, str, str]] = field(default_factory=list)
    endless: bool = False  # 「終了 なし」：行き来を繰り返し、終わらない状態（記録済 ⇄ 端末保存済 など）
    line: int = 0


@dataclass
class ResultDef:
    name: str
    variants: list[tuple[str, list[tuple[str, Type]]]]  # 種類ごとの中身（名前と型）。次の処理へは名前と型で渡す
    line: int = 0


@dataclass
class EffectDecl:
    name: str  # 決済.請求
    params: list[tuple[str, Type]]
    line: int = 0
    failures: list[str] = field(default_factory=list)  # 論理的な失敗の理由（相手の拒否など）。待つ先のトリガに、理由ごとの応答が要る
    no_wait: Optional[str] = None  # 待たない理由（人が決めた根拠）。None なら、結果を待つ経路が必須


@dataclass
class Part:
    """外から足す部品。保証は、呼んだ直後に戻り値（result）と引数を使って確かめる式"""
    name: str
    params: list  # [(名前, 型)]
    ret: "Type"
    guarantee: object
    line: int = 0
    body: object = None  # convert（geas の式で書いた変換）のとき、その式。外の実装は無く、guarantee は無い


@dataclass
class Invariant:
    data: str
    cond: object
    line: int = 0


@dataclass
class Flow:
    name: str
    entries: list[tuple[str, list[tuple[str, str]]]]
    line: int = 0


@dataclass
class Trigger:
    """待ちの宣言。要因（応答／期限）ごとに行き先の処理を持つ。先に起きた要因が勝つ。"""
    name: str
    params: list[tuple[str, Type]]
    causes: list[tuple[str, str, str]]  # (種別 応答／期限, 要因 応答名／秒数, 行き先の処理)
    line: int = 0


@dataclass
class Process:
    name: str
    inputs: list[tuple[str, Type]] = field(default_factory=list)
    refs: list[tuple[str, Type]] = field(default_factory=list)
    reads: list[str] = field(default_factory=list)
    writes: list[str] = field(default_factory=list)
    transitions: list[tuple[str, str, str]] = field(default_factory=list)  # (データ, 元 or "(新規)", 先)
    result: str = ""
    effects: list[str] = field(default_factory=list)
    body: list = field(default_factory=list)
    line: int = 0


@dataclass
class Column:
    """問い合わせの結果の列。種類は 項目（自分の項目）／参照先（参照の先の項目）／合計・件数（写像の項目の集計）"""
    name: str
    kind: str
    field: str
    via: Optional[str] = None  # 参照先のとき、参照の項目名


@dataclass
class Query:
    """読むだけの処理（問い合わせ）。対象のデータの一覧を、条件・絞り込み・並びで返す。"""
    name: str
    inputs: list[tuple[str, "Type"]] = field(default_factory=list)
    refs: list[tuple[str, "Type"]] = field(default_factory=list)
    target: str = ""
    conds: list[list[tuple[str, str, str]]] = field(default_factory=list)  # where 1行 = または でつないだ組。行どうしは かつ。要素は (項目, 比較, 状態名か数)
    filters: list[tuple[str, str, str]] = field(default_factory=list)  # (項目, 以上／以下／含む／直近, 入力名／秒数)
    order: Optional[tuple[str, str]] = None  # (項目, 新しい順／古い順)
    page: Optional[str] = None  # 1ページの件数 N を持つ入力名（数を直接書いたときは page_n）
    page_n: Optional[int] = None  # 1ページの件数（数を直接書いたとき）
    select: list[Column] = field(default_factory=list)  # 空なら、対象のIDの列を返す
    reads: list[str] = field(default_factory=list)
    line: int = 0


@dataclass
class Event:
    """外の出来事（アプリを開いた・つながりが戻った など）が、何を起こすか。
    steps は ("各", 問い合わせ名, 処理名)＝問い合わせの結果の1件ずつに処理を起こす、
    または ("外から", "", 処理名)＝出来事といっしょに外から届く入力で処理を起こす。"""
    name: str
    steps: list[tuple[str, str, str]] = field(default_factory=list)
    line: int = 0


@dataclass
class Program:
    data: list[Data] = field(default_factory=list)
    states: list[State] = field(default_factory=list)
    results: list[ResultDef] = field(default_factory=list)
    effects: list[EffectDecl] = field(default_factory=list)
    parts: list[Part] = field(default_factory=list)
    invariants: list[Invariant] = field(default_factory=list)
    processes: list[Process] = field(default_factory=list)
    flows: list[Flow] = field(default_factory=list)
    triggers: list[Trigger] = field(default_factory=list)
    queries: list[Query] = field(default_factory=list)
    events: list[Event] = field(default_factory=list)
    composites: list = field(default_factory=list)  # 合成型（宣言の順）。型は Type(kind=合成)
    labels: dict = field(default_factory=dict)  # 名前 → {言語: 表示名}。説明コメント（///）から。仕様書の表示だけに使い、意味は変えない


# ---- 式 ----
@dataclass
class Num:
    v: int


@dataclass
class Tuple:
    """値の組。いまは冪等キーだけに使う（idempotency_key = (注文ID, k)）"""
    items: list


@dataclass
class Null:
    """無いかもしれない項目が、実際に無いこと（null の値）。1件や項目そのものが無いこと（undefined）とは別"""


@dataclass
class Dur:
    """期間の数値（秒）。30 min のように、数と単位で書く。時刻に足し引きするためだけの値で、項目には入れられない"""
    secs: int
    text: str = ""


@dataclass
class Name:
    n: str


@dataclass
class Attr:
    e: object
    n: str


@dataclass
class Index:
    d: str
    e: object


@dataclass
class Call:
    n: str
    args: list


@dataclass
class Make:
    """合成型の値を、型名と成分をまとめて作る式（点 { x: 1, y: 2 }）"""
    n: str
    fields: list  # [(成分名, 式)]
    line: int = 0


@dataclass
class Bin:
    op: str
    l: object
    r: object


@dataclass
class Binder:
    names: list[str]
    src: object


@dataclass
class SetB:
    elem: object
    binder: Binder
    cond: Optional[object] = None


@dataclass
class SumE:
    elem: object
    binder: Binder
    cond: Optional[object] = None


@dataclass
class Pred:
    e: object
    kind: str  # 空 空でない ある ない


@dataclass
class Implies:
    l: object
    r: object


# ---- 文 ----
@dataclass
class Return:
    variant: str
    args: list
    line: int = 0


@dataclass
class Assign:
    name: str
    e: object
    absent: Optional[Return] = None
    line: int = 0


@dataclass
class Create:
    name: str
    data: str
    fields: list[tuple[str, object]]
    line: int = 0


@dataclass
class If:
    cond: object
    body: list  # 条件が成り立つときの文（return・項目の更新・update・request・入れ子の if）
    line: int = 0


@dataclass
class Update:
    target: object  # Attr(Index(D, k), f)
    op: str
    value: object
    binder: Binder
    cond: Optional[object] = None
    line: int = 0


@dataclass
class SetField:
    var: str
    f: str
    e: object
    line: int = 0


@dataclass
class Request:
    effect: str
    args: list
    idem: Optional[object] = None
    line: int = 0


@dataclass
class ForEach:
    """集合の要素ごとに、引いて要求する。要素どうしは独立で、全体で1つの束（外部作用の束）になる"""
    binder: Binder
    body: list
    line: int = 0


def route_map(routes) -> dict:
    """結果の種類 → 行き先のならび。同じ結果に行き先を続けて書くと、書いた順に起こす（並列っぽい書き方。実行は書いた順）"""
    m: dict[str, list[str]] = {}
    for v, d in routes:
        m.setdefault(v, []).append(d)
    return m


def show_range(r: Optional[Range]) -> str:
    if r is None:
        return ""
    return f" {'' if r.lo is None else r.lo}..{'' if r.hi is None else r.hi}"


def show_chars(cs: tuple) -> str:
    esc = lambda c: ("\\" + chr(c)) if chr(c) in "\\-]" else chr(c)
    return "".join(esc(a) if a == b else f"{esc(a)}-{esc(b)}" for a, b in cs)


def show_type(t: Type) -> str:
    k = t.kind
    if k in ("ID", "日時"):
        return k
    if k == "文字列":
        return "文字列" + show_range(t.range) + ("" if t.chars is None else f" 文字種 [{show_chars(t.chars)}]")
    if k == "バイト列":
        return "バイト列" + show_range(t.range)
    if k == "整数":
        return "整数" + show_range(t.range)
    if k == "割合":
        return "割合" + show_range(t.range)
    if k == "金額":
        return f"金額({t.unit or '円'})" + show_range(t.range)
    if k == "参照":
        return f"{t.target} を参照"
    if k == "名前":
        return t.target
    if k == "合成":
        return f"{t.target}（{'、'.join(f'{n}: {show_type(pt)}' for n, pt in t.parts)}）"
    if k == "写像":
        return f"{t.key} → {show_type(t.value)}"
    if k == "集合":
        return f"{show_type(t.value)} の集合"
    raise ValueError(k)


def show(e) -> str:
    if isinstance(e, Num):
        return str(e.v)
    if isinstance(e, Null):
        return "null"
    if isinstance(e, Dur):
        return e.text or f"{e.secs} sec"
    if isinstance(e, Tuple):
        return "(" + ", ".join(map(show, e.items)) + ")"
    if isinstance(e, Name):
        return e.n
    if isinstance(e, Attr):
        return f"{show(e.e)}.{e.n}"
    if isinstance(e, Index):
        return f"{e.d}[{show(e.e)}]"
    if isinstance(e, Call):
        return f"{e.n}({', '.join(show(a) for a in e.args)})"
    if isinstance(e, Make):
        return f"{e.n} {{ {', '.join(f'{n}: {show(x)}' for n, x in e.fields)} }}"
    if isinstance(e, Bin):
        return f"{show(e.l)} {e.op} {show(e.r)}"
    if isinstance(e, Pred):
        return f"{show(e.e)} が {e.kind}"
    if isinstance(e, Implies):
        return f"{show(e.l)} なら {show(e.r)}"
    if isinstance(e, (SetB, SumE)):
        b = e.binder
        names = b.names[0] if len(b.names) == 1 else f"({', '.join(b.names)})"
        head = "Σ " if isinstance(e, SumE) else ""
        cond = f", {show(e.cond)}" if e.cond is not None else ""
        return f"{head}{{ {show(e.elem)} | {names} in {show(b.src)}{cond} }}"
    raise ValueError(e)
