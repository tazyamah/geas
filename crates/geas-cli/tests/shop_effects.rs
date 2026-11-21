// SPDX-License-Identifier: GPL-3.0-or-later
//! 小さなお店の「外部作用・流れ・型」の検査の試験：冪等キー、par（並列の行き先）、定義の重複、整数と金額の型、「無いかもしれない」項目。
//! 元は Python 版の tests/test_geas.py の中ほど（タグ python-final）。
mod common;
use common::*;
use geas_core::check::{ERROR, WARN};
use geas_core::spec::build;
use serde_json::json;

fn code() -> String {
    std::fs::read_to_string(root().join("tests/fixtures/shop/shop.geas")).unwrap()
}

fn lv(src: &str, level: &str) -> Vec<String> {
    with_check(src, false, |ck| ck.diags.iter().filter(|d| d.level == level).map(|d| d.msg.clone()).collect())
}

fn errs(src: &str) -> Vec<String> {
    lv(src, ERROR)
}

fn any(v: &[String], needle: &str) -> bool {
    v.iter().any(|m| m.contains(needle))
}

fn ready(src: &str) -> bool {
    with_check(src, false, |ck| {
        assert!(ck.ready(), "{:?}", ck.diags.iter().map(|d| d.msg.clone()).collect::<Vec<_>>());
        true
    })
}

const EFFECT: &str = "effect Payment. /// 請求\n  charge(order_id: id, /// 金額\n    amount: money(JPY))";

const BULK: &str = r####"
deed 一括完了 {
  input 他: IdSet
  writes Order.status
  result ReleaseResult
  effects none

  update { Order[k].status = Completed | k in 他 }
  return Applied
}
"####;

// ---- 外部作用とフローの重複・矛盾 ----
const FOR: &str = r####"
deed 引当 {
  input items: ProductQuantity required, order_id: id required
  result ReleaseResult
  effects 倉庫.引当

  for (k, v) in items {
    request 倉庫.引当(k, v, idempotency_key = KEY)
  }
  return Applied
}

nexus 引当の流れ {
  引当: Applied → reply, NoSuchOrder → reply, WrongStatus → reply
}
"####;

fn with_for(key: &str) -> String {
    let src = mutate(&code(), EFFECT, &format!("{}\neffect 倉庫.引当(product_id: id, 数: Quantity) no_wait \"試し\"", EFFECT));
    format!("{}{}", src, FOR.replace("KEY", key))
}

#[test]
fn forの中の冪等キーは回す値と外の値の組にする() {
    assert!(any(&errs(&with_for("order_id")), "for で回す値によりません"));
    assert!(any(&errs(&with_for("k")), "for で回す値だけです")); // 別の注文で同じ商品を回すとぶつかる
    assert!(!any(&errs(&with_for("(order_id, k)")), "冪等"));
    assert!(!any(&errs(&with_for("(k, order_id)")), "冪等"));
    assert!(any(&errs(&with_for("(order_id, v)")), "冪等キーは ID か文字列"));
}

#[test]
fn join_idem_keyは区切りがあいまいにならない() {
    let src = with_for("(order_id, k)");
    assert!(ready(&src));
    let main = "fn main() {\n    println!(\"{}\", geas_app::join_idem_key(&[\"a,b\".to_string(), \"c\".to_string()]));\n    println!(\"{}\", geas_app::join_idem_key(&[\"a\".to_string(), \"b\\\",\\\"c\".to_string()]));\n    println!(\"{}\", geas_app::join_idem_key(&[\"O1\".to_string(), \"P1\".to_string()]));\n}\n";
    let out = run_app(&src, main);
    assert!(out.len() == 3 && out[0] != out[1] && out[2] == "[\"O1\",\"P1\"]", "{:?}", out);
}

#[test]
fn 冪等キーはidか文字列() {
    assert!(any(&errs(&mutate(&code(), "idempotency_key = order_id)", "idempotency_key = 1)")), "冪等キーは ID か文字列"));
}

