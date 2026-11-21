// SPDX-License-Identifier: GPL-3.0-or-later
//! 小さなお店（tests/fixtures/shop）の検査・突合・トリガ・台帳の試験。
//! 検査が止める例（宣言の漏れ・遷移・不変条件の反例など）を、お店のコードを 1 か所ずつ壊して確かめる。
//! 元は Python 版の tests/test_geas.py の前半（タグ python-final）。
mod common;
use common::*;
use geas_core::check::{ERROR, QUESTION};
use geas_core::interview as iv;
use geas_core::plan::{compare_static, read_plan, A_ONLY, B_ONLY, DIFF};
use geas_core::rustgen::generate;
use geas_core::spec::build;
use serde_json::json;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_geas");

pub fn code() -> String {
    std::fs::read_to_string(root().join("tests/fixtures/shop/shop.geas")).unwrap()
}

fn plan_text() -> String {
    std::fs::read_to_string(root().join("tests/fixtures/shop/shop_plan.md")).unwrap()
}

/// 検査の診断の文言（レベル指定）
fn lv(src: &str, level: &str) -> Vec<String> {
    with_check(src, false, |ck| ck.diags.iter().filter(|d| d.level == level).map(|d| d.msg.clone()).collect())
}

fn any(v: &[String], needle: &str) -> bool {
    v.iter().any(|m| m.contains(needle))
}

// ---- 正常系 ----
#[test]
fn お店の例は検査を通る() {
    with_check(&code(), false, |ck| {
        assert!(ck.ok(), "{:?}", ck.diags);
        assert!(!ck.diags.iter().any(|d| d.level == QUESTION));
    });
}

#[test]
fn 不変条件は証明済みと出る() {
    with_check(&code(), false, |ck| {
        let text: Vec<String> = ck.invariant_status.iter().flat_map(|(_, rows)| rows.iter().map(|(_, s)| s.clone())).collect();
        let text = text.join(" ");
        assert!(!text.contains("証明できない") && text.contains("減算の前に不足を確認"), "{}", text);
    });
}

#[test]
fn 仕様書は読み書きと遷移をコードから取り出す() {
    with_check(&code(), false, |ck| {
        let spec = build(ck);
        let shop = spec["処理"].as_array().unwrap().iter().find(|x| x["処理"] == "受注").unwrap().clone();
        assert_eq!(shop["書く"], json!(["注文", "商品.在庫"]));
        assert!(spec["遷移"].as_array().unwrap().contains(&json!(["注文", "受付", "決済要求済", "決済要求"])));
        let pay = spec["処理"].as_array().unwrap().iter().find(|x| x["処理"] == "決済要求").unwrap().clone();
        assert_eq!(pay["外部"], json!(["決済.請求(冪等キー=注文ID)"]));
    });
}

#[test]
fn 物理数は範囲から選ばれる() {
    with_check(&code(), false, |ck| {
        let spec = build(ck);
        let real = spec["実現"].as_array().unwrap().iter().find(|r| r[0] == "商品.在庫").unwrap().clone();
        assert_eq!(real[1], "u32");
        let order = spec["処理"].as_array().unwrap().iter().find(|x| x["処理"] == "受注").unwrap().clone();
        assert!(order["物理数"].as_array().unwrap().contains(&json!(["明細.値", "u8"])));
    });
}

// ---- 検査が止める例（構文のスケッチ §3 の表）----
#[test]
fn 丸めの指定が無いと問いになる() {
    let src = mutate(&code(), "let total = subtotal + round(subtotal * tax_rate, floor, JPY)", "let total = subtotal + subtotal * tax_rate");
    assert!(any(&lv(&src, QUESTION), "端数をどう丸めますか"));
}

#[test]
fn 結果に行き先が無いと止まる() {
    let src = mutate(&code(), ", NoSuchCustomer → reply\n", "\n");
    assert!(lv(&src, ERROR).iter().any(|m| m.contains("顧客なし") && m.contains("行き先")));
}

#[test]
fn 冪等キーが無いと止まる() {
    assert!(any(&lv(&mutate(&code(), ", idempotency_key = order_id)", ")"), ERROR), "冪等キー"));
}

#[test]
fn 宣言に無い遷移は止まる() {
    let src = mutate(&code(), "  order.status = PaymentRequested", "  order.status = Completed");
    with_check(&src, false, |ck| assert!(ck.diags.iter().any(|d| d.level == ERROR && d.code.contains("遷移") && d.msg.contains("完了")), "{:?}", ck.diags));
}

