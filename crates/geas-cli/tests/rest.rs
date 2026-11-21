// SPDX-License-Identifier: GPL-3.0-or-later
//! リソース（resource）：URL の決め方・検査（状態の対応が必須、重なりなし）、計画書との突合、URL・画面の具体例からのテスト生成。
//! 元は Python 版の tests/test_rest.py（タグ python-final）。
mod common;
use common::*;
use geas_core::check::{defs::check, Checker, ERROR, WARN};
use geas_core::hostgen::definitions;
use geas_core::parser::parse;
use geas_core::plan::{compare_static, coverage, read_plan, Row};
use geas_core::receipt::{build_receipt, to_markdown};
use geas_core::rest::{routes, screen_url, singular, snake, INTERNAL, OPEN};
use geas_core::spec::{build, render};
use geas_core::testgen::{screen_cases, url_rows, write_screen_tests, write_url_tests};
use serde_json::{json, Value};
use std::path::PathBuf;

fn shop_path() -> PathBuf {
    root().join("tests").join("fixtures").join("overseas_shop").join("overseas_shop.geas")
}

fn code() -> String {
    fixture("overseas_shop/overseas_shop.geas")
}

fn plan_md() -> String {
    fixture("overseas_shop/overseas_shop_plan.md")
}

/// 検査して、結果を closure に渡す（Checker はプログラムを借りるので、この形にする）
fn with_ck<R>(src: &str, f: impl FnOnce(&Checker) -> R) -> R {
    let mut prog = parse(src, Some(&shop_path())).unwrap_or_else(|e| panic!("構文の誤り: {:?}", e));
    let ck = check(&mut prog, true);
    f(&ck)
}

fn level_msgs(ck: &Checker, level: &str) -> Vec<String> {
    ck.diags.iter().filter(|d| d.level == level).map(|d| d.msg.clone()).collect()
}

fn mutate(src: &str, old: &str, new: &str) -> String {
    assert!(src.contains(old), "{}", old);
    src.replacen(old, new, 1)
}

fn cs(v: &Value, i: usize) -> String {
    match &v[i] {
        Value::String(s) => s.clone(),
        o => o.to_string(),
    }
}

fn compare(code_src: &str, plan_text: &str) -> Vec<String> {
    with_ck(code_src, |ck| compare_static(&build(ck), &read_plan(plan_text)).iter().map(|f| format!("{}｜{}", f.where_, f.msg)).collect())
}

#[test]
fn 名前からurlが決まる() {
    assert_eq!(snake("ProductPrices"), "product_prices");
    assert_eq!(singular("customers"), "customer");
    assert_eq!(singular("categories"), "category");
    with_ck(&code(), |ck| {
        let table: std::collections::HashMap<(String, String), (String, String)> = routes(ck.p).into_iter().map(|r| ((r.method, r.path), (r.kind, r.target))).collect();
        let g = |m: &str, p: &str| table.get(&(m.to_string(), p.to_string())).cloned().unwrap_or_else(|| panic!("{} {}", m, p));
        assert_eq!(g("POST", "/orders"), ("create".into(), "place_order".into()));
        assert_eq!(g("GET", "/orders"), ("index".into(), "order_list".into()));
        // CustomerOrders under Customers は、親の名前を落とす
        assert_eq!(g("GET", "/customers/:customer_id/orders"), ("index".into(), "orders_of".into()));
        assert_eq!(g("GET", "/product_prices/:jan"), ("show".into(), "price_by_jan".into()));
        assert_eq!(g("POST", "/order_settlements"), ("create".into(), "settle_order".into()));
        assert_eq!(g("POST", "/order_recheck_requests"), ("create".into(), "recheck_payment".into()));
    });
}

