//! Geas が生成したアプリの窓つき exe。画面と処理を 1 つの実行ファイルにまとめ、OS の WebView の窓で開く。
//! ポートを開かず、外部と通信しない。画面から処理を呼ぶ道は WebView の IPC（プロセス内）で、HTTP を通らない。
//! 画面のファイル（HTML・JS・CSS）は、カスタムプロトコル geas:// で窓の中に配る（読み込みだけ。ネットワークには出ない）。
//! 使い方: geas_window [--db app.sqlite] [--seed 別の最初のデータ.json] [--secrets シークレット.json] [--selftest]
//! シークレット（外部作用の認証）は、環境変数 GEAS_SECRET_外部_名前（大文字）か、データ用フォルダの secrets.json（{"外部.名前": "値"}）で渡す
//! --db を付けなければ、利用者のデータ用フォルダ（Windows: %APPDATA%、macOS: ~/Library/Application Support、
//!        それ以外: $XDG_DATA_HOME か ~/.local/share）の下のアプリ名のフォルダに置く。
//! --selftest：窓を開き、画面が定義を IPC で受けて描かれるのを確かめて、終了コード 0（成功）か 1 で終わる（試験用）
use geas_server::http::{self, Opts, Req};
use std::borrow::Cow;
use std::path::PathBuf;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

const INDEX_HTML: &str = include_str!("../../ui/index.html");
const UI_JS: &str = include_str!("../../ui/ui.js");
const UI_CSS: &str = include_str!("../../ui/ui.css");
const ICON_SVG: &str = include_str!("../../ui/icon.svg");

/// 窓の中の画面に付ける Content-Security-Policy。通信先は無し（呼び出しは IPC なので connect-src は要らない）
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

enum UserEvent {
    /// 画面へ返す JavaScript（IPC の答え）
    Script(String),
    Selftest(bool),
}

fn arg(name: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter().position(|x| x == name).and_then(|i| a.get(i + 1).cloned())
}

/// 利用者のデータ用フォルダ。環境変数だけで決める（外部の部品を足さない）
fn data_dir() -> PathBuf {
    let home = || std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(home)
    } else if cfg!(target_os = "macos") {
        home().join("Library").join("Application Support")
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local").join("share"))
    };
    // フォルダ名に使えない文字は _ にする
    let name: String = geas_app::APP_TITLE.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect();
    base.join(name)
}

fn asset(path: &str) -> Option<(&'static str, &'static str)> {
    match path {
        "/" | "/index.html" => Some(("text/html; charset=utf-8", INDEX_HTML)),
        "/ui.js" => Some(("text/javascript; charset=utf-8", UI_JS)),
        "/ui.css" => Some(("text/css; charset=utf-8", UI_CSS)),
        "/icon.svg" => Some(("image/svg+xml", ICON_SVG)),
        _ => None,
    }
}

/// 画面のファイルの配信。画面（screen）の URL を開き直したときも、殻（index.html）を返す
fn serve_asset(path: &str) -> wry::http::Response<Cow<'static, [u8]>> {
    let found = asset(path).or_else(|| if http::is_screen_url(path) { asset("/") } else { None });
    let (status, ctype, body) = match found {
        Some((t, b)) => (200, t, Cow::Borrowed(b.as_bytes())),
        None => (404, "text/plain; charset=utf-8", Cow::Borrowed("ありません".as_bytes())),
    };
    wry::http::Response::builder()
        .status(status)
        .header("Content-Type", ctype)
        .header("Cache-Control", "no-cache")
        .header("Content-Security-Policy", CSP)
        .header("X-Content-Type-Options", "nosniff")
        .body(body)
        .unwrap()
}

/// IPC で届く 1 件の呼び出し：{"id", "method", "url", "headers": {…}, "body"}。経路の処理（http::handle）にそのまま渡す
fn call(msg: &str, engine: &std::sync::Mutex<geas_server::engine::Engine>, opts: &Opts) -> Option<(String, bool)> {
    let m: serde_json::Value = serde_json::from_str(msg).ok()?;
    let id = m["id"].as_u64()?;
    let headers = m["headers"].as_object().map(|h| h.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect()).unwrap_or_default();
    let req = Req { method: m["method"].as_str()?.to_ascii_uppercase(), url: m["url"].as_str()?.to_string(), headers, body: m["body"].as_str().unwrap_or("").as_bytes().to_vec() };
    let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| http::handle(req, engine, std::path::Path::new("."), std::path::Path::new("."), opts)));
    let (status, headers, body) = match out {
        Ok(r) => (r.status, r.headers, String::from_utf8_lossy(&r.body).into_owned()),
        Err(_) => (500, Vec::new(), r#"{"error":"内部の異常"}"#.to_string()),
    };
    let hs: serde_json::Map<String, serde_json::Value> = headers.into_iter().map(|(k, v)| (k, serde_json::Value::String(v))).collect();
    let script = format!("window.__geasReply({},{},{},{})", id, status, serde_json::Value::Object(hs), serde_json::Value::String(body));
    Some((script, status < 400))
}

