# SPDX-License-Identifier: GPL-3.0-or-later
"""字句解析。名前は、空白と記号以外の連なり（日本語の名前をそのまま使える）。"""
from dataclasses import dataclass, field


class LexError(Exception):
    def __init__(self, msg: str, line: int = 0, col: int = 0):
        super().__init__(msg)
        self.msg, self.line, self.col, self.file = msg, line, col, ""

    def __str__(self):
        from pathlib import Path
        head = (f"{Path(self.file).name} の " if self.file else "") + (f"{self.line}行目" if self.line else "")
        return f"{head}: {self.msg}" if head else self.msg


@dataclass
class Tok:
    kind: str  # NAME NUM STR OP NL EOF
    val: str
    line: int
    doc: str = ""  # この字句の直前にあった説明コメント（///）。仕様書の表示名に使う。意味は変えない
    col: int = field(default=0, compare=False, kw_only=True)  # 行の中の位置（1 から、文字で数える）。エラーの表示だけに使う
    file: str = field(default="", compare=False, kw_only=True)  # use で取り込んだファイルの字句なら、そのファイル（エラーの表示だけに使う）


PUNCT = set('{}()[],:;.=<>+-*/%|→Σ!')
TWO = ("..", "->", "==", "!=", "<=", ">=", "-=", "+=")
SPACE = " \t\r　"
ESCAPES = {"n": "\n", "t": "\t", '"': '"', "\\": "\\"}  # 文字列の中で \ のあとに書けるもの
ESC_HOW = ('\\ のあとに書けるのは n（改行）・t（タブ）・"（" そのもの）・\\（\\ そのもの）と、行の終わり（次の行へ続ける）だけです。'
           "正規表現などで \\ そのものを書くときは \\\\ と書きます")
STR_HOW = "文字列を次の行へ続けるときは行の終わりに \\ を書き（次の行の頭の空白は読み飛ばします）、改行そのものは \\n と書きます"


def lex(src: str) -> list[Tok]:
    toks: list[Tok] = []
    i, line, n = 0, 1, len(src)
    start = 0  # いまの行の先頭の位置（列を数えるため）
    docs: list[str] = []  # 続けて書かれた /// の行。次の字句に付ける

    def add(tok: Tok):
        if docs and tok.kind not in ("NL", "EOF"):
            tok.doc = "\n".join(docs)
            docs.clear()
        toks.append(tok)

    join_next = False
    depth = 0  # ( ) の中の改行は式の続き（長い式を折り返せる）

    def nl():
        if depth == 0 and toks and toks[-1].kind != "NL":
            toks.append(Tok("NL", "\n", line, col=i - start + 1))

    while i < n:
        c = src[i]
        if c == "\n":
            if join_next:
                join_next = False
            else:
                nl()
            line += 1
            i += 1
            start = i
        elif c in SPACE:
            i += 1
        elif src.startswith("//", i) or c == "#":
            j = i
            while j < n and src[j] != "\n":
                j += 1
            if src.startswith("///", i):  # 説明コメント。コンパイラは読み飛ばし、仕様書の生成だけが使う
                docs.append(src[i + 3 : j].strip())
                if toks and toks[-1].kind != "NL":  # 行の途中に書いた説明は、次の行の名前に付く（その改行は文の区切りにしない）
                    join_next = True
            i = j
        elif c == '"':  # 文字列。中で行は変えられない（次の行へ続けるときは行末に \、改行そのものは \n）
            sline, scol = line, i - start + 1
            buf, j = [], i + 1
            while True:
                if j >= n:
                    raise LexError('文字列が閉じていません（" で始めた文字列の終わりの " がありません）', sline, scol)
                ch = src[j]
                if ch == '"':
                    break
                if ch == "\n":
                    raise LexError('文字列の途中で行が変わっています（終わりの " を書き忘れたかもしれません）。' + STR_HOW, sline, scol)
                if ch != "\\":
                    buf.append(ch)
                    j += 1
                    continue
                k = j + 1
                while k < n and src[k] in " \t\r":  # \ と改行のあいだの見えない空白は、続けるつもりの \ とみなす
                    k += 1
                if src[j + 1 : j + 2] in ESCAPES:
                    buf.append(ESCAPES[src[j + 1]])
                    j += 2
                elif k < n and src[k] == "\n":  # 行の続き：改行と、次の行の頭の空白を読み飛ばす（Rust と同じ）
                    while k < n and src[k] in " \t\r\n":
                        if src[k] == "\n":
                            line += 1
                            start = k + 1
                        k += 1
                    j = k
                elif j + 1 >= n:
                    raise LexError('文字列が閉じていません（" で始めた文字列の終わりの " がありません）', sline, scol)
                else:
                    raise LexError(f"文字列の中に「\\{src[j + 1]}」は書けません。" + ESC_HOW, line, j - start + 1)
            add(Tok("STR", "".join(buf), sline, col=scol))
            i = j + 1
        elif src[i : i + 2] in TWO:
            v = src[i : i + 2]
            add(Tok("OP", "→" if v == "->" else v, line, col=i - start + 1))
            i += 2
        elif c in PUNCT:
            depth = max(0, depth + (c == "(") - (c == ")"))
            add(Tok("OP", c, line, col=i - start + 1))
            i += 1
        else:
            j = i
            while j < n and src[j] not in SPACE and src[j] != "\n" and src[j] not in PUNCT and src[j] != '"':
                j += 1
            run = src[i:j]
            if run.isdigit() and not run.isascii():  # ９９（全角）・①・² は数字の仲間だが、数としては読まない
                raise LexError(f"数は半角の 0〜9 で書きます（「{run}」は半角の数字ではありません）", line, i - start + 1)
            if run.isdigit() and src.startswith(".", j) and src[j + 1 : j + 2].isdigit():  # 0.5 のような小数（範囲の .. は上で字句にしてある）
                k = j + 1
                while k < n and src[k].isdigit():
                    k += 1
                raise LexError(f"小数は書けません（「{src[i:k]}」）。割合は rule の値に 分子/分母（10/100 など）で書き、"
                               "金額は最小単位の整数（ドルならセントの数）で書きます", line, i - start + 1)
            add(Tok("NUM" if run.isdigit() else "NAME", run, line, col=i - start + 1))
            i = j
    nl()
    toks.append(Tok("EOF", "", line, col=i - start + 1))
    return toks
