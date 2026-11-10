"""実行時にユーザーが入力するパス（ltype 名前 = path）：実行中の OS の書式だけを受け、展開は OS の「場所の別名」（mac・Linux は先頭の ~、Windows は %名前%）だけ。
$名前 は展開しない。絶対パスで、.. を含まないこと。失敗は入力不正。"""
import os
import shutil
import subprocess
from pathlib import Path

import pytest

from geas.check import ERROR, check
from geas.examples import write_crate
from geas.parser import parse
from geas.rustgen import generate

needs_cargo = pytest.mark.skipif(shutil.which("cargo") is None, reason="cargo が無い")

SRC = """ltype ExportDir = path
entity Export {
  export_id: id unique
  dir: ExportDir required
}
result PutResult {
  Put(export_id: id)
}
deed put_export {
  input dir: ExportDir required
  writes Export
  result PutResult
  effects none

  let e = create Export { dir: dir }
  return Put(e.export_id)
}
nexus Flow {
  put_export: Put → reply
}
"""


def chk(src):
    return check(parse(src))


def errors(ck):
    return [d.msg for d in ck.diags if d.level == ERROR]


def test_path型は_入力と項目に使え_変換は言語が足す():
    ck = chk(SRC)
    assert ck.ready and not errors(ck), ck.diags
    out = generate(ck)
    assert "pub fn export_dir_os_path(r: &str) -> Result<String, String>" in out and "fn os_path_expand(" in out
    assert "export_dir_os_path(&self.dir)" in out  # 入力の取り込みで、型の変換が働く


def test_os_path_は_raw_から変換する_convert_の中でだけ使える():
    bad = SRC.replace("let e = create Export { dir: dir }", "let e = create Export { dir: os_path(dir) }")
    assert any("convert" in m or "os_path" in m for m in errors(chk(bad)))


def _cargo_env():
    """HOME を差し替えても cargo が動くよう、rustup と cargo の置き場は元の HOME から固定する"""
    home = Path(os.environ.get("HOME", "/root"))
    env = dict(os.environ)
    env.setdefault("RUSTUP_HOME", str(home / ".rustup"))
    env.setdefault("CARGO_HOME", str(home / ".cargo"))
    return env



@needs_cargo
def test_パスは_実行中のOSの書式で展開され_不正なら入力不正(tmp_path):
    ck = chk(SRC)
    main = """use geas_app::host::call;
fn main() {
    for v in ["/srv/out", "~/out", "~", "$HOME/out", "out/dir", "/a/../b", "~bob/x", "  ", "/tmp/x\\ny"] {
        let esc = v.replace('"', "\\\\\\"").replace('\\n', "\\\\n");
        let req = format!(r#"{{"name":"put_export","ctx":{{"new_Export_id":"A"}},"input":{{"dir":"{}"}},"store":{{}}}}"#, esc);
        println!("{:?}", call(&req));
    }
}
"""
    write_crate(ck, generate(ck), main, tmp_path, host=True)
    env = {**_cargo_env(), "HOME": "/home/tester"}
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True, env=env)
    assert r.returncode == 0, r.stderr[-2500:]
    lines = r.stdout.splitlines()
    assert len(lines) == 9, r.stdout
    ok, home_slash, home, dollar, rel, dotdot, user, blank, newline = lines
    assert ok.startswith("Ok(") and "/srv/out" in ok, ok
    assert home_slash.startswith("Ok(") and '\\"dir\\":\\"~/out\\"' in home_slash, home_slash  # 先頭の ~ は展開できると確かめる。値は入力のまま（展開は、パスを使う口の仕事）
    assert home.startswith("Ok("), home
    assert dollar.startswith("Err(") and "絶対パス" in dollar, dollar  # $名前 は展開しない（文字どおり。絶対パスでもない）
    assert rel.startswith("Err(") and "絶対パス" in rel, rel
    assert dotdot.startswith("Err(") and ".." in dotdot, dotdot
    assert user.startswith("Err(") and "~ユーザー名" in user, user
    assert blank.startswith("Err("), blank
    assert newline.startswith("Err(") and "改行" in newline, newline
    # HOME が無ければ、~ は展開できないので、入力不正（黙って別の場所にしない）
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True, env={k: v for k, v in _cargo_env().items() if k != "HOME"})
    assert r.returncode == 0, r.stderr[-2500:]
    no_home = r.stdout.splitlines()
    assert no_home[0].startswith("Ok(") and no_home[1].startswith("Err(") and "HOME" in no_home[1] and no_home[2].startswith("Err("), no_home


@needs_cargo
def test_Windowsの書式の分岐も_Linuxで動かして確かめる(tmp_path):
    """cfg!(windows) を true に置き換えた同じ関数を、Linux で動かす（Windows 実機が無くても、展開と書式の判定を確かめる）"""
    from geas.rustgen import OS_PATH_RS
    main = OS_PATH_RS.replace("cfg!(windows)", "true") + r'''
fn main() {
    for v in [r"C:\Users\x", "C:/Users/x", r"%APPDATA%\out", r"\\srv\share\x", r"%NOPE%\x", r"%APPDATA\x", "relative", r"D:\a\..\b", r"C:\a?b", "$HOME/x", "~/x", r"C:\a:b", "/usr/x"] {
        println!("{:?}", os_path_expand(v));
    }
}
'''
    (tmp_path / "main.rs").write_text(main, encoding="utf-8")
    r = subprocess.run(["rustc", "-O", "--edition", "2021", str(tmp_path / "main.rs"), "-o", str(tmp_path / "t")], capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2000:]
    r = subprocess.run([str(tmp_path / "t")], capture_output=True, text=True, env={**os.environ, "APPDATA": r"C:\Users\me\AppData\Roaming"})
    got = r.stdout.splitlines()
    assert got[0].startswith("Ok(") and got[1].startswith("Ok(") and got[3].startswith("Ok(")
    assert got[2] == r'Ok("C:\\Users\\me\\AppData\\Roaming\\out")'  # %名前% は展開する
    assert "NOPE が設定されていません" in got[4] and "閉じていません" in got[5]  # 未定義・閉じていない % は失敗
    assert all(g.startswith("Err(") and "絶対パス" in g for g in (got[6], got[9], got[10], got[12]))  # 相対・$名前・~ は、Windows では展開せず、絶対パスでもない
    assert ".. は使えません" in got[7] and "使えない文字" in got[8] and "使えない文字" in got[11]