fn main() {
    let dir = data_dir();
    let db_path = arg("--db").unwrap_or_else(|| {
        std::fs::create_dir_all(&dir).expect("データ用のフォルダを作れません");
        dir.join("app.sqlite").to_string_lossy().into_owned()
    });
    let seed_path = arg("--seed");
    let seed = || match seed_path.as_deref() {
        None => geas_server::embedded_seed(),
        Some(p) => std::fs::read_to_string(p).map_err(|x| format!("最初のデータ {} を読めません: {}", p, x)),
    };
    let engine = geas_server::open_engine(&db_path, seed).unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(1)
    });
    {
        // シークレット：環境変数 GEAS_SECRET_外部_名前、なければ --secrets かデータ用フォルダの secrets.json
        let path = arg("--secrets").map(PathBuf::from).unwrap_or_else(|| dir.join("secrets.json"));
        let mut en = geas_server::engine::lock(&engine);
        if let Ok(t) = std::fs::read_to_string(&path) {
            en.secrets_file = serde_json::from_str(&t).unwrap_or_else(|_| {
                eprintln!("{} は JSON のオブジェクトではありません", path.display());
                std::process::exit(1)
            });
        }
        en.warn_missing_secrets();
    }
    geas_server::spawn_ticker(engine.clone());
    let selftest = std::env::args().any(|a| a == "--selftest");
    // 窓の中からは --test の口（時計・模擬の操作）は開けない。Host の検査は、ネットワークから来ないので要らない
    let opts = std::sync::Arc::new(Opts { test_api: false, allow_hosts: Vec::new() });

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let window = WindowBuilder::new().with_title(geas_app::APP_TITLE).build(&event_loop).expect("窓を作れません");

    let ok_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (eng, op, px, oc) = (engine.clone(), opts.clone(), proxy.clone(), ok_calls.clone());
    let builder = WebViewBuilder::new()
        .with_custom_protocol("geas".into(), |_id, request| serve_asset(request.uri().path()))
        .with_ipc_handler(move |req| {
            // 処理は別のスレッドで走らせる（窓の描画を止めない）。答えは窓のスレッドへ戻して画面に渡す
            let msg = req.into_body();
            let (eng, op, px, oc) = (eng.clone(), op.clone(), px.clone(), oc.clone());
            std::thread::spawn(move || {
                if let Some((script, ok)) = call(&msg, &eng, &op) {
                    if ok {
                        oc.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    let _ = px.send_event(UserEvent::Script(script));
                }
            });
        })
        // 窓の中の移動は自分の画面だけ（外のページへは移らない）
        .with_navigation_handler(|url| url.starts_with("geas://") || url.starts_with("http://geas.localhost"))
        .with_url(if cfg!(windows) { "http://geas.localhost/" } else { "geas://localhost/" });

    #[cfg(target_os = "linux")]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        builder.build_gtk(window.default_vbox().expect("窓の枠を取れません")).expect("WebView を作れません")
    };
    #[cfg(not(target_os = "linux"))]
    let webview = builder.build(&window).expect("WebView を作れません");

    if selftest {
        // 定義が IPC で届き、画面が描かれたら成功。15 秒で描かれなければ失敗
        let px = proxy.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(15));
            let _ = px.send_event(UserEvent::Selftest(false));
        });
    }
    let mut polls = 0u32;
    event_loop.run(move |event, _, flow| {
        *flow = if selftest { ControlFlow::WaitUntil(std::time::Instant::now() + std::time::Duration::from_millis(300)) } else { ControlFlow::Wait };
        match event {
            Event::UserEvent(UserEvent::Script(s)) => {
                let _ = webview.evaluate_script(&s);
            }
            Event::UserEvent(UserEvent::Selftest(ok)) => {
                if !ok {
                    eprintln!("selftest: 画面が描かれませんでした（成功した IPC 呼び出し {} 件）", ok_calls.load(std::sync::atomic::Ordering::Relaxed));
                }
                std::process::exit(if ok { 0 } else { 1 });
            }
            Event::NewEvents(_) if selftest => {
                polls += 1;
                let (px, calls) = (proxy.clone(), ok_calls.load(std::sync::atomic::Ordering::Relaxed));
                if polls > 2 && calls > 0 {
                    let _ = webview.evaluate_script_with_callback("document.body.innerText.length", move |n| {
                        if n.trim().parse::<u32>().unwrap_or(0) > 0 {
                            let _ = px.send_event(UserEvent::Selftest(true));
                        }
                    });
                }
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *flow = ControlFlow::Exit,
            _ => {}
        }
    });
}
