// SPDX-License-Identifier: GPL-3.0-or-later
//! 小さなお店の「処理の中身」の検査の試験：if の中の状態の確認、else、問い合わせの条件と select、確認が効く範囲、書き込みごとの範囲の証明。
//! お店のコードを 1 か所ずつ壊して、検査が止める（または通す）ことと、生成した Rust の形を確かめる。
//! 元は Python 版の tests/test_geas.py の中ほど（タグ python-final）。
mod common;
use common::*;
use geas_core::ast::{If, Stmt};
use geas_core::check::{ERROR, QUESTION};
use geas_core::crategen::write_crate;
use geas_core::rustgen::generate;
use geas_core::spec::build;
use serde_json::json;
use std::process::Command;

fn code() -> String {
    std::fs::read_to_string(root().join("tests/fixtures/shop/shop.geas")).unwrap()
}

fn lv(src: &str, level: &str) -> Vec<String> {
    with_check(src, false, |ck| ck.diags.iter().filter(|d| d.level == level).map(|d| d.msg.clone()).collect())
}

fn any(v: &[String], needle: &str) -> bool {
    v.iter().any(|m| m.contains(needle))
}

/// 「遷移」の誤りで、元の状態として「受付」が出ているか
fn transition_err(src: &str) -> bool {
    with_check(src, false, |ck| ck.diags.iter().any(|d| d.level == ERROR && d.code.contains("遷移") && d.msg.contains("受付")))
}

fn ok(src: &str) -> bool {
    with_check(src, false, |ck| {
        assert!(ck.ok(), "{:?}", ck.diags.iter().map(|d| d.msg.clone()).collect::<Vec<_>>());
        true
    })
}

const GUARD: &str = "  if order.status != Received { return WrongStatus }\n  if order.total <= 0 { return ZeroTotal }\n  order.status = PaymentRequested\n";
const COND: &str = "  if order.total <= 0 { return ZeroTotal }\n  if order.status == Received {\n    order.status = PaymentRequested\n  }\n";
const TAIL: &str = "  request Payment.charge(order_id, order.total, idempotency_key = order_id)\n  return Requested\n";

// ---- 条件つきの状態変更 ----
#[test]
fn ifの中で元の状態を確かめてから変えるのは通る() {
    with_check(&mutate(&code(), GUARD, COND), false, |ck| assert!(!ck.diags.iter().any(|d| d.level == ERROR && d.code == "遷移")));
}

#[test]
fn ifの中でも元の状態を確かめていないと止まる() {
    assert!(transition_err(&mutate(&code(), GUARD, "  if order.total > 0 {\n    order.status = PaymentRequested\n  }\n")));
}

#[test]
fn ifの中の確認は外へ持ち出さない() {
    assert!(transition_err(&mutate(&code(), GUARD, "  if order.status == Received {\n    request Payment.charge(order_id, order.total, idempotency_key = order_id)\n  }\n  order.status = PaymentRequested\n")));
}

#[test]
fn ifの中にletを書ける() {
    // 2026-11-16 から：枝の中の let は書ける（名前は枝を抜けたら消える。試験は reliability.rs）
    let src = mutate(&code(), GUARD, "  if order.status == Received {\n    let x = Order[order_id] else { return NoSuchOrder }\n  }\n");
    assert!(!any(&lv(&src, ERROR), "if の中"));
}

#[test]
fn 条件つきの変更はrustのifの中に出る() {
    let src = with_check(&mutate(&code(), GUARD, COND), false, |ck| generate(ck));
    assert!(src.contains("    if (order.status == OrderStatus::Received) {\n        writes.push(Write::Order_status(order.order_id.clone(), OrderStatus::PaymentRequested));\n    }"));
}

