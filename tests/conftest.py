# SPDX-License-Identifier: GPL-3.0-or-later
# 環境変数 GEAS_RECORD_CORPUS=出力フォルダ を付けて pytest を回すと、テストが parse() に渡した .geas の文字列を
# 全部書き出す（Rust 版との突き合わせの入力にする。tools/golden.py corpus が使う）。付けなければ何もしない。
import hashlib
import json
import os
from pathlib import Path

_OUT = os.environ.get("GEAS_RECORD_CORPUS")

if _OUT:
    import geas.parser as _parser

    _seen: dict = {}
    _orig = _parser.parse

    def _record(src, path=None):
        key = hashlib.sha1(src.encode("utf-8")).hexdigest()[:12]
        # use の相対参照に使うので、path は「隣のファイルを探す」基準として相対で残す
        _seen.setdefault(key, {"source": src, "path": str(path) if path else None})
        return _orig(src, path)

    _parser.parse = _record

    def pytest_sessionfinish(session, exitstatus):
        out = Path(_OUT)
        out.mkdir(parents=True, exist_ok=True)
        manifest = {}
        for key, v in sorted(_seen.items()):
            (out / f"{key}.geas").write_text(v["source"], encoding="utf-8")
            manifest[key] = v["path"]
        (out / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=1, sort_keys=True), encoding="utf-8")
