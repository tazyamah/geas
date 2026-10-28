# SPDX-License-Identifier: GPL-3.0-or-later
"""page を持つ screen（ブラウザ拡張の画面）：検査と、拡張の生成。"""
import json
import subprocess
import sys
from pathlib import Path

from geas.check import ERROR, check
from geas.extgen import defs_of, ext_screens, write_ext
from geas.parser import parse

DIR = Path(__file__).parent / "fixtures" / "overseas_shop"
CODE = (DIR / "overseas_shop.geas").read_text()
READER = DIR / "app" / "extension" / "reader.js"


def chk(src):
    return check(parse(src))


def test_pageを持つ画面は拡張として生成される(tmp_path):
    ck = chk(CODE)
    assert [s.name for s in ext_screens(ck)] == ["PriceCheck"]
    write_ext(ck, tmp_path, READER, "http://localhost:9999")
    ext = tmp_path / "ext"
    assert {f.name for f in ext.iterdir()} == {"background.js", "content.js", "content.css", "defs.js", "manifest.json", "popup.html", "popup.js", "reader.js"}
    m = json.loads((ext / "manifest.json").read_text())
    assert m["content_scripts"][0]["matches"] == ["*://www.amazon.co.jp/*", "*://www.amazon.com/*"]
    assert "http://localhost:9999/*" in m["host_permissions"]
    d = defs_of(ck, ext_screens(ck)[0], "http://localhost:9999")
    assert d["route"] == {"method": "GET", "path": "/product_prices/:jan"} and d["input"]["label"] == "JANコード"


def test_拡張の画面は問い合わせを1つ見せるだけ():
    bad = CODE.replace("  field jan\n  show price_by_jan\n", "  field jan\n  show price_by_jan\n  show product_list\n")
    assert bad != CODE and any("page を持つ画面" in d.msg for d in chk(bad).diags if d.level == ERROR)
    bad = CODE.replace('page "*://www.amazon.co.jp/*", "*://www.amazon.com/*"', 'page "amazon"')
    assert any("URL のパターンではありません" in d.msg for d in chk(bad).diags if d.level == ERROR)


def test_readerが無ければ生成を止める(tmp_path):
    r = subprocess.run([sys.executable, "-m", "geas", "rust", str(DIR / "overseas_shop.geas"), "-o", str(tmp_path / "o"), "--host"], capture_output=True, text=True)
    assert r.returncode == 1 and "--reader" in r.stderr


def test_応答の結果の種類は検査され_模擬の定義になる():
    from geas.hostgen import definitions
    ck = chk(CODE)
    m = definitions(ck)["mock"]
    assert m["effects"]["Payment.charge"]["reply"] == "PaymentSettled"
    assert [o["name"] for o in m["replies"]["PaymentSettled"]["outcomes"]] == ["Approved", "Declined"]
    assert m["replies"]["StockSettled"]["outcomes"][1] == {"name": "Short", "label": "在庫不足", "gives": ["short"]}
    assert m["events"] == [{"name": "Reconnected", "label": "つながりが戻った"}]
    bad = CODE.replace("Approved gives auth_id", "Approved gives nothing_here")
    assert any("応答の中身にありません" in d.msg for d in chk(bad).diags if d.level == ERROR)
    bad = CODE.replace("replies StockSettled", "replies NoSuchReply")
    assert any("未定義の応答" in d.msg for d in chk(bad).diags if d.level == ERROR)


def test_開発用の操作パネルは汎用のページとして生成される(tmp_path):
    from geas.devgen import write_dev
    write_dev(chk(CODE), tmp_path)
    assert {f.name for f in (tmp_path / "dev").iterdir()} == {"test.html", "test.js"}


def test_サーバーの実行部は生成物と一緒に置かれ_app_rs_だけを差し替えられる(tmp_path):
    from geas.servergen import write_server
    write_server(tmp_path)
    s = tmp_path / "server"
    assert {f.name for f in (s / "src").iterdir()} == {"app.rs", "db.rs", "engine.rs", "http.rs", "main.rs", "mock.rs"}
    assert 'geas_app = { path = ".." }' in (s / "Cargo.toml").read_text() and (s / "Cargo.lock").exists()
    assert "取り決めがありません" in (s / "src" / "app.rs").read_text()  # 既定は何も持たない
    mine = tmp_path / "mine.rs"
    mine.write_text("// 自分の取り決め\n")
    write_server(tmp_path, mine)
    assert (s / "src" / "app.rs").read_text() == "// 自分の取り決め\n"