#[test]
fn 海外店は検査を通り仕様書にルートが出る() {
    with_ck(&code(), |ck| {
        assert!(ck.ok(), "{:?}", ck.diags);
        assert!(!level_msgs(ck, WARN).iter().any(|m| m.contains("リソース") || m.contains("割り当てて")));
        let text = render(ck, &build(ck), "");
        assert!(text.contains("## ルート") && text.contains("| POST | /orders | create | 注文 | 受付→201,") && text.contains("/customers/:customer_id/orders"));
    });
}

#[test]
fn 流れの途中の処理のurlは内部として仕様書に出て定義に印が付く() {
    // 返事・期限・前の処理の結果から進む処理（nexus と trigger の行き先）の URL は「内部」。サーバーは --test のときだけ答える。
    // 画面の操作や、出来事から起こす処理の URL は「呼べる」
    with_ck(&code(), |ck| {
        let spec = build(ck);
        let route_rows = spec["ルート"].as_array().unwrap();
        let outside = |m: &str, p: &str| -> String {
            let r = route_rows.iter().find(|r| cs(r, 0) == m && cs(r, 1) == p).unwrap_or_else(|| panic!("{} {}", m, p));
            cs(r, 5)
        };
        assert_eq!(outside("POST", "/orders"), OPEN);
        assert_eq!(outside("PUT", "/products/:id"), OPEN);
        assert_eq!(outside("GET", "/orders"), OPEN);
        assert_eq!(outside("POST", "/order_completions"), INTERNAL); // 決済と在庫の見立ての結果から（前の処理の結果）
        assert_eq!(outside("POST", "/point_grants"), INTERNAL); // 受付のあとの並列の枝
        assert_eq!(outside("POST", "/order_settlements"), INTERNAL); // 返事がそろったら（trigger）
        assert_eq!(outside("POST", "/expiry_cancellations"), INTERNAL); // 期限（trigger）
        assert_eq!(outside("POST", "/order_recheck_requests"), OPEN); // 出来事（つながりが戻った）から起こす処理
        let text = render(ck, &spec, "");
        assert!(text.contains("| POST | /order_completions | create | 注文の確定 | 反映済→201, 注文なし→404, 状態違い→409, 在庫が足りなかった→422 | 内部（流れの途中） |"));
        let defs = definitions(ck).unwrap();
        let def = |m: &str, p: &str| defs["routes"].as_array().unwrap().iter().find(|r| r["method"] == m && r["path"] == p).unwrap().clone();
        assert_eq!(def("POST", "/order_completions")["internal"], json!(true));
        assert!(def("POST", "/orders").get("internal").is_none());
    });
}

#[test]
fn 画面の操作に流れの途中の処理は使えない() {
    let src = mutate(&code(), "restock_product: Restocked → reply,", "restock_product: Restocked → withdraw_product,");
    with_ck(&src, |ck| {
        // 名前は表示名に置き換わる
        assert!(level_msgs(ck, ERROR).iter().any(|m| m.contains("操作の処理「商品の取り下げ」は流れの途中の処理")));
    });
}

#[test]
fn 計画書のルートの表に外からの列があれば突合する() {
    let inner = ["/payment_requests", "/stock_holds", "/point_grants", "/campaign_point_grants", "/order_settlements", "/order_completions", "/unpaid_cancellations", "/refund_cancellations", "/expiry_cancellations"];
    let mut lines: Vec<String> = vec![];
    for line in plan_md().split('\n') {
        let mut line = line.to_string();
        if line == "| メソッド | 経路 | 動作 | 呼ぶもの | 結果→状態 |" {
            line += " 外から |";
        } else if line == "|---|---|---|---|---|" && lines.last().map_or(false, |l| l.ends_with("外から |")) {
            line += "---|";
        } else if ["| POST | /", "| GET | /", "| PUT | /", "| DELETE | /"].iter().any(|p| line.starts_with(p)) && line.matches('|').count() == 6 {
            let path = line.split(" | ").nth(1).unwrap().to_string();
            line += if inner.contains(&path.as_str()) { " 内部（流れの途中） |" } else { " 呼べる |" };
        }
        lines.push(line);
    }
    let plan = lines.join("\n");
    assert!(plan.contains("| POST | /order_completions | create | 注文の確定 | 反映済→201, 注文なし→404, 状態違い→409, 在庫が足りなかった→422 | 内部（流れの途中） |"));
    assert!(compare(&code(), &plan).is_empty());
    let f = compare(&code(), &plan.replace("在庫が足りなかった→422 | 内部（流れの途中） |", "在庫が足りなかった→422 | 呼べる |"));
    assert_eq!(f.iter().map(|x| x.split('｜').next().unwrap().to_string()).collect::<Vec<_>>(), vec!["ルート POST /order_completions の外から"]);
}

