// SPDX-License-Identifier: GPL-3.0-or-later
//! 海外店のアプリ（tests/fixtures/overseas_shop）の通し確認。geas から生成した Rust をサーバーに組み、localhost で動かして、JSON の口を確かめる。
//! 計画書の具体例から生成したテスト（server/tests/url_examples.rs・screen_examples.rs）も、組んだサーバーで通す。
//! 元は Python 版の tests/test_shop_app.py（タグ python-final）。
mod common;
use common::serve::{build_app, target_dir, App, Srv};
use common::*;
use serde_json::{json, Value};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

fn app() -> &'static App {
    static A: OnceLock<App> = OnceLock::new();
    A.get_or_init(|| {
        let d = root().join("tests/fixtures/overseas_shop");
        let seed = d.join("app/seed.json");
        let plan = d.join("overseas_shop_plan.md");
        build_app("shop", &d.join("overseas_shop.geas"), &["--seed", seed.to_str().unwrap(), "--plan", plan.to_str().unwrap()])
    })
}

struct Shop {
    s: Srv,
}

impl Shop {
    fn new() -> Shop {
        let a = app();
        let dev = a.dir.join("dev");
        let s = Srv::start(a, &["--test", "--dev", dev.to_str().unwrap()]);
        let sh = Shop { s };
        sh.reset_mock();
        sh
    }

    fn call(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        self.s.call(method, &format!("/api/{}", path), None, body)
    }

    fn reset_mock(&self) {
        self.call("POST", "test/settings", Some(json!({"delay_secs": 1, "replies": {"PaymentSettled": {"outcome": "Approved", "drop": false}, "StockSettled": {"outcome": "Held", "drop": false}}})));
    }

    fn order(&self, customer: &str, items: Value, currency: &str, country: &str) -> Value {
        self.call("POST", "deeds/place_order", Some(json!({"input": {"customer_id": customer, "items": items, "ship_to": {"country": country, "postal_code": "100-0001", "line": "x"}}, "slots": {"currency": currency}}))).1
    }

    fn order_jp(&self, customer: &str, items: Value) -> Value {
        self.order(customer, items, "JPY", "JP")
    }

    fn rows(&self, customer: &str) -> Vec<Value> {
        self.call("GET", &format!("queries/orders_of?customer={}", customer), None).1["rows"].as_array().unwrap().clone()
    }

    fn status_of(&self, oid: &str, customer: &str) -> String {
        self.rows(customer).iter().find(|r| r["order_id"] == oid).unwrap()["status"].as_str().unwrap().to_string()
    }

    /// 模擬の返事が返るのを待つ（返事までの秒数は 1 秒）
    fn settle(&self, secs: f64) {
        std::thread::sleep(Duration::from_millis((secs * 1000.0) as u64));
    }

    fn effects(&self) -> Vec<Value> {
        self.call("GET", "test/state", None).1["effects"].as_array().unwrap().clone()
    }

    fn oid(r: &Value) -> String {
        r["body"]["order_id"].as_str().unwrap().to_string()
    }
}

#[test]
fn 注文は決済と倉庫の返事がそろって確定し_在庫とポイントが動く() {
    let b = Shop::new();
    let r = b.order_jp("c-001", json!({"tea-jp": 2, "knife-jp": 1}));
    assert_eq!(r["result"], "Received");
    let deeds: Vec<&str> = r["trace"].as_array().unwrap().iter().take(2).map(|t| t["deed"].as_str().unwrap()).collect();
    assert_eq!(deeds, vec!["place_order", "request_payment"]); // 受注のあと、並列の枝が動く
    let oid = Shop::oid(&r);
    assert_eq!(b.status_of(&oid, "c-001"), "PaymentRequested"); // 返事がそろうまでは待つ
    b.settle(3.0);
    assert_eq!(b.status_of(&oid, "c-001"), "Completed");
    let row = b.rows("c-001").into_iter().find(|x| x["order_id"] == oid.as_str()).unwrap();
    assert_eq!(row["total"], 10160 + 1016 + 500); // 小計 + 税10% + 国内送料
    let stock: std::collections::HashMap<String, i64> = b.call("GET", "queries/product_list", None).1["rows"].as_array().unwrap().iter().map(|p| (p["product_id"].as_str().unwrap().to_string(), p["stock"].as_i64().unwrap())).collect();
    assert!(stock["tea-jp"] == 118 && stock["knife-jp"] == 14, "{:?}", stock);
    assert!(b.effects().iter().any(|e| e["name"] == "Shipping.ship"));
}