#[test]
fn 応答と処理を同じ結果に並べると止まる() {
    let src = mutate(&code(), "OutOfStock → reply, NoSuchProduct", "OutOfStock → par { reply, request_payment }, NoSuchProduct");
    assert!(any(&errs(&src), "par に入れられません"));
    let e = parse_err(&mutate(&code(), "OutOfStock → reply, NoSuchProduct", "OutOfStock → reply, OutOfStock → request_payment, NoSuchProduct")).unwrap();
    assert!(e.contains("par"), "{}", e);
}

const NOTIFY: &str = r####"
effect 通知.メール(order_id: id) no_wait "通知は結果を待たない"
effect 通知.プッシュ(order_id: id) no_wait "通知は結果を待たない"
result 通知結果 {
  Requested
}
deed メール通知 {
  input order_id: id required
  result 通知結果
  effects 通知.メール

  request 通知.メール(order_id, idempotency_key = order_id)
  return Requested
}
deed プッシュ通知 {
  input order_id: id required
  result 通知結果
  effects 通知.プッシュ

  request 通知.プッシュ(order_id, idempotency_key = order_id)
  return Requested
}
nexus メール {
  メール通知: Requested → reply
}
nexus プッシュ {
  プッシュ通知: Requested → reply
}
"####;
const FANOUT: (&str, &str) = ("place_order: Received → request_payment, OutOfStock", "place_order: Received → par { メール通知, プッシュ通知 }, OutOfStock");

fn fanout(old: &str, new: &str) -> String {
    mutate(&format!("{}{}", code(), NOTIFY), old, new)
}

#[test]
fn 同じ結果に処理を続けて書くと_書いた順に起こす行き先になる() {
    let src = fanout(FANOUT.0, FANOUT.1);
    with_check(&src, false, |ck| {
        assert!(ck.ready(), "{:?}", ck.diags);
        assert_eq!(build(ck)["処理"][0]["結果"][0], json!(["受付", "並列［メール通知 ＋ プッシュ通知］"]));
    });
    assert!(any(&errs(&fanout(FANOUT.0, "place_order: Received → par { メール通知, メール通知 }, OutOfStock")), "2回書かれています"));
    let rs = with_check(&src, false, |ck| geas_core::rustgen::generate(ck));
    assert!(rs.contains("vec![Dest::Deed(\"メール通知\"), Dest::Deed(\"プッシュ通知\")]"));
}

#[test]
fn parの枝は書く項目が重ならなければデータを書いてよい() {
    assert!(ready(&fanout(FANOUT.0, "place_order: Received → par { メール通知, request_payment }, OutOfStock")));
}

#[test]
fn parの枝が同じ項目に書くか_書かれる項目を読むと止まる() {
    let m = errs(&fanout(FANOUT.0, "place_order: Received → par { request_payment, expire_payment }, OutOfStock"));
    assert!(any(&m, "同じ項目「注文.状態」に書いています"), "{:?}", m);
    assert!(any(&m, "どちらも 注文 の状態を動かします"));
    let m = errs(&fanout(FANOUT.0, "place_order: Received → par { request_payment, place_order }, OutOfStock"));
    assert!(m.iter().any(|x| x.contains("par の枝") && x.contains("新規作成する")), "{:?}", m);
}

const STOCK_DEEDS: &str = r####"
result 減らす結果 {
  Reduced
  Short
}
deed 在庫を減らすA {
  input items: ProductQuantity required
  reads Product.stock
  writes Product.stock
  result 減らす結果
  effects none

  let short = { k | (k, v) in items, Product[k].stock < v }
  if short is nonempty { return Short }
  update { Product[k].stock -= v | (k, v) in items }
  return Reduced
}
deed 在庫を減らすB {
  input items: ProductQuantity required
  reads Product.stock
  writes Product.stock
  result 減らす結果
  effects none

  let short = { k | (k, v) in items, Product[k].stock < v }
  if short is nonempty { return Short }
  update { Product[k].stock -= v | (k, v) in items }
  return Reduced
}
nexus 減らすA {
  在庫を減らすA: Reduced → reply, Short → reply
}
nexus 減らすB {
  在庫を減らすB: Reduced → reply, Short → reply
}
"####;

