# SPDX-License-Identifier: GPL-3.0-or-later
"""開発用の操作パネル（模擬の設定・時計・外部作用の記録）を作る。お店を知らない汎用のページ（geas/dev）を dest/dev に置く。
中身は、サーバーの /api/defs の mock（応答の結果の種類・出来事の宣言）と /api/test/state から作られる。"""
import shutil
from pathlib import Path


def write_dev(ck, dest) -> None:
    out = Path(dest) / "dev"
    out.mkdir(parents=True, exist_ok=True)
    for f in (Path(__file__).parent / "dev").iterdir():
        shutil.copy(f, out / f.name)