#[test]
fn 元の状態の確認が無いと止まる() {
    assert!(any(&lv(&mutate(&code(), "  if order.status != Received { return WrongStatus }\n", ""), ERROR), "確認"));
}

#[test]
fn 在庫不足の判定を削ると不変条件が示せない() {
    assert!(any(&lv(&mutate(&code(), "  if short is nonempty { return OutOfStock(short) }\n", ""), ERROR), "反例"));
}

#[test]
fn 書く項目の宣言漏れは止まる() {
    assert!(any(&lv(&mutate(&code(), "  writes Product.stock, Order\n", "  writes Order\n"), ERROR), "商品.在庫"));
}

#[test]
fn 合計ゼロの確認を削ると不変条件が示せない() {
    assert!(any(&lv(&mutate(&code(), "  if order.total <= 0 { return ZeroTotal }\n", ""), ERROR), "合計 > 0"));
}

#[test]
fn 存在を確かめない参照は問いになる() {
    let src = mutate(&code(), "  let customer = Customer[customer_id] else { return NoSuchCustomer }", "  let customer = Customer[customer_id]");
    assert!(any(&lv(&src, QUESTION), "存在しない"));
}

#[test]
fn 構文エラーは行番号つき() {
    let e = parse_err("entity Product {\n  stock int\n}\n").unwrap();
    assert!(e.contains("行目"), "{}", e);
}

// ---- Rust 生成 ----
#[test]
fn エラーのあるコードからは生成しない() {
    let src = mutate(&code(), ", idempotency_key = order_id)", ")");
    with_check(&src, false, |ck| assert!(!ck.ok()));
}

#[test]
fn 生成は決定的で_コミットした生成物と一致する() {
    let a = with_check(&code(), false, |ck| generate(ck));
    assert_eq!(a, with_check(&code(), false, |ck| generate(ck)));
    let committed = std::fs::read_to_string(root().join("tests/fixtures/shop/shop_rust/src/lib.rs")).unwrap();
    assert!(committed == a, "geas rust で生成物を作り直してください");
}

// ---- 突合 ----
fn static_findings(plan: &str) -> Vec<geas_core::plan::Finding> {
    with_check(&code(), false, |ck| compare_static(&build(ck), &read_plan(plan)))
}

#[test]
fn お店の例は計画と静的に一致する() {
    let fs: Vec<_> = static_findings(&plan_text()).into_iter().filter(|f| f.kind != "未規定").collect();
    assert!(fs.is_empty(), "{:?}", fs);
}

#[test]
fn 計画にある遷移がコードに無いと検出する() {
    let plan = mutate(&plan_text(), "| TR2 |", "| TR9 | 注文 | 受付 | 完了 | 即時完了 | 仮 |\n| TR2 |");
    assert!(static_findings(&plan).iter().any(|f| f.kind == A_ONLY && f.where_.contains("受付→完了")));
}

#[test]
fn コードが計画に無いものを書き込むと検出する() {
    let plan = mutate(&plan_text(), "| 商品.在庫, 注文 |", "| 商品.在庫 |");
    assert!(static_findings(&plan).iter().any(|f| f.kind == DIFF && f.where_.contains("書く") && f.msg.contains("注文")));
}

#[test]
fn 範囲の食い違いを検出する() {
    let plan = mutate(&plan_text(), "| 在庫 | 整数 0..1000000 |", "| 在庫 | 整数 1..1000000 |");
    assert!(static_findings(&plan).iter().any(|f| f.kind == DIFF && f.where_.contains("商品.在庫")));
}

#[test]
fn 計画に無い項目がコードにあると検出する() {
    let plan = mutate(&plan_text(), "| DA3 | 商品 | 単価 | 金額(円) 0..10000000 | ○ |  | 仮 |\n", "");
    assert!(static_findings(&plan).iter().any(|f| f.kind == B_ONLY && f.where_.contains("商品.単価")));
}

fn run_examples(plan: &str, tag: &str) -> (Vec<geas_core::plan::Finding>, indexmap::IndexMap<String, (String, String, String)>, Vec<geas_core::plan::Finding>) {
    let t = Tmp::new(tag);
    with_check(&code(), false, |ck| {
        let rows = read_plan(plan).get("具体例").cloned().unwrap_or_default();
        let (src, problems) = geas_core::examples::runner_source(ck, &rows);
        geas_core::crategen::write_crate(ck, &generate(ck), &src, &t.0, false).unwrap();
        let actual = geas_core::examples::run_crate(&t.0, &|s| ck.p.show_names(s)).unwrap();
        let fs = geas_core::examples::compare_examples(&rows, &actual);
        (problems, actual, fs)
    })
}