#[test]
fn 流れの途中から外れて外から呼べるようになったurlはレシートの弱くなったものに出る() {
    let t = Tmp::new("receipt");
    let f = t.0.join("app.geas");
    let c = code();
    let opened = mutate(&c, "settle_order: Approved → complete_order,", "settle_order: Approved → reply,");
    std::fs::write(&f, &opened).unwrap();
    let fp = f.to_string_lossy().to_string();
    with_ck(&opened, |o| {
        with_ck(&c, |b| {
            let r = build_receipt(o, b, &fp, Some(&c), "前の版", None);
            assert_eq!(r.weakened.iter().map(|w| (w.0.clone(), w.1.clone(), w.2.clone())).collect::<Vec<_>>(), vec![("URL POST /order_completions".to_string(), "内部（流れの途中）".to_string(), "呼べる".to_string())]);
            assert!(to_markdown(&r).contains("| URL POST /order_completions | 内部（流れの途中） → 呼べる | 流れの途中から外れ、外から直接呼べるようになりました |"));
        });
    });
    std::fs::write(&f, &c).unwrap();
    // 逆向き（流れの途中に入った）は、強くなったほう
    with_ck(&c, |o| {
        with_ck(&opened, |b| {
            let back = build_receipt(o, b, &fp, Some(&opened), "前の版", None);
            assert!(back.weakened.is_empty());
            assert_eq!(back.strengthened.iter().map(|w| (w.0.clone(), w.1.clone(), w.2.clone())).collect::<Vec<_>>(), vec![("URL POST /order_completions".to_string(), "呼べる".to_string(), "内部（流れの途中）".to_string())]);
        });
    });
}

#[test]
fn 結果の状態が抜けるとエラー() {
    let src = mutate(&code(), "    NoSuchCustomer → 404\n  }\n  index order_list", "  }\n  index order_list");
    with_ck(&src, |ck| assert!(level_msgs(ck, ERROR).iter().any(|m| m.contains("結果「顧客なし」の HTTP の状態がありません"))));
}

#[test]
fn 存在しない結果や範囲外の状態はエラー() {
    with_ck(&mutate(&code(), "    Received → 201\n", "    Received → 201\n    Nothing → 404\n"), |ck| assert!(level_msgs(ck, ERROR).iter().any(|m| m.contains("結果の種類ではありません"))));
    with_ck(&mutate(&code(), "Received → 201", "Received → 99"), |ck| assert!(level_msgs(ck, ERROR).iter().any(|m| m.contains("200〜599"))));
}

#[test]
fn パスに載らない入力は絞り込みの条件になる() {
    // 入れ子にしないと、問い合わせ orders_of の入力 customer は、絞り込みの条件 q[customer] で渡る
    with_ck(&mutate(&code(), "resource CustomerOrders under Customers", "resource CustomerOrders"), |ck| {
        assert!(!level_msgs(ck, ERROR).iter().any(|m| m.contains("CustomerOrders")));
        assert!(build(ck)["ルート"].as_array().unwrap().iter().any(|r| cs(r, 1) == "/customer_orders?q[customer]=…"));
    });
    // show にキーが無い
    with_ck(&mutate(&code(), "resource ProductPrices key jan", "resource ProductPrices"), |ck| assert!(level_msgs(ck, ERROR).iter().any(|m| m.contains("key"))));
}

