# SPDX-License-Identifier: GPL-3.0-or-later
"""サーバーの実行部（geas/server）を、生成物と一緒に dest/server に置く。アプリを知らない汎用の crate で、
生成した Geas の crate（dest）を path 依存で組む。外から埋める値（slot）の決め方は仕様の rule、最初のデータは seed.json（geas rust --seed）で、
手で書くコードは無い。"""
import shutil
from pathlib import Path

CARGO = '''[package]
name = "geas_server"
version = "0.1.0"
edition = "2021"

# 生成した Geas のクレート（ひとつ上）
[dependencies]
geas_app = { path = ".." }
rusqlite = { version = "0.37", features = ["bundled"] }
serde_json = "1"
tiny_http = "0.12"
'''


def write_server(dest, seed: Path | None = None) -> None:
    src = Path(__file__).parent / "server"
    out = Path(dest) / "server"
    (out / "src").mkdir(parents=True, exist_ok=True)
    for f in (src / "src").iterdir():
        shutil.copy(f, out / "src" / f.name)
    shutil.copy(src / "Cargo.lock", out / "Cargo.lock")
    (out / "Cargo.toml").write_text(CARGO)
    (out / ".gitignore").write_text("target/\n*.sqlite\n*.sqlite-*\n")
    # 最初のデータ（DB が空のときだけ入る）。無ければ空
    (out / "src" / "seed.json").write_text(Path(seed).read_text() if seed else "{}")
