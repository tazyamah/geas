// SPDX-License-Identifier: GPL-3.0-or-later
//! 画面（screen）の宣言：検査・仕様書・計画書との突合・生成物（画面の定義・UI・アイコン・MCP の定義）と、
//! page を持つ screen（ブラウザ拡張の画面）：検査と、拡張の生成。
//! 元は Python 版の tests/test_screens.py・test_extgen.py（タグ python-final）。
mod common;
use common::*;
use geas_core::check::{defs::check, Checker, ERROR, WARN};
use geas_core::crategen::{write_dev, write_ext, write_server, write_ui};
use geas_core::hostgen::definitions;
use geas_core::mcp::tool_defs;
use geas_core::mcpbridge::handle;
use geas_core::parser::parse;
use geas_core::plan::{compare_static, read_plan};
use geas_core::rest::{internal_targets, routes};
use geas_core::review::{review, review_changes};
use geas_core::rustgen::generate;
use geas_core::screengen::query_def;
use geas_core::spec::{build, render};
use regex::Regex;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn shop_path() -> PathBuf {
    root().join("tests").join("fixtures").join("overseas_shop").join("overseas_shop.geas")
}

fn code() -> String {
    fixture("overseas_shop/overseas_shop.geas")
}

fn plan_md() -> String {
    fixture("overseas_shop/overseas_shop_plan.md")
}

fn with_ck<R>(src: &str, strict: bool, f: impl FnOnce(&Checker) -> R) -> R {
    // 標準モジュール Gtin（use "gtin.geas"）の隣のファイルとして読む
    let mut prog = parse(src, Some(&shop_path())).unwrap_or_else(|e| panic!("構文の誤り: {}", e));
    let ck = check(&mut prog, strict);
    f(&ck)
}

fn msgs(src: &str, strict: bool, level: &str) -> Vec<String> {
    with_ck(src, strict, |ck| ck.diags.iter().filter(|d| d.level == level).map(|d| d.msg.clone()).collect())
}

fn mutate(src: &str, old: &str, new: &str) -> String {
    assert!(src.contains(old), "{}", old);
    src.replacen(old, new, 1)
}

