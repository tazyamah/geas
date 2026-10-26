"""字句解析。名前は、空白と記号以外の連なり（日本語の名前をそのまま使える）。"""
from dataclasses import dataclass


class LexError(Exception):
    pass


@dataclass
class Tok:
    kind: str  # NAME NUM STR OP NL EOF
    val: str
    line: int


PUNCT = set('{}()[],:;.=<>+-*|→Σ!')
TWO = ("..", "->", "==", "!=", "<=", ">=", "-=", "+=")
SPACE = " \t\r　"


def lex(src: str) -> list[Tok]:
    toks: list[Tok] = []
    i, line, n = 0, 1, len(src)

    def nl():
        if toks and toks[-1].kind != "NL":
            toks.append(Tok("NL", "\n", line))

    while i < n:
        c = src[i]
        if c == "\n":
            nl()
            line += 1
            i += 1
        elif c in SPACE:
            i += 1
        elif src.startswith("//", i) or c == "#":
            while i < n and src[i] != "\n":
                i += 1
        elif c == '"':
            j = src.find('"', i + 1)
            if j < 0:
                raise LexError(f"{line}行目: 文字列が閉じていません")
            toks.append(Tok("STR", src[i + 1 : j], line))
            i = j + 1
        elif src[i : i + 2] in TWO:
            v = src[i : i + 2]
            toks.append(Tok("OP", "→" if v == "->" else v, line))
            i += 2
        elif c in PUNCT:
            toks.append(Tok("OP", c, line))
            i += 1
        else:
            j = i
            while j < n and src[j] not in SPACE and src[j] != "\n" and src[j] not in PUNCT and src[j] != '"':
                j += 1
            run = src[i:j]
            toks.append(Tok("NUM" if run.isdigit() else "NAME", run, line))
            i = j
    nl()
    toks.append(Tok("EOF", "", line))
    return toks
