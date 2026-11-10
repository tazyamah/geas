# SPDX-License-Identifier: GPL-3.0-or-later
"""窓つき exe の実行部（geas/window）を、生成物と一緒に dest/window に置く。アプリを知らない汎用の crate で、
サーバーの実行部（dest/server を lib として）と生成した画面（dest/ui）を、1 つの実行ファイルにまとめる。
ポートを開かず、画面から処理への呼び出しは WebView の IPC（プロセスの中）で渡す。手で書くコードは無い。"""
import shutil
from pathlib import Path

CARGO = '''[package]
name = "geas_window"
version = "0.1.0"
edition = "2021"

# 生成した Geas のクレート（ひとつ上）と、サーバーの実行部（経路の処理・保存・処理を動かす層。HTTP の待ち受けは使わない）
[dependencies]
geas_app = { path = ".." }
geas_server = { path = "../server" }
serde_json = "1"
tao = "0.34"
wry = "0.54"

# 整数の溢れの確認は、リリースビルドでも付ける（server と同じ理由。起点のクレートの [profile] だけが使われる）
[profile.release]
overflow-checks = true
'''


def write_window(dest) -> None:
    src = Path(__file__).parent / "window"
    out = Path(dest) / "window"
    (out / "src").mkdir(parents=True, exist_ok=True)
    shutil.copy(src / "src" / "main.rs", out / "src" / "main.rs")
    lock = src / "Cargo.lock"
    if lock.exists():
        shutil.copy(lock, out / "Cargo.lock")
    (out / "Cargo.toml").write_text(CARGO, encoding="utf-8")
    (out / ".gitignore").write_text("target/\n*.sqlite\n*.sqlite-*\n", encoding="utf-8")