#[test]
fn 書いた後に読む項目があるときだけ_変数に写しを持つ() {
    let gen = |body: &str| with_check(&mutate(&code(), &format!("{}{}", GUARD, TAIL), body), false, |ck| generate(ck));
    // 書いた項目を後で読まない。写さず、保存の行への参照のまま（実行時の手間を足さない）
    assert!(gen(&format!("{}{}", GUARD, TAIL)).contains("    let order = match Store.Order.get("));
    // 書いた項目を後で読むと、写しを持つ
    assert!(gen(&format!("{}  if order.status == PaymentRequested {{ return Requested }}\n{}", GUARD, TAIL)).contains("    let mut order = match Store.Order.get("));
    assert!(gen(&format!("{}  if order == order {{ return Requested }}\n{}", GUARD, TAIL)).contains("    let mut order = match Store.Order.get(")); // 1件をまるごと読む
    // return で終わる道で書いたものは、後ろの文に届かない
    let early = "  if order.total > 1000 {\n    order.status = PaymentRequested\n    return Requested\n  }\n  if order.status == Received { return WrongStatus }\n";
    let src = mutate(&code(), &format!("{}{}", GUARD, TAIL), &format!("{}{}", early, TAIL));
    assert!(with_check(&src, false, |ck| generate(ck)).contains("    let order = match Store.Order.get("));
}

// ---- 文の if の else・else if ----
const ELSE: &str = "  if order.status != Received {\n    return WrongStatus\n  } else if order.total <= 0 {\n    return ZeroTotal\n  } else {\n    order.status = PaymentRequested\n    request Payment.charge(order_id, order.total, idempotency_key = order_id)\n  }\n  return Requested\n";

#[test]
fn elseの中では_条件が成り立たないことを使える() {
    let src = mutate(&code(), &format!("{}{}", GUARD, TAIL), ELSE);
    with_check(&src, false, |ck| assert!(!ck.diags.iter().any(|d| d.level == ERROR), "{:?}", ck.diags));
    let prog = geas_core::parser::parse(&src, None).unwrap();
    let st = &prog.processes.iter().find(|x| x.name == "request_payment").unwrap().body[1];
    let Stmt::If(If { els: Some(els), .. }) = st else { panic!("if ではありません") };
    // else if は、入れ子の if 1つ
    assert!(matches!(&els[0], Stmt::If(i) if i.els.is_some()));
    let rs = with_check(&src, false, |ck| generate(ck));
    assert!(rs.contains("    } else if ((order.total as i128) <= 0i128) {\n") && rs.contains("    } else {\n        writes.push(Write::Order_status("));
    // 成り立つ側では、成り立たない側の確認は使えない
    let bad = mutate(&code(), &format!("{}{}", GUARD, TAIL), &ELSE.replace("order.status != Received", "order.status == Received"));
    with_check(&bad, false, |ck| assert!(ck.diags.iter().any(|d| d.level == ERROR && d.code.contains("遷移"))));
}

#[test]
fn ifとelseで同じ項目に書き分けられ_同じ道で2回書くと止まる() {
    let both = "  if order.status != Received { return WrongStatus }\n  if order.total <= 0 { return ZeroTotal }\n  if order.total > 1000 {\n    order.status = PaymentRequested\n  } else {\n    order.status = PaymentRequested\n  }\n  return Requested\n";
    let src = mutate(&mutate(&code(), &format!("{}{}", GUARD, TAIL), both), "  effects Payment.charge\n\n  let order = Order", "  effects none\n\n  let order = Order");
    assert!(ok(&src));
    let twice = mutate(&src, "  } else {\n    order.status = PaymentRequested\n  }\n", "  }\n  order.status = PaymentRequested\n");
    let lines: Vec<&str> = twice.split('\n').collect();
    let start = lines.iter().position(|l| *l == "  if order.total > 1000 {").unwrap() + 3;
    let line = (start + lines[start..].iter().position(|l| *l == "  order.status = PaymentRequested").unwrap() + 1) as i64;
    with_check(&twice, false, |ck| {
        let got: Vec<(String, i64)> = ck.diags.iter().filter(|d| d.code == "書込").map(|d| (d.msg.clone(), d.line)).collect();
        assert_eq!(got, vec![("1つの処理で「注文.状態」に2回書いています（要素が重なると結果が順序で変わります。if と else のように、同時には通らない道で書き分けるのはかまいません）".to_string(), line)]);
    });
    // 先に return する道で書いて、残りの道で書くのも、同じ道ではない
    let early = mutate(&src, "  if order.total > 1000 {\n    order.status = PaymentRequested\n  } else {\n    order.status = PaymentRequested\n  }\n", "  if order.total > 1000 {\n    order.status = PaymentRequested\n    return Requested\n  }\n  order.status = PaymentRequested\n");
    assert!(ok(&early));
}