#[test]
fn 同じ処理を2か所に割り当てられないurlの重なりもエラー() {
    with_ck(&(code() + "\nresource Orders2 {\n  index order_list\n}\n"), |ck| assert!(level_msgs(ck, ERROR).iter().any(|m| m.contains("すでに"))));
}

#[test]
fn 割り当てのない処理は警告() {
    let src = mutate(&code(), "/// 商品\nresource Products {\n  index product_list\n  update restock_product {\n    Restocked → 200\n    NoSuchProduct → 404\n    StockFull → 409\n  }\n  destroy withdraw_product {\n    Withdrawn → 200\n    NoSuchProduct → 404\n  }\n}\n", "");
    with_ck(&src, |ck| assert!(ck.diags.iter().any(|d| d.level == WARN && (d.where_.contains("product_list") || d.where_.contains("商品一覧")))));
}

#[test]
fn 画面はリソースの動作のurlに付く() {
    with_ck(&code(), |ck| {
        let urls: Vec<String> = ck.p.screens.iter().map(|sc| screen_url(ck.p, sc)).collect();
        assert!(!urls.is_empty());
        assert!(urls.iter().all(|u| u.is_empty() || u.starts_with('/')));
    });
}

#[test]
fn 計画書のルートの表と突合する() {
    let plan = plan_md();
    assert!(compare(&code(), &plan).is_empty());
    // 経路が違う／状態が違う／計画に無いルート
    let f = compare(&code(), &plan.replace("| POST | /order_settlements |", "| POST | /order_settlement |"));
    assert!(f.iter().any(|x| x.contains("ルート POST /order_settlement") && x.contains("コードから生成した仕様書に無い")));
    let f = compare(&code(), &plan.replacen("注文なし→404, 状態違い→409 |\n| POST | /order_completions", "注文なし→400, 状態違い→409 |\n| POST | /order_completions", 1));
    assert!(f.iter().any(|x| x.contains("結果→状態")));
    let f = compare(&code(), &plan.replace("| GET | /products | index | 商品一覧 |  |\n", ""));
    assert!(f.iter().any(|x| x.contains("ルート GET /products")));
}

#[test]
fn 計画書の画面のurlが違えば食い違う() {
    let f = compare(&code(), &plan_md().replace("| /orders/new |", "| /orders/add |"));
    assert!(f.iter().any(|x| x.contains("画面 注文の画面 のURL")));
}

#[test]
fn ユースケースの手順のurlは画面のurlと突合する() {
    let plan = plan_md();
    assert!(compare(&code(), &plan).is_empty());
    let f = compare(&code(), &plan.replacen("| 注文の画面 | 注文 | 受付 | /orders/new |", "| 注文の画面 | 注文 | 受付 | /orders |", 1));
    assert!(f.iter().any(|x| x.contains("ユースケース") && x.contains("URL")));
}

#[test]
fn 内部urlごとの確認の網羅はユースケースと具体例から数える() {
    with_ck(&code(), |ck| {
        let spec = build(ck);
        let mut plan = read_plan(&plan_md());
        let (rows, got, total) = coverage(&spec, &plan);
        let by = |rows: &Vec<Vec<String>>, m: &str, p: &str, r: &str| rows.iter().find(|x| x[0] == m && x[1] == p && x[2] == r).cloned().unwrap_or_else(|| panic!("{} {} {}", m, p, r));
        assert!(by(&rows, "POST", "/orders", "受付")[6] == "○" && by(&rows, "POST", "/orders", "受付")[3] == "201");
        assert!(by(&rows, "POST", "/orders", "在庫不足")[5].starts_with("在庫が足りなかった注文をやり直す"));
        assert_eq!(by(&rows, "POST", "/order_settlements", "承認")[6], "未");
        assert_eq!(by(&rows, "GET", "/products", "")[6], "○"); // 画面を通る手順がある問い合わせ
        assert!(0 < got && got < total);
        // 具体例が当たれば、その行も確認済みになる
        let mut ex: Row = Row::new();
        ex.insert("ID".into(), "EX1".into());
        ex.insert("処理".into(), "決済と在庫の見立て".into());
        ex.insert("期待する結果".into(), "承認(N1)".into());
        plan.insert("具体例".into(), vec![ex]);
        let (rows2, got2, _) = coverage(&spec, &plan);
        assert_eq!(got2, got + 1);
        assert_eq!(by(&rows2, "POST", "/order_settlements", "承認")[4], "EX1");
    });
}

