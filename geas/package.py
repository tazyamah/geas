# SPDX-License-Identifier: GPL-3.0-or-later
"""モジュールのパッケージ（docs/modules.md の段階2）。インターフェース（検査結果）と、モジュールから生成した Rust の crate を出す。
既定でソースも同梱する（ソース付きを強く推す。外すときだけ no_source を明示する）。
インターフェースは検査器が出すもので、人が手で書かない。今のところ、ほかのモジュールを取り込むモジュールはパッケージにできない。"""
import dataclasses
import hashlib
import json
from pathlib import Path

from .ast import show
from .check import check
from .modules import ModuleError, load_standalone
from .parser import parse_toks
from .rustgen import cargo_toml, generate

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


def interface(ck, mod: str, version: str, source: bytes, lib: str) -> dict:
    """公開するもの：型の元の組み込み型と制約、変換・部品の入出力の型と範囲と保証、結果の形、処理の入力と結果。中身は入れない"""
    p = ck.p
    return {
        "format": INTERFACE_FORMAT,
        "module": mod,
        "version": version,
        "checker": CHECKER_VERSION,
        "source_sha256": sha256(source),
        "crate_sha256": sha256(lib.encode()),
        "ltypes": {n: plain(t) for n, t in p.ltype_defs.items()},
        "converts": [{"name": x.name, "params": plain(x.params), "ret": plain(x.ret)} for x in p.parts if x.body is not None],
        "parts": [{"name": x.name, "params": plain(x.params), "ret": plain(x.ret), "guarantee": show(x.guarantee)} for x in p.parts if x.body is None],
        "results": [{"name": r.name, "variants": plain(r.variants)} for r in p.results],
        "deeds": [{"name": x.name, "inputs": plain(x.inputs), "result": x.result} for x in p.processes],
        "labels": p.labels,
        "origin": {m: list(o) for m, o in p.origin.items()},
    }


def build_package(src: Path, out: Path, no_source: bool = False) -> tuple[Path, str]:
    """パッケージを out に作る。(インターフェースのファイル, そのハッシュ) を返す"""
    src = Path(src)
    toks, origin, mod, loaded = load_standalone(src)
    if len(loaded.names) > 1:
        raise PackageError("ほかのモジュールを取り込むモジュールは、まだパッケージにできません")
    ck = check(parse_toks(toks, origin), strict_names=True)
    if not ck.ok:
        raise PackageError("検査にエラーがあるので、パッケージにできません:\n" + "\n".join(str(d) for d in ck.diags))
    lib = generate(ck)
    source = src.read_bytes()
    out.mkdir(parents=True, exist_ok=True)
    crate = out / "crate"
    (crate / "src").mkdir(parents=True, exist_ok=True)
    (crate / "Cargo.toml").write_text(cargo_toml(f"geas_mod_{mod.lower()}"))
    (crate / "src" / "lib.rs").write_text(lib)
    iface = out / f"{mod.lower()}.geasi"
    iface.write_text(json.dumps(interface(ck, mod, loaded.versions.get(mod, ""), source, lib), ensure_ascii=False, indent=2, sort_keys=True) + "\n")
    if not no_source:
        (out / src.name).write_bytes(source)
    return iface, sha256(iface.read_bytes())