#[test]
fn parの枝は加算減算の更新だけなら同じ項目に書いてよい() {
    let src = mutate(&format!("{}{}", code(), STOCK_DEEDS), FANOUT.0, "place_order: Received → par { 在庫を減らすA, 在庫を減らすB }, OutOfStock");
    // place_order の結果の中身（受付の注文ID）と合わないので、行き先の受け渡しの誤りだけが出る。並列の誤りは出ない
    assert!(!any(&errs(&src), "par の枝"));
    // 片方が = で書くと、順序で結果が変わるので止まる
    assert!(any(&errs(&fanout(FANOUT.0, "place_order: Received → par { request_payment, expire_payment }, OutOfStock")), "同じ項目"));
}

#[test]
fn 行き先が複数のフローも生成コードがビルドできる() {
    build_only(&fanout(FANOUT.0, FANOUT.1), "fn main() {}\n");
}

#[test]
fn 同じ処理を違う行き先で2つのネクサスに書くと止まる() {
    let src = format!("{}\nnexus 二重 {{\n  place_order: Received → reply, OutOfStock → reply, NoSuchProduct → reply, NoSuchCustomer → reply\n}}\n", code());
    assert!(any(&errs(&src), "違う行き先"));
}

#[test]
fn 失敗の理由を宣言して待たないのは矛盾() {
    assert!(any(&errs(&mutate(&code(), EFFECT, &format!("{} failures 残高不足 no_wait \"試し\"", EFFECT))), "no_wait"));
}

// ---- 生成した Rust が必ずビルドできる ----
#[test]
fn 定義の名前の重複は止まる() {
    for (old, new) in [
        ("entity Customer {", "entity Customer {\n  customer_id: id unique\n}\n\nentity Customer {"),
        ("  stock: StockCount required\n", "  stock: StockCount required\n  stock: NonNegMoney\n"),
        ("  input order_id: id required\n  reads Order.total", "  input order_id: id required\n  slot order_id: id\n  reads Order.total"),
    ] {
        assert!(any(&errs(&mutate(&code(), old, new)), "2つ以上"), "{}", new);
    }
}

#[test]
fn 未定義の型の名前は止まる() {
    assert!(any(&errs(&mutate(&code(), "  input order_id: id required\n  reads Order.total", "  input order_id: id required, 謎: 謎型\n  reads Order.total")), "謎型"));
}

#[test]
fn 最後にreturnが無いと止まる() {
    let src = mutate(&code(), "  request Payment.charge(order_id, order.total, idempotency_key = order_id)\n  return Requested\n", "  request Payment.charge(order_id, order.total, idempotency_key = order_id)\n");
    assert!(any(&errs(&src), "return で終えて"));
}

#[test]
fn ifの条件は真偽() {
    let src = mutate(&code(), "  if order.total <= 0 { return ZeroTotal }\n", "  if order.total <= 0 { return ZeroTotal }\n  if order.total { return ZeroTotal }\n");
    assert!(any(&errs(&src), "if の条件"));
}

#[test]
fn updateの値の型を確かめる() {
    let src = format!("{}{}", code(), BULK.replace("一括完了", "一括ゼロ").replace("writes Order.status", "writes Order.total").replace("Order[k].status = Completed", "Order[k].total = Completed"));
    assert!(any(&errs(&src), "型の違う値"));
}

#[test]
fn 割合の参照があってもホストつきでビルドできる() {
    let main = "fn main() {\n    let r = geas_app::host::read_ratio(\"1/10\", Some(0), Some(1)).unwrap();\n    assert_eq!(r.to_string(), \"1/10\");\n    assert!(geas_app::host::read_ratio(\"3/2\", Some(0), Some(1)).is_err());\n}\n";
    run_app_host(&code(), main);
}

