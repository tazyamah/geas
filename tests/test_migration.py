# SPDX-License-Identifier: GPL-3.0-or-later
"""保存の形の移行（migration 版 { add 実体.項目 = 値 ／ rename 実体.項目 to 新しい名前 ／ drop 実体.項目 }）。
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
M2 = "\nmigration 2 {\n  add Parcel.weight = 5\n  add Parcel.memo = null\n}\n"
M3 = "\nmigration 3 {\n  rename Parcel.memo to note\n  drop Parcel.legacy\n}\n"


def app(memo: str, migrations: str) -> str:
    """見本の小包に、重さ（必ずある）とメモ（無いかもしれない。名前は memo）を足した版"""
    t = V1.replace("  refund_ref: id\n", f"  refund_ref: id\n  /// 重さ\n  weight: Weight required\n  /// メモ\n  {memo}: Kind\n", 1)
    t = t.replace("ltype Fee = money(JPY) 0..\n", "ltype Fee = money(JPY) 0..\n/// 重さ\nltype Weight = int 0..1000\n", 1)
    t = t.replace("create Parcel { kind: kind, fee: fee, status: Received }", "create Parcel { kind: kind, fee: fee, status: Received, weight: 0 }", 1)
    assert t.count("weight") == 2
    return t + migrations


def v2(m2: str = M2) -> str:
    """版 2：重さとメモ（memo）を足した"""
    return app("memo", m2)


def v3(m3: str = M3, m2: str = M2) -> str:
    """版 3：メモの名前を note に変え、昔の項目 legacy を消した"""
    return app("note", m2 + m3)


def ck_of(text: str):
    return check(parse(text, str(SRC)), strict_names=True)


def errors(text: str) -> list[str]:
    return [d.msg for d in ck_of(text).diags if d.level == "エラー"]


def has(text: str, word: str) -> bool:
    return any(word in m for m in errors(text))


def test_add_の検査():
    assert errors(v2()) == []
    assert has(v2(M2.replace("migration 2", "migration 3")), "版は 2 から 1 ずつ")
    assert any("型の範囲" in m and "の外" in m for m in errors(v2(M2.replace("= 5", "= 1001"))))
    assert has(v2(M2.replace("weight = 5", "weight = null")), "null にできません")
    assert has(v2(M2.replace("= 5", '= "5"')), "値は数です")
    assert has(v2(M2.replace("Parcel.memo", "Parcel.nowhere")), "今の実体の項目にありません")
    assert has(v2(M2.replace("Parcel.memo = null", 'Parcel.parcel_id = "x"')), "行のキー")
    assert has(v2(M2.replace("add Parcel.memo = null", "add Parcel.weight = 6")), "add が 2 つ")
    assert has(v2(M2.replace("add Parcel.memo = null", "add Parcel.status = Nowhere")), "状態 ParcelStatus の名前")
    assert has(v2(M2.replace("Parcel.memo", "Package.memo")), "実体「Package」がありません")


def test_rename_と_drop_の検査():
    assert errors(v3()) == []
    # 名前を変えたのに、実体の項目が前の名前のまま
    assert has(app("memo", M2 + M3), "名前を変えたなら、実体の項目も「note」にしてください")
    # 消すと書いたのに、実体にまだある
    assert has(v3(M3.replace("drop Parcel.legacy", "drop Parcel.weight")), "消すなら、実体の項目からも消してください")
    # 変えた先が、今の実体に無い
    assert has(v3(M3.replace("to note", "to nota")), "今の実体の項目にありません")
    assert has(v3(M3.replace("rename Parcel.memo to note", "rename Parcel.note to note")), "新しい名前が、前と同じです")
    # 足してから名前を変えた項目の値は、変えた先の型で確かめる（note は 1〜10 文字）
    assert errors(v3(m2=M2.replace("memo = null", 'memo = "abc"'))) == []
    assert has(v3(m2=M2.replace("memo = null", 'memo = "12345678901"')), "値の長さ 11")
    # 足して、あとで消す項目は、今の実体に無くてよい
    assert errors(v3(M3.replace("drop Parcel.legacy", "drop Parcel.tmp"), M2.replace("add Parcel.memo", "add Parcel.tmp = 1\n  add Parcel.memo"))) == []
    # 消したあとに、同じ名前をもう一度足せる
    assert errors(v3(M3 + "\nmigration 4 {\n  drop Parcel.weight\n}\n\nmigration 5 {\n  add Parcel.weight = 7\n}\n")) == []
    with pytest.raises(ParseError, match="to が要ります"):
        parse(v3(M3.replace("to note", "→ note")), str(SRC))
    with pytest.raises(ParseError, match="rename 実体.項目 to 新しい名前"):
        parse(v3(M3.replace("drop Parcel.legacy", "remove Parcel.legacy")), str(SRC))


def test_前の版と比べて_migration_に書いていない変更は止まる(tmp_path):
    from geas.cli import main
    from geas.review import review
    from geas.spec import build, render

    def against(new_text, base_text):
        ck = ck_of(new_text)
        ck.check_against(ck_of(base_text))
        return [d.msg for d in ck.diags if d.level == "エラー"]

    assert against(V1, V1) == [] and against(v2(), V1) == [] and against(v3(), V1) == [] and against(v3(), v2()) == []
    # 足した項目を書いていない
    missing = against(v2(""), V1)
    assert len(missing) == 2 and all("増えましたが、migration に書いてありません" in m for m in missing) and "migration 2 { add Parcel.重さ = 値 }" in missing[0]
    # 版 2 から、名前を変えたのに書いていない（増えた note と、無くなった memo の両方を言う）
    renamed = against(app("note", M2), v2())
    assert len(renamed) == 3 and any("「メモ」が増えました" in m for m in renamed)  # 残りの 1 つは、版 2 の add の先（memo）が今の実体に無い、という検査
    assert any("「Parcel.memo」は、今の実体の項目にありません" in m for m in renamed)
    assert any("前の版にあった項目" in m and "行に、この項目の値が残ります" in m and "drop Parcel." in m for m in renamed)
    # 前の migration を消した
    assert any("migration は消さずに残します" in m for m in against(app("memo", ""), v2()))
    # コマンドでも同じ（check --base）
    base, new = tmp_path / "v1.geas", tmp_path / "new.geas"
    base.write_text(V1, encoding="utf-8")
    new.write_text(v2(""), encoding="utf-8")
    assert main(["check", str(new), "--base", str(base)]) == 1
    new.write_text(v3(), encoding="utf-8")
    assert main(["check", str(new), "--base", str(base)]) == 0
    # 仕様書に出る
    ck = ck_of(v3())
    spec = build(ck)
    md, html = render(ck, spec, ""), review(ck, None, [], "")
    assert "| 版 2 | 項目を足す | Parcel.重さ | 前からある行へ入れる値： 5 |" in md
    assert "| 版 3 | 名前を変える | Parcel.memo | 値はそのまま、項目 メモ に移る |" in md  # 新しい名前 note は、表示名（メモ）で出る and "| 版 3 | 項目を消す | Parcel.legacy | 値は捨てる |" in md
    assert "今は版 3" in md and "保存の形の変更" in html and "空（無い）" in html and "名前を変える" in html and "項目を消す" in html


@pytest.fixture(scope="module")
def built(tmp_path_factory):
    if shutil.which("cargo") is None:
        pytest.skip("cargo が無い")
    out = tmp_path_factory.mktemp("migration")
    code = out / "v3.geas"
    code.write_text(v3(), encoding="utf-8")
    subprocess.run([sys.executable, "-m", "geas", "rust", str(code), "-o", str(out / "app"), "--host", "--runtime"], check=True, capture_output=True, text=True, cwd=ROOT)
    r = subprocess.run(["cargo", "build", "--release", "--offline", "--quiet"], cwd=out / "app" / "server", capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    return out / "app" / "server" / "target" / "release" / "geas_server"


OLD = {"parcel_id": "p-1", "kind": "light", "fee": 300, "status": "Received", "refund_ref": None}  # 版 1 の行（重さとメモが無い）
NOW = {**OLD, "weight": 5, "note": None}  # 版 3 の行


def a_db(path: Path, row: dict, version: int | None = None) -> Path:
    """前の版のアプリが作った DB（小包が 1 行）。version を渡さなければ、版を覚えていない DB（版 1 として扱われる）"""
    with sqlite3.connect(path) as c:
        c.execute('CREATE TABLE "Parcel" (k TEXT PRIMARY KEY, v TEXT NOT NULL)')
        c.execute('INSERT INTO "Parcel" VALUES (?, ?)', (row["parcel_id"], json.dumps(row)))
        if version is not None:
            c.execute("CREATE TABLE geas_kv (k TEXT PRIMARY KEY, v INTEGER NOT NULL)")
            c.execute("INSERT INTO geas_kv VALUES ('schema_version', ?)", (version,))
    return path


def start(exe, db) -> subprocess.CompletedProcess:
    """起動して、すぐ止める（起動時の点検と移行までを行って待ち受けに入る。入ったら止める）"""
    try:
        return subprocess.run([str(exe), "--port", "0", "--db", str(db)], capture_output=True, text=True, timeout=4)
    except subprocess.TimeoutExpired as e:
        err = e.stderr.decode("utf-8", "replace") if isinstance(e.stderr, bytes) else (e.stderr or "")
        return subprocess.CompletedProcess(e.cmd, 0, "", err)


def state(db) -> tuple[dict, int | None]:
    with sqlite3.connect(db) as c:
        rows = {k: json.loads(v) for k, v in c.execute('SELECT k, v FROM "Parcel"')}
        kv = c.execute("SELECT v FROM geas_kv WHERE k = 'schema_version'").fetchone() if c.execute("SELECT 1 FROM sqlite_master WHERE name = 'geas_kv'").fetchone() else None
    return rows, kv[0] if kv else None


def test_版1のDBは_起動時に今の形になり_2回目は何もしない(built, tmp_path):
    db = a_db(tmp_path / "v1.sqlite", {**OLD, "legacy": 1})
    assert start(built, db).returncode == 0
    assert state(db) == ({"p-1": NOW}, 3)  # 重さが入り、メモは note で空、legacy は消えた
    with sqlite3.connect(db) as c:  # 移行のあとに変えた値は、次の起動で初期値に戻らない
        c.execute("""UPDATE "Parcel" SET v = replace(v, '"weight":5', '"weight":7')""")
    assert start(built, db).returncode == 0
    assert state(db) == ({"p-1": {**NOW, "weight": 7}}, 3)


def test_版2のDBは_足りない版だけが当たる(built, tmp_path):
    db = a_db(tmp_path / "v2.sqlite", {**OLD, "weight": 9, "memo": "hi", "legacy": 1}, version=2)
    assert start(built, db).returncode == 0
    assert state(db) == ({"p-1": {**OLD, "weight": 9, "note": "hi"}}, 3)  # 版 2 の add は当たらない（重さは 9 のまま）。メモの値は note に移る


def test_新しいDBは_今の版として始まる(built, tmp_path):
    db = tmp_path / "new.sqlite"
    assert start(built, db).returncode == 0
    assert state(db) == ({}, 3)


def test_合わない行があれば_何も変えずに止まる(built, tmp_path):
    bad = {**OLD, "fee": -1, "legacy": 1}
    db = a_db(tmp_path / "bad.sqlite", bad)
    r = start(built, db)
    assert r.returncode != 0 and "今の保存の形に合わせられません" in r.stderr and "Parcel.fee=-1 は範囲外" in r.stderr and "DB は変えていません" in r.stderr, r.stderr[-800:]
    assert state(db) == ({"p-1": bad}, None)
    # 前の名前と新しい名前の両方がある行は、どちらの値を残すか決まらない
    both = {**OLD, "weight": 1, "memo": "a", "note": "b"}
    db = a_db(tmp_path / "both.sqlite", both, version=2)
    r = start(built, db)
    assert r.returncode != 0 and "前の名前「memo」と新しい名前「note」の両方があります" in r.stderr, r.stderr[-800:]
    assert state(db) == ({"p-1": both}, 2)


def test_このアプリより新しい版のDBは開かない(built, tmp_path):
    db = a_db(tmp_path / "newer.sqlite", NOW, version=4)
    r = start(built, db)
    assert r.returncode != 0 and "版 4" in r.stderr and "より新しい" in r.stderr, r.stderr[-800:]
    assert state(db)[1] == 4


def test_drop_を書かずに消した項目は_行に残ったままになる(built, tmp_path):
    """drop が要る理由。知らない項目のある行も読めるので、migration に書かずに項目を消すと、起動は通り、値は行に残る。
    だから check --base は、書いていない削除をエラーにする"""
    row = {**NOW, "old_field": "残る値"}
    db = a_db(tmp_path / "left.sqlite", row, version=3)
    assert start(built, db).returncode == 0
    assert state(db) == ({"p-1": row}, 3)