#[test]
fn ruleは仕様書と定義に出る() {
    with_ck(&code(), |ck| {
        let defs = definitions(ck).unwrap();
        assert_eq!(defs["rules"]["tax_rate"], json!([{"when": [["currency", "JPY"]], "value": "10/100"}, {"when": [], "value": "0/1"}]));
        let md = render(ck, &build(ck), "");
        assert!(md.contains("slot の決め方") && md.contains("配送先.国 = JP"));
    });
}

#[test]
fn ruleの誤りは止まる() {
    let bad = [
        ("最後が default でない", "  default: 0/1\n", ""),
        ("決められない型", "rule tax_rate {", "rule currency {\n  default: 1\n}\nrule tax_rate {"),
        ("値の種類違い", "currency == JPY: 10/100", "currency == JPY: 10"),
        ("条件の名前が未知", "currency == JPY: 10/100", "nothing == JPY: 10/100"),
    ];
    for (why, old, new) in bad {
        with_ck(&mutate(&code(), old, new), |ck| assert!(ck.diags.iter().any(|d| d.level == ERROR && d.where_.contains("rule")), "{}", why));
    }
}

#[test]
fn urlの具体例はルートと突き合わされテストに変わる() {
    let md = plan_md();
    with_ck(&code(), |ck| {
        let sp = build(ck);
        let plan = read_plan(&md);
        assert_eq!(plan["URL例"].len(), 16);
        assert!(!compare_static(&sp, &plan).iter().any(|f| f.where_.contains("URL例")));
        let rows = url_rows(ck, &plan).unwrap();
        let by = |id: &str| rows.iter().find(|r| r.0 == id).unwrap_or_else(|| panic!("{}", id)).clone();
        let ux6 = by("UX6");
        assert_eq!((ux6.1.as_str(), ux6.2.as_str(), ux6.4.as_str(), ux6.5), ("POST", "/orders", "Received", 201));
        assert_eq!(ux6.7, vec![("order_id".to_string(), "body.order_id".to_string())]);
        assert_eq!(by("UX13").4, "Withdrawn");
        assert_eq!(by("UX3").4, ""); // 表示名はコードの名前に、「－」は結果なしに
        let t = Tmp::new("urlex");
        let f = write_url_tests(ck, &plan, &t.0).unwrap().unwrap();
        assert_eq!(f, t.0.join("server").join("tests").join("url_examples.rs"));
        assert!(std::fs::read_to_string(&f).unwrap().contains("fn url_examples_pass_in_order"));
        let (cov, _, _) = coverage(&sp, &plan);
        let c = |m: &str, p: &str, r: &str| cov.iter().find(|x| x[0] == m && x[1] == p && x[2] == r).cloned().unwrap();
        assert!(c("POST", "/orders", "受付")[4].contains("UX6"));
        assert_eq!(c("PUT", "/products/:id", "商品なし")[6], "○"); // 網羅の表に URL の具体例が数えられる
        for (old, new, what) in [("| UX4 | POST | /orders |", "| UX4 | POST | /nothing |", "ルートがない"), ("| 通貨が違う | 422 |", "| 通貨が違う | 200 |", "状態が違う"), ("| 受付 | 201 |", "| 受付済 | 201 |", "結果がない")] {
            let bad = read_plan(&mutate(&md, old, new));
            let found = compare_static(&sp, &bad).iter().any(|f| f.where_.contains("URL例")) || url_rows(ck, &bad).is_err();
            assert!(found, "{}", what);
        }
    });
}

