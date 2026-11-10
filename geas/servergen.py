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

[features]
{NET_FEATURES}
# 整数の溢れの確認は、リリースビルドでも付ける（値の範囲の保証の網）。Cargo は組み立ての起点（このクレート）の
# [profile] しか使わず、geas_app の Cargo.toml に書いた設定はここでは効かないので、ここにも書く
[profile.release]
overflow-checks = true
'''


# 宣言に `over http`（外部作用の実通信）があるアプリだけに足す。無ければ、通信の部品は依存にも実行ファイルにも入らない
NET_DEP = 'ureq = { version = "2", optional = true, default-features = false, features = ["tls"] }\n'


def write_server(dest, seed: Path | None = None, net: bool = False) -> None:
    src = Path(__file__).parent / "server"
    out = Path(dest) / "server"
    (out / "src").mkdir(parents=True, exist_ok=True)
    for f in (src / "src").iterdir():
        shutil.copy(f, out / "src" / f.name)
    shutil.copy(src / "Cargo.lock", out / "Cargo.lock")
    cargo = CARGO.replace('tiny_http = "0.12"\n', 'tiny_http = "0.12"\n' + (NET_DEP if net else ""), 1)
    cargo = cargo.replace("{NET_FEATURES}\n", 'net = ["dep:ureq"]\ndefault = ["net"]\n' if net else "net = []\n", 1)
    (out / "Cargo.toml").write_text(cargo, encoding="utf-8")
    (out / ".gitignore").write_text("target/\n*.sqlite\n*.sqlite-*\n", encoding="utf-8")
    # 最初のデータ（DB が空のときだけ入る）。無ければ空
    (out / "src" / "seed.json").write_text(Path(seed).read_text(encoding="utf-8") if seed else "{}", encoding="utf-8")
