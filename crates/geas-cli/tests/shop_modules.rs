// SPDX-License-Identifier: GPL-3.0-or-later
//! モジュール（取り込み・循環・version・ハッシュ）、応答の中身、金額の単位、合成型を使える場所、海外店の例、通貨コードの slot の試験。
//! 元は Python 版の tests/test_geas.py の終わりのほう（タグ python-final）。
mod common;
use common::*;
use geas_core::ast::route_map;
use geas_core::check::{ERROR, WARN};
use geas_core::crategen::write_crate;
use geas_core::rustgen::generate;
use geas_core::spec::build;
use std::path::Path;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_geas");

fn code() -> String {
    std::fs::read_to_string(root().join("tests/fixtures/shop/shop.geas")).unwrap()
}

fn overseas_path() -> std::path::PathBuf {
    root().join("tests/fixtures/overseas_shop/overseas_shop.geas")
}

fn overseas() -> String {
    std::fs::read_to_string(overseas_path()).unwrap()
}

fn lv(src: &str, level: &str) -> Vec<String> {
    with_check(src, false, |ck| ck.diags.iter().filter(|d| d.level == level).map(|d| d.msg.clone()).collect())
}

fn errs(src: &str) -> Vec<String> {
    lv(src, ERROR)
}

/// 海外店は標準モジュール Gtin を use するので、そのファイルの隣として読む
fn lv_ov(src: &str, level: &str) -> Vec<String> {
    with_check_at(src, Some(&overseas_path()), false, |ck| ck.diags.iter().filter(|d| d.level == level).map(|d| d.msg.clone()).collect())
}

fn any(v: &[String], needle: &str) -> bool {
    v.iter().any(|m| m.contains(needle))
}

fn pe(src: &str) -> String {
    parse_err(src).unwrap_or_else(|| panic!("構文の誤りになるはず: {}", src))
}

const GEO: &str = r####"module Geo

/// 位置の数
ltype Dots = int 0..100
/// 位置
ltype Pos { x: Dots, y: Dots optional }

/// そのまま
convert same(d: Dots) -> Dots = d
"####;
const GEO_APP: &str = r####"use "geo.geas"

ltype Table = id → Geo.Pos
result Shown {
  Big
  Small
}
deed show {
  input table: Table required, limit: Geo.Dots required
  result Shown
  effects none

  let n = sum { v.x | (k, v) in table }
  if n > Geo.same(limit) { return Big }
  return Small
}
nexus Flow {
  show: Big → reply, Small → reply
}
"####;

fn mod_dir(dir: &Path, files: &[(&str, &str)]) {
    for (n, text) in files {
        std::fs::write(dir.join(format!("{}.geas", n)), text).unwrap();
    }
}

fn parse_at(dir: &Path, name: &str) -> Result<geas_core::ast::Program, String> {
    let p = dir.join(format!("{}.geas", name));
    let src = std::fs::read_to_string(&p).unwrap();
    geas_core::parser::parse(&src, Some(&p)).map_err(|e| format!("{}", e))
}

fn parse_at_err(dir: &Path, name: &str) -> String {
    parse_at(dir, name).err().expect("取り込みの誤りになるはず")
}

fn load_strict<R>(dir: &Path, f: impl FnOnce(&geas_core::check::Checker) -> R) -> R {
    let p = dir.join("app.geas");
    with_check_at(&std::fs::read_to_string(&p).unwrap(), Some(&p), true, f)
}

#[test]
fn モジュールの取り込みの誤りは止まる_続き() {
    let t = Tmp::new("moderr2");
    mod_dir(&t.0, &[("geo", &GEO.replace("module Geo\n", "")), ("app", GEO_APP)]);
    assert!(parse_at_err(&t.0, "app").contains("module 名前"));
    mod_dir(&t.0, &[("app", &GEO_APP.replace("\"geo.geas\"", "\"nothing.geas\""))]);
    assert!(parse_at_err(&t.0, "app").contains("がありません"));
    assert!(pe(GEO_APP).contains("ファイルから読む"));
}

