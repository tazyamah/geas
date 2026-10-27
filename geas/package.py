# SPDX-License-Identifier: GPL-3.0-or-later
"""モジュールのパッケージ（docs/modules.md の段階2）。インターフェース（検査結果）と、モジュールのソースを出す。
ソースを必ず同梱する（ソースを公開しないモジュールは使えない）。使っているモジュールがあれば、そのソースも同梱し、名前と version とハッシュを interface の dependencies に明示する。crate は出さない：使う側はソースから生成する。
インターフェースは検査器が出すもので、人が手で書かない。"""
import dataclasses
import hashlib
import json
import os
from pathlib import Path

from .ast import show
from .check import check
from .modules import ModuleError, load_standalone
from .parser import parse_toks

CHECKER_VERSION = "0.1.0"  # 検査器の version。インターフェースの意味が変わったら上げる
INTERFACE_FORMAT = 1


class PackageError(Exception):
    pass


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def plain(x):
    """データクラス・タプルを JSON にできる形にする（None の項目は省く）"""
    if dataclasses.is_dataclass(x) and not isinstance(x, type):
        return {f.name: plain(getattr(x, f.name)) for f in dataclasses.fields(x) if getattr(x, f.name) is not None and getattr(x, f.name) is not False}
    if isinstance(x, (list, tuple)):
        return [plain(v) for v in x]
    if isinstance(x, dict):
        return {str(k): plain(v) for k, v in x.items()}
    return x


def interface(ck, mod: str, version: str, source: bytes, source_file: str, dependencies: list) -> dict:
    """公開するもの：型の元の組み込み型と制約、変換・部品の入出力の型と範囲と保証、結果の形、処理の入力と結果。中身は入れない"""
    p = ck.p
    return {
        "format": INTERFACE_FORMAT,
        "module": mod,
        "version": version,
        "checker": CHECKER_VERSION,
        "source_file": source_file,
        "source_sha256": sha256(source),
        "dependencies": dependencies,
        "ltypes": {n: plain(t) for n, t in p.ltype_defs.items()},
        "converts": [{"name": x.name, "params": plain(x.params), "ret": plain(x.ret)} for x in p.parts if x.body is not None],
        "parts": [{"name": x.name, "params": plain(x.params), "ret": plain(x.ret), "guarantee": show(x.guarantee)} for x in p.parts if x.body is None],
        "results": [{"name": r.name, "variants": plain(r.variants)} for r in p.results],
        "deeds": [{"name": x.name, "inputs": plain(x.inputs), "result": x.result} for x in p.processes],
        "labels": p.labels,
        "origin": {m: list(o) for m, o in p.origin.items()},
    }


def build_package(src: Path, out: Path) -> tuple[Path, str]:
    """パッケージを out に作る。(インターフェースのファイル, そのハッシュ) を返す"""
    src = Path(src)
    toks, origin, mod, loaded = load_standalone(src)
    deps = []  # 使っているモジュール：名前・version・同梱するソースの場所とハッシュ（依存の順）
    for f, m in loaded.files.items():
        if m == mod:
            continue
        rel = os.path.relpath(f, src.resolve().parent)
        if rel.startswith(".."):
            raise PackageError(f"使っているモジュール {m} のソース {f.name} が、パッケージの置き場所の外にあります。同じフォルダか、その下に置いてください")
        if not loaded.versions.get(m):
            raise PackageError(f'使っているモジュール {m} に version がありません（`module {m} version "1.0.0"` と書いてください）')
        deps.append({"module": m, "version": loaded.versions[m], "source_file": rel.replace(os.sep, "/"), "source_sha256": sha256(f.read_bytes())})
    ck = check(parse_toks(toks, origin), strict_names=True)
    if not ck.ok:
        raise PackageError("検査にエラーがあるので、パッケージにできません:\n" + "\n".join(str(d) for d in ck.diags))
    source = src.read_bytes()
    out.mkdir(parents=True, exist_ok=True)
    iface = out / f"{mod.lower()}.geasi"
    iface.write_text(json.dumps(interface(ck, mod, loaded.versions.get(mod, ""), source, src.name, deps), ensure_ascii=False, indent=2, sort_keys=True) + "\n")
    (out / src.name).write_bytes(source)
    for d in deps:  # 使っているモジュールのソースも、同じ相対位置に同梱する
        dest = out / d["source_file"]
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes((src.resolve().parent / d["source_file"]).read_bytes())
    return iface, sha256(iface.read_bytes())
