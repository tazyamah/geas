// SPDX-License-Identifier: GPL-3.0-or-later
//! 外部作用の動作テスト。
//! - over http：つぶやきの見本（tests/fixtures/solo_tweet）から、サーバーまで生成して組み、手元の模擬の X（HTTP サーバー）に向けて動かす。
//!   宣言した形で送られること（メソッド・認証・JSON の本文）、状態が結果の種類に結ばれること、シークレットが無いときは送らないこと、リダイレクトに従わないこと。
//! - over file：ノートの見本（tests/fixtures/file_demo）から、サーバーまで生成して組み、実際のフォルダに書く。宣言どおりのファイルができること、
//!   すでにあれば書かないこと、場所が無ければ結果の種類に結ばれること、OS の書式でない場所は入力不正で止まること。
//! 元は Python 版の tests/test_http_effect.py・test_file_effect.py（タグ python-final）。
mod common;
use common::serve::{build_app, target_dir, App, Srv};
use common::*;
use geas_core::rustgen::OS_PATH_RS;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

// ---- 手元の模擬の X ----
struct FakeX {
    seen: Arc<Mutex<Vec<Value>>>,
    status: Arc<Mutex<u16>>,
    redirect_to: Arc<Mutex<Option<String>>>,
    url: String,
}

impl FakeX {
    fn new() -> FakeX {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let f = FakeX { seen: Arc::new(Mutex::new(vec![])), status: Arc::new(Mutex::new(201)), redirect_to: Arc::new(Mutex::new(None)), url: format!("http://127.0.0.1:{}/2/tweets", port) };
        let (seen, status, redirect) = (f.seen.clone(), f.status.clone(), f.redirect_to.clone());
        std::thread::spawn(move || {
            for st in l.incoming() {
                let Ok(mut st) = st else { continue };
                let mut buf = vec![];
                let mut chunk = [0u8; 4096];
                loop {
                    let n = st.read(&mut chunk).unwrap_or(0);
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..p]).into_owned();
                        let hv = |name: &str| head.lines().find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.trim().to_string()));
                        let cl: usize = hv("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
                        if buf.len() >= p + 4 + cl {
                            let path = head.lines().next().unwrap().split_whitespace().nth(1).unwrap().to_string();
                            let body: Value = serde_json::from_slice(&buf[p + 4..p + 4 + cl]).unwrap_or(json!({}));
                            seen.lock().unwrap().push(json!({"path": path, "auth": hv("authorization"), "type": hv("content-type"), "body": body}));
                            let resp = if let Some(to) = redirect.lock().unwrap().clone() {
                                format!("HTTP/1.1 302 Found\r\nLocation: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", to)
                            } else {
                                let code = *status.lock().unwrap();
                                let out = if code == 201 { json!({"data": {"id": "1999", "text": "x"}}) } else { json!({"title": "Forbidden"}) }.to_string();
                                format!("HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", code, out.len(), out)
                            };
                            let _ = st.write_all(resp.as_bytes());
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
            }
        });
        f
    }

    fn seen(&self) -> Vec<Value> {
        self.seen.lock().unwrap().clone()
    }
}

fn tweet_app() -> &'static App {
    static A: OnceLock<App> = OnceLock::new();
    A.get_or_init(|| {
        let a = build_app("solo_tweet", &root().join("tests/fixtures/solo_tweet/solo_tweet.geas"), &[]);
        // over http を宣言したアプリだけに、通信の部品が入る
        assert!(std::fs::read_to_string(a.dir.join("server").join("Cargo.toml")).unwrap().contains("ureq"));
        a
    })
}