#[test]
fn ドルの注文は税なしで_通貨の違う商品を混ぜると止まる() {
    let b = Shop::new();
    assert_eq!(b.order("c-001", json!({"tea-jp": 1}), "USD", "JP")["result"], "OtherCurrency");
    let r = b.order("c-001", json!({"tea-us": 1}), "USD", "US");
    assert_eq!(r["result"], "Received");
    let oid = Shop::oid(&r);
    let last = b.rows("c-001").into_iter().find(|x| x["order_id"] == oid.as_str()).unwrap();
    assert_eq!((last["currency"].clone(), last["total"].clone()), (json!("USD"), json!(699 + 3000))); // 税0%、海外送料30ドル（セント）
    b.settle(3.0);
}

#[test]
fn 決済の返事が落ちても_つながりが戻ると同じ冪等キーで引き直して確定する() {
    let b = Shop::new();
    b.call("POST", "test/settings", Some(json!({"replies": {"PaymentSettled": {"drop": true}}})));
    let oid = Shop::oid(&b.order_jp("c-002", json!({"bowl-jp": 1})));
    b.settle(3.0);
    assert_eq!(b.status_of(&oid, "c-002"), "PaymentRequested"); // 倉庫の返事だけが届いて、待ち続ける
    b.call("POST", "test/settings", Some(json!({"replies": {"PaymentSettled": {"drop": false}}})));
    let ev = b.call("POST", "events/Reconnected", Some(json!({}))).1;
    let deeds: Vec<String> = ev["runs"].as_array().unwrap().iter().flat_map(|run| run["trace"].as_array().unwrap().iter().map(|t| t["deed"].as_str().unwrap().to_string())).collect();
    assert_eq!(deeds, vec!["recheck_payment"]);
    b.settle(3.0);
    assert_eq!(b.status_of(&oid, "c-002"), "Completed");
    let charges: Vec<Value> = b.effects().into_iter().filter(|e| e["name"] == "Payment.charge" && e["args"]["order_id"] == oid.as_str()).collect();
    let keys: std::collections::HashSet<String> = charges.iter().map(|c| c["idem_key"].to_string()).collect();
    assert!(charges.len() == 2 && keys.len() == 1, "{:?}", charges); // 請求は2回出したが、冪等キーは同じ
    assert!(b.call("POST", "events/Reconnected", Some(json!({}))).1["runs"].as_array().unwrap().is_empty()); // 返事待ちはもう無い
}

#[test]
fn 返事がそろわないまま30分たつと取り消す() {
    let b = Shop::new();
    b.call("POST", "test/settings", Some(json!({"replies": {"PaymentSettled": {"drop": true}}})));
    let oid = Shop::oid(&b.order_jp("c-002", json!({"bowl-jp": 1})));
    b.settle(1.5);
    let fired = b.call("POST", "test/advance", Some(json!({"secs": 1801}))).1["fired"].as_array().unwrap().clone();
    assert!(fired.iter().any(|f| f["deadline"] == "OrderPending"), "{:?}", fired);
    assert_eq!(b.status_of(&oid, "c-002"), "Canceled");
}

#[test]
fn 倉庫が在庫不足を返すと返金して取り消し_決済が拒否されると取り消す() {
    let b = Shop::new();
    b.call("POST", "test/settings", Some(json!({"replies": {"StockSettled": {"outcome": "Short"}}})));
    let oid = Shop::oid(&b.order_jp("c-002", json!({"bowl-jp": 1})));
    b.settle(3.0);
    assert_eq!(b.status_of(&oid, "c-002"), "Canceled");
    assert!(b.effects().iter().any(|e| e["name"] == "Payment.refund" && e["args"]["order_id"] == oid.as_str()));
    b.reset_mock();
    b.call("POST", "test/settings", Some(json!({"replies": {"PaymentSettled": {"outcome": "Declined"}}})));
    let oid2 = Shop::oid(&b.order_jp("c-002", json!({"bowl-jp": 1})));
    b.settle(3.0);
    assert_eq!(b.status_of(&oid2, "c-002"), "Canceled");
    assert!(!b.effects().iter().any(|e| e["name"] == "Payment.refund" && e["args"]["order_id"] == oid2.as_str()));
}