#[test]
fn 具体例を実際に動かして計画と一致する() {
    let (problems, actual, fs) = run_examples(&plan_text(), "shopex1");
    assert!(problems.iter().all(|p| p.kind == "未規定"), "{:?}", problems);
    assert_eq!(actual["EX1"].0, "受付(N1)");
    assert!(fs.is_empty(), "{:?}", fs);
}

#[test]
fn 具体例が計画と違えば検出する() {
    let plan = mutate(&plan_text(), "| 在庫不足({A}) | なし |", "| 受付(N1) | なし |");
    let (_, _, fs) = run_examples(&plan, "shopex2");
    assert!(fs.iter().any(|f| f.where_.contains("EX2") && f.where_.contains("結果")), "{:?}", fs);
}

fn compare_cli(plan_path: &std::path::Path) -> (bool, String) {
    let r = Command::new(BIN).env("GEAS_LANG", "ja").arg("compare").arg(root().join("tests/fixtures/shop/shop.geas")).arg(plan_path).arg("--no-run").output().unwrap();
    (r.status.success(), String::from_utf8_lossy(&r.stdout).to_string() + &String::from_utf8_lossy(&r.stderr))
}

#[test]
fn cliのcompareは一致すれば0で終わる() {
    let (ok, out) = compare_cli(&root().join("tests/fixtures/shop/shop_plan.md"));
    assert!(ok, "{}", out);
}

#[test]
fn 計画と必須の入力が違うと突合で出る() {
    let t = Tmp::new("shopplan");
    let plan = plan_text().replace("| 決済要求 | 注文ID（必須） |", "| 決済要求 | 注文ID |");
    std::fs::write(t.0.join("plan.md"), plan).unwrap();
    let (ok, out) = compare_cli(&t.0.join("plan.md"));
    assert!(!ok && out.contains("注文ID"), "{}", out);
}

// ---- トリガ ----
#[test]
fn トリガの要因が仕様書に出る() {
    with_check(&code(), false, |ck| {
        let spec = build(ck);
        let t = spec["トリガ"].as_array().unwrap();
        assert!(t.contains(&json!(["決済待ち", "応答 決済成功", "決済結果の反映"])));
        assert!(t.contains(&json!(["決済待ち", "期限 現在+30分", "決済の期限切れ"])));
    });
}

#[test]
fn 期限の無いトリガは止まる() {
    assert!(any(&lv(&mutate(&code(), "  deadline now + 30 min → expire_payment\n", ""), ERROR), "期限がありません"));
}

#[test]
fn 待つ先のトリガが未定義だと止まる() {
    assert!(any(&lv(&mutate(&code(), "Requested → wait PaymentPending", "Requested → wait 承認待ち"), ERROR), "承認待ち"));
}

#[test]
fn トリガの行き先が未定義の処理だと止まる() {
    assert!(any(&lv(&mutate(&code(), "→ expire_payment\n}", "→ 期限切れ処理\n}"), ERROR), "期限切れ処理"));
}

#[test]
fn トリガの引数と処理の入力が合わないと止まる() {
    let m = lv(&mutate(&code(), "trigger PaymentPending(order_id: id)", "trigger PaymentPending(order: id)"), ERROR);
    assert!(m.iter().any(|x| x.contains("合いません") || x.contains("入力にもっていません")), "{:?}", m);
}

#[test]
fn 計画にあるトリガの行き先がコードと違えば検出する() {
    let plan = mutate(&plan_text(), "| 期限 現在+30分 | 決済の期限切れ |", "| 期限 現在+30分 | 決済結果の反映 |");
    assert!(static_findings(&plan).iter().any(|f| f.kind == DIFF && f.where_.contains("トリガ")));
}

#[test]
fn 入力の範囲が項目より広いと止まる() {
    let src = mutate(&code(), "ltype IdSet = set of id\n", "ltype IdSet = set of id\nltype 広い数量 = int 1..200\nltype 広い商品数量 = Product → 広い数量\n");
    let src = mutate(&src, "customer_id: id required, items: ProductQuantity required", "customer_id: id required, items: 広い商品数量 required");
    assert!(any(&lv(&src, ERROR), "範囲の広い入力"));
}

