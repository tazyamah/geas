# SPDX-License-Identifier: GPL-3.0-or-later
"""モジュール（状態を持たない部品の集まり）の取り込み。docs/modules.md の段階1：ソースのまま取り込んで、全体を1つのプログラムにする。
モジュールのファイルは `module 名前` で始まり、論理型・変換・部品・結果の形と、状態に触れない処理（deed）だけを書ける。
取り込む側は `use "path"` と書き、モジュールの名前は `名前.定義名` で指す。
内部では、モジュールの定義名を `名前_定義名` に付け替えて、1つの字句の列につなげる（大文字で始まる名前には _ が入らず、
小文字の名前は大文字で始まらないので、利用者の名前とは重ならない）。"""
from pathlib import Path

from .lexer import Tok, lex

# モジュールに書ける宣言（状態を持たないもの）
MODULE_DECLS = {"ltype", "convert", "part", "result", "deed"}
# 宣言を始めるキーワード（モジュールには書けないものを見つけるため）
ALL_DECLS = {"entity", "ltype", "store", "state", "result", "effect", "part", "convert", "invariant", "deed", "nexus", "flow", "trigger",
             "query", "event", "data"}


class ModuleError(Exception):
    def __init__(self, msg: str, line: int = 0):
        super().__init__(f"{line}行目: {msg}" if line else msg)
        self.line = line


def statement_starts(toks: list[Tok]):
    """行頭（{ } の外）の字句の位置。宣言は、ここから始まる"""
    depth, at_start = 0, True
    for i, t in enumerate(toks):
        if t.kind == "NL":
            at_start = True
            continue
        if at_start and depth == 0:
            yield i
        at_start = False
        if t.kind == "OP" and t.val == "{":
            depth += 1
        elif t.kind == "OP" and t.val == "}":
            depth -= 1


def mangle(module: str, name: str) -> str:
    return f"{module}_{name}"


class Loaded:
    """取り込んだモジュールの全体。toks は依存の順につないだ字句、origin は付け替えた名前 → (モジュール, 元の名前)"""

    def __init__(self):
        self.toks: list[Tok] = []
        self.origin: dict[str, tuple[str, str]] = {}
        self.files: dict[Path, str] = {}  # 読んだファイル → モジュール名
        self.names: dict[str, dict[str, str]] = {}  # モジュール名 → {元の名前: 付け替えた名前}
        self.versions: dict[str, str] = {}  # モジュール名 → version（`module 名前 version "1.0.0"`。書かなければ空）


def split_uses(toks: list[Tok]):
    """`use "path"` の行を取り出す。(path と行番号の列, 残りの字句)"""
    uses, keep, skip_to = [], [], -1
    starts = set(statement_starts(toks))
    for i, t in enumerate(toks):
        if i < skip_to:
            continue
        if i in starts and t.kind == "NAME" and t.val == "use":
            if i + 1 >= len(toks) or toks[i + 1].kind != "STR":
                raise ModuleError('use の後に、取り込むファイルを "path" の形で書きます', t.line)
            uses.append((toks[i + 1].val, t.line))
            skip_to = i + 2
            continue
        keep.append(t)
    return uses, keep


def replace_refs(toks: list[Tok], visible: dict[str, dict[str, str]]) -> list[Tok]:
    """`モジュール名 . 定義名` を、付け替えた名前の1つの字句にする"""
    out, i = [], 0
    while i < len(toks):
        t = toks[i]
        if t.kind == "NAME" and t.val in visible and i + 2 < len(toks) and toks[i + 1].kind == "OP" and toks[i + 1].val == "." and toks[i + 2].kind == "NAME":
            mod, n = t.val, toks[i + 2].val
            if n not in visible[mod]:
                raise ModuleError(f"モジュール {mod} に「{n}」はありません", t.line)
            out.append(Tok("NAME", visible[mod][n], t.line, t.doc))
            i += 3
        else:
            out.append(t)
            i += 1
    return out