#[test]
fn ifとelseの両方でreturnすると_その後ろには届かない() {
    let ends = mutate(&code(), &format!("{}{}", GUARD, TAIL), "  if order.status != Received { return WrongStatus }\n  if order.total <= 0 {\n    return ZeroTotal\n  } else {\n    order.status = PaymentRequested\n    request Payment.charge(order_id, order.total, idempotency_key = order_id)\n    return Requested\n  }\n");
    assert!(ok(&ends)); // 処理の最後が、両方 return する if でもよい
    let dead = mutate(&ends, "    return Requested\n  }\n", "    return Requested\n  }\n  return Requested\n");
    let line = dead.split('\n').position(|l| l == "  return Requested").unwrap() as i64 + 1;
    with_check(&dead, false, |ck| assert_eq!(ck.diags.iter().filter(|d| d.msg.contains("届きません")).map(|d| d.line).collect::<Vec<_>>(), vec![line]));
    let after_return = mutate(&code(), &format!("{}{}", GUARD, TAIL), &format!("{}  return Requested\n{}", GUARD, TAIL));
    assert!(any(&lv(&after_return, ERROR), "届きません"));
    let open_end = mutate(&ends, "    return Requested\n  }\n", "  }\n");
    assert!(any(&lv(&open_end, ERROR), "最後は return"));
}

#[test]
fn elseの中も書ける文は_ifの中と同じで_空のelseは止まる() {
    // 2026-11-16 から：枝の中にも let を書ける
    let let_in = mutate(&code(), &format!("{}{}", GUARD, TAIL), &format!("  if order.total <= 0 {{\n    return ZeroTotal\n  }} else {{\n    let x = Order[order_id] else {{ return NoSuchOrder }}\n  }}\n{}{}", GUARD, TAIL));
    assert!(!any(&lv(&let_in, ERROR), "else の中"));
    let empty = mutate(&code(), TAIL, "  if order.total > 0 {\n    request Payment.charge(order_id, order.total, idempotency_key = order_id)\n  } else {\n  }\n  return Requested\n");
    assert!(any(&lv(&empty, ERROR), "else の中が空です"));
}

#[test]
fn elseのある1行の確認は_変異検査で消さない() {
    let src = "  if a > 1 { return X }\n  if a > 2 { return X } else { return Y }\n";
    let lines: Vec<usize> = geas_core::mutate::mutants_of(src, None).into_iter().filter(|m| m.kind == "確認を消す").map(|m| m.line).collect();
    assert_eq!(lines, vec![1]);
}

// ---- 問い合わせの条件（または・数との比較） ----
const QUERY: &str = r####"ltype 重さ値 = int 0..

entity メモ {
  メモID: id unique
  状態: メモ状態
  重さ: 重さ値
}
state メモ状態 {
  initial 下書き
  middle 公開
  final 削除
}
query 探す {
  from メモ
  where 状態 == 公開 or 状態 == 下書き
  where 重さ >= 10
  reads メモ.状態, メモ.重さ
}
"####;

#[test]
fn 問い合わせの条件に_orと数の比較を書ける() {
    with_check(QUERY, false, |ck| {
        assert!(!ck.diags.iter().any(|d| d.level == ERROR), "{:?}", ck.diags);
        assert_eq!(build(ck)["問い合わせ"][0]["条件"], json!(["状態 == 公開 または 状態 == 下書き", "重さ >= 10"]));
    });
}

#[test]
fn 数との比較は数の項目にだけ使える() {
    assert!(any(&lv(&QUERY.replace("重さ >= 10", "状態 >= 10"), ERROR), "数との比較"));
}