fn find_step<'a>(cases: &'a [Value], name: &str) -> &'a Vec<Value> {
    cases.iter().find(|c| c["name"] == name).unwrap_or_else(|| panic!("{}", name))["steps"].as_array().unwrap()
}

#[test]
fn 画面の具体例はユースケースの手順と結ばれ通しテストの手順に変わる() {
    let md = plan_md();
    with_ck(&code(), |ck| {
        let plan = read_plan(&md);
        let cases = screen_cases(ck, &plan).unwrap();
        let order = find_step(&cases, "商品を見つけて注文する");
        assert_eq!(order.iter().map(|s| s["op"].as_str().unwrap().to_string()).collect::<Vec<_>>(), vec!["look", "open", "act", "look"]);
        assert!(order[1]["row"] == "tea-jp" && order[1]["opens"] == "OrderForm");
        let placed = &order[2];
        assert!(placed["deed"] == "place_order" && placed["result"] == "Received" && placed["to"] == "MyOrders");
        let set = placed["set"].as_array().unwrap();
        assert!(set.contains(&json!({"as": "field", "field": "ship_to.country", "map": false, "value": "JP"}))); // 合成の部品は 欄.部品
        assert!(set.contains(&json!({"as": "count", "row": "knife-jp", "value": 1})));
        assert_eq!(order[0]["design"]["layout"], "grid"); // 画面デザインの表から
        let rej = find_step(&cases, "注文が受け付けられない理由を知る");
        assert_eq!(rej[3]["replace"], json!([{"as": "slot", "slot": "currency", "value": "USD"}]));
        assert_eq!(rej[3]["result"], "OtherCurrency");
        assert_eq!(find_step(&cases, "取り下げた商品を扱おうとして失敗する")[1]["replace"][0]["field"], "product_id");
        let exp = &find_step(&cases, "返事が来ない注文は、30分で取り消される")[2];
        assert_eq!(exp["drop"], json!(["PaymentSettled"]));
        assert_eq!(exp["advance"], 1800); // 返事を落とし、時計を進める（実際には待たない）
        let ext = &find_step(&cases, "商品ページでなければ、何も出さない")[0];
        assert!(ext["ext"].as_bool().unwrap_or(false) && ext["origin"] == "https://www.amazon.co.jp");
        assert_eq!(ext["expect"], json!([{"as": "表示なし", "value": ""}]));
        let t = Tmp::new("screenex");
        let f = write_screen_tests(ck, &plan, &t.0).unwrap().unwrap();
        let d = t.0.join("server").join("tests");
        assert_eq!(f, d.join("screen_examples.rs"));
        assert!(d.join("screen_runner.js").exists() && d.join("screen_cases.json").exists());
        for (old, new, what) in [
            ("| 商品を見つけて注文する | 3 | 明細.knife-jp", "| 商品を見つけて注文する | 3 | 明細.knife-jp = 1; 備考 = x; 明細.knife-jp", "ない入力欄"),
            ("| 通知: 受付 |\n| 商品を見つけて注文する | 4", "| 通知: 受付; 色: 赤 |\n| 商品を見つけて注文する | 4", "読めない見えること"),
            ("| 商品を入荷する | 2 | 商品ID = bowl-jp", "| 商品を入荷する | 9 | 商品ID = bowl-jp", "手順がない"),
        ] {
            assert!(screen_cases(ck, &read_plan(&mutate(&md, old, new))).is_err(), "{}", what);
        }
    });
}