#[test]
fn モジュールが循環したり名前が重なったりすると止まる() {
    let t = Tmp::new("modcycle");
    mod_dir(&t.0, &[("a", "use \"b.geas\"\nmodule A\nltype One = int 0..\n"), ("b", "use \"a.geas\"\nmodule B\nltype Two = int 0..\n"), ("app", "use \"a.geas\"\nresult R {\n  Done\n}\n")]);
    assert!(parse_at_err(&t.0, "app").contains("循環"));
    mod_dir(&t.0, &[("a", "module Same\nltype One = int 0..\n"), ("b", "module Same\nltype Two = int 0..\n"), ("app", "use \"a.geas\"\nuse \"b.geas\"\nresult R {\n  Done\n}\n")]);
    assert!(parse_at_err(&t.0, "app").contains("2つのファイル"));
}

#[test]
fn モジュールがモジュールを取り込み_同じモジュールは1度だけ読む() {
    let t = Tmp::new("modnest");
    mod_dir(&t.0, &[
        ("base", "module Base\nltype Count = int 0..100\n"),
        ("mid", "use \"base.geas\"\nmodule Mid\nltype Few = int 0..5\n"),
        ("app", "use \"base.geas\"\nuse \"mid.geas\"\n\nentity Box {\n  box_id: id unique\n  few: Mid.Few required\n  many: Base.Count required\n}\n"),
    ]);
    load_strict(&t.0, |ck| {
        assert!(ck.ok(), "{:?}", ck.diags);
        assert_eq!(ck.p.ltype_names.iter().filter(|n| *n == "Base_Count").count(), 1);
    });
}

#[test]
fn モジュールの処理は入力から結果を返すだけなら書け_アプリのフローにつなげられる() {
    let t = Tmp::new("modproc");
    mod_dir(&t.0, &[
        ("judge", "module Judge\n\nltype Score = int 0..100\n/// 判定\nresult Verdict {\n  Pass\n  Fail\n}\n/// 合否\ndeed grade {\n  input score: Score required\n  result Verdict\n  effects none\n\n  if score >= 60 { return Pass }\n  return Fail\n}\n"),
        ("app", "use \"judge.geas\"\n\nnexus Flow {\n  Judge.grade: Pass → reply, Fail → reply\n}\n"),
    ]);
    load_strict(&t.0, |ck| {
        assert!(ck.ok(), "{:?}", ck.diags);
        assert!(ck.procs.contains_key("Judge_grade") && ck.p.show_names("Judge_grade") == "合否");
    });
}

#[test]
fn モジュールの処理がデータや外部作用に触れるとエラー() {
    let t = Tmp::new("modbad");
    mod_dir(&t.0, &[
        ("bad", "module Bad\n\nresult Done {\n  Ok\n}\ndeed touch {\n  input item_id: id required\n  result Done\n  effects none\n\n  let item = Item[item_id] else { return Ok }\n  return Ok\n}\n"),
        ("app", "use \"bad.geas\"\n\nltype Digit = int 0..9\nentity Item {\n  item_id: id unique\n  count: Digit\n}\n"),
    ]);
    load_strict(&t.0, |ck| assert!(!ck.ok() && ck.diags.iter().any(|d| d.msg.contains("モジュールの処理は") && d.msg.contains("データを読む")), "{:?}", ck.diags));
}

#[test]
fn 表示名の無い名前は警告で教える() {
    let t = Tmp::new("modlabel");
    mod_dir(&t.0, &[("geo", &GEO.replace("/// 位置\n", "")), ("app", GEO_APP)]);
    load_strict(&t.0, |ck| {
        let gaps: Vec<String> = ck.diags.iter().filter(|d| d.code == "表示名").map(|d| d.msg.clone()).collect();
        assert!(ck.ok() && gaps.iter().any(|m| m.contains("「Pos」に表示名")), "{:?}", gaps);
        assert!(!gaps.iter().any(|m| m.contains("「Dots」"))); // /// 位置の数 がある
    });
    // 表示名があれば警告しない。名前の書式を検査しない時（check の既定）も警告しない
    with_check(&code(), false, |ck| assert!(!ck.diags.iter().any(|d| d.code == "表示名")));
}