#[test]
fn トリガの型の食い違いと期限の誤りは止まる() {
    assert!(any(&lv(&mutate(&code(), "trigger PaymentPending(order_id: id)", "trigger PaymentPending(order_id: int)"), ERROR), "型"));
    assert!(any(&lv(&mutate(&code(), "  deadline now + 30 min → expire_payment", "  deadline now + 30 min → expire_payment\n  deadline now + 1 hour → expire_payment"), ERROR), "期限が複数"));
    assert!(any(&lv(&mutate(&code(), "deadline now + 30 min", "deadline now + 0 min"), ERROR), "0より長く"));
}

// ---- 初回の対話の台帳 ----
fn runtime_all(v: serde_json::Value) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    for (k, _) in iv::RUNTIME {
        m.insert(k.to_string(), v.clone());
    }
    serde_json::Value::Object(m)
}

#[test]
fn 台帳は次に聞くことを示し_揃うまで開始不可() {
    let mut led = iv::new_ledger("メモ");
    let (todo, ready) = iv::status(&led);
    assert!(!ready && todo.iter().any(|t| t.contains("一言")));
    led["summary"] = json!("メモを残す");
    led["out_of_scope"] = json!(["共有"]);
    led["usecases"].as_array_mut().unwrap().push(json!({"id": "UC1", "actor": "本人", "goal": "メモを書く", "level": "利用者目標",
        "main": ["書く", "保存する"], "extensions": [{"at": "2", "cond": "容量不足", "handling": "エラー表示"}],
        "examples": [{"given": "空", "when": "書く", "then": "1件"}]}));
    let (todo, ready) = iv::status(&led);
    assert!(!ready && todo.iter().any(|t| t.contains("R1")));
    led["runtime"] = runtime_all(json!({"value": "x"}));
    assert_eq!(iv::status(&led), (vec![], true));
}

fn base_ledger() -> serde_json::Value {
    let mut led = iv::new_ledger("メモ");
    led["summary"] = json!("a");
    led["out_of_scope"] = json!(["b"]);
    led["usecases"].as_array_mut().unwrap().push(json!({"id": "UC1", "level": "利用者目標", "main": ["a"], "extensions": [{"at": "1", "cond": "c", "handling": "h"}], "examples": [{"given": "g", "when": "w", "then": "t"}]}));
    led
}

#[test]
fn 未解決の問いと未仕分けの付加情報は開始を止める() {
    let mut led = base_ledger();
    led["runtime"] = runtime_all(json!({"value": "x"}));
    led["questions"].as_array_mut().unwrap().push(json!({"id": "Q1", "text": "空のとき", "status": "open"}));
    assert!(!iv::status(&led).1);
    led["questions"][0]["status"] = json!("unspec");
    led["extras"].as_array_mut().unwrap().push(json!({"id": "X1", "text": "スマホで使いたい"}));
    let (todo, ready) = iv::status(&led);
    assert!(todo.iter().any(|t| t.contains("仕分け")) && !ready);
}

#[test]
fn 台帳の状態の打ち間違いと_r1の未規定は止まる() {
    let mut led = base_ledger();
    led["questions"].as_array_mut().unwrap().push(json!({"id": "Q1", "text": "x", "status": "Open"}));
    assert!(iv::status(&led).0.iter().any(|t| t.contains("不正")));
    led["questions"][0]["status"] = json!("unspec");
    led["questions"][0]["answer"] = json!("結果に影響しないため");
    led["runtime"] = runtime_all(json!({"status": "answered", "value": "x"}));
    led["runtime"]["R1"] = json!({"status": "unspec", "value": "決めない"});
    let (todo, ready) = iv::status(&led);
    assert!(!ready && todo.iter().any(|t| t.contains("R1") && t.contains("未規定にできません")), "{:?}", todo);
    led["runtime"]["R1"] = json!({"status": "answered", "value": "ブラウザ"});
    led["runtime"]["R4"] = json!({"status": "unspec", "value": "決めない"});
    assert_eq!(iv::status(&led), (vec![], true));
}

#[test]
fn 台帳から出したplanの表は突合の対象にならない() {
    let mut led = iv::new_ledger("メモ");
    led["usecases"].as_array_mut().unwrap().push(json!({"id": "UC1", "goal": "書く", "main": ["書く"]}));
    assert!(read_plan(&iv::render(&led)).is_empty());
}