#[test]
fn 問い合わせの条件は_rustで_orになる() {
    let src = with_check(QUERY, false, |ck| generate(ck));
    assert!(src.contains("if !(r.状態 == メモ状態::公開 || r.状態 == メモ状態::下書き) { return false; }"));
    // 重さは required でないので、null は満たさない
    assert!(src.contains("if !(r.重さ.map_or(false, |v| (v as i128) >= 10)) { return false; }"));
}

// ---- 問い合わせの select（結合と集計）----
#[test]
fn selectで参照先の項目と集計を返せる() {
    with_check(&code(), false, |ck| {
        let spec = build(ck);
        let row = spec["問い合わせ"].as_array().unwrap().iter().find(|q| q["問い合わせ"] == "注文一覧").unwrap().clone();
        assert_eq!(row["列"], json!(["注文ID", "顧客", "点数"]));
    });
}

#[test]
fn selectの参照先が参照でないと止まる() {
    assert!(any(&lv(&mutate(&code(), "customer.customer_id as customer", "total.customer_id as customer"), ERROR), "参照（ref）ではありません"));
}

#[test]
fn sumはキーつきの集まりにだけ使える() {
    assert!(any(&lv(&mutate(&code(), "sum(items) as /// 点数\n    item_count", "sum(total) as /// 点数\n    item_count"), ERROR), "集計できません"));
}

#[test]
fn selectに書いた参照先は読むの宣言に要る() {
    assert!(any(&lv(&mutate(&code(), "Order.customer, Customer.customer_id, Order.items\n}", "Order.customer, Order.items\n}"), ERROR), "顧客.顧客ID"));
}

#[test]
fn selectのある問い合わせは_eachに使えない() {
    let src = mutate(&code(), "invariant Product: stock >= 0", "event 開いた {\n  each order_list → request_payment\n}\n\ninvariant Product: stock >= 0");
    assert!(any(&lv(&src, ERROR), "select"));
}

// ---- 文字列検索の同一視（contains の軸）----
const MEMO: &str = r####"entity メモ {
  メモID: id unique
  本文: text
}
query 探す {
  input 単語: text
  from メモ
  filter 本文 contains 単語 ignore_case ignore_kana ignore_width
  reads メモ.本文
}
"####;

#[test]
fn containsの同一視する軸は書いたものだけ() {
    with_check(MEMO, false, |ck| {
        assert!(!ck.diags.iter().any(|d| d.level == ERROR));
        assert_eq!(build(ck)["問い合わせ"][0]["条件"], json!(["本文 含む(大小無視,かな無視,幅無視) 単語（単語が無ければ絞らない）"]));
    });
    let plain = with_check(&MEMO.replace(" ignore_case ignore_kana ignore_width", ""), false, |ck| build(ck)["問い合わせ"][0]["条件"].clone());
    assert_eq!(plain, json!(["本文 含む 単語（単語が無ければ絞らない）"]));
    let req = with_check(&MEMO.replace("input 単語: text", "input 単語: text required"), false, |ck| build(ck)["問い合わせ"][0]["条件"].clone());
    assert_eq!(req, json!(["本文 含む(大小無視,かな無視,幅無視) 単語"])); // 必須の入力なら、無いときの動きは無い
}

#[test]
fn 生成した照合は大小_かな_幅をそろえる() {
    if Command::new("rustc").arg("--version").output().is_err() {
        return;
    }
    let t = Tmp::new("fold");
    let src = with_check(MEMO, false, |ck| generate(ck));
    let start = src.find("pub fn fold_width_kana").unwrap(); // そろえの 1 文字分（証明の対象）と fold_text
    let end = src.find("pub fn show_set").unwrap();
    let main = format!("{}\nfn main() {{\n    assert_eq!(fold_text(\"ＡＢＣ　abc\", true, false, true), \"abc　abc\");\n    assert_eq!(fold_text(\"カタカナ\", false, true, false), \"かたかな\");\n    assert_eq!(fold_text(\"Ａｂ\", false, false, false), \"Ａｂ\");\n}}\n", &src[start..end]);
    let f = t.0.join("t.rs");
    std::fs::write(&f, main).unwrap();
    let r = Command::new("rustc").args(["--edition", "2021", "-o"]).arg(t.0.join("t")).arg(&f).output().unwrap();
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 1500));
    assert!(Command::new(t.0.join("t")).status().unwrap().success());
}

