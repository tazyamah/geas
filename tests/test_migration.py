# SPDX-License-Identifier: GPL-3.0-or-later
"""保存の形の移行（migration 版 { add 実体.項目 = 値 }）。
前の版の DB を、起動時に 1 回でまとめて今の形へ書き換える。途中で失敗したら、何も変えずに止まる。
cargo が無い環境では、組んで動かす試験を飛ばす。"""
import json
import shutil
import sqlite3
import subprocess
import sys
from pathlib import Path

import pytest

from geas.check import check
from geas.parser import ParseError, parse

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "tests" / "fixtures" / "runtime_demo" / "runtime_demo.geas"
V1 = SRC.read_text(encoding="utf-8")
MIGRATION = "\nmigration 2 {\n  add Parcel.weight = 5\n  add Parcel.memo = null\n}\n"


def v2(migration: str = MIGRATION) -> str:
    """見本の小包に、重さ（必ずある）とメモ（無いかもしれない）を足した版"""
    t = V1.replace("  refund_ref: id\n", "  refund_ref: id\n  /// 重さ\n  weight: Weight required\n  /// メモ\n  memo: Kind\n", 1)
    t = t.replace("ltype Fee = money(JPY) 0..\n", "ltype Fee = money(JPY) 0..\n/// 重さ\nltype Weight = int 0..1000\n", 1)
    t = t.replace("create Parcel { kind: kind, fee: fee, status: Received }", "create Parcel { kind: kind, fee: fee, status: Received, weight: 0 }", 1)
    assert t.count("weight") == 2
    return t + migration


def errors(text: str) -> list[str]:
    return [d.msg for d in check(parse(text, str(SRC)), strict_names=True).diags if d.level == "エラー"]


def test_migration_の検査():
    assert errors(v2()) == []
    assert any("版は 2 から 1 ずつ" in m for m in errors(v2(MIGRATION.replace("migration 2", "migration 3"))))
    assert any("型の範囲" in m and "の外" in m for m in errors(v2(MIGRATION.replace("= 5", "= 1001"))))
    assert any("null にできません" in m for m in errors(v2(MIGRATION.replace("weight = 5", "weight = null"))))
    assert any("値は数です" in m for m in errors(v2(MIGRATION.replace("= 5", '= "5"'))))
    assert any("今の実体の項目にありません" in m for m in errors(v2(MIGRATION.replace("Parcel.memo", "Parcel.note"))))
    assert any("行のキー" in m for m in errors(v2(MIGRATION.replace("Parcel.memo = null", 'Parcel.parcel_id = "x"'))))
    assert any("add が 2 つ" in m for m in errors(v2(MIGRATION.replace("add Parcel.memo = null", "add Parcel.weight = 6"))))
    assert any("状態 ParcelStatus の名前" in m for m in errors(v2(MIGRATION.replace("add Parcel.memo = null", "add Parcel.status = Nowhere"))))
    with pytest.raises(ParseError, match="まだ書けません"):
        parse(v2(MIGRATION.replace("add Parcel.memo = null", "drop Parcel.memo")), str(SRC))


def test_前の版と比べて_migration_の無い追加は止まる(tmp_path):
    from geas.cli import main
    from geas.review import review
    from geas.spec import build, render

    def against(new_text):
        ck = check(parse(new_text, str(SRC)), strict_names=True)
        ck.check_against(check(parse(V1, str(SRC)), strict_names=True))
        return [d.msg for d in ck.diags if d.level == "エラー"]

    assert against(v2()) == []
    missing = against(v2(""))
    assert len(missing) == 2 and all("migration に add がありません" in m for m in missing) and "migration 2 { add Parcel.重さ = 値 }" in missing[0]
    assert against(V1) == []
    # コマンドでも同じ（check --base）
    base, new = tmp_path / "v1.geas", tmp_path / "v2.geas"
    base.write_text(V1, encoding="utf-8")
    new.write_text(v2(""), encoding="utf-8")
    assert main(["check", str(new), "--base", str(base)]) == 1
    new.write_text(v2(), encoding="utf-8")
    assert main(["check", str(new), "--base", str(base)]) == 0
    # 仕様書に出る
    ck = check(parse(v2(), str(SRC)), strict_names=True)
    spec = build(ck)
    md, html = render(ck, spec, ""), review(ck, None, [], "")
    assert "| 版 2 | 項目を足す | Parcel.重さ | 5 |" in md and "保存の形の変更" in html and "空（無い）" in html


