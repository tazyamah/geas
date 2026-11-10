"""データ用フォルダ（data_folder）の検査：OS ごとのパスの書き方、相対パスは OS なし、環境変数は必ずある名前か needs env で宣言、組む OS（--target）の行が要る。"""
import subprocess
import sys
from pathlib import Path

from geas.check import check
from geas.parser import ParseError, parse
from geas.spec import build

ROOT = Path(__file__).resolve().parent.parent
FIXTURE = ROOT / "tests" / "fixtures" / "solo_tweet" / "solo_tweet.geas"
SRC = "\n".join(l for l in FIXTURE.read_text(encoding="utf-8").splitlines() if not l.startswith("data_folder"))  # 見本の宣言は外して、試験ごとに足す
B = "\\\\"  # geas の文字列の中の \ は \\ と書く


def with_lines(lines):
    return SRC.replace("needs secret", lines + "\nneeds secret", 1)


def errors(lines):
    return [d.msg for d in check(parse(with_lines(lines))).diags if d.level == "エラー"]


def test_見本の宣言は通る():
    rows = dict(build(check(parse(FIXTURE.read_text(encoding="utf-8"))))["データ用フォルダ"])
    assert set(rows) == {"データ用フォルダ（windows）", "データ用フォルダ（macos）", "データ用フォルダ（linux）"}


def test_各OSの書き方と相対パスは通る_仕様書に出る():
    ok = "\n".join([
        f'data_folder on windows "%APPDATA%{B}ひとりつぶやき"',
        'data_folder on macos "~/Library/Application Support/tweet"',
        'data_folder on linux "${HOME}/.local/share/tweet"',
        'data_folder "data"',
    ])
    assert errors(ok) == []
    rows = dict(build(check(parse(with_lines(ok))))["データ用フォルダ"])
    assert rows["データ用フォルダ（windows）"] == "%APPDATA%\\ひとりつぶやき" and "相対パス" in "".join(rows)


def test_OS付きの行は_その_OS_の書き方の絶対パス():
    assert any("Windows のパスの書き方" in m for m in errors('data_folder on windows "$HOME/x"'))
    assert any("Windows のパスの書き方" in m for m in errors('data_folder on windows "~/x"'))
    assert any("絶対パス" in m for m in errors('data_folder on windows "data"'))
    assert any("mac・Linux のパスの書き方" in m for m in errors('data_folder on linux "%APPDATA%/x"'))
    assert any("mac・Linux のパスの書き方" in m for m in errors(f'data_folder on macos "C:{B}x"'))
    assert any("絶対パス" in m for m in errors('data_folder on linux "data/x"'))


def test_OSなしの行は_相対パスだけ_外へ出られない():
    assert any("相対パス" in m for m in errors('data_folder "/var/x"'))
    assert any("相対パス" in m for m in errors('data_folder "%APPDATA%/x"'))
    assert any("exe のあるフォルダの外" in m for m in errors('data_folder "../x"'))


def test_環境変数は_必ずある名前か_needs_env_で宣言する():
    assert any("needs env FOO" in m for m in errors(f'data_folder on windows "%FOO%{B}x"'))
    assert any("needs env XDG_DATA_HOME" in m for m in errors('data_folder on linux "$XDG_DATA_HOME/x"'))
    assert errors(f'needs env FOO "置き場の指定"\ndata_folder on windows "%FOO%{B}x"') == []
    assert any("2つ以上" in m for m in errors('needs env FOO "a"\nneeds env foo "b"'))


def test_同じOSの行は重ねられない_書き方の誤り():
    assert any("2つ以上" in m for m in errors('data_folder on linux "$HOME/a"\ndata_folder on linux "$HOME/b"'))
    try:
        parse(with_lines('data_folder on beos "x"'))
        assert False
    except ParseError:
        pass


def test_組むOSの行が無ければ_生成を止める(tmp_path):
    src = tmp_path / "a.geas"
    src.write_text(with_lines('data_folder on linux "$HOME/.x"'), encoding="utf-8")
    r = subprocess.run([sys.executable, "-m", "geas", "rust", str(src), "-o", str(tmp_path / "o"), "--host", "--runtime", "--standalone", "--target", "windows"],
                       capture_output=True, text=True, cwd=ROOT)
    assert r.returncode == 1 and "windows" in r.stderr and "data_folder" in r.stderr