fn any(v: &[String], needle: &str) -> bool {
    v.iter().any(|m| m.contains(needle))
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

// ---- 画面の宣言 ----
#[test]
fn 海外店の画面は検査を通り仕様書に画面と遷移とデザインが出る() {
    with_ck(&code(), true, |ck| {
        assert!(ck.ok(), "{:?}", ck.diags);
        let spec = build(ck);
        let screens = spec["画面"].as_array().unwrap();
        assert_eq!(screens.iter().map(|x| s(&x[0])).collect::<Vec<_>>(), vec!["商品一覧の画面", "注文の画面", "注文の履歴の画面", "価格の確認の画面（Chrome 拡張）", "商品の管理の画面"]);
        assert_eq!(screens.iter().map(|x| s(&x[4])).collect::<Vec<_>>(), vec!["○", "", "", "○", "○"]);
        let trans = spec["画面遷移"].as_array().unwrap();
        let has = |a: &str, b: &str, c: &str, d: &str| trans.iter().any(|x| s(&x[0]) == a && s(&x[1]) == b && s(&x[2]) == c && s(&x[3]) == d);
        assert!(has("商品一覧の画面", "行を選ぶ", "", "注文の画面"));
        assert!(has("注文の画面", "注文", "受付", "注文の履歴の画面"));
        let md = render(ck, &spec, "");
        assert!(md.contains("## 画面デザイン（突合しない。配置と色）") && md.contains("| 商品一覧の画面 | accent | #0b5cad |"));
    });
}

#[test]
fn 結果の行き先に漏れがあるとエラー() {
    let m = msgs(&mutate(&code(), "    OtherCurrency → stay\n", ""), true, ERROR);
    assert!(m.iter().any(|x| x.contains("結果「OtherCurrency」の行き先がありません") || x.contains("違う商品") || x.contains("通貨が違う")), "{:?}", m);
}

#[test]
fn 処理の入力を埋めるものが無いとエラー() {
    let m = msgs(&mutate(&code(), "field customer_id from customer_list, items from product_list, ship_to", "field customer_id from customer_list, items from product_list"), true, ERROR);
    assert!(m.iter().any(|x| x.contains("入力「") && x.contains("埋めるものがありません")), "{:?}", m);
}

#[test]
fn 行き先が実在しないとエラー_入口から行けない画面は警告() {
    assert!(any(&msgs(&mutate(&code(), "Received → MyOrders", "Received → Nowhere"), true, ERROR), "行き先"));
    assert!(any(&msgs(&mutate(&code(), "screen PriceCheck entry {", "screen PriceCheck {"), true, WARN), "入口の画面から行けません"));
}

#[test]
fn designの値の形を検査する() {
    assert!(any(&msgs(&mutate(&code(), "accent \"#e47911\"", "accent \"red\""), true, ERROR), "accent"));
    assert!(any(&msgs(&mutate(&code(), "layout stack", "layout wild"), true, ERROR), "layout"));
    assert!(any(&msgs(&mutate(&code(), "layout stack", "font big"), true, ERROR), "使えません"));
}

#[test]
fn 計画書との突合は画面と遷移を比べデザインは比べない() {
    with_ck(&code(), true, |ck| {
        let spec = build(ck);
        let mut lines: Vec<String> = vec!["| 画面 | ◆見せる | ◆入力欄 | ◆受け取る値 | ◆入口 | ◆選ばせる slot |".into(), "|---|---|---|---|---|---|".into()];
        for x in spec["画面"].as_array().unwrap() {
            lines.push(format!("| {} | {} | {} | {} | {} | {} |", s(&x[0]), s(&x[1]), s(&x[2]), s(&x[3]), s(&x[4]), s(&x[5])));
        }
        lines.extend(["".to_string(), "| 画面 | ◆操作 | ◆結果 | ◆行き先 |".to_string(), "|---|---|---|---|".to_string()]);
        for x in spec["画面遷移"].as_array().unwrap() {
            lines.push(format!("| {} | {} | {} | {} |", s(&x[0]), s(&x[1]), s(&x[2]), s(&x[3])));
        }
        let plan = lines.join("\n");
        let only: Vec<_> = compare_static(&spec, &read_plan(&plan)).into_iter().filter(|f| f.where_.starts_with("画面")).collect();
        assert!(only.is_empty(), "{:?}", only.iter().map(|f| format!("{}｜{}", f.where_, f.msg)).collect::<Vec<_>>());
        let bad = plan.replace("| 注文の画面 | 注文 | 受付 | 注文の履歴の画面 |", "| 注文の画面 | 注文 | 受付 | 商品一覧の画面 |");
        assert!(compare_static(&spec, &read_plan(&bad)).iter().any(|f| f.where_.contains("行き先")));
    });
}

#[test]
fn ユースケースの手順が画面遷移として実在しつながっているかを突合する() {
    with_ck(&code(), true, |ck| {
        let spec = build(ck);
        let f = |plan: &str| compare_static(&spec, &read_plan(plan));
        assert!(f(&plan_md()).is_empty());
        // 存在しない操作
        let bad = plan_md().replace("| 商品を見つけて注文する | 3 | 注文の画面 | 注文 | 受付 |", "| 商品を見つけて注文する | 3 | 注文の画面 | 注文 | 決済済み |");
        assert!(f(&bad).iter().any(|x| x.msg.contains("コードの画面遷移にありません")));
        // つながっていない手順（行き先と次の手順の画面が違う）
        let bad = plan_md().replace("| 商品を見つけて注文する | 4 | 注文の履歴の画面 | 見る |  |", "| 商品を見つけて注文する | 4 | 商品一覧の画面 | 見る |  |");
        assert!(f(&bad).iter().any(|x| x.msg.contains("前の手順のあとに見えている画面")));
        // どのユースケースにも使われない遷移
        let bad = plan_md().replace("| 注文が受け付けられない理由を知る | 5 | 注文の画面 | 注文 | 顧客なし | /orders/new |\n", "");
        assert!(f(&bad).iter().any(|x| x.msg.contains("使われていません")));
    });
}

#[test]
fn 人向けの仕様書は画面の遷移とモックとデザインを含むhtmlになる() {
    with_ck(&code(), true, |ck| {
        let findings = compare_static(&build(ck), &read_plan(&plan_md()));
        let html = review(ck, Some(&plan_md()), &findings, "（海外店）", false).unwrap();
        for x in ["画面の流れ", "画面デザイン", "ユースケースの動作の一覧", "id=\"screens-data\"", "商品を見つけて注文する", "誰が何をするか", "状態の移り変わり：注文", "処理の流れ：", "class=\"diagram\""] {
            assert!(html.contains(x), "{}", x);
        }
        assert!(!html.contains("mermaid") && !html.contains("https://cdnjs")); // 図は自前の SVG で、外部の読み込みに頼らない
        assert!(!html.contains("受け入れ可") && !html.contains("前の版からの変更") && !html.contains("仕様レビュー")); // 件数・変更点は別の HTML
        for (label, key) in [("ユースケース", "uc"), ("動作", "act"), ("仕様", "spec")] {
            assert!(html.contains(&format!("data-view=\"{}\"", key)), "{}", label); // タブは3種類
        }
        assert!(html.contains("id=\"origin\"") && html.contains("id=\"specsel\"") && html.contains("data-dev=\"pc\"") && html.contains("id=\"pad\"")); // 起点の選択・分類・端末の切り替え・コントローラー
    });
}

#[test]
fn 前の版を渡すと変更点だけの別htmlに差が出る() {
    with_ck(&code(), true, |ck| {
        with_ck(&mutate(&code(), "    OtherCurrency → stay\n", "    OtherCurrency → reply\n"), true, |old| {
            let html = review_changes(ck, &[], "（海外店）", Some(old), "前の版", false, None);
            assert!(html.contains("前の版からの変更") && html.contains("比較元：前の版") && html.contains("class=\"removed\"") && html.contains("class=\"added\""));
            assert!(html.contains("受け入れ可"));
        });
    });
}

#[test]
fn 画面の定義とdesignが生成物に載る() {
    with_ck(&code(), true, |ck| {
        let defs = definitions(ck).unwrap();
        let d = &defs["screens"];
        assert_eq!(d["title"], "海外にも売るお店");
        assert_eq!(d["screens"]["ProductList"]["design"], json!({"layout": "grid", "accent": "#0b5cad", "mode": "light"}));
        assert_eq!(d["screens"]["PriceCheck"]["design"]["mode"], "dark");
        let form = &d["screens"]["OrderForm"];
        assert_eq!(form["fields"].as_array().unwrap().iter().map(|f| f["from"].clone()).collect::<Vec<_>>(), vec![json!("customer_list"), json!("product_list"), Value::Null]);
        assert!(form["fields"][1]["type"]["kind"] == "map" && form["slots"][0]["name"] == "currency");
        let price = d["queries"]["product_list"]["columns"].as_array().unwrap().iter().find(|c| c["name"] == "price").unwrap().clone();
        assert!(price["kind"] == "money" && price["unit"] == "currency");
        assert_eq!(d["events_on"], json!({"Reconnected": "online"}));
        let rs = generate(ck);
        assert!(rs.contains("pub const APP_TITLE: &str = \"海外にも売るお店\";"));
        assert!(rs.contains("ScreenDef { name: \"PriceCheck\"") && rs.contains("accent: \"#e47911\", mode: \"dark\""));
    });
}

#[test]
fn 画面の汎用uiとアイコンを生成物へ置く() {
    let t = Tmp::new("ui");
    with_ck(&code(), true, |ck| write_ui(ck, &t.0).unwrap());
    let ui = t.0.join("ui");
    let names: std::collections::BTreeSet<String> = std::fs::read_dir(&ui).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    for n in ["index.html", "ui.js", "ui.css", "sw.js", "icon.svg", "icon-192.png", "icon-512.png"] {
        assert!(names.contains(n), "{}", n);
    }
    assert!(std::fs::read_to_string(ui.join("icon.svg")).unwrap().contains("#0b5cad"));
    assert_eq!(&std::fs::read(ui.join("icon-192.png")).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
}

#[test]
fn 画面の定義はリソースのurlを持つ() {
    with_ck(&code(), true, |ck| {
        let d = definitions(ck).unwrap()["screens"]["screens"].clone();
        assert_eq!(d["OrderForm"]["url"], "/orders/new");
        assert_eq!(d["MyOrders"]["url"], "/customers/:customer_id/orders");
    });
}

#[test]
fn 固定の通貨の金額の列は通貨を持って定義に出る() {
    // money(JPY) のような固定の通貨の列は、通貨そのものを持つ。money(currency) の列は、通貨を持つ列の名前を持つ
    let demo = fixture("runtime_demo/runtime_demo.geas");
    with_check(&demo, true, |fixed| {
        let q = fixed.queries["parcel_list"];
        let def = query_def(fixed, q, &|x| x.to_string());
        let col = |name: &str| def["columns"].as_array().unwrap().iter().find(|c| c["name"] == name).unwrap().clone();
        let fee = col("fee");
        assert!(fee["kind"] == "money" && fee.get("currency") == Some(&json!("JPY")) && fee.get("unit").is_none(), "{}", fee);
    });
    with_ck(&code(), true, |ck| {
        let def = query_def(ck, ck.queries["orders_of"], &|x| x.to_string());
        let total = def["columns"].as_array().unwrap().iter().find(|c| c["name"] == "total").unwrap().clone();
        assert!(total["unit"] == "currency" && total.get("currency").is_none());
    });
}

#[test]
fn mcpのツール定義はurlの定義から機械的に作られ仕様書の動作とその他に出る() {
    with_ck(&code(), true, |ck| {
        let defs = tool_defs(ck).unwrap();
        let tools: std::collections::HashMap<String, Value> = defs.iter().map(|t| (s(&t["name"]), t.clone())).collect();
        let inner = internal_targets(ck.p);
        // 1 ルート＝1 ツール。流れの途中の処理の URL は、サーバーが外に出さないのでツールにしない
        assert_eq!(tools.len(), routes(ck.p).iter().filter(|r| !inner.contains(&r.target)).count());
        assert!(!tools.contains_key("create_order_completions") && !tools.contains_key("create_order_settlements") && tools.contains_key("create_order_recheck_requests"));
        let o = &tools["create_orders"]["inputSchema"];
        assert_eq!(o["required"], json!(["customer_id", "items", "ship_to", "slots"]));
        assert!(o["properties"]["slots"]["properties"].get("tax_rate").is_none()); // サーバーが決める slot は渡させない
        assert!(tools["show_product_prices"]["annotations"]["readOnlyHint"] == json!(true) && tools["destroy_products"]["annotations"]["destructiveHint"] == json!(true));
        assert_eq!(tools["update_products"]["inputSchema"]["required"], json!(["id", "quantity", "if_match"])); // 書き込みには、読んだときの行の版が要る
        let html = review(ck, Some(&plan_md()), &[], "（海外店）", false).unwrap();
        assert!(html.contains("value=\"mcp\"") && html.contains("MCP（AI から）") && html.contains("data-view=\"etc\"") && html.contains("注文の状況を AI に聞く"));
        assert!(!html.contains('⚠')); // 台本のツール呼び出しは、定義と合っている
        let bad = plan_md().replace("show_product_prices `{\"jan\":\"12345\"}`", "show_product_prices `{\"code\":\"12345\"}`");
        assert!(review(ck, Some(&bad), &[], "（海外店）", false).unwrap().contains("定義にない入力：code"));
    });
}

#[test]
fn mcpの橋渡しはツールの呼び出しをrestに中継する() {
    // 中継先は、受けた要求を記録するだけの模擬のサーバー
    let seen: Arc<Mutex<Vec<(String, String, String, String)>>> = Arc::new(Mutex::new(vec![]));
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen2 = seen.clone();
    std::thread::spawn(move || {
        for st in l.incoming() {
            let mut st = st.unwrap();
            let mut buf = vec![];
            let mut chunk = [0u8; 4096];
            loop {
                let n = st.read(&mut chunk).unwrap();
                buf.extend_from_slice(&chunk[..n]);
                if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..p]).into_owned();
                    let cl: usize = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
                    if buf.len() >= p + 4 + cl {
                        let mut first = head.lines().next().unwrap().split_whitespace();
                        let (m, path) = (first.next().unwrap().to_string(), first.next().unwrap().to_string());
                        let im = head.lines().find_map(|l| l.strip_prefix("If-Match: ")).unwrap_or("").to_string();
                        let body = String::from_utf8_lossy(&buf[p + 4..p + 4 + cl]).into_owned();
                        let (code, out) = if m == "PUT" { ("200 OK", "{\"result\":\"ok\"}") } else { ("404 Not Found", "{\"error\":\"x\"}") };
                        seen2.lock().unwrap().push((m, path, body, im));
                        let _ = st.write_all(format!("HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", code, out.len(), out).as_bytes());
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
        }
    });
    let base = format!("http://127.0.0.1:{}", port);
    let tools = with_ck(&code(), true, |ck| tool_defs(ck).unwrap());
    let call = |v: Value| handle(v.as_object().unwrap(), &tools, &base).unwrap();
    let r = call(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "update_products", "arguments": {"id": "tea-jp", "quantity": 5, "if_match": "r0123"}}}));
    // 書き込みのツールは、読んだときの行の版（if_match）を受け、ヘッダ If-Match で送る（本文には入れない）
    assert_eq!(r["result"]["isError"], json!(false), "{}", r);
    {
        let g = seen.lock().unwrap();
        let last = g.last().unwrap();
        assert!(last.0 == "PUT" && last.1 == "/products/tea-jp" && serde_json::from_str::<Value>(&last.2).unwrap() == json!({"quantity": 5}) && last.3 == "\"r0123\"", "{:?}", last);
    }
    let upd = tools.iter().find(|t| t["name"] == "update_products").unwrap();
    assert!(upd["inputSchema"]["required"].as_array().unwrap().contains(&json!("if_match")) && s(&upd["inputSchema"]["properties"]["if_match"]["description"]).contains("_etag"));
    let r = call(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "show_product_prices", "arguments": {"jan": "x y"}}}));
    assert_eq!(r["result"]["isError"], json!(true));
    assert_eq!(seen.lock().unwrap().last().unwrap().1, "/product_prices/x%20y");
    let r = call(json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}));
    assert_eq!(r["result"]["tools"].as_array().unwrap().len(), tools.len());
}

