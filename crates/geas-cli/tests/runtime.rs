// SPDX-License-Identifier: GPL-3.0-or-later
//! サーバーの実行部の動作テスト：小さな .geas（tests/fixtures/runtime_demo）から、サーバーまで生成して localhost で動かし、
//! 並列（par）・待ち合わせ（all）・期限（deadline）・出来事（each）・rule・冪等キー・REST の状態・再起動を、お店アプリを使わずに確かめる。
//! どの機能にどのテストがあるかは docs/language-test-map.md。（Python 版 tests/test_runtime.py の移し）
//! 生成したサーバーを組む（初回は依存の crate を取るので通信が要り、時間もかかる）。組みは 1 回だけで、テストごとに別のポート・DB でサーバーを起こす。
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_geas");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn demo(name: &str) -> PathBuf {
    root().join("tests").join("fixtures").join("runtime_demo").join(name)
}

/// 生成して組んだ crate の置き場（1 回だけ作る）
fn built() -> &'static Path {
    static B: OnceLock<PathBuf> = OnceLock::new();
    B.get_or_init(|| {
        let out = std::env::temp_dir().join(format!("geas-runtime-demo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let r = Command::new(BIN).env("GEAS_LANG", "ja").arg("rust").arg(demo("runtime_demo.geas")).arg("-o").arg(&out).args(["--host", "--runtime", "--plan"]).arg(demo("runtime_demo_plan.md")).output().unwrap();
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        let b = cargo(&["build", "--release", "--quiet"], &out.join("server"));
        assert!(b.status.success(), "{}", tail(&String::from_utf8_lossy(&b.stderr), 2500));
        out
    })
}

/// 生成した crate の cargo。作業フォルダはプロセスごとに別だが、組んだ依存は共有の target に置いて、次の実行に使い回す
fn target_dir() -> PathBuf {
    std::env::temp_dir().join("geas-runtime-demo-target")
}

fn cargo(args: &[&str], dir: &Path) -> std::process::Output {
    Command::new("cargo").args(args).env("CARGO_TARGET_DIR", target_dir()).current_dir(dir).output().unwrap()
}

fn tail(s: &str, n: usize) -> String {
    let cs: Vec<char> = s.chars().collect();
    cs[cs.len().saturating_sub(n)..].iter().collect()
}

fn exe() -> PathBuf {
    target_dir().join("release").join("geas_server")
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

struct Resp {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Resp {
    fn header(&self, k: &str) -> Option<String> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.clone())
    }
    fn json(&self) -> Value {
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
fn http(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: Option<&[u8]>) -> Resp {
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
        let _ = s.write_all(b); // 大きすぎる本文は、先に断られて書き切れないことがある
    }
    let mut raw = vec![];
    let _ = s.read_to_end(&mut raw);
    parse_resp(raw)
}

struct Server {
    db: PathBuf,
    extra: Vec<String>,
    test_api: bool,
    child: Option<Child>,
    port: u16,
    etag: Option<String>,
    _tmp: Option<PathBuf>,
}

impl Server {
    fn new(db: &Path, extra: &[&str], test_api: bool) -> Server {
        Server { db: db.to_path_buf(), extra: extra.iter().map(|s| s.to_string()).collect(), test_api, child: None, port: 0, etag: None, _tmp: None }
    }

    /// 既定の（--test つきの）サーバーを、新しい DB で起こす。模擬の返事は、返事までを 5 秒にして時計を進めて返させる
    fn standard() -> Server {
        let d = std::env::temp_dir().join(format!("geas-rt-{}-{}", std::process::id(), free_port()));
        std::fs::create_dir_all(&d).unwrap();
        let mut s = Server::new(&d.join("demo.sqlite"), &[], true);
        s._tmp = Some(d);
        s.start();
        s.mock(json!({"PaySettled": {"outcome": "Approved", "drop": false}, "DepotSettled": {"outcome": "Reserved", "drop": false}}));
        s
    }

    fn start(&mut self) {
        self.port = free_port();
        let b = built();
        let mut cmd = Command::new(exe());
        cmd.args(["--port", &self.port.to_string(), "--db"]).arg(&self.db).arg("--pwa").arg(b.join("ui"));
        if self.test_api {
            cmd.arg("--test");
        }
        cmd.arg("--dev").arg(b.join("dev")).args(&self.extra).stdout(Stdio::null()).stderr(Stdio::null());
        self.child = Some(cmd.spawn().unwrap());
        for _ in 0..100 {
            if TcpStream::connect(("127.0.0.1", self.port)).is_ok() && http(self.port, "GET", "/api/health", &[], None).status == 200 {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        self.stop();
        panic!("サーバーが起動しません");
    }

    fn stop(&mut self) {
        if let Some(c) = &mut self.child {
            let _ = c.kill();
            let _ = c.wait();
        }
        self.child = None;
    }

    fn call(&mut self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        self.call_if(method, path, body, None)
    }

    fn call_if(&mut self, method: &str, path: &str, body: Option<Value>, if_match: Option<&str>) -> (u16, Value) {
        let mut hs = vec![("Content-Type", "application/json")];
        if let Some(t) = if_match {
            hs.push(("If-Match", t));
        }
        let b = body.map(|v| serde_json::to_vec(&v).unwrap());
        let r = http(self.port, method, path, &hs, b.as_deref());
        self.etag = r.header("ETag");
        (r.status, r.json())
    }

    /// 一覧で読んだ、その小包の行の版（PUT・DELETE の If-Match に載せる値）。行が無ければ、どの行の版でもない値
    fn tag(&mut self, pid: &str) -> String {
        match self.parcels().get(pid) {
            Some(row) => format!("\"{}\"", row["_etag"].as_str().unwrap()),
            None => "\"none\"".to_string(),
        }
    }

    /// PUT・DELETE：一覧で読んだ行の版を If-Match に載せて呼ぶ
    fn write(&mut self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let t = self.tag(path.rsplit('/').next().unwrap());
        self.call_if(method, path, body, Some(&t))
    }

    fn mock(&mut self, replies: Value) {
        self.call("POST", "/api/test/settings", Some(json!({"delay_secs": 5, "replies": replies})));
    }

    fn state(&mut self) -> Value {
        self.call("GET", "/api/test/state", None).1
    }

    fn advance(&mut self, secs: i64) -> Vec<Value> {
        self.call("POST", "/api/test/advance", Some(json!({"secs": secs}))).1["fired"].as_array().cloned().unwrap_or_default()
    }

    fn parcels(&mut self) -> std::collections::BTreeMap<String, Value> {
        self.call("GET", "/parcels", None).1["rows"].as_array().unwrap().iter().map(|p| (p["parcel_id"].as_str().unwrap().to_string(), p.clone())).collect()
    }

    fn status_of(&mut self, pid: &str) -> String {
        self.parcels()[pid]["status"].as_str().unwrap().to_string()
    }

    fn accept(&mut self, kind: &str) -> (String, Value) {
        let (code, j) = self.call("POST", "/parcels", Some(json!({"kind": kind})));
        assert_eq!(code, 201, "{}", j);
        (j["body"]["parcel_id"].as_str().unwrap().to_string(), j)
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
        if let Some(d) = &self._tmp {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("geas-rt-{}-{}-{}", tag, std::process::id(), free_port()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn effects(s: &mut Server, name: &str) -> Vec<Value> {
    s.state()["effects"].as_array().unwrap().iter().filter(|e| e["name"] == name).cloned().collect()
}

fn keys(v: &[Value]) -> Vec<String> {
    v.iter().map(|e| e["idem_key"].as_str().unwrap().to_string()).collect()
}

fn has_deed(j: &Value, deeds: &[&str]) -> bool {
    let t: std::collections::BTreeSet<&str> = j["trace"].as_array().unwrap().iter().map(|t| t["deed"].as_str().unwrap()).collect();
    t == deeds.iter().copied().collect()
}

#[test]
fn ruleは上から順に当てはまった行の値になり新しいidは自動で採番される() {
    let mut s = Server::standard();
    let (a, _) = s.accept("light");
    let (b, _) = s.accept("heavy");
    assert_eq!((a.as_str(), b.as_str()), ("parcel-0001", "parcel-0002"));
    let rows = s.parcels();
    assert!(rows[&a]["fee"] == 300 && rows[&b]["fee"] == 900); // kind == "heavy" の行だけ 900、他は default
}

#[test]
fn parの枝は両方動き待ち合わせは全部の返事がそろうまで進まない() {
    let mut s = Server::standard();
    let (pid, j) = s.accept("light");
    assert_eq!(j["trace"][0]["deed"], "accept");
    assert!(has_deed(&j, &["accept", "request_pay", "request_reserve"]));
    assert_eq!(s.status_of(&pid), "Waiting");
    assert_eq!(keys(&effects(&mut s, "Pay.charge")), vec![pid.clone()]);
    assert_eq!(keys(&effects(&mut s, "Depot.reserve")), vec![pid.clone()]); // joins で、決済と同じ待ちに合流する
    let w = s.state()["waits"].as_array().unwrap().clone();
    assert!(w.len() == 1 && w[0]["trigger"] == "Pending" && w[0]["args"] == json!({"parcel_id": pid}));
    // 決済の返事だけ届いても、倉庫の返事を待つ
    s.mock(json!({"DepotSettled": {"drop": true}}));
    let (pid2, _) = s.accept("light");
    s.advance(6);
    assert_eq!(s.status_of(&pid2), "Waiting");
    assert!(s.state()["waits"].as_array().unwrap().iter().any(|x| x["args"] == json!({"parcel_id": pid2}) && !x["collected"]["PaySettled"].is_null()));
}

#[test]
fn 返事がそろうと行き先へ進み承認の中身が渡る() {
    let mut s = Server::standard();
    let (pid, _) = s.accept("light");
    let fired = s.advance(6);
    assert!(fired.iter().any(|f| f["outcome"]["fired"] == "settle"), "{:?}", fired);
    assert_eq!(s.status_of(&pid), "Shipped"); // Approved → ship
    assert_eq!(keys(&effects(&mut s, "Courier.send")), vec![pid]); // no_wait の外部作用も、冪等キーつきで出る
    assert!(s.state()["waits"].as_array().unwrap().is_empty());
}

#[test]
fn 拒否の返事は結果の種類が違うので別の行き先へ進む() {
    let mut s = Server::standard();
    s.mock(json!({"PaySettled": {"outcome": "Declined"}}));
    let (pid, _) = s.accept("light");
    s.advance(6);
    assert_eq!(s.status_of(&pid), "Canceled"); // PayFailed → cancel（auth_id が無い）
    assert!(effects(&mut s, "Courier.send").is_empty());
}

#[test]
fn 期限が来るまでは待ち来たら期限の行き先へ進む() {
    let mut s = Server::standard();
    s.mock(json!({"PaySettled": {"drop": true}}));
    let (pid, _) = s.accept("light");
    s.advance(6);
    s.advance(30 * 60 - 20);
    assert_eq!(s.status_of(&pid), "Waiting"); // 30 分に少し足りない
    let fired = s.advance(30);
    assert!(fired.iter().any(|f| f["deadline"] == "Pending"));
    assert_eq!(s.status_of(&pid), "Canceled");
    assert!(s.state()["waits"].as_array().unwrap().is_empty());
}

#[test]
fn つながりが戻る出来事は返事待ちの1件ずつに引き直し同じ冪等キーで請求する() {
    let mut s = Server::standard();
    // 返事待ちが無ければ、何も起きない
    let (code, ev) = s.call("POST", "/reconnections", Some(json!({})));
    assert!(code == 201 && ev["runs"].as_array().unwrap().is_empty());
    s.mock(json!({"PaySettled": {"drop": true}}));
    let (d, _) = s.accept("light");
    let (e, _) = s.accept("light");
    s.advance(6);
    s.mock(json!({"PaySettled": {"drop": false}}));
    let (code, ev) = s.call("POST", "/reconnections", Some(json!({})));
    assert_eq!(code, 201);
    let deeds: Vec<&str> = ev["runs"].as_array().unwrap().iter().flat_map(|r| r["trace"].as_array().unwrap().iter().map(|t| t["deed"].as_str().unwrap())).collect();
    assert_eq!(deeds, vec!["recheck", "recheck"]);
    let charges = effects(&mut s, "Pay.charge");
    let uniq: std::collections::BTreeSet<String> = keys(&charges).into_iter().collect();
    assert!(charges.len() == 4 && uniq == [d.clone(), e.clone()].into_iter().collect()); // 1 件につき 2 回出したが、冪等キーは 1 つだけ
    s.advance(6);
    assert_eq!([s.status_of(&d), s.status_of(&e)], ["Shipped", "Shipped"]);
}

#[test]
fn 結果の種類ごとのhttpの状態を返し入力の範囲外は境界で止める() {
    let mut s = Server::standard();
    let (pid, _) = s.accept("light");
    s.mock(json!({"PaySettled": {"drop": true}}));
    assert_eq!(s.call("POST", "/settlements", Some(json!({"parcel_id": "parcel-9999"}))).0, 404); // NoParcel
    let (code, j) = s.call("POST", "/cancellations", Some(json!({"parcel_id": pid})));
    assert!(code == 201 && j["result"] == "Applied");
    assert_eq!(s.call("POST", "/cancellations", Some(json!({"parcel_id": pid}))).0, 409); // 二度目は WrongStatus
    assert_eq!(s.call("POST", "/parcels", Some(json!({"kind": ""}))).0, 400); // text 1..10 の下限
    assert_eq!(s.call("POST", "/parcels", Some(json!({"kind": "x".repeat(11)}))).0, 400); // 上限
    assert_eq!(s.call("POST", "/parcels", Some(json!({}))).0, 400); // required の入力が無い
    assert_eq!(s.call("DELETE", "/parcels", None).0, 404); // 宣言していない動作は無い
    assert_eq!(s.parcels().len(), 1); // 弾かれた注文は残らない
}

#[test]
fn 再起動しても待ちと時計の先は続く() {
    let mut s = Server::standard();
    s.mock(json!({"PaySettled": {"drop": true}}));
    let (pid, _) = s.accept("light");
    s.advance(6);
    s.stop();
    s.start();
    assert_eq!(s.status_of(&pid), "Waiting");
    assert_eq!(s.state()["waits"].as_array().unwrap().len(), 1);
    s.advance(30 * 60 + 10);
    assert_eq!(s.status_of(&pid), "Canceled");
    let (pid2, _) = s.accept("light");
    assert_eq!(pid2, "parcel-0002"); // ID の採番も再起動で戻らない
}

fn run_exe(args: &[&str]) -> (i32, String) {
    let r = Command::new(exe()).args(args).output().unwrap();
    (r.status.code().unwrap_or(-1), String::from_utf8_lossy(&r.stderr).into_owned())
}

#[test]
fn 最初のデータはdbが空のときだけ読む() {
    // 行のある DB で起こすときは、最初のデータ（埋め込みか --seed）を読まない（大きいと、起動のたびに時間とメモリを使う）。
    // 読めない最初のデータを渡しても、行のある DB なら起動し、空の DB なら入れられずに止まる
    let d = tmpdir("seed");
    let broken = d.join("broken_seed.json");
    std::fs::write(&broken, "{ JSON ではない").unwrap();
    let mut s = Server::new(&d.join("seeded.sqlite"), &[], true);
    s.start();
    let (pid, _) = s.accept("light");
    s.stop();
    s.extra = vec!["--seed".into(), broken.to_string_lossy().into_owned()];
    s.start();
    assert_eq!(s.parcels().keys().cloned().collect::<Vec<_>>(), vec![pid]);
    s.stop();
    let (code, err) = run_exe(&["--port", "0", "--db", d.join("empty.sqlite").to_str().unwrap(), "--seed", broken.to_str().unwrap()]);
    assert!(code != 0 && err.contains("seed.json"), "{}", tail(&err, 1000));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn 型の範囲の外の行は最初のデータからもdbからも入らない() {
    // 検査器は「保存の値は型の範囲に入っている」を前提に証明する。だから保存の行も、読むときに型の範囲で確かめる。
    // 範囲の外の最初のデータは入れずに止まり（DB は空のまま）、外で書き換えた DB の行は、起動のときに止まる
    let d = tmpdir("range");
    let row = json!({"parcel_id": "p-1", "kind": "light", "fee": 300, "status": "Received", "refund_ref": null});
    for (bad, word) in [(json!({"fee": -1}), "Parcel.fee"), (json!({"kind": "x".repeat(11)}), "Parcel.kind")] {
        let mut r = row.clone();
        for (k, v) in bad.as_object().unwrap() {
            r[k] = v.clone();
        }
        let seed = d.join("bad_seed.json");
        std::fs::write(&seed, json!({"Parcel": {"p-1": r}}).to_string()).unwrap();
        let db = d.join(format!("bad_{}.sqlite", word));
        let (code, err) = run_exe(&["--port", "0", "--db", db.to_str().unwrap(), "--seed", seed.to_str().unwrap()]);
        assert!(code != 0 && err.contains(word) && err.contains("範囲外"), "{}", tail(&err, 1000));
        let c = rusqlite::Connection::open(&db).unwrap();
        let n: i64 = c.query_row("SELECT count(*) FROM \"Parcel\"", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "範囲の外の行が DB に入っている");
    }
    // 行を置くキーと、行の中の ID が違う最初のデータも入らない
    let seed = d.join("key_seed.json");
    std::fs::write(&seed, json!({"Parcel": {"p-2": row}}).to_string()).unwrap();
    let (code, err) = run_exe(&["--port", "0", "--db", d.join("key.sqlite").to_str().unwrap(), "--seed", seed.to_str().unwrap()]);
    assert!(code != 0 && err.contains("キー「p-2」と、行の中の ID「p-1」が違います"), "{}", tail(&err, 1000));
    // 範囲の中の行は入る
    let seed = d.join("good_seed.json");
    std::fs::write(&seed, json!({"Parcel": {"p-1": row}}).to_string()).unwrap();
    let mut s = Server::new(&d.join("good.sqlite"), &["--seed", seed.to_str().unwrap()], true);
    s.start();
    assert_eq!(s.parcels()["p-1"]["fee"], 300);
    s.stop();
    // DB の行を外から書き換えると、次の起動で止まる
    {
        let c = rusqlite::Connection::open(&s.db).unwrap();
        c.execute("UPDATE \"Parcel\" SET v = replace(v, '\"fee\":300', '\"fee\":-5')", []).unwrap();
    }
    let (code, err) = run_exe(&["--port", "0", "--db", s.db.to_str().unwrap()]);
    assert!(code != 0 && err.contains("Parcel.fee=-5 は範囲外"), "{}", tail(&err, 1000));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn 計画書のurlの具体例から生成したテストが通る() {
    let r = cargo(&["test", "--release", "--quiet", "--test", "url_examples"], &built().join("server"));
    let out = String::from_utf8_lossy(&r.stdout).into_owned() + &String::from_utf8_lossy(&r.stderr);
    assert!(r.status.success() && out.contains("1 passed"), "{}", tail(&out, 3000));
}

#[test]
fn 計画書の画面の具体例から生成したテストが通る() {
    // 入力欄・行・操作・通知・design を、実際の画面（PWA）で通す。node と Playwright が無ければ、その中で飛ばされる
    let r = cargo(&["test", "--release", "--quiet", "--test", "screen_examples"], &built().join("server"));
    let out = String::from_utf8_lossy(&r.stdout).into_owned() + &String::from_utf8_lossy(&r.stderr);
    assert!(r.status.success(), "{}", tail(&out, 3000));
}

#[test]
fn update_destroy入れ子は宣言のurlと状態で動く() {
    let mut s = Server::standard();
    let (pid, _) = s.accept("light");
    s.advance(6); // 返事がそろって発送済みになる
    assert_eq!(s.status_of(&pid), "Shipped");
    assert_eq!(s.write("PUT", &format!("/parcels/{}", pid), Some(json!({"kind": "heavy"}))).0, 200); // update：URL の ID が処理の ID の入力になる
    assert_eq!(s.parcels()[&pid]["kind"], "heavy");
    assert_eq!(s.write("PUT", "/parcels/parcel-9999", Some(json!({"kind": "heavy"}))).0, 404);
    assert_eq!(s.write("PUT", &format!("/parcels/{}", pid), Some(json!({"kind": ""}))).0, 400);
    let (code, j) = s.call("GET", &format!("/parcels/{}/details", pid), None); // 入れ子：親の ID が問い合わせの入力になる
    assert!(code == 200 && j["rows"].as_array().unwrap().iter().map(|r| r["parcel_id"].as_str().unwrap()).collect::<Vec<_>>() == vec![pid.as_str()]);
    let (other, _) = s.accept("light");
    assert_eq!(s.write("DELETE", &format!("/parcels/{}", other), None).0, 409); // 消してよい状態（発送済み）ではない
    assert_eq!(s.write("DELETE", "/parcels/parcel-9999", None).0, 404);
    assert_eq!(s.write("DELETE", &format!("/parcels/{}", pid), None).0, 200); // destroy
    let rows = s.parcels();
    assert!(!rows.contains_key(&pid) && rows.contains_key(&other));
}

#[test]
fn putとdeleteは読んだときの行の版が要る() {
    // 書き込みの版の確認。If-Match が無ければ 428、今の版と違えば 412 で、どちらも処理を走らせない。版は、一覧の行の _etag
    let mut s = Server::standard();
    let (pid, _) = s.accept("light");
    let seen = s.tag(&pid);
    let path = format!("/parcels/{}", pid);
    let (code, j) = s.call("PUT", &path, Some(json!({"kind": "heavy"})));
    assert!(code == 428 && j["error"].as_str().unwrap().contains("If-Match") && s.etag.is_none()); // 無い：断る。今の版は教えない
    assert!(s.call("DELETE", &path, None).0 == 428 && s.parcels()[&pid]["kind"] == "light");
    assert_eq!(s.call("PUT", "/parcels/parcel-9999", Some(json!({"kind": "heavy"}))).0, 428); // 行が無くても、まず版を求める
    assert_eq!(s.call_if("PUT", &path, Some(json!({"kind": "heavy"})), Some(&seen)).0, 200);
    let after = s.etag.clone().unwrap();
    assert!(after != seen && after == s.tag(&pid)); // 書き換えたあとの版を返す。一覧の版も変わる
    // 前に読んだ版では、もう書けない（別の人が先に書き換えた、と同じ）
    let (code, j) = s.call_if("PUT", &path, Some(json!({"kind": "small"})), Some(&seen));
    assert!(code == 412 && j["error"].as_str().unwrap().contains("変わっています") && s.parcels()[&pid]["kind"] == "heavy");
    assert!(s.call_if("DELETE", &path, None, Some(&seen)).0 == 412 && s.parcels().contains_key(&pid));
    assert_eq!(s.call_if("PUT", &path, Some(json!({"kind": "small"})), Some("*")).0, 412); // どの版でもよい、は無い
    assert_eq!(s.call_if("PUT", &path, Some(json!({"kind": "small"})), Some(&format!("W/{}", after))).0, 412);
    // 並べて書いた版のどれかが合えばよい。中身が元に戻れば、版も元に戻る（中身から作る値）
    assert_eq!(s.call_if("PUT", &path, Some(json!({"kind": "light"})), Some(&format!("\"r0\", {}", after))).0, 200);
    assert_eq!(s.tag(&pid), seen);
    // POST の命令（作成）には要らない
    assert_ne!(s.accept("light").0, pid);
}

#[test]
fn どれか1つの返事で進むトリガは1つ届けば進み届かなければ期限で進む() {
    let mut s = Server::standard();
    let (pid, _) = s.accept("light");
    s.advance(6);
    assert_eq!(s.call("POST", "/refund_requests", Some(json!({"parcel_id": "parcel-9999"}))).0, 404);
    assert_eq!(s.call("POST", "/refund_requests", Some(json!({"parcel_id": pid}))).0, 201);
    let w: Vec<Value> = s.state()["waits"].as_array().unwrap().iter().map(|w| w["trigger"].clone()).collect();
    assert_eq!(w, vec![json!("RefundPending")]);
    let fired = s.advance(6); // RefundSettled の返事が 1 つ届けば、待ち合わせずに行き先へ
    assert!(fired.iter().any(|f| f["outcome"]["fired"] == "mark_refunded"), "{:?}", fired);
    assert!(s.state()["waits"].as_array().unwrap().is_empty());
    // 返事が落ちると、期限（10 分）の行き先へ進む
    s.mock(json!({"RefundSettled": {"drop": true}}));
    assert_eq!(s.call("POST", "/refund_requests", Some(json!({"parcel_id": pid}))).0, 201);
    s.advance(6);
    assert_eq!(s.state()["waits"].as_array().unwrap().len(), 1);
    let fired = s.advance(10 * 60);
    assert!(fired.iter().any(|f| f["deadline"] == "RefundPending" && f["outcome"]["fired"] == "mark_refunded"), "{:?}", fired);
}

#[test]
fn アプリを開いた出来事も返事待ちを引き直す() {
    let mut s = Server::standard();
    s.mock(json!({"PaySettled": {"drop": true}}));
    let (pid, _) = s.accept("light");
    s.advance(6);
    s.mock(json!({"PaySettled": {"drop": false}}));
    let (code, ev) = s.call("POST", "/openings", Some(json!({})));
    let deeds: Vec<&str> = ev["runs"].as_array().unwrap().iter().flat_map(|r| r["trace"].as_array().unwrap().iter().map(|t| t["deed"].as_str().unwrap())).collect();
    assert!(code == 201 && deeds == vec!["recheck"]);
    s.advance(6);
    assert_eq!(s.status_of(&pid), "Shipped");
}

#[test]
fn 画面とapiの応答にはcspが付き生成したhtmlにインラインのスクリプトは無い() {
    let s = Server::standard();
    for path in ["/", "/parcels/new", "/ui.js", "/manifest.webmanifest", "/api/defs", "/api/health"] {
        let csp = http(s.port, "GET", path, &[], None).header("Content-Security-Policy").unwrap_or_default();
        assert!(csp.contains("script-src 'self'") && csp.contains("connect-src 'self'") && csp.contains("object-src 'none'"), "{} {}", path, csp); // 見つからない応答（404）にも付く
    }
    // 生成した HTML には、インラインのスクリプト・イベントハンドラ属性・style 属性が無い（CSP で止まるものを書いていない）
    // 先読み（(?!…)）は regex に無いので、<script の開きタグを取り出して、src= があるかを見る
    let tag = regex::Regex::new(r"<script[^>]*>").unwrap();
    let src = regex::Regex::new(r"\ssrc=").unwrap();
    let on = regex::Regex::new(r"\son[a-z]+\s*=").unwrap();
    for dir in ["ui", "ext", "dev"] {
        for e in std::fs::read_dir(built().join(dir)).unwrap() {
            let p = e.unwrap().path();
            if p.extension().map_or(false, |x| x == "html") {
                let text = std::fs::read_to_string(&p).unwrap();
                assert!(!tag.find_iter(&text).any(|m| !src.is_match(m.as_str())) && !on.is_match(&text) && !text.contains("style=\""), "{}", p.display());
            }
        }
    }
    let m: Value = serde_json::from_str(&std::fs::read_to_string(built().join("ext").join("manifest.json")).unwrap()).unwrap();
    assert_eq!(m["content_security_policy"]["extension_pages"], "script-src 'self'; object-src 'self'");
}

#[test]
fn テスト用の口と宣言を通らない口はtestを付けたときだけ開く() {
    // --test なしで外から呼べるのは、仕様書の URL の表にある口と、画面が使う /api/defs・/api/health だけ。
    // 時計を進める・模擬を変える・処理を名前で呼ぶ（resource の宣言を通らない）口は 404
    let d = tmpdir("plain");
    let mut s = Server::new(&d.join("plain.sqlite"), &[], false);
    s.start();
    let cases: [(&str, &str, Option<Value>); 9] = [
        ("GET", "/api/test/state", None),
        ("POST", "/api/test/advance", Some(json!({"secs": 3600}))),
        ("POST", "/api/test/settings", Some(json!({"delay_secs": 0}))),
        ("POST", "/api/test/tick", Some(json!({}))),
        ("GET", "/api/queries/parcel_list", None),
        ("POST", "/api/deeds/accept", Some(json!({"input": {"kind": "light"}}))),
        ("POST", "/api/events/Reconnected", Some(json!({}))),
        ("GET", "/test.html", None),
        ("GET", "/test.js", None),
    ];
    for (m, p, b) in cases {
        assert_eq!(s.call(m, p, b).0, 404, "{} {}", m, p);
    }
    assert!(s.call("GET", "/parcels", None).1["rows"].as_array().unwrap().is_empty()); // 名前で呼ぶ口からは、何も作られていない
    assert_eq!(s.call("POST", "/parcels", Some(json!({"kind": "light"}))).0, 201);
    assert!(s.call("GET", "/api/defs", None).0 == 200 && s.call("GET", "/api/health", None).0 == 200);
    assert_eq!(http(s.port, "GET", "/", &[], None).status, 200);
    s.stop();
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn 流れの途中の処理のurlはtestなしでは答えず流れの中では動く() {
    // 仕様書の URL の表で「内部」の URL（返事・期限・前の処理の結果から進む処理）は、外から呼ぶと 404。
    // 呼べると、決済の返事を待たずに先へ進められる。流れの中（返事がそろったら settle → ship）では、ふつうに動く
    let d = tmpdir("inner");
    let mut s = Server::new(&d.join("inner.sqlite"), &[], false);
    s.start();
    let (pid, _) = s.accept("light");
    for path in ["/settlements", "/cancellations"] {
        let (code, j) = s.call("POST", path, Some(json!({"parcel_id": pid})));
        assert!(code == 404 && j.get("result").is_none(), "{} {}", path, j); // 処理の結果（NoParcel など）ではなく、口が無い
    }
    assert_eq!(s.status_of(&pid), "Waiting"); // 外からは進められない
    let until = Instant::now() + Duration::from_secs(20);
    while s.status_of(&pid) == "Waiting" && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(200)); // 模擬の返事（既定で 1 秒後）がそろうのを待つ
    }
    assert_eq!(s.status_of(&pid), "Shipped"); // 流れの中では settle → ship が動く
    s.stop();
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn testを付けると開発用の模擬パネルとテスト用の口が開く() {
    let mut s = Server::standard();
    assert_eq!(http(s.port, "GET", "/test.html", &[], None).status, 200);
    assert_eq!(s.call("GET", "/api/test/state", None).0, 200);
}

#[test]
fn postはjsonと名乗るものだけを受けutf8でない本文は断る() {
    // 別のサイトのページは、フォームや text/plain の POST なら事前の確認なしに送らせられる（CSRF）。JSON と名乗る POST だけを受ける
    let mut s = Server::standard();
    let body = serde_json::to_vec(&json!({"kind": "light"})).unwrap();
    for ct in ["text/plain", "application/x-www-form-urlencoded", "multipart/form-data; boundary=x", "application/jsonp"] {
        assert_eq!(http(s.port, "POST", "/parcels", &[("Content-Type", ct)], Some(&body)).status, 415, "{}", ct);
    }
    assert!(s.parcels().is_empty()); // 断った要求では、何も作られていない
    assert_eq!(http(s.port, "POST", "/parcels", &[("Content-Type", "Application/JSON; charset=utf-8")], Some(&body)).status, 201);
    let r = http(s.port, "POST", "/parcels", &[("Content-Type", "application/json")], Some(b"{\"kind\": \"\xff\"}"));
    assert!(r.status == 400 && r.json()["error"].as_str().unwrap().contains("UTF-8"));
    assert_eq!(s.parcels().len(), 1);
    for path in ["/api/health", "/", "/ui.js"] {
        let r = http(s.port, "GET", path, &[], None);
        assert!(r.header("X-Content-Type-Options").as_deref() == Some("nosniff") && r.header("Referrer-Policy").as_deref() == Some("same-origin"), "{}", path);
    }
}

#[test]
fn hostが手元の名前でも足した名前でもなければ断る() {
    // 悪意のあるページが DNS の付け替え（DNS rebinding）で 127.0.0.1 に届いても、ブラウザは Host にそのページの名前を書く。
    // その名前の要求は、読むのも書くのも断る。localhost・IP の書き方・--allow-host で足した名前は受ける
    let d = tmpdir("host");
    let mut s = Server::new(&d.join("host.sqlite"), &["--allow-host", "shop.example"], true);
    s.start();
    let port = s.port;
    let send = |method: &str, path: &str, host: &str, body: Option<Value>| -> u16 {
        let b = body.map(|v| serde_json::to_vec(&v).unwrap());
        http(port, method, path, &[("Content-Type", "application/json"), ("Host", host)], b.as_deref()).status
    };
    for host in [format!("evil.example:{}", port), format!("127.0.0.1.evil.example:{}", port)] {
        assert_eq!(send("GET", "/parcels", &host, None), 403, "{}", host);
        assert_eq!(send("POST", "/parcels", &host, Some(json!({"kind": "light"}))), 403, "{}", host);
        assert_eq!(send("GET", "/api/defs", &host, None), 403, "{}", host);
    }
    assert!(s.call("GET", "/parcels", None).1["rows"].as_array().unwrap().is_empty()); // 断った要求では、何も作られていない
    for host in [format!("localhost:{}", port), format!("127.0.0.1:{}", port), format!("[::1]:{}", port), "shop.example".to_string(), format!("SHOP.example:{}", port)] {
        assert_eq!(send("GET", "/parcels", &host, None), 200, "{}", host);
    }
    assert_eq!(send("POST", "/parcels", "shop.example", Some(json!({"kind": "light"}))), 201);
    s.stop();
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn 期限の待ちが1件失敗し続けてもほかの期限は発火する() {
    // 発火に失敗する待ち（ここでは、無いトリガの待ち）が先にあっても、うしろの待ちの期限は止まらない
    let mut s = Server::standard();
    {
        let c = rusqlite::Connection::open(&s.db).unwrap();
        c.execute("INSERT INTO geas_waits (trigger, args, collected, deadline) VALUES ('Nowhere', '{}', '{}', 0)", []).unwrap();
    }
    s.mock(json!({"PaySettled": {"drop": true}}));
    let (pid, _) = s.accept("light");
    s.advance(6);
    let fired = s.advance(30 * 60 + 10);
    assert!(fired.iter().any(|f| f["deadline"] == "Nowhere" && f.get("error").is_some()), "{:?}", fired);
    assert!(fired.iter().any(|f| f["deadline"] == "Pending" && f["outcome"]["fired"] == "expire"), "{:?}", fired);
    assert_eq!(s.status_of(&pid), "Canceled");
}

#[test]
fn 同時に来たリクエストは直列に並べた場合と同じ結果になる() {
    let mut s = Server::standard();
    // 同じ小包を同時に取り消す：状態が Waiting のうちに通るのは 1 件だけで、残りは WrongStatus（409）
    let (pid, _) = s.accept("light");
    let port = s.port;
    let pid_c = pid.clone();
    let codes = par(8, move |_| http(port, "POST", "/cancellations", &[("Content-Type", "application/json")], Some(json!({"parcel_id": pid_c}).to_string().as_bytes())).status);
    let mut sorted = codes.clone();
    sorted.sort();
    assert_eq!(sorted, [vec![201u16], vec![409; 7]].concat());
    assert_eq!(s.status_of(&pid), "Canceled");
    // 同時に受け付けても、採番は重ならず、全件が保存される
    let ids = par(24, move |_| accept_on(port, "light"));
    let uniq: std::collections::BTreeSet<&String> = ids.iter().collect();
    assert_eq!(uniq.len(), 24, "{:?}", ids);
    let rows = s.parcels();
    assert!(ids.iter().all(|i| rows.contains_key(i)));
}

/// n 件を、同時に走らせて、結果を集める
fn par<T: Send + 'static>(n: usize, f: impl Fn(usize) -> T + Send + Sync + 'static) -> Vec<T> {
    let f = std::sync::Arc::new(f);
    let hs: Vec<_> = (0..n)
        .map(|i| {
            let f = f.clone();
            std::thread::spawn(move || f(i))
        })
        .collect();
    hs.into_iter().map(|h| h.join().unwrap()).collect()
}

fn accept_on(port: u16, kind: &str) -> String {
    let r = http(port, "POST", "/parcels", &[("Content-Type", "application/json")], Some(json!({"kind": kind}).to_string().as_bytes()));
    assert_eq!(r.status, 201);
    r.json()["body"]["parcel_id"].as_str().unwrap().to_string()
}

#[test]
fn サーバーの単体テストが通る汚れたmutexでも続けて動く() {
    let r = cargo(&["test", "--release", "--quiet", "--lib", "--bins"], &built().join("server"));
    assert!(r.status.success(), "{}{}", tail(&String::from_utf8_lossy(&r.stdout), 2000), tail(&String::from_utf8_lossy(&r.stderr), 2000));
}

#[test]
fn 同じdbファイルを使うサーバーが2つあっても直列に並べた場合と同じ結果になる() {
    let mut s = Server::standard();
    let mut other = Server::new(&s.db, &[], true);
    other.start();
    let (pid, _) = s.accept("light");
    let ports = [s.port, other.port];
    let codes = par(16, move |i| http(ports[i % 2], "POST", "/cancellations", &[("Content-Type", "application/json")], Some(json!({"parcel_id": pid}).to_string().as_bytes())).status);
    let mut sorted = codes.clone();
    sorted.sort();
    assert_eq!(sorted, [vec![201u16], vec![409; 15]].concat()); // 待たされるだけで、「database is locked」で落ちるリクエストは無い
    assert_eq!(other.parcels().values().next().map(|p| p["status"].clone()), Some(json!("Canceled")));
    let ids = par(24, move |i| accept_on(ports[i % 2], "light"));
    let uniq: std::collections::BTreeSet<&String> = ids.iter().collect();
    assert_eq!(uniq.len(), 24, "{:?}", ids);
}

#[test]
fn 同じdbファイルを使うサーバーが2つあっても期限の発火は待ち1件につき1回だけになる() {
    let mut s = Server::standard();
    let n = 40;
    s.mock(json!({"PaySettled": {"drop": true}, "DepotSettled": {"drop": true}}));
    let mut other = Server::new(&s.db, &[], true);
    other.start();
    let pids: Vec<String> = (0..n).map(|_| s.accept("light").0).collect();
    for pid in &pids {
        assert_eq!(s.call("POST", "/nudge_requests", Some(json!({"parcel_id": pid}))).0, 201);
    }
    // 時計は 1 台ごとに進むので、2 台を同時に期限の先へ進める。期限の一覧を両方が読む
    let ports = [s.port, other.port];
    par(2, move |i| http(ports[i], "POST", "/api/test/advance", &[("Content-Type", "application/json")], Some(json!({"secs": 10 * 60 + 30}).to_string().as_bytes())).status);
    std::thread::sleep(Duration::from_secs(3)); // ticker（300ms ごと）が両方の台で回るのを待つ
    // /api/test/state の effects は件数が切れるので、DB を直接数える
    let c = rusqlite::Connection::open(&s.db).unwrap();
    let mut st = c.prepare("SELECT idem_key FROM geas_effects WHERE name = 'Courier.send'").unwrap();
    let mut got: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect();
    got.sort();
    let mut want = pids.clone();
    want.sort();
    assert_eq!(got, want, "期限の発火は {} 回のはずが {} 回", n, got.len());
}

#[test]
fn 大きすぎる本文は読み込まずに413で断り上限ちょうどの文字列は通る() {
    let mut s = Server::standard();
    let big = vec![b'x'; 16 * 1024 * 1024 + 1];
    assert_eq!(http(s.port, "POST", "/parcels", &[("Content-Type", "application/json")], Some(&big)).status, 413);
    assert_eq!(s.call("POST", "/parcels", Some(json!({"kind": "light"}))).0, 201); // そのあとも普通に動く
}

#[test]
fn workerの本数を指定でき範囲外の指定は起動前に断る() {
    let d = tmpdir("workers");
    let mut one = Server::new(&d.join("one.sqlite"), &["--workers", "1"], true);
    one.start();
    for _ in 0..3 {
        assert_eq!(one.call("POST", "/parcels", Some(json!({"kind": "light"}))).0, 201); // worker が 1 本でも、続けて処理できる
    }
    one.stop();
    for bad in ["0", "257", "many"] {
        let (code, err) = run_exe(&["--workers", bad, "--db", d.join("bad.sqlite").to_str().unwrap()]);
        assert!(code == 2 && err.contains("--workers"), "{} {} {}", bad, code, err);
    }
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn サーバーのログをgeas_traceで仕様書の行に結べる() {
    // 構造化ログ（本文は載らない）が、仕様書のルート表の行・処理・結果の種類に戻れる。仕様書にない経路・状態は「結べなかったもの」に出る
    let d = tmpdir("trace");
    let log = d.join("server.log");
    let port = free_port();
    let mut child = Command::new(exe())
        .args(["--port", &port.to_string(), "--db"])
        .arg(d.join("t.sqlite"))
        .arg("--pwa")
        .arg(built().join("ui"))
        .arg("--dev")
        .arg(built().join("dev"))
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    for _ in 0..100 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let body = json!({"kind": "light", "secret_note": "個人の値"}).to_string();
    http(port, "POST", "/parcels", &[("Content-Type", "application/json")], Some(body.as_bytes()));
    let _ = child.kill();
    let _ = child.wait();
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.contains("\"log\":\"geas.route\"") && !text.contains("個人の値"));
    let r = Command::new(BIN).env("GEAS_LANG", "ja").arg("trace").arg(demo("runtime_demo.geas")).arg(&log).output().unwrap();
    let out = String::from_utf8_lossy(&r.stdout).into_owned();
    assert!(r.status.success() && out.contains("POST") && out.contains("/parcels") && out.contains("仕様書に結べたもの"), "{}{}", out, String::from_utf8_lossy(&r.stderr));
    let bad = d.join("bad.log");
    std::fs::write(&bad, "{\"log\":\"geas.route\",\"ts_ms\":1,\"method\":\"GET\",\"route\":\"/nowhere\",\"target\":\"x\",\"result\":null,\"status\":200,\"ms\":1}\n").unwrap();
    let r = Command::new(BIN).env("GEAS_LANG", "ja").arg("trace").arg(demo("runtime_demo.geas")).arg(&bad).output().unwrap();
    assert!(r.status.code() == Some(1) && String::from_utf8_lossy(&r.stdout).contains("ルート表にない経路"));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn 保存はメモリに持つが別のサーバーやdbを直接書き換えた内容は次のリクエストで見える() {
    // サーバーは保存をメモリに持ち、処理が変えた行だけを DB に書く。別の接続（同じ DB を使う別のサーバー・人の手）が
    // 書いたことは DB の data_version で分かり、そのときだけ読み直す。古い内容で上書きしない
    let mut s = Server::standard();
    let mut other = Server::new(&s.db, &[], true);
    other.start();
    let (pid, _) = s.accept("light");
    assert_eq!(other.parcels()[&pid]["kind"], "light"); // 別のサーバーが作った行が見える
    assert_eq!(other.write("PUT", &format!("/parcels/{}", pid), Some(json!({"kind": "heavy"}))).0, 200);
    assert_eq!(s.parcels()[&pid]["kind"], "heavy"); // 別のサーバーが書き換えた行が見える
    let row: Value = {
        let c = rusqlite::Connection::open(&s.db).unwrap();
        let v: String = c.query_row("SELECT v FROM Parcel WHERE k = ?", [&pid], |r| r.get(0)).unwrap();
        let mut row: Value = serde_json::from_str(&v).unwrap();
        let orig = row.clone();
        row["kind"] = json!("medium");
        c.execute("UPDATE Parcel SET v = ? WHERE k = ?", rusqlite::params![row.to_string(), pid]).unwrap();
        orig
    };
    assert_eq!(s.parcels()[&pid]["kind"], "medium"); // DB を直接書き換えても見える
    assert_eq!(s.write("PUT", &format!("/parcels/{}", pid), Some(json!({"kind": "small"}))).0, 200);
    let got = other.parcels()[&pid].clone();
    assert!(got["kind"] == "small" && got["fee"] == row["fee"]);
    // DB にも、処理が変えた行の新しい内容が書かれている
    let c = rusqlite::Connection::open(&s.db).unwrap();
    let v: String = c.query_row("SELECT v FROM Parcel WHERE k = ?", [&pid], |r| r.get(0)).unwrap();
    let stored: Value = serde_json::from_str(&v).unwrap();
    assert!(stored["kind"] == "small" && stored["fee"] == row["fee"]);
}

#[test]
fn 一キロバイトを超える応答も書き込みの待ちなしで返る() {
    // tiny_http の respond は 1KB ずつ書くので、1KB を超える応答は 2 回目の書き込みが Nagle と相手の遅延 ACK に挟まれ、
    // 毎回 40ms ほど待っていた。応答は 1 回の書き込みで送る。同じつなぎ（keep-alive）で続けて引いて測る
    let mut s = Server::standard();
    for _ in 0..40 {
        s.accept("light");
    }
    let mut c = TcpStream::connect(("127.0.0.1", s.port)).unwrap();
    c.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut times = vec![];
    let mut last_len = 0;
    let mut last_rows = 0;
    for _ in 0..10 {
        let t = Instant::now();
        c.write_all(format!("GET /parcels HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n", s.port).as_bytes()).unwrap();
        let mut raw = vec![];
        let mut buf = [0u8; 8192];
        let (head_end, clen) = loop {
            let n = c.read(&mut buf).unwrap();
            assert!(n > 0, "つなぎが閉じられました");
            raw.extend_from_slice(&buf[..n]);
            if let Some(p) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&raw[..p]).to_ascii_lowercase();
                let cl: usize = head.lines().find_map(|l| l.strip_prefix("content-length:")).map(|v| v.trim().parse().unwrap()).expect("Content-Length が無い応答です");
                break (p + 4, cl);
            }
        };
        while raw.len() < head_end + clen {
            let n = c.read(&mut buf).unwrap();
            assert!(n > 0);
            raw.extend_from_slice(&buf[..n]);
        }
        times.push(t.elapsed().as_secs_f64());
        last_len = clen;
        last_rows = serde_json::from_slice::<Value>(&raw[head_end..head_end + clen]).unwrap()["rows"].as_array().unwrap().len();
    }
    assert!(last_len > 2048 && last_rows == 40);
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert!(times[times.len() / 2] < 0.02, "{:?}", times);
}

#[test]
fn 問い合わせの応答には版が付き保存が変わっていなければ304で本文を送らない() {
    // 画面は、見せている一覧を 2 秒ごとに引き直す。保存が変わっていなければ（処理が行を書いていない・別の接続も書いていない）、
    // サーバーは問い合わせを実行せずに 304 を返す（変わっていない一覧を毎回作って送らない）。版は URL ごとに違う
    let mut s = Server::standard();
    let get = |port: u16, path: &str, tag: Option<&str>, accept: &str| -> Resp {
        let mut hs = vec![("Accept", accept)];
        if let Some(t) = tag {
            hs.push(("If-None-Match", t));
        }
        http(port, "GET", path, &hs, None)
    };
    let (pid, _) = s.accept("light");
    let port = s.port;
    let r = get(port, "/parcels", None, "application/json");
    let tag = r.header("ETag").unwrap();
    assert!(r.status == 200 && r.json()["rows"].as_array().unwrap().iter().any(|p| p["parcel_id"] == pid.as_str()));
    let r = get(port, "/parcels", Some(&tag), "application/json");
    assert!(r.status == 304 && r.header("ETag").as_deref() == Some(tag.as_str()) && r.body.is_empty()); // 変わっていない：本文なし
    let details = format!("/parcels/{}/details", pid);
    let r = get(port, &details, None, "application/json");
    let other = r.header("ETag").unwrap();
    assert!(r.status == 200 && other != tag && get(port, &details, Some(&tag), "application/json").status == 200); // 別の URL の版では 304 にならない
    let r = get(port, &details, Some(&other), "text/html"); // 画面の URL を開く（殻の HTML）ときは、版を見ない
    assert!(r.status == 200 && String::from_utf8_lossy(&r.body).trim_start().to_lowercase().starts_with("<!doctype html"));
    s.accept("heavy"); // 処理が行を書いた
    let r = get(port, "/parcels", Some(&tag), "application/json");
    let tag2 = r.header("ETag").unwrap();
    assert!(r.status == 200 && tag2 != tag && r.json()["rows"].as_array().unwrap().len() == 2);
    assert_eq!(get(port, "/parcels", Some(&tag2), "application/json").status, 304);
    {
        // 別の接続（人の手・別のサーバー）が書いた
        let c = rusqlite::Connection::open(&s.db).unwrap();
        let v: String = c.query_row("SELECT v FROM Parcel WHERE k = ?", [&pid], |r| r.get(0)).unwrap();
        let mut row: Value = serde_json::from_str(&v).unwrap();
        row["kind"] = json!("medium");
        c.execute("UPDATE Parcel SET v = ? WHERE k = ?", rusqlite::params![row.to_string(), pid]).unwrap();
    }
    let r = get(port, "/parcels", Some(&tag2), "application/json");
    let tag3 = r.header("ETag").unwrap();
    let kinds: std::collections::BTreeMap<String, String> = r.json()["rows"].as_array().unwrap().iter().map(|p| (p["parcel_id"].as_str().unwrap().to_string(), p["kind"].as_str().unwrap().to_string())).collect();
    assert!(r.status == 200 && tag3 != tag2 && kinds[&pid] == "medium");
    s.stop(); // 再起動したら、前の版では 304 にならない（DB が止まっている間に書き換えられていても、古い一覧を使わせない）
    s.start();
    assert_eq!(get(s.port, "/parcels", Some(&tag3), "application/json").status, 200);
}

#[test]
fn testなしのapi_defsは画面が使う分だけを返す() {
    // --test なしの /api/defs は、画面と外から呼べる URL だけ。流れの途中の処理・処理の一覧・外部作用・模擬の設定は出さない。
    // --test のときは、開発用のパネルと通しの試験が使うので全部を返す
    let d = tmpdir("defs");
    let mut s = Server::new(&d.join("plain.sqlite"), &[], false);
    s.start();
    let defs = s.call("GET", "/api/defs", None).1;
    let mut names: Vec<&String> = defs.as_object().unwrap().keys().collect();
    names.sort();
    assert_eq!(names, ["routes", "screens"]);
    let routes = defs["routes"].as_array().unwrap();
    assert!(!routes.is_empty() && !routes.iter().any(|r| r["internal"].as_bool().unwrap_or(false)));
    s.stop();
    let mut t = Server::new(&d.join("test.sqlite"), &[], true);
    t.start();
    let full = t.call("GET", "/api/defs", None).1;
    assert!(["deeds", "effects", "mock", "flows"].iter().all(|k| full.get(k).is_some()) && full["routes"].as_array().unwrap().iter().any(|r| r["internal"].as_bool().unwrap_or(false)));
    t.stop();
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn headはgetと同じ状態で本文なしで答える() {
    let d = tmpdir("head");
    let mut s = Server::new(&d.join("head.sqlite"), &[], false);
    s.start();
    let r = http(s.port, "HEAD", "/parcels", &[], None);
    assert!(r.status == 200 && r.body.is_empty());
    s.stop();
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn 保存の行が今の定義で読めないdbでは起動のときに理由を言って止まる() {
    // .geas の実体の項目を変えたあとに、前の版の DB を開いた場合にあたる。起動してから全部のリクエストが失敗するのでなく、起動で止まる
    let d = tmpdir("old");
    let db = d.join("old.sqlite");
    let mut s = Server::new(&db, &[], false);
    s.start();
    assert_eq!(s.call("POST", "/parcels", Some(json!({"kind": "light"}))).0, 201);
    s.stop();
    {
        let c = rusqlite::Connection::open(&db).unwrap();
        let tables: Vec<String> = c.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'geas_%' AND name NOT LIKE 'sqlite_%'").unwrap().query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect();
        let table = tables.into_iter().find(|t| c.query_row(&format!("SELECT count(*) FROM \"{}\"", t), [], |r| r.get::<_, i64>(0)).unwrap() > 0).unwrap();
        c.execute(&format!("UPDATE \"{}\" SET v = ?", table), ["{}"]).unwrap(); // 項目の無い行（前の版の形のつもり）
    }
    let (code, err) = run_exe(&["--port", "0", "--db", db.to_str().unwrap(), "--pwa", built().join("ui").to_str().unwrap()]);
    assert!(code == 1 && err.contains("今の定義と合いません") && !err.contains("panicked"), "{}", err);
    let _ = std::fs::remove_dir_all(d);
}