// ---- モジュールの version とハッシュの固定 ----
fn geo_v() -> String {
    GEO.replace("module Geo\n", "module Geo version \"1.2.0\"\n")
}

fn sha_of(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(std::fs::read(path).unwrap()).iter().map(|b| format!("{:02x}", b)).collect()
}

fn use_app(dir: &Path, pins: &str) -> Result<geas_core::ast::Program, String> {
    std::fs::write(dir.join("app.geas"), GEO_APP.replace("use \"geo.geas\"", &format!("use \"geo.geas\"{}", pins))).unwrap();
    parse_at(dir, "app")
}

#[test]
fn useで依存のversionとソースのハッシュを明示でき_合っていれば取り込める() {
    let t = Tmp::new("modver");
    mod_dir(&t.0, &[("geo", &geo_v())]);
    let p = use_app(&t.0, &format!(" version \"1.2.0\" sha256 \"{}\"", sha_of(&t.0.join("geo.geas")))).unwrap();
    assert!(p.ltype_names.iter().any(|n| n == "Geo_Pos"));
    assert!(use_app(&t.0, "").unwrap().ltype_names.iter().any(|n| n == "Geo_Pos")); // どちらも書かなくてよい
    assert!(use_app(&t.0, " version \"1.2.0\"").unwrap().ltype_names.iter().any(|n| n == "Geo_Pos"));
}

#[test]
fn versionが違う_ハッシュが違うと取り込めない() {
    let t = Tmp::new("modver2");
    mod_dir(&t.0, &[("geo", &geo_v())]);
    assert!(use_app(&t.0, " version \"2.0.0\"").unwrap_err().contains("version は「1.2.0」で、use に書いた「2.0.0」と違います"));
    assert!(use_app(&t.0, &format!(" sha256 \"{}\"", "0".repeat(64))).unwrap_err().contains("ハッシュが、固定した値と違います"));
    mod_dir(&t.0, &[("geo", GEO)]); // モジュールに version が無い
    assert!(use_app(&t.0, " version \"1.2.0\"").unwrap_err().contains("（無し）"));
    mod_dir(&t.0, &[("geo", &geo_v())]);
    assert!(use_app(&t.0, " version").unwrap_err().contains("version の後に"));
}

// ---- 応答の中身 ----
fn reply_src() -> String {
    code().replace("/// 待ち解除結果", "reply PaymentDeclined(reason: id required)\nreply PaymentSucceeded(auth_id: id required)\n\n/// 待ち解除結果")
}

#[test]
fn 応答は中身を宣言でき_行き先の処理の入力に足される() {
    let src = reply_src().replace("deed apply_payment_result {\n  input order_id: id required", "deed apply_payment_result {\n  input order_id: id required, auth_id: id required");
    with_check(&src, false, |ck| {
        assert!(ck.ready(), "{:?}", ck.diags);
        assert_eq!(ck.p.replies.iter().map(|r| r.name.clone()).collect::<Vec<_>>(), vec!["PaymentDeclined", "PaymentSucceeded"]);
        assert!(build(ck)["応答"].as_array().unwrap().contains(&serde_json::json!(["決済成功", "auth_id（必須）"])));
        assert!(generate(ck).contains("ReplyDef { name: \"PaymentSucceeded\", fields: &[(\"auth_id\", \"ID\", true)] }"));
    });
}

#[test]
fn 応答の中身が行き先の処理の入力と合わないと止まる() {
    assert!(any(&errs(&reply_src()), "応答の中身")); // apply_payment_result は auth_id を入力にもたない
}

#[test]
fn 待ち合わせでは全部の応答の中身が渡り_名前が重なると止まる() {
    let src = reply_src().replace("reply PaymentSucceeded → apply_payment_result", "all reply PaymentSucceeded, PaymentDeclined → apply_payment_result");
    let dup = src.replace("reply PaymentDeclined(reason: id required)", "reply PaymentDeclined(auth_id: id required)");
    assert!(any(&errs(&dup), "同じ名前"));
    let ok = src.replace("deed apply_payment_result {\n  input order_id: id required", "deed apply_payment_result {\n  input order_id: id required, auth_id: id required, reason: id required");
    with_check(&ok, false, |ck| assert!(ck.ready(), "{:?}", ck.diags));
}

