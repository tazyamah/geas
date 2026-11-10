"""データ用フォルダ（path と data_folder）の検査：absolute の中は OS ごとのパスの書き方、relative は相対パス、環境変数は必ずある名前か needs env で宣言、組む OS（--target）の行が要る。"""
import subprocess
import sys
from pathlib import Path

from geas.check import check
from geas.parser import ParseError, parse
from geas.spec import build

ROOT = Path(__file__).resolve().parent.parent
FIXTURE = ROOT / "tests" / "fixtures" / "solo_tweet" / "solo_tweet.geas"
_FULL = FIXTURE.read_text(encoding="utf-8")
# 見本の宣言（path ～ data_folder）は外して、試験ごとに足す
SRC = _FULL[: _FULL.index("// データ用フォルダ")] + _FULL[_FULL.index("needs secret"):]
B = "\\\\"  # geas の文字列の中の \ は \\ と書く


def with_lines(lines):
    return SRC.replace("needs secret", lines + "\nneeds secret", 1)


def one(os_name, text, kind="absolute"):
    """1 つの OS の行だけを持つパスを、データ用フォルダにする"""
    if kind == "relative":
        return f'path P = relative "{text}"\ndata_folder P'
    return f'path P = absolute {{\n  {os_name} "{text}"\n}}\ndata_folder P'


def errors(lines):
    return [d.msg for d in check(parse(with_lines(lines))).diags if d.level == "エラー"]


def test_見本の宣言は通る_仕様書に出る():
    rows = dict(build(check(parse(_FULL)))["データ用フォルダ"])
    assert rows["データ用フォルダ"] == "パス DataRoot"
    assert rows["パス DataRoot（windows）"] == "%APPDATA%\\ひとりつぶやき" and "パス DataRoot（linux）" in rows and "パス DataRoot（macos）" in rows


def test_各OSの書き方と相対パスは通る():
    assert errors(one("windows", f"%APPDATA%{B}ひとり")) == []
    assert errors(one("windows", f"D:{B}data")) == []
    assert errors(one("macos", "~/Library/Application Support/tweet")) == []
    assert errors(one("linux", "${HOME}/.local/share/tweet")) == []
    assert errors(one("", "data/x", "relative")) == []


def test_absoluteの中は_その_OS_の書き方の絶対パス():
    assert any("Windows のパスの書き方" in m for m in errors(one("windows", "$HOME/x")))
    assert any("Windows のパスの書き方" in m for m in errors(one("windows", "~/x")))
    assert any("絶対パス" in m for m in errors(one("windows", "data")))
    assert any("mac・Linux のパスの書き方" in m for m in errors(one("linux", "%APPDATA%/x")))
    assert any("mac・Linux のパスの書き方" in m for m in errors(one("macos", f"C:{B}x")))
    assert any("絶対パス" in m for m in errors(one("linux", "data/x")))


def test_relativeは_相対パスだけ_外へ出られない():
    assert any("相対パス" in m for m in errors(one("", "/var/x", "relative")))
    assert any("相対パス" in m for m in errors(one("", "%APPDATA%/x", "relative")))
    assert any("exe のあるフォルダの外" in m for m in errors(one("", "../x", "relative")))


def test_環境変数は_必ずある名前か_needs_env_で宣言する():
    assert any("needs env FOO" in m for m in errors(one("windows", f"%FOO%{B}x")))
    assert any("needs env XDG_DATA_HOME" in m for m in errors(one("linux", "$XDG_DATA_HOME/x")))
    assert errors('needs env FOO "置き場の指定"\n' + one("windows", f"%FOO%{B}x")) == []
    assert any("2つ以上" in m for m in errors('needs env FOO "a"\nneeds env foo "b"'))


def test_書き方の誤り():
    assert any("2つ以上" in m for m in errors('path P = absolute {\n  linux "$HOME/a"\n  linux "$HOME/b"\n}\ndata_folder P'))
    assert any("OS の行がありません" in m for m in errors("path P = absolute {\n}\ndata_folder P"))
    for bad in ('path P = absolute {\n  beos "x"\n}', 'path P = something "x"', "path P = relative", 'path P = relative "a"\npath P = relative "b"',
                'path P = relative "a"\ndata_folder Q', 'path P = relative "a"\ndata_folder P\ndata_folder P'):
        try:
            parse(with_lines(bad))
            assert False, bad
        except ParseError:
            pass


def test_組むOSの行が無ければ_生成を止める(tmp_path):
    src = tmp_path / "a.geas"
    src.write_text(with_lines(one("linux", "$HOME/.x")), encoding="utf-8")
    r = subprocess.run([sys.executable, "-m", "geas", "rust", str(src), "-o", str(tmp_path / "o"), "--host", "--runtime", "--standalone", "--target", "windows"],
                       capture_output=True, text=True, cwd=ROOT)
    assert r.returncode == 1 and "windows" in r.stderr and "パス P" in r.stderr
