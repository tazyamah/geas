// SPDX-License-Identifier: GPL-3.0-or-later
//! 窓つき exe（geas-window）の動作テスト：生成して組み、仮想ディスプレイ（xvfb）で窓を開いて、画面が処理の定義を IPC で受けて描かれることを確かめる。
//! 窓を組む部品（WebKitGTK の開発ファイル）や xvfb が無い環境では、組めないのでスキップする（何も確かめずに通る）。
//! 元は Python 版の tests/test_window.py（タグ python-final）。
mod common;
use common::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_geas");

fn available() -> bool {
    let has = |c: &str, a: &[&str]| Command::new(c).args(a).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
    has("which", &["xvfb-run"]) && has("pkg-config", &["--exists", "webkit2gtk-4.1"])
}

struct Win {
    dir: PathBuf,
    exe: PathBuf,
}

fn build_window(tag: &str, code: &Path, extra: &[&str]) -> Win {
    let out = std::env::temp_dir().join(format!("geas-win-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let r = Command::new(BIN).env("GEAS_LANG", "ja").arg("rust").arg(code).arg("-o").arg(&out).args(["--host", "--runtime", "--standalone"]).args(extra).output().unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let b = Command::new("cargo").args(["build", "--release", "--quiet"]).current_dir(out.join("window")).output().unwrap();
    assert!(b.status.success(), "{}", tail(&String::from_utf8_lossy(&b.stderr), 2500));
    let exe = out.join("window").join("target").join("release").join("geas_window");
    Win { dir: out, exe }
}

fn demo() -> &'static Win {
    static W: OnceLock<Win> = OnceLock::new();
    W.get_or_init(|| build_window("demo", &root().join("tests/fixtures/runtime_demo/runtime_demo.geas"), &[]))
}

fn resident() -> &'static Win {
    static W: OnceLock<Win> = OnceLock::new();
    W.get_or_init(|| build_window("resident", &root().join("tests/fixtures/solo_tweet/solo_tweet.geas"), &[]))
}

fn selftest(w: &Win, db: &Path, envs: &[(&str, &str)], drop: &[&str]) -> (Option<i32>, String) {
    let mut c = Command::new("xvfb-run");
    c.args(["-a"]).arg(&w.exe);
    if !db.as_os_str().is_empty() {
        c.arg("--db").arg(db); // 空なら --db を付けず、data_folder の置き場に任せる
    }
    c.arg("--selftest");
    for d in drop {
        c.env_remove(d);
    }
    c.envs(envs.iter().copied());
    let r = c.output().unwrap();
    (r.status.code(), String::from_utf8_lossy(&r.stdout).to_string() + &String::from_utf8_lossy(&r.stderr))
}

#[test]
fn 窓は画面を開き_処理の呼び出しはipcで通る() {
    if !available() {
        return;
    }
    let t = Tmp::new("winipc");
    let (code, log) = selftest(demo(), &t.0.join("a.sqlite"), &[], &[]);
    assert_eq!(code, Some(0), "{}", tail(&log, 2000));
    // 画面が呼んだ問い合わせは、経路の処理が受けた（HTTP ではなく IPC から）
    assert!(log.contains("\"log\":\"geas.route\"") && log.contains("\"status\":200"), "{}", tail(&log, 2000));
}

fn descendants(roots: &[u32]) -> HashSet<u32> {
    let mut parent = std::collections::HashMap::new();
    for e in std::fs::read_dir("/proc").unwrap().flatten() {
        if let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() {
            if let Ok(s) = std::fs::read_to_string(e.path().join("stat")) {
                if let Some(pp) = s.rsplit(')').next().and_then(|r| r.split_whitespace().nth(1)).and_then(|v| v.parse::<u32>().ok()) {
                    parent.insert(pid, pp);
                }
            }
        }
    }
    let mut found: HashSet<u32> = roots.iter().copied().collect();
    loop {
        let more: Vec<u32> = parent.iter().filter(|(p, pp)| found.contains(pp) && !found.contains(p)).map(|(p, _)| *p).collect();
        if more.is_empty() {
            return found;
        }
        found.extend(more);
    }
}

/// pids が持つ IP のソケット（TCP・UDP。待ち受けも接続も）。UNIX ドメインのソケット（プロセス間の連絡）は数えない
fn inet_sockets(pids: &HashSet<u32>) -> HashSet<String> {
    let mut inodes = HashSet::new();
    for pid in pids {
        if let Ok(rd) = std::fs::read_dir(format!("/proc/{}/fd", pid)) {
            for fd in rd.flatten() {
                if let Ok(l) = std::fs::read_link(fd.path()) {
                    let l = l.to_string_lossy().to_string();
                    if let Some(n) = l.strip_prefix("socket:[").and_then(|x| x.strip_suffix(']')) {
                        inodes.insert(n.to_string());
                    }
                }
            }
        }
    }
    let mut inet = HashSet::new();
    for f in ["tcp", "tcp6", "udp", "udp6"] {
        if let Ok(s) = std::fs::read_to_string(format!("/proc/net/{}", f)) {
            for line in s.lines().skip(1) {
                if let Some(i) = line.split_whitespace().nth(9) {
                    inet.insert(i.to_string());
                }
            }
        }
    }
    inodes.intersection(&inet).cloned().collect()
}