#[test]
fn 使われない応答の宣言は警告() {
    assert!(any(&lv(&reply_src().replace("reply PaymentDeclined", "reply Unused"), WARN), "使われていません"));
}

// ---- 金額の単位 ----
fn usd_src() -> String {
    code().replace("  unit_price: NonNegMoney required\n", "  unit_price: NonNegMoney required\n  price_usd: money(USD) required\n  note_usd: money(USD)\n")
}

fn usd_deed(body: &str, reads: &str, writes: &str) -> String {
    let w = if writes.is_empty() { String::new() } else { format!("  writes {}\n", writes) };
    format!("{}\ndeed usd_probe {{\n  input product_id: id required\n  reads {}\n{}  result ReleaseResult\n  effects none\n\n  let p = Product[product_id] else {{ return NoSuchOrder }}\n{}\n  return Applied\n}}\n", usd_src(), reads, w, body)
}

fn usd(body: &str) -> String {
    usd_deed(body, "Product.unit_price, Product.price_usd", "")
}

fn usd_w(body: &str, writes: &str) -> String {
    usd_deed(body, "Product.unit_price, Product.price_usd", writes)
}

fn unit_errors(src: &str) -> Vec<String> {
    errs(src).into_iter().filter(|m| (m.contains("金額") && m.contains("単位")) || m.contains("換算")).collect()
}

#[test]
fn 円とドルの金額は足せず比べられない() {
    assert!(!unit_errors(&usd("  let t = p.unit_price + p.price_usd")).is_empty());
    assert!(!unit_errors(&usd("  if p.unit_price > p.price_usd { return WrongStatus }")).is_empty());
    assert!(unit_errors(&usd("  let t = p.price_usd + p.price_usd\n  if t > p.price_usd { return WrongStatus }")).is_empty());
}

#[test]
fn 金額を入れる先の単位が違うと止まる() {
    let bad = usd_w("  p.note_usd = p.unit_price", "Product.note_usd");
    assert!(any(&errs(&bad), "ドルの金額ですが、円の金額"), "{:?}", errs(&bad));
    assert!(unit_errors(&usd_w("  p.note_usd = p.price_usd * 2", "Product.note_usd")).is_empty());
    // let の変数、掛け算、丸めを通しても単位は引き継がれる
    assert!(!unit_errors(&usd_w("  let t = p.unit_price * 3\n  p.note_usd = t", "Product.note_usd")).is_empty());
}

#[test]
fn ドルの金額に_0以外の数字は直接書けず_moneyで最小単位から作る() {
    assert!(any(&errs(&usd_w("  p.note_usd = 500", "Product.note_usd")), "直接書けません"));
    assert!(unit_errors(&usd_w("  p.note_usd = money(500, USD)", "Product.note_usd")).is_empty());
    assert!(any(&errs(&usd("  if p.price_usd > 100 { return WrongStatus }")), "直接書けません"));
    assert!(unit_errors(&usd("  if p.price_usd > 0 { return WrongStatus }")).is_empty());
    assert!(any(&errs(&usd_w("  p.note_usd = money(500, GBP)", "Product.note_usd")), "単位"));
}

#[test]
fn 丸めの単位は金額の単位と合わせる() {
    let ok = usd_w("  p.note_usd = round(p.price_usd * tax, floor, USD)", "Product.note_usd").replace("input product_id: id required", "input product_id: id required\n  slot tax: TaxRateValue");
    assert!(unit_errors(&ok).is_empty());
    assert!(any(&errs(&ok.replace("floor, USD", "floor, JPY")), "単位で丸めています"));
}