@pytest.fixture(scope="module")
def built(tmp_path_factory):
    if shutil.which("cargo") is None:
        pytest.skip("cargo が無い")
    out = tmp_path_factory.mktemp("migration")
    code = out / "v2.geas"
    code.write_text(v2(), encoding="utf-8")
    subprocess.run([sys.executable, "-m", "geas", "rust", str(code), "-o", str(out / "app"), "--host", "--runtime"], check=True, capture_output=True, text=True, cwd=ROOT)
    r = subprocess.run(["cargo", "build", "--release", "--offline", "--quiet"], cwd=out / "app" / "server", capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    return out / "app" / "server" / "target" / "release" / "geas_server"


OLD = {"parcel_id": "p-1", "kind": "light", "fee": 300, "status": "Received", "refund_ref": None}  # 前の版（重さとメモが無い）の行


def old_db(path: Path, row: dict = OLD, version: int | None = None) -> Path:
    """前の版のアプリが作った DB（小包が 1 行）"""
    with sqlite3.connect(path) as c:
        c.execute('CREATE TABLE "Parcel" (k TEXT PRIMARY KEY, v TEXT NOT NULL)')
        c.execute('INSERT INTO "Parcel" VALUES (?, ?)', (row["parcel_id"], json.dumps(row)))
        if version is not None:
            c.execute("CREATE TABLE geas_kv (k TEXT PRIMARY KEY, v INTEGER NOT NULL)")
            c.execute("INSERT INTO geas_kv VALUES ('schema_version', ?)", (version,))
    return path


def start(exe, db) -> subprocess.CompletedProcess:
    """起動して、すぐ止める（--port 0 は、起動時の点検と移行までを行って待ち受けに入る。入ったら止める）"""
    try:
        return subprocess.run([str(exe), "--port", "0", "--db", str(db)], capture_output=True, text=True, timeout=4)
    except subprocess.TimeoutExpired as e:
        return subprocess.CompletedProcess(e.cmd, 0, "", (e.stderr or b"").decode("utf-8", "replace") if isinstance(e.stderr, bytes) else (e.stderr or ""))


def state(db) -> tuple[dict, int | None]:
    with sqlite3.connect(db) as c:
        rows = {k: json.loads(v) for k, v in c.execute('SELECT k, v FROM "Parcel"')}
        kv = c.execute("SELECT v FROM geas_kv WHERE k = 'schema_version'").fetchone() if c.execute("SELECT 1 FROM sqlite_master WHERE name = 'geas_kv'").fetchone() else None
    return rows, kv[0] if kv else None


def test_前の版のDBは_起動時に今の形になり_2回目は何もしない(built, tmp_path):
    db = old_db(tmp_path / "old.sqlite")
    assert start(built, db).returncode == 0
    rows, version = state(db)
    assert rows == {"p-1": {**OLD, "weight": 5, "memo": None}} and version == 2
    with sqlite3.connect(db) as c:  # 移行のあとに変えた値は、次の起動で初期値に戻らない
        c.execute("""UPDATE "Parcel" SET v = replace(v, '"weight":5', '"weight":7')""")
    assert start(built, db).returncode == 0
    assert state(db)[0]["p-1"]["weight"] == 7


def test_新しいDBは_今の版として始まる(built, tmp_path):
    db = tmp_path / "new.sqlite"
    assert start(built, db).returncode == 0
    assert state(db) == ({}, 2)


def test_今の定義に合わない行があれば_何も変えずに止まる(built, tmp_path):
    db = old_db(tmp_path / "bad.sqlite", {**OLD, "fee": -1})
    r = start(built, db)
    assert r.returncode != 0 and "今の保存の形に合わせられません" in r.stderr and "Parcel.fee=-1 は範囲外" in r.stderr and "DB は変えていません" in r.stderr, r.stderr[-800:]
    assert state(db) == ({"p-1": {**OLD, "fee": -1}}, None)


def test_このアプリより新しい版のDBは開かない(built, tmp_path):
    db = old_db(tmp_path / "newer.sqlite", {**OLD, "weight": 1, "memo": None}, version=3)
    r = start(built, db)
    assert r.returncode != 0 and "版 3" in r.stderr and "より新しい" in r.stderr, r.stderr[-800:]
    assert state(db)[1] == 3
