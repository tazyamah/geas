# SPDX-License-Identifier: GPL-3.0-or-later
"""構文木を JSON にして出す（Rust 版との突き合わせ用。`python -m geas astdump ファイル.geas`）。

dataclass は {"_": 型の名前, 項目…}、list と tuple は配列、dict は {"_dict": [[キー, 値], …]}（挿入順を保つ）にする。
構文エラーは {"_error": 種類, …} にして、終了コード 2 で出す。"""
import dataclasses
import json
from pathlib import Path

from .lexer import LexError
from .modules import ModuleError
from .parser import ParseError, parse


def dump(x):
    if dataclasses.is_dataclass(x) and not isinstance(x, type):
        out = {"_": type(x).__name__}
        for f in dataclasses.fields(x):
            out[f.name] = dump(getattr(x, f.name))
        return out
    if isinstance(x, (list, tuple)):
        return [dump(v) for v in x]
    if isinstance(x, dict):
        return {"_dict": [[k, dump(v)] for k, v in x.items()]}
    return x


def run(path: str) -> int:
    try:
        prog = parse(Path(path).read_text(encoding="utf-8"), path)
    except (ParseError, LexError, ModuleError) as e:
        kind = {ParseError: "Parse", LexError: "Lex", ModuleError: "Module"}[type(e)]
        err = {"_error": kind, "text": str(e), "msg": e.msg, "line": e.line, "col": getattr(e, "col", 0),
               "file": Path(e.file).name if e.file else "", "hint": getattr(e, "hint", "")}
        print(json.dumps(err, ensure_ascii=False))
        return 2
    print(json.dumps(dump(prog), ensure_ascii=False))
    return 0