// ---- 合成型を使える場所 ----
fn addr_src() -> String {
    let mut s = code().replacen("/// 商品\nentity Product", "ltype Zip = text 7..7 of \"0-9\"\nltype Line = text 1..50\nltype Addr { zip: Zip, line: Line }\n/// 商品\nentity Product", 1);
    s = s.replacen("  NoSuchCustomer\n}", "  NoSuchCustomer\n  Shipped(to: Addr)\n}", 1);
    s = s.replacen("/// 決済\neffect Payment.", "effect Ship. send(order_id: id, to: Addr) no_wait \"配送の失敗は人が見る\"\n\n/// 決済\neffect Payment.", 1);
    s = s.replace("place_order: Received → request_payment,", "place_order: Shipped → reply, Received → request_payment,");
    s.push_str("\ndeed ship_order {\n  input order_id: id required, addr: Addr required\n  reads Order.status\n  result PlaceOrderResult\n  effects Ship.send\n\n  let order = Order[order_id] else { return NoSuchCustomer }\n  if order.status != Received { return NoSuchCustomer }\n  request Ship.send(order_id, addr, idempotency_key = order_id)\n  return Shipped(addr)\n}\n\nnexus ShipFlow {\n  ship_order: Shipped → reply, NoSuchCustomer → reply, Received → reply, OutOfStock → reply, NoSuchProduct → reply\n}\n");
    s
}

#[test]
fn 合成型は結果の中身_外部作用の引数_応答の中身にも使える() {
    with_check(&addr_src(), false, |ck| {
        assert!(ck.ready(), "{:?}", ck.diags);
        let g = generate(ck);
        assert!(g.contains("Shipped(") && g.contains("to: Addr"));
        assert!(geas_core::hostgen::host_source(ck).unwrap().contains("js_str(\"zip\")")); // 画面とやりとりする JSON にも、成分が平らに出る
    });
    let with_reply = addr_src().replace("/// 待ち解除結果", "reply Delivered(to: Addr required)\n\n/// 待ち解除結果");
    assert!(!errs(&with_reply).iter().any(|m| m.contains("合成型")));
}

// ---- 海外にも売るお店（残件4件を使う題材）----
#[test]
fn 海外店の例は通り_4件の機能がそろって使われる() {
    with_check_at(&overseas(), Some(&overseas_path()), false, |ck| {
        assert!(ck.ready(), "{:?}", ck.diags);
        assert!(!ck.diags.iter().any(|d| d.level == WARN), "{:?}", ck.diags);
        // par
        assert!(ck.p.flows.iter().any(|fl| fl.entries.iter().any(|(_, routes)| route_map(routes).values().any(|ds| ds.len() > 1))));
        // par の枝が書く
        assert!(["request_payment", "grant_points"].iter().any(|d| !ck.infos[*d].writes.is_empty()));
        // 応答の中身
        assert_eq!(ck.p.replies.iter().map(|r| r.name.clone()).collect::<Vec<_>>(), vec!["PaymentSettled", "StockSettled"]);
        // 合成型 Address を結果・外部作用・項目に使う
        assert!(ck.p.composites.iter().all(|t| t.kind == "合成") && !ck.p.composites.is_empty());
        // 通貨を slot で渡すので、円用・ドル用に分けない
        assert_eq!(ck.p.processes.iter().filter(|p| p.name.starts_with("place_order")).count(), 1);
        let a = generate(ck);
        with_check_at(&overseas(), Some(&overseas_path()), false, |c2| assert!(a == generate(c2)));
    });
}

#[test]
fn 海外店の例_通貨を取り違えると止まる() {
    let m = lv_ov(&overseas(), ERROR);
    assert!(!m.iter().any(|x| x.contains("単位") || x.contains("通貨")), "{:?}", m);
    let no_check = overseas().replace("  let other_currency = { k | (k, v) in items, Product[k].currency != currency }\n  if other_currency is nonempty { return OtherCurrency(other_currency) }\n", "");
    assert!(any(&lv_ov(&no_check, ERROR), "別の通貨"));
    let wrong = overseas().replace("request Payment.charge(order_id, order.currency, order.total,", "request Payment.charge(order_id, USD, order.total,");
    assert!(any(&lv_ov(&wrong, ERROR), "金額ですが"));
}

