// SPDX-License-Identifier: GPL-3.0-or-later
//! 処理の中身（どの条件でどの結果になるか、何をいくつ書くか）が、人が読む仕様書と変更点に出ること。
//! 出ていないと、コードの決まりを変えても仕様書が 1 文字も変わらず、コードを読まないレビューで見つけられない。
//! 元は Python 版の tests/test_spec_steps.py（タグ python-final）。
mod common;
use common::*;
use geas_core::ast::{show, Expr};
use geas_core::check::defs::check;
use geas_core::parser::parse;
use geas_core::review::{changes_section, review};
use geas_core::spec::{build, render, Spec};
use serde_json::json;
use std::path::PathBuf;

fn src_path() -> PathBuf {
    root().join("tests").join("fixtures").join("overseas_shop").join("overseas_shop.geas")
}

fn base() -> String {
    fixture("overseas_shop/overseas_shop.geas")
}

/// (仕様, 仕様書の Markdown, 画面の HTML)
fn views(text: &str) -> (Spec, String, String) {
    let mut prog = parse(text, Some(&src_path())).unwrap_or_else(|e| panic!("{}", e));
    let ck = check(&mut prog, true);
    assert!(ck.ok(), "{:?}", ck.diags);
    let spec = build(&ck);
    let md = render(&ck, &spec, "");
    let html = review(&ck, None, &[], "", false).unwrap();
    (spec, md, html)
}

fn changed(old: &str, new: &str) -> (Spec, Spec, String, String, String) {
    let b = base();
    assert!(b.contains(old), "{}", old);
    let (s0, md0, _) = views(&b);
    let (s1, md1, html1) = views(&b.replacen(old, new, 1));
    (s0, s1, md0, md1, html1)
}

#[test]
fn 処理の中身が仕様書に出る() {
    let (_, md, html) = views(&base());
    for text in [&md, &html] {
        assert!(text.contains("顧客.ポイント に足す： 10")); // HTML では図の中の文字
        assert!(text.contains("顧客.ポイント に足す： 5"));
    }
    assert!(md.contains("order.状態 ≠ 決済要求済 なら 結果 状態違い"));
    assert!(html.contains("order.状態 ≠ 決済要求済") && html.contains("処理の中身 ポイントの加算")); // HTML はフローチャート（分かれ道と結果は別の箱）
}

#[test]
fn 比較の向きを変えると仕様書と変更点に出る() {
    let (s0, s1, md0, md1, html1) = changed("Product[k].stock < v }", "Product[k].stock <= v }");
    assert!(md0 != md1 && html1.contains("商品[k].在庫 ≦ v"));
    assert_ne!(changes_section(&s1, &s0, "前"), changes_section(&s0, &s0, "前"));
}

#[test]
fn 足す数を変えると仕様書と変更点に出る() {
    let (s0, s1, md0, md1, _) = changed("customer.points += 10", "customer.points += 9");
    assert_ne!(md0, md1);
    assert!(changes_section(&s1, &s0, "前").contains("に足す： 9"));
}

#[test]
fn 条件を逆にすると仕様書と変更点に出る() {
    let (s0, s1, md0, md1, _) = changed("if order.status != PaymentRequested { return WrongStatus }", "if order.status == PaymentRequested { return WrongStatus }");
    assert_ne!(md0, md1);
    assert_ne!(changes_section(&s1, &s0, "前"), changes_section(&s0, &s0, "前"));
}

#[test]
fn 仕様書に出ていなかった宣言が出る() {
    let (_, md, html) = views(&base());
    for text in [&md, &html] {
        assert!(text.contains("海外にも売るお店")); // アプリの名前
        assert!(text.contains("（最大 100 件）")); // 集まりの要素数の上限
        assert!(text.contains("B0SHOPTEA1→4901234567894")); // 拡張の対応表の中身
        assert!(text.contains("[A-Z0-9]{10}")); // 拡張が要素を読むときの形
        assert!(text.contains("送料: 金額(通貨:通貨) 0..100000")); // slot に使う型の範囲
    }
}

#[test]
fn 返事から値を取り出す場所が出る() {
    let p = root().join("tests").join("fixtures").join("solo_tweet").join("solo_tweet.geas");
    let text = std::fs::read_to_string(&p).unwrap();
    let mut prog = parse(&text, Some(&p)).unwrap();
    let ck = check(&mut prog, true);
    let spec = build(&ck);
    assert!(render(&ck, &spec, "").contains("data.id") && review(&ck, None, &[], "", false).unwrap().contains("data.id"));
}

