"""窓つき exe（geas/window）の動作テスト：生成して組み、仮想ディスプレイ（xvfb）で窓を開いて、画面が処理の定義を IPC で受けて描かれることを確かめる。
窓を組む部品（WebKitGTK の開発ファイル）や xvfb が無い環境では、組めないのでスキップする。"""
import shutil
import subprocess
import sys
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "tests" / "fixtures" / "runtime_demo" / "runtime_demo.geas"

pytestmark = pytest.mark.skipif(
    not shutil.which("xvfb-run") or subprocess.run(["pkg-config", "--exists", "webkit2gtk-4.1"]).returncode != 0,
    reason="窓の試験には xvfb と libwebkit2gtk-4.1-dev が要る",
)


@pytest.fixture(scope="module")
def built(tmp_path_factory):
    out = tmp_path_factory.mktemp("window_demo")
    subprocess.run([sys.executable, "-m", "geas", "rust", str(SRC), "-o", str(out), "--host", "--runtime", "--standalone"], check=True, capture_output=True, text=True, cwd=ROOT)
    r = subprocess.run(["cargo", "build", "--release", "--offline", "--quiet"], cwd=out / "window", capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    return out / "window" / "target" / "release" / "geas_window"


def test_窓は画面を開き_処理の呼び出しはIPCで通る(built, tmp_path):
    r = subprocess.run(["xvfb-run", "-a", str(built), "--db", str(tmp_path / "a.sqlite"), "--selftest"], capture_output=True, text=True, timeout=90)
    log = r.stdout + r.stderr  # xvfb-run は子の標準エラーを標準出力へ回すことがある
    assert r.returncode == 0, log[-2000:]
    # 画面が呼んだ問い合わせは、経路の処理が受けた（HTTP ではなく IPC から）
    assert '"log":"geas.route"' in log and '"status":200' in log


def _descendants(roots):
    """roots と、その子孫のプロセス番号（窓の下で動く WebKit の補助プロセスも含める）"""
    parent = {}
    for d in Path("/proc").iterdir():
        if d.name.isdigit():
            try:
                parent[int(d.name)] = int((d / "stat").read_text().rsplit(")", 1)[1].split()[1])
            except (OSError, ValueError, IndexError):
                pass
    found = set(roots)
    while True:
        more = {p for p, pp in parent.items() if pp in found and p not in found}
        if not more:
            return found
        found |= more


def _inet_sockets(pids):
    """pids が持つ IP のソケット（TCP・UDP。待ち受けも接続も）の番号。UNIX ドメインのソケット（プロセス間の連絡）は数えない"""
    inodes = set()
    for pid in pids:
        try:
            for fd in (Path("/proc") / str(pid) / "fd").iterdir():
                link = str(fd.readlink()) if fd.is_symlink() else ""
                if link.startswith("socket:["):
                    inodes.add(link[8:-1])
        except OSError:
            pass
    inet = set()
    for f in ("tcp", "tcp6", "udp", "udp6"):
        if Path(f"/proc/net/{f}").exists():  # IPv6 が無い環境には tcp6・udp6 が無い
            for line in Path(f"/proc/net/{f}").read_text().splitlines()[1:]:
                inet.add(line.split()[9])
    return inodes & inet


def test_窓の実行ファイルはポートを開かず_外へも接続しない(built, tmp_path):
    p = subprocess.Popen(["xvfb-run", "-a", str(built), "--db", str(tmp_path / "b.sqlite")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        time.sleep(8)
        roots = [int(x) for x in subprocess.run(["pgrep", "-x", "geas_window"], capture_output=True, text=True).stdout.split()]
        assert roots, "窓が起きていません"
        # （仮想ディスプレイ Xvfb は窓の親側で、窓の補助プロセスではないので数えない）
        # 待ち受けも接続も、窓とその補助プロセスが持つ IP のソケットは 1 つも無い（画面と処理の間は IPC で、通信路を使わない）
        assert _inet_sockets(_descendants(roots)) == set()
    finally:
        subprocess.run(["pkill", "-f", str(built)])
        p.wait(timeout=20)


def test_窓の画面の呼び出しは_fetchを通らず_ipc経由になっている():
    """画面（ui.js）は、window.ipc があれば呼び出しを IPC で渡す。ふつうのブラウザでは fetch のまま"""
    js = (ROOT / "geas" / "ui" / "ui.js").read_text(encoding="utf-8")
    assert js.count("fetch(") == js.count("xfetch(") + 1  # fetch( の直接の呼び出しは xfetch の定義の 1 か所だけ
    assert "window.ipc.postMessage" in js and "viaIpc" in js


@pytest.fixture(scope="module")
def built_resident(tmp_path_factory):
    """常駐（resident）と実通信（over http）を宣言したつぶやきの見本から、窓を組む"""
    out = tmp_path_factory.mktemp("window_resident")
    subprocess.run([sys.executable, "-m", "geas", "rust", str(ROOT / "tests" / "fixtures" / "solo_tweet" / "solo_tweet.geas"), "-o", str(out), "--host", "--runtime", "--standalone"],
                   check=True, capture_output=True, text=True, cwd=ROOT)
    r = subprocess.run(["cargo", "build", "--release", "--offline", "--quiet"], cwd=out / "window", capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    return out / "window" / "target" / "release" / "geas_window"


def test_常駐の部品は_residentを宣言したアプリだけに入る(built, built_resident):
    assert "tray-icon" not in (built.parents[2] / "Cargo.toml").read_text()
    assert "tray-icon" in (built_resident.parents[2] / "Cargo.toml").read_text()


def test_常駐の窓は開き_トレイのアイコンを作る(built_resident, tmp_path):
    r = subprocess.run(["xvfb-run", "-a", str(built_resident), "--db", str(tmp_path / "r.sqlite"), "--selftest"], capture_output=True, text=True, timeout=90)
    log = r.stdout + r.stderr
    assert r.returncode == 0, log[-2000:]
    # トレイが使える環境ではアイコンを作る。使えない環境では、窓を出したまま、常駐にせずに動く（隠れて開けなくならない）
    assert "常駐：トレイのアイコンを作りました" in log or "常駐にせず" in log


def test_常駐のアプリは_同じ保存先で1つだけ起動できる(built_resident, tmp_path):
    db = str(tmp_path / "s.sqlite")
    first = subprocess.Popen(["xvfb-run", "-a", str(built_resident), "--db", db], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        time.sleep(6)
        second = subprocess.run(["xvfb-run", "-a", str(built_resident), "--db", db], capture_output=True, text=True, timeout=60)
        assert second.returncode == 0 and "すでに起動しています" in second.stdout + second.stderr
    finally:
        subprocess.run(["pkill", "-f", str(built_resident)])
        first.wait(timeout=20)


def test_data_folder_は_環境変数を展開して作り_未設定なら止まる(built, tmp_path_factory, tmp_path):
    import os
    src = SRC.read_text(encoding="utf-8")
    src += '\nneeds env GEAS_TEST_DATA "試験で置き場を渡す"\npath D = absolute {\n  linux "$GEAS_TEST_DATA/app"\n}\ndata_folder D\n'
    code = tmp_path / "df.geas"
    code.write_text(src, encoding="utf-8")
    out = tmp_path_factory.mktemp("window_df")
    subprocess.run([sys.executable, "-m", "geas", "rust", str(code), "-o", str(out), "--host", "--runtime", "--standalone", "--target", "linux"], check=True, capture_output=True, text=True, cwd=ROOT)
    env = {**os.environ, "CARGO_TARGET_DIR": str(built.parent.parent)}  # 部品の組み直しを省く
    r = subprocess.run(["cargo", "build", "--release", "--offline", "--quiet"], cwd=out / "window", capture_output=True, text=True, env=env)
    assert r.returncode == 0, r.stderr[-2500:]
    exe = tmp_path / "df_window"
    shutil.copy(built.parent / "geas_window", exe)  # 共有の組み先を使うので、組んだ物を取り出す（他の試験の exe を上書きしない）
    base = {k: v for k, v in os.environ.items() if k != "GEAS_TEST_DATA"}
    # 設定されていれば、その下に作って、そこに DB を置く
    r = subprocess.run(["xvfb-run", "-a", str(exe), "--selftest"], capture_output=True, text=True, timeout=90, env={**base, "GEAS_TEST_DATA": str(tmp_path / "d")})
    assert r.returncode == 0, (r.stdout + r.stderr)[-2000:]
    assert (tmp_path / "d" / "app" / "app.sqlite").is_file()
    # 未設定なら、別の場所に逃げず、止まる
    r = subprocess.run(["xvfb-run", "-a", str(exe), "--selftest"], capture_output=True, text=True, timeout=90, env=base)
    assert r.returncode == 1 and "GEAS_TEST_DATA が設定されていません" in r.stdout + r.stderr