#[test]
fn 海外店の例_parの枝が同じ項目を書くと止まる() {
    let clash = overseas().replace("par { request_payment, request_stock_hold, grant_points, grant_campaign_points }", "par { request_payment, cancel_unpaid, request_stock_hold }");
    assert!(any(&lv_ov(&clash, ERROR), "par の枝"));
}

#[test]
fn 海外店の例_応答の中身が行き先の入力と合わないと止まる() {
    let bad = overseas().replace("input order_id: id required, auth_id: id, short: IdSet\n  reads Order.status\n  result SettleResult", "input order_id: id required, short: IdSet\n  reads Order.status\n  result SettleResult");
    let bad = bad.replace("  let auth = auth_id else { return PaymentFailed(order_id) }\n  let lacking = short else { return Approved(order_id) }\n  return StockShort(order_id, auth)", "  let lacking = short else { return Approved(order_id) }\n  return PaymentFailed(order_id)");
    assert!(any(&lv_ov(&bad, ERROR), "応答の中身"));
}

#[test]
fn 海外店の例は生成したrustがビルドできる() {
    let t = Tmp::new("ovbuild");
    with_check_at(&overseas(), Some(&overseas_path()), false, |ck| write_crate(ck, &generate(ck), "fn main() {}\n", &t.0, false).unwrap());
    let r = Command::new("cargo").args(["build", "--quiet", "--offline"]).env("CARGO_TARGET_DIR", common::serve::target_dir()).current_dir(&t.0).output().unwrap();
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 1500));
}

#[test]
fn 問い合わせは_equalsで文字列やidの完全一致で絞れる() {
    with_check_at(&overseas(), Some(&overseas_path()), false, |ck| {
        assert!(ck.ready(), "{:?}", ck.diags);
        let q = build(ck)["問い合わせ"].to_string();
        assert!(q.contains("jan 一致 jan") || q.contains("JANコード"), "{}", q);
        assert!(generate(ck).contains("r.jan == *x"));
    });
    let bad = overseas().replace("filter jan equals jan", "filter price equals jan");
    assert!(any(&lv_ov(&bad, ERROR), "同じ種類"));
}

// ---- 通貨コードを slot で渡す金額（money(currency)）----
fn shop_currency() -> String {
    let mut s = code();
    s = s.replacen("/// 単価\n  unit_price: NonNegMoney required\n", "/// 通貨\n  currency: currency required\n  /// 単価\n  unit_price: CurUnitPrice required\n", 1);
    s = s.replacen("  total: OrderTotal required\n", "  total: CurOrderTotal required\n  /// 通貨\n  currency: currency required\n", 1);
    s = s.replacen("ltype ProductQuantity", "ltype CurUnitPrice = money(currency) 0..10000000\nltype CurOrderTotal = money(currency) 0..1000000000000\nltype ProductQuantity", 1);
    s = s.replacen("  slot tax_rate: TaxRateValue\n", "  slot tax_rate: TaxRateValue\n  slot currency: currency\n", 1);
    s = s.replacen("  reads Product.stock, Product.unit_price, Customer\n", "  reads Product.stock, Product.unit_price, Product.currency, Customer\n", 1);
    s = s.replacen("  let short = { k | (k, v) in items, Product[k].stock < v }\n", "  let wrong = { k | (k, v) in items, Product[k].currency != currency }\n  if wrong is nonempty { return OutOfStock(wrong) }\n  let short = { k | (k, v) in items, Product[k].stock < v }\n", 1);
    s = s.replace("create Order { customer: customer_id,", "create Order { currency: currency, customer: customer_id,");
    s = s.replace("round(subtotal * tax_rate, floor, JPY)", "round(subtotal * tax_rate, floor, currency)");
    s = s.replace("  reads Order.total, Order.status\n  writes Order.status\n  transition Order: Received → PaymentRequested", "  reads Order.total, Order.currency, Order.status\n  writes Order.status\n  transition Order: Received → PaymentRequested");
    s = s.replace("amount: money(JPY))", "amount: money(currency), currency: currency)");
    s = s.replace("request Payment.charge(order_id, order.total, idempotency_key = order_id)", "request Payment.charge(order_id, order.total, order.currency, idempotency_key = order_id)");
    s
}