fn pids_named(name: &str) -> Vec<u32> {
    let o = Command::new("pgrep").args(["-x", name]).output().unwrap();
    String::from_utf8_lossy(&o.stdout).split_whitespace().filter_map(|x| x.parse().ok()).collect()
}

#[test]
fn 窓の実行ファイルはポートを開かず_外へも接続しない() {
    if !available() {
        return;
    }
    let t = Tmp::new("winnet");
    let w = demo();
    let mut p = Command::new("xvfb-run").args(["-a"]).arg(&w.exe).arg("--db").arg(t.0.join("b.sqlite")).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    std::thread::sleep(Duration::from_secs(8));
    let roots = pids_named("geas_window");
    let socks = inet_sockets(&descendants(&roots));
    let _ = Command::new("pkill").arg("-f").arg(&w.exe).status();
    let _ = p.wait();
    assert!(!roots.is_empty(), "窓が起きていません");
    // 窓とその補助プロセスが持つ IP のソケットは 1 つも無い（画面と処理の間は IPC で、通信路を使わない）
    assert!(socks.is_empty(), "{:?}", socks);
}

#[test]
fn 窓の画面の呼び出しは_fetchを通らず_ipc経由になっている() {
    let js = std::fs::read_to_string(root().join("crates/geas-core/assets/ui/ui.js")).unwrap();
    // fetch( の直接の呼び出しは xfetch の定義の 1 か所だけ
    assert_eq!(js.matches("fetch(").count(), js.matches("xfetch(").count() + 1);
    assert!(js.contains("window.ipc.postMessage") && js.contains("viaIpc"));
}

#[test]
fn 常駐の部品は_residentを宣言したアプリだけに入る() {
    if !available() {
        return;
    }
    assert!(!std::fs::read_to_string(demo().dir.join("window/Cargo.toml")).unwrap().contains("tray-icon"));
    assert!(std::fs::read_to_string(resident().dir.join("window/Cargo.toml")).unwrap().contains("tray-icon"));
}

#[test]
fn 常駐の窓は開き_トレイのアイコンを作る() {
    if !available() {
        return;
    }
    let t = Tmp::new("winres");
    let (code, log) = selftest(resident(), &t.0.join("r.sqlite"), &[], &[]);
    assert_eq!(code, Some(0), "{}", tail(&log, 2000));
    // トレイが使える環境ではアイコンを作る。使えない環境では、窓を出したまま、常駐にせずに動く（隠れて開けなくならない）
    assert!(log.contains("常駐：トレイのアイコンを作りました") || log.contains("常駐にせず"), "{}", tail(&log, 2000));
}

#[test]
fn 常駐のアプリは_同じ保存先で1つだけ起動できる() {
    if !available() {
        return;
    }
    let t = Tmp::new("winone");
    let w = resident();
    let db = t.0.join("s.sqlite");
    let mut first = Command::new("xvfb-run").args(["-a"]).arg(&w.exe).arg("--db").arg(&db).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    std::thread::sleep(Duration::from_secs(6));
    let second = Command::new("xvfb-run").args(["-a"]).arg(&w.exe).arg("--db").arg(&db).output().unwrap();
    let _ = Command::new("pkill").arg("-f").arg(&w.exe).status();
    let _ = first.wait();
    let log = String::from_utf8_lossy(&second.stdout).to_string() + &String::from_utf8_lossy(&second.stderr);
    assert!(second.status.code() == Some(0) && log.contains("すでに起動しています"), "{}", tail(&log, 2000));
}

#[test]
fn data_folderは環境変数を展開して作り_未設定なら止まる() {
    if !available() {
        return;
    }
    let t = Tmp::new("windf");
    let src = std::fs::read_to_string(root().join("tests/fixtures/runtime_demo/runtime_demo.geas")).unwrap()
        + "\nneeds env GEAS_TEST_DATA \"試験で置き場を渡す\"\npath D = absolute {\n  linux \"$GEAS_TEST_DATA/app\"\n}\ndata_folder D\n";
    let code = t.0.join("df.geas");
    std::fs::write(&code, src).unwrap();
    let w = build_window("df", &code, &["--target", "linux"]);
    let d = t.0.join("d");
    let (c, log) = selftest(&w, Path::new(""), &[("GEAS_TEST_DATA", d.to_str().unwrap())], &[]);
    assert_eq!(c, Some(0), "{}", tail(&log, 2000));
    assert!(d.join("app").join("app.sqlite").is_file());
    // 未設定なら、別の場所に逃げず、止まる
    let (c, log) = selftest(&w, Path::new(""), &[], &["GEAS_TEST_DATA"]);
    assert!(c == Some(1) && log.contains("GEAS_TEST_DATA が設定されていません"), "{}", tail(&log, 2000));
}