#[test]
fn 外から来る値は境界で確かめる() {
    let b = Shop::new();
    // 商品は PUT・DELETE で書き換える実体なので、各行に行の版（_etag）が付く。ここでは列の値だけを比べる
    let rows: Vec<Value> = b.call("GET", "queries/price_by_jan?jan=4901234567894", None).1["rows"].as_array().unwrap().iter().map(|r| {
        let mut r = r.clone();
        r.as_object_mut().unwrap().remove("_etag");
        r
    }).collect();
    assert_eq!(rows, vec![json!({"currency": "JPY", "price": 680, "product_id": "tea-jp"}), json!({"currency": "USD", "price": 699, "product_id": "tea-us"})]);
    let (code, j) = b.call("GET", "queries/price_by_jan?jan=4901234567895", None);
    assert!(code == 400 && j["error"].as_str().unwrap().contains("検査数字"), "{} {}", code, j);
    assert_eq!(b.call("GET", "queries/price_by_jan?jan=12345", None).0, 400);
    assert_eq!(b.order_jp("c-999", json!({"tea-jp": 1}))["result"], "NoSuchCustomer");
    let place = |items: Value, cur: &str| b.call("POST", "deeds/place_order", Some(json!({"input": {"customer_id": "c-001", "items": items, "ship_to": {"country": "JP", "postal_code": "1", "line": "x"}}, "slots": {"currency": cur}})));
    let (code, j) = place(json!({"tea-jp": 0}), "JPY");
    assert!(code == 400 && j["error"].as_str().unwrap().contains("範囲外"), "{} {}", code, j);
    assert_eq!(place(json!({"tea-jp": 1}), "GBP").0, 400);
    assert_eq!(b.call("POST", "deeds/nothing", Some(json!({}))).0, 400);
}

#[test]
fn 再起動しても注文と返事待ちは残る() {
    // SQLite に保存してあるので、保存先に注文と返事待ちが残る
    let b = Shop::new();
    b.call("POST", "test/settings", Some(json!({"replies": {"PaymentSettled": {"drop": true}}})));
    let oid = Shop::oid(&b.order_jp("c-002", json!({"bowl-jp": 1})));
    b.settle(1.5);
    let waits = b.call("GET", "test/state", None).1["waits"].as_array().unwrap().clone();
    assert!(waits.iter().any(|w| w["args"]["order_id"] == oid.as_str()));
    let con = rusqlite::Connection::open(b.s.db()).unwrap();
    let n: i64 = con.query_row("select count(*) from \"Order\" where k = ?", [&oid], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
    let w: i64 = con.query_row("select count(*) from geas_waits where args like ?", [format!("%{}%", oid)], |r| r.get(0)).unwrap();
    assert_eq!(w, 1);
}

fn cargo_test(name: &str) -> (bool, String) {
    let r = Command::new("cargo").args(["test", "--release", "--quiet", "--test", name]).env("CARGO_TARGET_DIR", target_dir()).current_dir(app().dir.join("server")).output().unwrap();
    (r.status.success(), String::from_utf8_lossy(&r.stdout).to_string() + &String::from_utf8_lossy(&r.stderr))
}

#[test]
fn 計画書のurlの具体例から生成したテストが通る() {
    // update／destroy を含め、HTTP の通し（結果の種類ごとの状態）は、手書きでなくこの表が持つ
    let (ok, out) = cargo_test("url_examples");
    assert!(ok, "{}", tail(&out, 3000));
}

#[test]
fn 計画書の画面の具体例から生成したテストが通る() {
    // PWA と Chrome 拡張の通し（node と Playwright が無ければ、その中で飛ばされる）
    let (ok, out) = cargo_test("screen_examples");
    assert!(ok, "{}", tail(&out, 3000));
}