#[test]
fn エラーのあるコードでも仕様書は落ちずに出る() {
    for drop in ["ltype Quantity = int 1..99", "  restock_product: Restocked → reply, NoSuchProduct → reply, StockFull → reply"] {
        let b = base();
        assert!(b.contains(drop), "{}", drop);
        let mut prog = parse(&b.replacen(drop, "", 1), Some(&src_path())).unwrap();
        let ck = check(&mut prog, true);
        assert!(!ck.ok());
        let spec = build(&ck);
        assert!(!render(&ck, &spec, "").is_empty() && !review(&ck, None, &[], "", false).unwrap().is_empty());
    }
}

#[test]
fn 式は読める言葉で出て括弧は落ちない() {
    let (_, md, _) = views(&base());
    assert!(md.contains("合計 = subtotal + 丸め（subtotal * 税率、通貨 の単位に切り捨て） + 送料"));
    assert!(md.contains("subtotal = 合計〔明細 の各 (k, v)について： 商品[k].価格 * v〕"));
    assert!(md.contains("不足 = 集まり〔明細 の各 (k, v) のうち 商品[k].在庫 < v のものについて： k〕"));
    // 括弧の位置で意味が変わる式は、表示も変わる
    let (a, b, c) = (|| Expr::name("a"), || Expr::name("b"), || Expr::name("c"));
    assert_eq!(show(&Expr::bin("*", Expr::bin("+", a(), b()), c())), "(a + b) * c");
    assert_eq!(show(&Expr::bin("+", a(), Expr::bin("*", b(), c()))), "a + b * c");
    assert_eq!(show(&Expr::bin("-", a(), Expr::bin("-", b(), c()))), "a - (b - c)");
    assert_eq!(show(&Expr::bin("-", Expr::bin("-", a(), b()), c())), "a - b - c");
    assert_eq!(show(&Expr::Not { e: Box::new(Expr::bin("かつ", a(), b())) }), "not (a かつ b)");
}

fn spec_of(v: serde_json::Value) -> Spec {
    v.as_object().unwrap().clone()
}

#[test]
fn 並びだけの変更と同じ行の追加も変更点に出る() {
    let r1 = json!(["送料", "通貨 = JPY", "500"]);
    let r2 = json!(["送料", "上のどれでもない", "1500"]);
    let a = spec_of(json!({"決め方": [r1, r2]}));
    let b = spec_of(json!({"決め方": [r2, r1]}));
    let c = spec_of(json!({"決め方": [r1, r2, r1]}));
    let same = changes_section(&a, &a, "前の版");
    assert!(changes_section(&b, &a, "前の版").contains("並びが変わりました"));
    assert!(changes_section(&c, &a, "前の版") != same && changes_section(&c, &a, "前の版").contains("＋"));
}

#[test]
fn 同じ処理の行が流れに2つあると検査で止まる() {
    let b = base();
    let line = b.split('\n').find(|l| l.trim_start().starts_with("place_order:")).unwrap().to_string();
    let mut prog = parse(&b.replacen(&line, &format!("{}\n{}", line, line), 1), Some(&src_path())).unwrap();
    let ck = check(&mut prog, true);
    assert!(!ck.ok() && ck.diags.iter().any(|d| d.msg.contains("同じ流れに 2 つあります")));
}

#[test]
fn 同じ宣言を2回書くと検査で止まる() {
    // 同じ行を 2 回書くと、2 回起こるもの（出来事の each）と、届かない行になるもの（rule）がある。黙って通さない
    let cases = [("solo_tweet", r"^\s*each saved_tweets", "同じ処理が 2 回起こります"), ("solo_tweet", r"^\s*open TweetDetail", "が 2 つあります"), ("overseas_shop", r"^\s*currency == JPY: 2500", "同じ条件の行が 2 つあります")];
    for (fx, pat, word) in cases {
        let p = root().join("tests").join("fixtures").join(fx).join(format!("{}.geas", fx));
        let text = std::fs::read_to_string(&p).unwrap();
        let re = regex::Regex::new(pat).unwrap();
        let line = text.split('\n').find(|l| re.is_match(l)).unwrap().to_string();
        let mut prog = parse(&text.replacen(&line, &format!("{}\n{}", line, line), 1), Some(&p)).unwrap();
        let ck = check(&mut prog, true);
        assert!(!ck.ok() && ck.diags.iter().any(|d| d.msg.contains(word)), "{} {} {:?}", fx, pat, ck.diags.iter().map(|d| d.msg.clone()).collect::<Vec<_>>());
    }
}