fn build_ok(src: &str, tag: &str, host: bool) {
    let t = Tmp::new(tag);
    with_check(src, false, |ck| write_crate(ck, &generate(ck), "fn main() {}\n", &t.0, host).unwrap());
    let r = Command::new("cargo").args(["build", "--quiet", "--offline"]).env("CARGO_TARGET_DIR", common::serve::target_dir()).current_dir(&t.0).output().unwrap();
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 1500));
}

#[test]
fn 外部作用の無いアプリでも生成したcrateがビルドできる() {
    build_ok(MEMO, "memobuild", true);
}

// ---- 確認は「どのキーで引いたものか」に効く（データ名には効かない）----
const PAY: &str = "  let order = Order[order_id] else { return NoSuchOrder }\n  if order.status != Received { return WrongStatus }\n";
const TWO: &str = "deed request_payment {\n  input order_id: id required, 別ID: id required\n";

fn two_orders(body: &str) -> String {
    mutate(&mutate(&code(), "deed request_payment {\n  input order_id: id required\n", TWO), PAY, body)
}

#[test]
fn 別の注文の状態を確かめても_こちらは遷移できない() {
    assert!(transition_err(&two_orders("  let 別 = Order[別ID] else { return NoSuchOrder }\n  let order = Order[order_id] else { return NoSuchOrder }\n  if 別.status != Received { return WrongStatus }\n")));
}

#[test]
fn 同じ名前で引き直すと確認は消える() {
    assert!(transition_err(&two_orders(&format!("{}  let order = Order[別ID] else {{ return NoSuchOrder }}\n", PAY))));
}

#[test]
fn 存在の確認は同じキーにだけ効く() {
    let q = |s: &str| lv(&two_orders(s), QUESTION);
    assert!(any(&q(&format!("{}  if Order[別ID].total <= 0 {{ return ZeroTotal }}\n", PAY)), "注文[別ID]"));
    assert!(!any(&q(&format!("{}  if Order[別ID] is absent {{ return NoSuchOrder }}\n  if Order[別ID].total <= 0 {{ return ZeroTotal }}\n", PAY)), "注文[別ID]"));
}

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

#[test]
fn updateで状態の項目は変えられない() {
    assert!(any(&lv(&format!("{}{}", code(), BULK), ERROR), "update で状態の項目"));
}

#[test]
fn 状態は状態名で変える() {
    let src = two_orders(&format!("{}  let 別 = Order[別ID] else {{ return NoSuchOrder }}\n", PAY)).replacen("  order.status = PaymentRequested\n", "  order.status = 別.status\n", 1);
    assert!(any(&lv(&src, ERROR), "状態名で書いてください"));
}

#[test]
fn 新規作成の状態は状態名で書く() {
    assert!(any(&lv(&mutate(&code(), "status: Received }", "status: customer_id }"), ERROR), "状態名で書いてください"));
}

#[test]
fn 空のifはエラーで止まる() {
    assert!(any(&lv(&mutate(&code(), "  if order.total <= 0 { return ZeroTotal }\n", "  if order.total <= 0 { }\n  if order.total <= 0 { return ZeroTotal }\n"), ERROR), "if の中が空"));
}

// ---- 不変条件と項目の範囲は、書き込むその場で、値の範囲から示す ----
fn reflect(src: &str) -> String {
    mutate(src, "  writes Order.status\n  transition Order: PaymentRequested → Completed", "  writes Order.status, Order.total\n  transition Order: PaymentRequested → Completed")
}

#[test]
fn 項目の更新で在庫を減らすと範囲も不変条件も示せない() {
    let src = mutate(&mutate(&code(), "  reads Order.total, Order.status\n  writes Order.status\n", "  reads Order.total, Order.status, Product.stock\n  writes Order.status, Product.stock\n"),
        "  order.status = PaymentRequested\n",
        "  order.status = PaymentRequested\n  let 品 = Order[order_id] else { return NoSuchOrder }\n  let 商 = Product[order_id] else { return NoSuchOrder }\n  商.stock = 商.stock - 1\n");
    assert!(lv(&src, ERROR).iter().any(|m| m.contains("反例") && m.contains("商品.在庫")));
}