def load_module(path: Path, loaded: Loaded, stack: list[Path], line: int) -> str:
    """モジュールのファイルを読み込み、モジュール名を返す。同じファイルは1度だけ読む"""
    path = path.resolve()
    if path in loaded.files:
        return loaded.files[path]
    if path in stack:
        raise ModuleError("モジュールが循環して取り込まれています: " + " → ".join(p.name for p in [*stack, path]), line)
    if not path.exists():
        raise ModuleError(f"モジュールのファイル {path.name} がありません", line)
    toks = [t for t in lex(path.read_text()) if t.kind != "EOF"]
    uses, toks = split_uses(toks)
    visible = use_all(uses, path, loaded, stack + [path])
    starts = list(statement_starts(toks))
    if not starts or toks[starts[0]].val != "module" or toks[starts[0] + 1].kind != "NAME":
        raise ModuleError(f"{path.name}: モジュールのファイルは `module 名前` で始めます", toks[0].line if toks else 0)
    mod = toks[starts[0] + 1].val
    if mod in loaded.names:
        raise ModuleError(f"モジュール名「{mod}」が2つのファイルにあります", toks[starts[0]].line)
    declared: dict[str, str] = {}
    for i in starts[1:]:
        t = toks[i]
        if t.kind == "NAME" and t.val in ALL_DECLS and t.val not in MODULE_DECLS:
            raise ModuleError(f"モジュールには {t.val} を書けません（書けるのは {'、'.join(sorted(MODULE_DECLS))}。状態は持てません）", t.line)
        if t.kind == "NAME" and t.val in MODULE_DECLS and toks[i + 1].kind == "NAME":
            n = toks[i + 1].val
            if n in declared:
                raise ModuleError(f"モジュール {mod} に「{n}」が2つあります", t.line)
            declared[n] = mangle(mod, n)
    # モジュールの先頭の `module 名前 [version "文字列"]` を除く
    head = 2
    if toks[starts[0] + 2].kind == "NAME" and toks[starts[0] + 2].val == "version":
        if toks[starts[0] + 3].kind != "STR":
            raise ModuleError('version の後に、"1.0.0" のような文字列を書きます', toks[starts[0]].line)
        loaded.versions[mod] = toks[starts[0] + 3].val
        head = 4
    toks = toks[:starts[0]] + toks[starts[0] + head:]
    toks = replace_refs(toks, visible)
    toks = [Tok(t.kind, declared[t.val], t.line, t.doc) if t.kind == "NAME" and t.val in declared else t for t in toks]
    loaded.files[path] = mod
    loaded.names[mod] = declared
    loaded.origin.update({m: (mod, n) for n, m in declared.items()})
    loaded.toks += [t for t in toks if not (t.kind == "NL" and loaded.toks and loaded.toks[-1].kind == "NL")] + [Tok("NL", "\n", toks[-1].line if toks else 0)]
    return mod


def use_all(uses, base: Path, loaded: Loaded, stack: list[Path]) -> dict[str, dict[str, str]]:
    visible: dict[str, dict[str, str]] = {}
    for rel, line in uses:
        mod = load_module(base.parent / rel, loaded, stack, line)
        visible[mod] = loaded.names[mod]
    return visible


def expand(toks: list[Tok], path: Path | None) -> tuple[list[Tok], dict[str, tuple[str, str]]]:
    """取り込む側の字句から、use を取り除き、モジュールの字句を前に足す。(字句の列, 付け替えた名前の対応)"""
    uses, toks = split_uses(toks)
    if not uses:
        return toks, {}
    if path is None:
        raise ModuleError("use は、ファイルから読むときだけ使えます", toks[0].line if toks else 0)
    loaded = Loaded()
    visible = use_all(uses, Path(path).resolve(), loaded, [Path(path).resolve()])
    return loaded.toks + replace_refs(toks, visible), loaded.origin


def load_standalone(path) -> tuple[list[Tok], dict[str, tuple[str, str]], str, Loaded]:
    """モジュールのファイルだけを読む（パッケージを作るとき）。(字句の列, 付け替えた名前の対応, モジュール名, 読んだ結果)"""
    path = Path(path).resolve()
    loaded = Loaded()
    mod = load_module(path, loaded, [], 0)
    last = loaded.toks[-1].line if loaded.toks else 0
    return loaded.toks + [Tok("EOF", "", last)], loaded.origin, mod, loaded