// ---- 誤検出と警告の漏れ ----
#[test]
fn updateの書き込みは_ほかで読んだ項目を消さない() {
    let src = format!("{}{}", code(), BULK.replace("一括完了", "一括ゼロ").replace("writes Order.status", "reads Order.total\n  writes Order.total")
        .replace("  update { Order[k].status = Completed | k in 他 }\n", "  update { Order[k].total = 1 | k in 他, Order[k].total > 0 }\n"));
    let m = errs(&src);
    assert!(!m.iter().any(|x| x.contains("注文.合計") && x.contains("使っていません")), "{:?}", m);
}

#[test]
fn 使わない外部作用と届かない状態は警告() {
    assert!(any(&lv(&mutate(&code(), EFFECT, &format!("{}\neffect 倉庫.引当(product_id: id) no_wait \"試し\"", EFFECT)), WARN), "どの処理からも要求されていません"));
    let w = lv(&mutate(&code(), "transition Order: Received → PaymentRequested", "transition Order: Received → Received"), WARN);
    assert!(w.iter().any(|m| m.contains("決済要求済") && m.contains("進む処理")), "{:?}", w);
}

// ---- ネクサスで次の処理へは、結果の中身を名前と型で渡す ----
#[test]
fn 結果の中身には名前が要る() {
    let e = parse_err(&mutate(&code(), "  Received(order_id: id)\n", "  Received(id)\n")).unwrap();
    assert!(e.contains("名前を付けて"), "{}", e);
}

#[test]
fn 結果の中身と次の処理の入力は名前と型で突き合わせる() {
    assert!(any(&errs(&mutate(&code(), "OutOfStock → reply, NoSuchProduct", "OutOfStock → request_payment, NoSuchProduct")), "中身「不足」を、処理 決済要求 は入力にもっていません"));
    assert!(any(&errs(&mutate(&code(), "  Received(order_id: id)\n", "  Received(番号: id)\n")), "入力「注文ID」が、結果「受付」の中身にありません"));
    let m = errs(&mutate(&code(), "  Received(order_id: id)\n", "  Received(order_id: IdSet)\n"));
    assert!(m.iter().any(|x| x.contains("型") && x.contains("受付")), "{:?}", m);
}

#[test]
fn ホストは結果の中身を名前つきで返す() {
    let req = "{\"name\":\"place_order\",\"ctx\":{\"tax_rate\":\"1/10\",\"new_Order_id\":\"N1\"},\"input\":{\"customer_id\":\"C1\",\"items\":{\"A\":2}},\"store\":{\"Product\":{\"A\":{\"product_id\":\"A\",\"stock\":5,\"unit_price\":100}},\"Customer\":{\"C1\":{\"customer_id\":\"C1\"}}}}";
    let main = format!("fn main() {{\n    println!(\"{{}}\", geas_app::host::call(r#\"{}\"#).unwrap());\n}}\n", req);
    let out = run_app_host(&code(), &main).join("\n");
    assert!(out.contains("\"body\":{\"order_id\":\"N1\"}"), "{}", out);
}

// ---- 整数と金額は違う型。そのままでは混ぜず、money( )・int( ) で変えてから計算する ----
const SUB: &str = "  let total = subtotal + round(subtotal * tax_rate, floor, JPY)\n";

#[test]
fn 整数と金額はそのままでは足し引きできない() {
    let src = mutate(&code(), SUB, &SUB.replace("subtotal + round", "subtotal + total_count + round").replace("  let total", "  let total_count = sum { v | (k, v) in items }\n  let total"));
    assert!(any(&errs(&src), "整数と金額は、そのままでは足し引きできません"));
}

#[test]
fn 金額どうしは掛けられない() {
    assert!(any(&errs(&mutate(&code(), "Product[k].unit_price * v |", "Product[k].unit_price * Product[k].unit_price |")), "金額どうしは掛けられません"));
}