#[test]
fn 同じ減算を2回すると不足の確認は効かない() {
    let up = "  update { Product[k].stock -= v | (k, v) in items }\n";
    assert!(any(&lv(&mutate(&code(), up, &format!("{}{}", up, up)), ERROR), "反例"));
}

#[test]
fn 状態がsでないと分かっていれば値を下げてよい() {
    assert!(ok(&mutate(&reflect(&code()), "  order.status = Completed\n", "  order.status = Completed\n  order.total = 0\n")));
    assert!(any(&lv(&mutate(&reflect(&code()), "  order.status = Completed\n", "  order.total = 0\n  order.status = Completed\n"), ERROR), "合計 > 0"));
}

#[test]
fn sで新規作成するなら値の確認が要る() {
    let src = mutate(&mutate(&code(), "transition Order: (new) → Received", "transition Order: (new) → PaymentRequested"), "status: Received }", "status: PaymentRequested }");
    assert!(any(&lv(&src, ERROR), "合計 > 0"));
}

#[test]
fn 分岐の中の書き込みで確認は崩れる() {
    let src = mutate(&mutate(&code(), "  reads Order.total, Order.status\n  writes Order.status\n", "  reads Order.total, Order.status\n  writes Order.status, Order.total\n"),
        "  order.status = PaymentRequested\n", "  if order.total > 100 {\n    order.total = 0\n  }\n  order.status = PaymentRequested\n");
    assert!(any(&lv(&src, ERROR), "合計 > 0"));
}

#[test]
fn 遷移した後は元の状態の確認が消える() {
    let src = mutate(&mutate(&code(), "transition Order: Received → PaymentRequested", "transition Order: Received → PaymentRequested\n  transition Order: Received → Canceled"),
        "  order.status = PaymentRequested\n", "  order.status = PaymentRequested\n  order.status = Canceled\n");
    assert!(transition_err(&src));
}

#[test]
fn 同じデータの別の束縛の確認も遷移で消える() {
    let src = mutate(&two_orders(&format!("{}  let 別 = Order[別ID] else {{ return NoSuchOrder }}\n  if 別.status != Received {{ return WrongStatus }}\n", PAY)),
        "  order.status = PaymentRequested\n", "  order.status = PaymentRequested\n  別.status = PaymentRequested\n");
    assert!(transition_err(&src));
}

#[test]
fn 項目と定数を比べる不変条件は_減らすだけの処理なら示せる() {
    // 2026-11-16 から：同じ行の 2 項目（か項目と定数）の比較を、書き込みのたびに示す。前は「まだ検査できません」で止めていた形
    with_check(&format!("{}\ninvariant Product: stock <= 100\n", code()), false, |ck| {
        assert!(!ck.diags.iter().any(|d| d.level == ERROR), "{:?}", ck.diags);
        let rows = &ck.invariant_status.iter().find(|(k, _)| k == "Product: stock <= 100").unwrap().1;
        assert_eq!(rows[0], ("place_order".to_string(), "証明済み（書く前に確かめている）".to_string()));
    });
}

#[test]
fn 項目の更新は型を確かめる() {
    assert!(any(&lv(&mutate(&reflect(&code()), "  order.status = Completed\n", "  order.status = Completed\n  order.total = order_id\n"), ERROR), "型の違う値"));
}

#[test]
fn 項目へ数を書くと物理数へ変換される() {
    build_ok(&mutate(&reflect(&code()), "  order.status = Completed\n", "  order.status = Completed\n  order.total = 0\n"), "totalbuild", false);
}

#[test]
fn 減らした後に確かめた不足の集まりは古い() {
    let chk_line = "  if short is nonempty { return OutOfStock(short) }\n";
    let up = "  update { Product[k].stock -= v | (k, v) in items }\n";
    let src = mutate(&code(), up, &format!("{}{}{}", up, chk_line, up));
    assert!(ok(&code()));
    assert!(any(&lv(&src, ERROR), "反例"));
}