// ---- page を持つ screen（ブラウザ拡張の画面） ----
#[test]
fn pageを持つ画面は拡張として生成される() {
    let t = Tmp::new("ext");
    with_ck(&code(), false, |ck| {
        assert_eq!(geas_core::crategen::ext_screens(ck).iter().map(|x| x.name.clone()).collect::<Vec<_>>(), vec!["PriceCheck"]);
        write_ext(ck, &t.0, "http://localhost:9999").unwrap();
    });
    let ext = t.0.join("ext");
    let names: std::collections::BTreeSet<String> = std::fs::read_dir(&ext).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["background.js", "content.js", "content.css", "defs.js", "manifest.json", "popup.html", "popup.js", "reader.js"].iter().map(|x| x.to_string()).collect());
    let m: Value = serde_json::from_str(&std::fs::read_to_string(ext.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(m["content_scripts"][0]["matches"], json!(["*://www.amazon.co.jp/*", "*://www.amazon.com/*"]));
    assert!(m["host_permissions"].as_array().unwrap().contains(&json!("http://localhost:9999/*")));
    let defs = std::fs::read_to_string(ext.join("defs.js")).unwrap();
    let d: Value = serde_json::from_str(defs.split_once('=').unwrap().1.trim().trim_end_matches(';')).unwrap();
    assert_eq!(d["route"], json!({"method": "GET", "path": "/product_prices/:jan"}));
    assert_eq!(d["input"]["label"], "JANコード");
}

#[test]
fn 拡張の画面は問い合わせを1つ見せるだけ() {
    let bad = code().replace("  field jan\n  show price_by_jan\n", "  field jan\n  show price_by_jan\n  show product_list\n");
    assert_ne!(bad, code());
    assert!(any(&msgs(&bad, false, ERROR), "page を持つ画面"));
    let bad = code().replace("page \"*://www.amazon.co.jp/*\", \"*://www.amazon.com/*\"", "page \"amazon\"");
    assert!(any(&msgs(&bad, false, ERROR), "URL のパターンではありません"));
}

#[test]
fn readは必須で読み方の誤りは止まる() {
    let re = Regex::new(r"(?s)  read \{.*?\n  \}\n").unwrap();
    let block = re.find(&code()).unwrap().as_str().to_string();
    assert!(any(&msgs(&code().replace(&block, ""), false, ERROR), "read が要ります"));
    let bad = code().replace("url \"/(?:dp", "url \"/(?:dp(").replace("(?:[/?]|$)", "");
    assert!(any(&msgs(&bad, false, ERROR), "正規表現"));
    assert!(any(&msgs(&code().replace("\"4901234567894\"", "\"4901234567895\""), false, ERROR), "検査数字"));
    assert!(any(&msgs(&code().replace("([A-Z0-9]{10})", "[A-Z0-9]{10}"), false, ERROR), "( ) が要ります"));
}

#[test]
fn 汎用のreaderが生成物に入り読み方はdefsに出る() {
    let t = Tmp::new("reader");
    with_ck(&code(), false, |ck| write_ext(ck, &t.0, "http://localhost:8787").unwrap());
    assert!(std::fs::read_to_string(t.0.join("ext").join("reader.js")).unwrap().starts_with("// ページから値を読む（汎用）"));
    let defs = std::fs::read_to_string(t.0.join("ext").join("defs.js")).unwrap();
    let d: Value = serde_json::from_str(defs.split_once('=').unwrap().1.trim().trim_end_matches(';')).unwrap();
    assert!(d["read"]["subject"] == "ASIN" && d["read"]["rows"] == json!(["JAN", "EAN", "GTIN"]) && d["input"]["requires"].as_array().unwrap().len() == 3); // 入力の型（標準モジュール Gtin）の変換の、拒否の条件（桁数・数字だけ・検査数字）
    assert_eq!(d["read"]["lookup"]["B0SHOPTEA1"], "4901234567894");
}

#[test]
fn 応答の結果の種類は検査され模擬の定義になる() {
    with_ck(&code(), false, |ck| {
        let m = definitions(ck).unwrap()["mock"].clone();
        assert_eq!(m["effects"]["Payment.charge"]["reply"], "PaymentSettled");
        assert_eq!(m["replies"]["PaymentSettled"]["outcomes"].as_array().unwrap().iter().map(|o| s(&o["name"])).collect::<Vec<_>>(), vec!["Approved", "Declined"]);
        assert_eq!(m["replies"]["StockSettled"]["outcomes"][1], json!({"name": "Short", "label": "在庫不足", "gives": ["short"]}));
        assert_eq!(m["events"], json!([{"name": "Reconnected", "label": "つながりが戻った"}]));
    });
    assert!(any(&msgs(&code().replace("Approved gives auth_id", "Approved gives nothing_here"), false, ERROR), "応答の中身にありません"));
    assert!(any(&msgs(&code().replace("replies StockSettled", "replies NoSuchReply"), false, ERROR), "未定義の応答"));
}

#[test]
fn 開発用の操作パネルは汎用のページとして生成される() {
    let t = Tmp::new("dev");
    write_dev(&t.0).unwrap();
    let names: std::collections::BTreeSet<String> = std::fs::read_dir(t.0.join("dev")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["test.html", "test.js"].iter().map(|x| x.to_string()).collect());
}

#[test]
fn サーバーの実行部は生成物と一緒に置かれ最初のデータは埋め込まれる() {
    let t = Tmp::new("serverparts");
    write_server(&t.0, None, false).unwrap();
    let sv = t.0.join("server");
    let names: std::collections::BTreeSet<String> = std::fs::read_dir(sv.join("src")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    let want: std::collections::BTreeSet<String> = ["app.rs", "db.rs", "engine.rs", "fileio.rs", "http.rs", "lib.rs", "main.rs", "mock.rs", "net.rs", "ospath.rs", "seed.json"].iter().map(|x| x.to_string()).collect();
    assert_eq!(names, want);
    let cargo = std::fs::read_to_string(sv.join("Cargo.toml")).unwrap();
    assert!(cargo.contains("geas_app = { path = \"..\" }") && sv.join("Cargo.lock").exists());
    // Cargo は組み立ての起点の [profile] しか使わないので、リリースでの溢れの確認はサーバーの側にも書く
    assert!(cargo.contains("[profile.release]\noverflow-checks = true\n"));
    assert_eq!(std::fs::read_to_string(sv.join("src").join("seed.json")).unwrap(), "{}"); // 既定は何も入れない
    let mine = t.0.join("mine.json");
    std::fs::write(&mine, "{\"A\": {}}").unwrap();
    write_server(&t.0, Some(&mine.to_string_lossy()), false).unwrap();
    assert_eq!(std::fs::read_to_string(sv.join("src").join("seed.json")).unwrap(), "{\"A\": {}}");
}