#[test]
fn 通貨コードのslotで金額を扱える_単位は同じslotの通貨どうし() {
    with_check(&shop_currency(), false, |ck| {
        assert!(ck.ready(), "{:?}", ck.diags);
        assert!(generate(ck).contains("pub enum Currency"));
        assert!(build(ck)["データ"].as_array().unwrap().iter().any(|r| r.to_string().contains("金額(通貨:")));
    });
}

#[test]
fn 通貨が同じと確かめずに_集まりの金額を使うと止まる() {
    let no_check = shop_currency().replace("  let wrong = { k | (k, v) in items, Product[k].currency != currency }\n  if wrong is nonempty { return OutOfStock(wrong) }\n", "");
    assert!(any(&errs(&no_check), "別の通貨"), "{:?}", errs(&no_check));
}

#[test]
fn 保存した金額を_固定の円の引数へ渡すと止まる() {
    let jpy_only = shop_currency().replace("amount: money(currency), currency: currency)", "amount: money(JPY))").replace("order.total, order.currency, idempotency_key", "order.total, idempotency_key");
    assert!(any(&errs(&jpy_only), "円の金額ですが、別の通貨の金額"), "{:?}", errs(&jpy_only));
}

#[test]
fn 通貨を取り違えて渡すと止まる_固定の通貨を渡すと別の単位() {
    let other = shop_currency().replace("order.total, order.currency, idempotency_key", "order.total, JPY, idempotency_key");
    assert!(any(&errs(&other), "金額ですが"), "{:?}", errs(&other));
}

#[test]
fn moneyの通貨の名前が同じ並びに無いと止まる() {
    let bad = format!("{}\neffect Foo. bar(a: money(currency)) no_wait \"試し\"\n", shop_currency());
    assert!(any(&errs(&bad), "同じ並びにありません"));
}

#[test]
fn 保存した1件の通貨を_slotと同じと確かめれば_その金額を使える() {
    // 通貨の確認なしで slot の金額と足すと止まり、確認すれば通る（x.通貨 != 通貨 なら返す）
    let base = shop_currency().replace("deed apply_payment_result {\n  input order_id: id required\n  reads Order.status", "deed apply_payment_result {\n  input order_id: id required, price: money(currency) required\n  slot currency: currency\n  reads Order.total, Order.currency, Order.status");
    let base = base.replace("  let order = Order[order_id] else { return NoSuchOrder }\n  if order.status != PaymentRequested { return WrongStatus }\n  order.status = Completed", "  let order = Order[order_id] else { return NoSuchOrder }\n  if order.status != PaymentRequested { return WrongStatus }\n  if order.total > price { return WrongStatus }\n  order.status = Completed");
    assert!(any(&errs(&base), "別の通貨"), "{:?}", errs(&base));
    let ok = base.replace("  if order.total > price { return WrongStatus }", "  if order.currency != currency { return WrongStatus }\n  if order.total > price { return WrongStatus }");
    assert!(!errs(&ok).iter().any(|m| m.contains("通貨") && m.contains("金額")), "{:?}", errs(&ok));
}

#[test]
fn 海外店の例は計画書と突合して食い違いがない() {
    let ov = root().join("tests/fixtures/overseas_shop");
    let r = Command::new(BIN).env("GEAS_LANG", "ja").arg("compare").arg(ov.join("overseas_shop.geas")).arg(ov.join("overseas_shop_plan.md")).arg("--no-run").output().unwrap();
    assert!(r.status.success(), "{}{}", String::from_utf8_lossy(&r.stdout), String::from_utf8_lossy(&r.stderr));
    let plan = std::fs::read_to_string(ov.join("overseas_shop_plan.md")).unwrap();
    for form in ["ブラウザ", "PC アプリ", "スマホアプリ", "Chrome 拡張"] {
        assert!(plan.contains(form), "{}", form); // 利用の形態の前提を、計画書に書いてある
    }
}
