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