fn wait_status(s: &Srv, list: &str, key: &str, id: &str, want: &str, secs: u64) -> bool {
    for _ in 0..secs * 10 {
        let rows = s.call("GET", list, None, None).1;
        if rows["rows"].as_array().unwrap().iter().any(|r| r[key] == id && r["status"] == want) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn row_of(s: &Srv, list: &str, key: &str, id: &str) -> Value {
    s.call("GET", list, None, None).1["rows"].as_array().unwrap().iter().find(|r| r[key] == id).unwrap().clone()
}

fn start_tweet(x: &FakeX, token: Option<&str>) -> Srv {
    let eu = format!("Twitter.post={}", x.url);
    let env: Vec<(&str, &str)> = token.map(|t| vec![("GEAS_SECRET_TWITTER_TOKEN", t)]).unwrap_or_default();
    Srv::start_env(tweet_app(), &["--test", "--effect-url", &eu], &env, &["GEAS_SECRET_"])
}

fn tweet(s: &Srv, body: &str) -> String {
    s.call("POST", "/tweets", None, Some(json!({"body": body}))).1["body"]["tweet_id"].as_str().unwrap().to_string()
}

#[test]
fn 宣言どおりに送られ_201は投稿済みになる() {
    let x = FakeX::new();
    let s = start_tweet(&x, Some("tok-123"));
    let tid = tweet(&s, "宣言どおり");
    assert_eq!(s.call("POST", "/x_postings", None, Some(json!({"tweet_id": tid}))).1["result"], "Asked");
    assert!(wait_status(&s, "/tweets", "tweet_id", &tid, "Posted", 8));
    // 本文のキーは宣言どおり（text）
    assert_eq!(x.seen(), vec![json!({"path": "/2/tweets", "auth": "Bearer tok-123", "type": "application/json", "body": {"text": "宣言どおり"}})]);
    let detail = s.call("GET", &format!("/tweets/{}/details", tid), None, None).1["rows"][0].clone();
    assert_eq!(detail["x_ref"], "1999"); // 返事の data.id が、応答の中身になった
}

#[test]
fn 四xxは拒否として結ばれ_失敗になる() {
    let x = FakeX::new();
    *x.status.lock().unwrap() = 403;
    let s = start_tweet(&x, Some("tok-123"));
    let tid = tweet(&s, "こんにちは");
    s.call("POST", "/x_postings", None, Some(json!({"tweet_id": tid})));
    assert!(wait_status(&s, "/tweets", "tweet_id", &tid, "PostFailed", 8));
    assert_eq!(x.seen().len(), 1);
}

#[test]
fn シークレットが無ければ送らず_返事を待つ先の期限で失敗になる() {
    let x = FakeX::new();
    let s = start_tweet(&x, None);
    let tid = tweet(&s, "こんにちは");
    s.call("POST", "/x_postings", None, Some(json!({"tweet_id": tid})));
    std::thread::sleep(Duration::from_millis(1500));
    assert!(x.seen().is_empty() && row_of(&s, "/tweets", "tweet_id", &tid)["status"] == "Posting"); // 認証が無いまま送らない
    s.call("POST", "/api/test/advance", None, Some(json!({"secs": 130}))); // 返事が来ないまま、トリガの期限（2 分）
    assert!(wait_status(&s, "/tweets", "tweet_id", &tid, "PostFailed", 8));
}

#[test]
fn リダイレクトには従わない() {
    let x = FakeX::new();
    let other = FakeX::new();
    *x.redirect_to.lock().unwrap() = Some(other.url.clone());
    let s = start_tweet(&x, Some("tok-123"));
    let tid = tweet(&s, "こんにちは");
    s.call("POST", "/x_postings", None, Some(json!({"tweet_id": tid})));
    assert!(wait_status(&s, "/tweets", "tweet_id", &tid, "PostFailed", 8)); // 3xx は status other → Rejected
    std::thread::sleep(Duration::from_millis(500));
    assert!(other.seen().is_empty()); // 宣言していない先へは出ない
}

#[test]
fn 試験用の通信先の差し替えはtestなしでは開かない() {
    let t = Tmp::new("effecturl");
    let a = tweet_app();
    let r = Command::new(&a.exe).args(["--port", "0", "--db"]).arg(t.0.join("z.sqlite")).arg("--pwa").arg(a.dir.join("ui")).args(["--effect-url", "Twitter.post=http://127.0.0.1:1/"]).output().unwrap();
    assert!(r.status.code() == Some(2) && String::from_utf8_lossy(&r.stderr).contains("--test"), "{:?} {}", r.status, String::from_utf8_lossy(&r.stderr));
}

// ---- over file ----
fn file_app() -> &'static App {
    static A: OnceLock<App> = OnceLock::new();
    A.get_or_init(|| {
        let a = build_app("file_demo", &root().join("tests/fixtures/file_demo/file_demo.geas"), &[]);
        // over http を書かなければ、通信の部品は入らない
        assert!(!std::fs::read_to_string(a.dir.join("server").join("Cargo.toml")).unwrap().contains("ureq"));
        // fileio の単体試験（engine の単体試験は、runtime_demo の小包に固有なので走らせない）
        let t = Command::new("cargo").args(["test", "--release", "--quiet", "fileio::"]).env("CARGO_TARGET_DIR", target_dir()).current_dir(a.dir.join("server")).output().unwrap();
        let out = String::from_utf8_lossy(&t.stdout).to_string() + &String::from_utf8_lossy(&t.stderr);
        assert!(t.status.success() && out.contains("3 passed"), "{}", tail(&out, 2500));
        a
    })
}

fn note(s: &Srv, body: &str) -> String {
    s.call("POST", "/notes", None, Some(json!({"body": body}))).1["body"]["note_id"].as_str().unwrap().to_string()
}

fn export(s: &Srv, nid: &str, dir: &str) -> Value {
    s.call("POST", "/note_exports", None, Some(json!({"note_id": nid, "dir": dir}))).1
}

#[test]
fn 宣言した場所に_宣言したファイルが_できる() {
    let s = Srv::start(file_app(), &["--test"]);
    let t = Tmp::new("fileout");
    let out_dir = t.0.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();
    let nid = note(&s, "ファイルに書く");
    assert_eq!(export(&s, &nid, &out_dir.to_string_lossy())["result"], "Asked");
    assert!(wait_status(&s, "/notes", "note_id", &nid, "Exported", 8));
    assert_eq!(std::fs::read_to_string(out_dir.join("note.txt")).unwrap(), "ファイルに書く");
    assert_eq!(row_of(&s, "/notes", "note_id", &nid)["size"], "ファイルに書く".len()); // 書いたバイト数が、応答の中身になった
    let names: Vec<String> = std::fs::read_dir(&out_dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, vec!["note.txt"]); // 宣言していないファイルは作らない
}

#[test]
fn すでにあれば書かない() {
    let s = Srv::start(file_app(), &["--test"]);
    let t = Tmp::new("fileexists");
    let out_dir = t.0.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();
    std::fs::write(out_dir.join("note.txt"), "元の中身").unwrap();
    let nid = note(&s, "上書きされない");
    export(&s, &nid, &out_dir.to_string_lossy());
    assert!(wait_status(&s, "/notes", "note_id", &nid, "ExportFailed", 8)); // exists → Refused
    assert_eq!(std::fs::read_to_string(out_dir.join("note.txt")).unwrap(), "元の中身");
}

#[test]
fn 場所が無ければ失敗になる() {
    let s = Srv::start(file_app(), &["--test"]);
    let t = Tmp::new("filenone");
    let nid = note(&s, "ひとこと");
    export(&s, &nid, &t.0.join("nothing").to_string_lossy());
    assert!(wait_status(&s, "/notes", "note_id", &nid, "ExportFailed", 8)); // not_found → Refused
    assert!(!t.0.join("nothing").exists()); // 場所を作らない
}

#[test]
fn osの書式でない場所は_入力不正で止まる() {
    let s = Srv::start(file_app(), &["--test"]);
    let t = Tmp::new("filebad");
    let out_dir = t.0.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();
    let nid = note(&s, "ひとこと");
    for bad in ["relative/dir".to_string(), format!("{}/../x", out_dir.to_string_lossy()), "$HOME/x".to_string(), String::new()] {
        let r = export(&s, &nid, &bad);
        assert_ne!(r.get("result"), Some(&json!("Asked")), "{} {}", bad, r);
    }
    assert!(row_of(&s, "/notes", "note_id", &nid)["status"] == "Draft" && std::fs::read_dir(&out_dir).unwrap().next().is_none());
}

#[test]
fn ospathは生成した変換と同じ本文() {
    let mine = std::fs::read_to_string(root().join("crates/geas-core/assets/server/src/ospath.rs")).unwrap();
    assert!(mine.contains(&OS_PATH_RS.replacen("\nfn os_path_expand", "\npub fn os_path_expand", 1)));
}