#[test]
fn 整数の変数は金額の項目へそのまま入れられない() {
    let src = mutate(&code(), SUB, &format!("  let total_count = sum {{ v | (k, v) in items }}\n{}", SUB));
    assert!(any(&errs(&mutate(&src, "total: total, status: Received }", "total: total_count, status: Received }")), "型の違う値（Int）"));
    let fixed = mutate(&src, "total: total, status: Received }", "total: money(total_count), status: Received }");
    with_check(&fixed, false, |ck| assert!(ck.ok(), "{:?}", ck.diags));
}

#[test]
fn 数字はどちらの型としても使える() {
    with_check(&code(), false, |ck| assert!(ck.ok())); // 合計 <= 0、在庫 < v など
    with_check(&mutate(&code(), SUB, "  let total = subtotal + 100\n"), false, |ck| assert!(ck.ok(), "{:?}", ck.diags));
}

#[test]
fn 変換の向きを取り違えると止まる() {
    assert!(any(&errs(&mutate(&code(), SUB, "  let total = money(subtotal)\n")), "money( ) で変えられるのは整数だけ"));
}

#[test]
fn 型の変換を使っても_rustがビルドできる() {
    let src = mutate(&code(), SUB, "  let total_count = sum { v | (k, v) in items }\n  let total = subtotal + money(total_count) + round(subtotal * tax_rate, floor, JPY)\n");
    with_check(&src, false, |ck| assert!(ck.ok(), "{:?}", ck.diags));
    build_only(&src, "fn main() {}\n");
}

// ---- すべては原則「無いかもしれない」。必須の約束は required ----
const NOTE: &str = r####"ltype 重さ値 = int 0..

entity メモ {
  メモID: id unique
  本文: text required
  題: text
  重さ: 重さ値
}
result 書いた結果 {
  書いた(メモID: id)
}
result 題の結果 {
  題あり(題: text)
  無し
  題なし
}
deed 書く {
  input 本文: text required
  writes メモ
  result 書いた結果
  effects none

  let メモ = create メモ { 本文: 本文 }
  return 書いた(メモ.メモID)
}
deed 題を見る {
  input メモID: id required
  reads メモ.題
  result 題の結果
  effects none

  let メモ = メモ[メモID] else { return 無し }
  let 題 = メモ.題 else { return 題なし }
  return 題あり(題)
}
nexus 流れ {
  書く: 書いた → 題を見る
  題を見る: 題あり → reply, 無し → reply, 題なし → reply
}
"####;

#[test]
fn データの項目にoptionalは書かない() {
    let e = parse_err(&NOTE.replace("  題: text\n", "  題: text optional\n")).unwrap();
    assert!(e.contains("required"), "{}", e);
}

#[test]
fn 無いかもしれない項目は確かめてから使う() {
    with_check(NOTE, false, |ck| assert!(ck.ok(), "{:?}", ck.diags));
    let a = NOTE.replace("  let 題 = メモ.題 else { return 題なし }\n  return 題あり(題)", "  return 題あり(メモ.題)");
    assert!(any(&errs(&a), "null の"));
    let b = NOTE.replace("  let 題 = メモ.題 else { return 題なし }\n  return 題あり(題)", "  if メモ.題 is null { return 題なし }\n  return 題あり(メモ.題)");
    with_check(&b, false, |ck| assert!(ck.ok(), "{:?}", ck.diags));
}

#[test]
fn 必須の項目は新規作成で省けず_nullも入れられない() {
    assert!(any(&errs(&NOTE.replace("create メモ { 本文: 本文 }", "create メモ { 題: 本文 }")), "必須の項目「本文」"));
    assert!(any(&errs(&NOTE.replace("create メモ { 本文: 本文 }", "create メモ { 本文: null }")), "null は入れられません"));
    let c = NOTE.replace("if メモ.題 is null", "if メモ.本文 is null").replace("  let 題 = メモ.題 else { return 題なし }\n", "  if メモ.本文 is null { return 題なし }\n  let 題 = メモ.題 else { return 題なし }\n");
    assert!(any(&errs(&c), "is null"));
}
