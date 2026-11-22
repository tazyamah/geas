// SPDX-License-Identifier: GPL-3.0-or-later
//! 生成したサーバーを組んで localhost で起こす道具（tests/runtime.rs と同じ作り）。組んだ依存は共有の target に置いて使い回す。
#![allow(dead_code)]
use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_geas");

/// 組む cargo を 1 本ずつにする（共有の target を同時に書かない）
static BUILD: Mutex<()> = Mutex::new(());

pub fn target_dir() -> PathBuf {
    std::env::temp_dir().join("geas-runtime-demo-target")
}

pub fn free_port() -> u16 {
    // 選んだ番号は手放してからサーバーが使うので、並んで動く別の試験が同じ番号を選びうる。このプロセスで渡した番号は、二度と渡さない
    static USED: std::sync::Mutex<Vec<u16>> = std::sync::Mutex::new(Vec::new());
    loop {
        let p = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let mut u = USED.lock().unwrap_or_else(|e| e.into_inner());
        if !u.contains(&p) {
            u.push(p);
            return p;
        }
    }
}

pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Resp {
    pub fn header(&self, k: &str) -> Option<String> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.clone())
    }
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

fn parse_resp(raw: Vec<u8>) -> Resp {
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").expect("HTTP の返事を読めません");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.lines();
    let status: u16 = lines.next().unwrap().split_whitespace().nth(1).unwrap().parse().unwrap();
    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))).collect();
    let chunked = headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked"));
    let mut data = raw[split + 4..].to_vec();
    if chunked {
        let (mut out, mut i) = (vec![], 0);
        while let Some(e) = data[i..].windows(2).position(|w| w == b"\r\n") {
            let n = usize::from_str_radix(String::from_utf8_lossy(&data[i..i + e]).trim(), 16).unwrap_or(0);
            i += e + 2;
            if n == 0 {
                break;
            }
            out.extend_from_slice(&data[i..i + n]);
            i += n + 2;
        }
        data = out;
    }
    Resp { status, headers, body: data }
}

/// 1 回ごとにつなぐ HTTP（Connection: close）。headers は追加のヘッダ
pub fn http(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: Option<&[u8]>) -> Resp {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let mut req = format!("{} {} HTTP/1.1\r\nConnection: close\r\n", method, path);
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
        req.push_str(&format!("Host: 127.0.0.1:{}\r\n", port));
    }
    for (k, v) in headers {
        req.push_str(&format!("{}: {}\r\n", k, v));
    }
    if let Some(b) = body {
        req.push_str(&format!("Content-Length: {}\r\n", b.len()));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).unwrap();
    if let Some(b) = body {
        let _ = s.write_all(b);
    }
    let mut raw = vec![];
    let _ = s.read_to_end(&mut raw);
    parse_resp(raw)
}

/// 生成して組んだアプリ。exe は組んだサーバーの写し（別のアプリを組んでも消えない）
pub struct App {
    pub dir: PathBuf,
    pub exe: PathBuf,
}

/// code（.geas）から `geas rust … --host --runtime` で生成し、サーバーを組む。extra は geas rust への追加の引数
pub fn build_app(tag: &str, code: &Path, extra: &[&str]) -> App {
    let out = std::env::temp_dir().join(format!("geas-app-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let r = Command::new(BIN).env("GEAS_LANG", "ja").arg("rust").arg(code).arg("-o").arg(&out).args(["--host", "--runtime"]).args(extra).output().unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let exe = {
        let _g = BUILD.lock().unwrap_or_else(|e| e.into_inner());
        let b = Command::new("cargo").args(["build", "--release", "--quiet"]).env("CARGO_TARGET_DIR", target_dir()).current_dir(out.join("server")).output().unwrap();
        assert!(b.status.success(), "{}", crate::common::tail(&String::from_utf8_lossy(&b.stderr), 2500));
        let exe = out.join("geas_server_copy");
        std::fs::copy(target_dir().join("release").join("geas_server"), &exe).unwrap();
        exe
    };
    App { dir: out, exe }
}

/// 起こしたサーバー（落とすとき・Drop で止める）
pub struct Srv {
    pub port: u16,
    child: Option<Child>,
    tmp: PathBuf,
}

impl Srv {
    /// 新しい DB で起こす。args はサーバーへの追加の引数（--test など）
    pub fn start(app: &App, args: &[&str]) -> Srv {
        Srv::start_env(app, args, &[], &[])
    }

    /// 環境変数つきで起こす。drop_prefix で始まる名前の環境変数は引き継がない
    pub fn start_env(app: &App, args: &[&str], env: &[(&str, &str)], drop_prefix: &[&str]) -> Srv {
        // free_port が選んだ番号を、別の試験のサーバーが先に取ることがある（起動した直後に落ちる）。そのときは番号を取り直して 3 回まで起こす
        for attempt in 0..3 {
            let port = free_port();
            let tmp = std::env::temp_dir().join(format!("geas-srv-{}-{}", std::process::id(), port));
            std::fs::create_dir_all(&tmp).unwrap();
            let mut cmd = Command::new(&app.exe);
            cmd.args(["--port", &port.to_string(), "--db"]).arg(tmp.join("a.sqlite")).arg("--pwa").arg(app.dir.join("ui")).args(args).stdout(Stdio::null()).stderr(Stdio::null());
            for (k, _) in std::env::vars() {
                if drop_prefix.iter().any(|p| k.starts_with(p)) {
                    cmd.env_remove(k);
                }
            }
            for (k, v) in env {
                cmd.env(k, v);
            }
            let child = cmd.spawn().unwrap();
            let mut s = Srv { port, child: Some(child), tmp };
            for _ in 0..100 {
                if TcpStream::connect(("127.0.0.1", port)).is_ok() && http(port, "GET", "/api/health", &[], None).status == 200 {
                    return s;
                }
                if s.child.as_mut().and_then(|c| c.try_wait().ok().flatten()).is_some() {
                    break; // 起動せずに終わった
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            s.stop();
            if attempt == 2 {
                panic!("サーバーが起動しません");
            }
        }
        unreachable!()
    }

    /// 保存先の SQLite のファイル
    pub fn db(&self) -> PathBuf {
        self.tmp.join("a.sqlite")
    }

    pub fn stop(&mut self) {
        if let Some(c) = &mut self.child {
            let _ = c.kill();
            let _ = c.wait();
        }
        self.child = None;
    }

    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// JSON の呼び出し。actor はヘッダ X-Geas-Actor（None なら付けない）
    pub fn call(&self, method: &str, path: &str, actor: Option<&str>, body: Option<Value>) -> (u16, Value) {
        let mut hs: Vec<(&str, &str)> = vec![("Content-Type", "application/json")];
        if let Some(a) = actor {
            hs.push(("X-Geas-Actor", a));
        }
        let b = body.map(|v| serde_json::to_vec(&v).unwrap());
        let r = http(self.port, method, path, &hs, b.as_deref());
        (r.status, r.json())
    }
}

impl Drop for Srv {
    fn drop(&mut self) {
        self.stop();
        let _ = std::fs::remove_dir_all(&self.tmp);
    }
}
