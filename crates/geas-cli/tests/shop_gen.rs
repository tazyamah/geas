// SPDX-License-Identifier: GPL-3.0-or-later
//! 返事の合流先（joins）・出来事・標準モジュール Gtin・ホストの定義・panic できない生成・範囲の証明・割合・trust・文字列・raw・dangerous の試験。
//! 元は Python 版の tests/test_geas.py の終わりのほう（タグ python-final）。
mod common;
use common::*;
use geas_core::check::{ERROR, WARN};
use geas_core::crategen::write_crate;
use geas_core::rustgen::generate;
use geas_core::spec::{build, render};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;

fn code() -> String {
    std::fs::read_to_string(root().join("tests/fixtures/shop/shop.geas")).unwrap()
}

fn ov_path() -> PathBuf {
    root().join("tests/fixtures/overseas_shop/overseas_shop.geas")
}

fn overseas() -> String {
    std::fs::read_to_string(ov_path()).unwrap()
}

fn lv(src: &str, level: &str) -> Vec<String> {
    with_check(src, false, |ck| ck.diags.iter().filter(|d| d.level == level).map(|d| d.msg.clone()).collect())
}

fn errs(src: &str) -> Vec<String> {
    lv(src, ERROR)
}

fn lv_ov(src: &str, level: &str) -> Vec<String> {
    with_check_at(src, Some(&ov_path()), false, |ck| ck.diags.iter().filter(|d| d.level == level).map(|d| d.msg.clone()).collect())
}

fn any(v: &[String], needle: &str) -> bool {
    v.iter().any(|m| m.contains(needle))
}

fn pe(src: &str) -> String {
    parse_err(src).unwrap_or_else(|| panic!("構文の誤りになるはず: {}", src))
}

fn ready(src: &str) {
    with_check(src, false, |ck| assert!(ck.ready(), "{:?}", ck.diags.iter().map(|d| d.msg.clone()).collect::<Vec<_>>()));
}

fn okk(src: &str) {
    with_check(src, false, |ck| assert!(ck.ok(), "{:?}", ck.diags.iter().map(|d| d.msg.clone()).collect::<Vec<_>>()));
}

// ---- 返事の合流先（joins）と 1件の可換な更新（+=）----
#[test]
fn 外部作用の返事は_joinsで同じ並列の枝が待つトリガに合流できる() {
    with_check_at(&overseas(), Some(&ov_path()), false, |ck| {
        assert!(ck.ready(), "{:?}", ck.diags);
        assert!(build(ck)["合流"].as_array().unwrap().contains(&json!(["倉庫.在庫の確保", "注文待ち"])));
    });
    // 待つ枝が同じ par にいないと、返事の行き場が無い
    let alone = overseas().replace("par { request_payment, request_stock_hold, grant_points, grant_campaign_points }", "par { request_stock_hold, grant_points, grant_campaign_points }");
    assert!(any(&lv_ov(&alone, ERROR), "同じ並列"));
}

#[test]
fn 合流先の誤りは止まる() {
    assert!(any(&lv_ov(&overseas().replace("joins OrderPending", "joins Nowhere"), ERROR), "未定義"));
    assert!(any(&lv_ov(&overseas().replace("joins OrderPending", "joins OrderPending no_wait \"二重\""), ERROR), "どちらかにしてください"));
    let other_key = overseas().replace("deed request_stock_hold {\n  input order_id: id required", "deed request_stock_hold {\n  input order_id: id");
    assert!(any(&lv_ov(&other_key, ERROR), "必須の入力にもっていません"));
}

#[test]
fn 加算減算は_parの枝で重なってよく_代入は止まる() {
    with_check_at(&overseas(), Some(&ov_path()), false, |ck| {
        for d in ["grant_points", "grant_campaign_points"] {
            assert_eq!(ck.infos[d].write_ops["Customer.points"].iter().cloned().collect::<Vec<_>>(), vec!["+=".to_string()]);
        }
    });
    let plain = overseas().replace("customer.points += 5", "customer.points = customer.points + 5");
    assert!(any(&lv_ov(&plain, ERROR), "同じ項目"));
}

// ---- クライアント側の出来事（つながりが戻った）と JAN のチェックディジット ----
#[test]
fn 海外店の例_つながりが戻ったら返事待ちの注文ごとに決済を引き直す() {
    with_check_at(&overseas(), Some(&ov_path()), false, |ck| {
        assert!(ck.ready(), "{:?}", ck.diags);
        assert_eq!(ck.p.events.iter().map(|e| (e.name.clone(), e.steps.clone())).collect::<Vec<_>>(), vec![("Reconnected".to_string(), vec![("各".to_string(), "awaiting_payment".to_string(), "recheck_payment".to_string())])]);
    });
    let m = lv_ov(&overseas().replace("each awaiting_payment → recheck_payment", "each awaiting_payment → settle_order"), ERROR);
    assert!(m.iter().any(|x| x.contains("each") || x.contains("入力")), "{:?}", m);
    assert!(any(&lv_ov(&overseas().replace("recheck_payment: Rechecked → wait OrderPending, ", "recheck_payment: Rechecked → reply, "), ERROR), "失敗の扱い"));
}

const GTIN: &str = r####"use "gtin.geas"
ltype Jan = Gtin.Gtin13
ltype Plain = text 13..13 of "0-9"
entity Item {
  item_id: id unique
  jan: Jan required
}
deed show_item {
  input item_id: id required, jan: Jan required, raw: Plain required
  reads Item
  writes Item.jan
  result R
  effects none

  let item = Item[item_id] else { return Missing }
  item.jan = jan
  return Done
}
result R {
  Done
  Missing
}
"####;

fn gtin_at() -> PathBuf {
    root().join("tests/fixtures/overseas_shop/gtin_demo.geas") // 標準モジュール Gtin（gtin.geas）の隣にあるものとして読む
}

fn gtin_errs(text: &str) -> Vec<String> {
    with_check_at(text, Some(&gtin_at()), false, |ck| ck.diags.iter().filter(|d| d.level == ERROR).map(|d| d.msg.clone()).collect())
}

#[test]
fn 検査数字は標準モジュールgtinの変換が境界で確かめ_確かめていない文字列は書けない() {
    assert!(gtin_errs(GTIN).is_empty(), "{:?}", gtin_errs(GTIN));
    let rs = with_check_at(GTIN, Some(&gtin_at()), false, |ck| generate(ck));
    assert!(rs.contains("pub fn Gtin_read13(") && !rs.contains("check_gtin")); // 確かめは geas の式から生成した変換で、組み込みの関数は無い
    let bad = gtin_errs(&GTIN.replace("item.jan = jan", "item.jan = raw"));
    assert!(bad.iter().any(|m| m.contains("変換") && m.contains("通った値と示せません")), "{:?}", bad);
}

#[test]
fn check_gtinは言語から外れ_直し方が出る() {
    assert!(pe("ltype Jan = text 13..13 of \"0-9\" check gtin\n").contains("標準モジュール Gtin"));
}

#[test]
fn 標準モジュールは_隣に置かなくても_同梱のものが使われる() {
    assert!(errs_at(GTIN, None).is_empty()); // ファイルから読まなくても、同梱の標準モジュールは取り込める
    let t = Tmp::new("stdmod");
    let f = t.0.join("app.geas");
    std::fs::write(&f, GTIN).unwrap();
    assert!(errs_at(GTIN, Some(&f)).is_empty()); // 隣に gtin.geas は無い
    std::fs::write(t.0.join("gtin.geas"), "module Gtin\nltype Other = int 0..1\n").unwrap();
    // 隣に同じ名前のファイルがあれば、そちらが使われる
    let r = geas_core::parser::parse(GTIN, Some(&f));
    let msg = match r {
        Err(e) => format!("{}", e),
        Ok(mut p) => geas_core::check::defs::check(&mut p, false).diags.iter().map(|d| d.msg.clone()).collect::<Vec<_>>().join("\n"),
    };
    assert!(msg.contains("Gtin13"), "{}", msg);
}

fn errs_at(src: &str, path: Option<&std::path::Path>) -> Vec<String> {
    with_check_at(src, path.or(Some(&ov_path())), false, |ck| ck.diags.iter().filter(|d| d.level == ERROR).map(|d| d.msg.clone()).collect())
}

#[test]
fn 標準モジュールgtinは_正しい検査数字だけ通す() {
    let ok: [(usize, &str); 4] = [(8, "49123456"), (12, "012345678905"), (13, "4901234567894"), (14, "14901234567891")];
    let bump = |g: &str| format!("{}{}", &g[..g.len() - 1], (g.chars().last().unwrap().to_digit(10).unwrap() + 1) % 10);
    let mut lines = vec![];
    for (n, good) in ok {
        lines.push(format!("  assert!(Gtin_read{}(\"{}\").is_ok());", n, good));
        lines.push(format!("  assert!(Gtin_read{}(\"{}\").is_err());", n, bump(good)));
        lines.push(format!("  assert!(Gtin_read{}(\"{}a\").is_err());", n, &good[..good.len() - 1]));
        lines.push(format!("  assert!(Gtin_read{}(\"{}0\").is_err());", n, good));
    }
    let main = format!("use geas_app::*;\nfn main() {{\n{}\n}}\n", lines.join("\n"));
    let t = Tmp::new("gtinok");
    with_check_at(GTIN, Some(&gtin_at()), false, |ck| {
        write_crate(ck, &generate(ck), &main, &t.0, false).unwrap();
        // 検査器の側の評価（画面に直に書いた値の確かめ・拡張の事前の確かめが使う木）も、同じ判定をする
        for (n, good) in ok {
            let req = geas_core::rawexpr::requires_tree(ck.parts.get(&format!("Gtin_read{}", n)).copied()).unwrap();
            let refuse = |s: &str| geas_core::rawexpr::refusal(&req, s);
            assert!(refuse(good).is_none() && refuse(&bump(good)).unwrap().contains("検査数字"));
            assert!(refuse(&format!("{}a", &good[..good.len() - 1])).unwrap().contains("数字だけ") && refuse(&format!("{}0", good)).unwrap().contains("桁"));
        }
    });
    let r = Command::new("cargo").args(["run", "--quiet", "--offline"]).env("CARGO_TARGET_DIR", common::serve::target_dir()).current_dir(&t.0).output().unwrap();
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 1500));
}

// ---- アプリ（サーバー・画面）向けの生成物 ----
fn literal_unescape(lit: &str) -> String {
    // rust_str の文字列リテラル（\\ ・ \" ・ \u{…} で逃がしてある）の中身を戻す
    let cs: Vec<char> = lit.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == '\\' {
            i += 1;
            if cs[i] == 'u' && cs[i + 1] == '{' {
                let end = cs[i..].iter().position(|c| *c == '}').unwrap() + i;
                let hex: String = cs[i + 2..end].iter().collect();
                out.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
                i = end + 1;
            } else {
                out.push(cs[i]);
                i += 1;
            }
        } else {
            out.push(cs[i]);
            i += 1;
        }
    }
    out
}

#[test]
fn hostは列つきの問い合わせの行を列名つきのjsonで返し_定義に外部作用の合流先を出す() {
    with_check_at(&overseas(), Some(&ov_path()), false, |ck| {
        let host = geas_core::hostgen::host_source(ck).unwrap();
        assert!(host.contains("product_list_row_json") && host.contains("js_str(\"price\")") && host.contains("\\\"rows\\\""));
        // DEFS_JSON は rust_str の文字列リテラル。改行も逃がすので、リテラルは 1 行
        let lit = host.split("DEFS_JSON: &str = ").nth(1).unwrap().split(";\n").next().unwrap();
        let defs: Value = serde_json::from_str(&literal_unescape(&lit[1..lit.len() - 1])).unwrap();
        assert_eq!(defs["effects"]["Warehouse.hold"]["joins"], "OrderPending");
        assert_eq!(defs["effects"]["Shipping.ship"]["no_wait"], true);
        assert_eq!(defs["effects"]["Payment.charge"]["args"], json!(["order_id", "currency", "amount"]));
    });
}

#[test]
fn 問い合わせの_equalsは参照の項目をidの入力で絞れる() {
    with_check_at(&overseas(), Some(&ov_path()), false, |ck| {
        assert!(ck.ready(), "{:?}", ck.diags);
        assert!(generate(ck).contains("r.customer == *x"));
    });
    assert!(any(&lv_ov(&overseas().replace("filter customer equals customer", "filter total equals customer"), ERROR), "equals"));
}

#[test]
fn 生成した処理に_panicできる構文が無い() {
    let lib = with_check(&code(), false, |ck| generate(ck));
    let body = lib.split("/// 端数の出る値").nth(1).unwrap(); // マクロ定義より後ろ（生成した処理の本体）
    for bad in ["unsafe", "unwrap_unchecked", ".expect(", ".unwrap()", "panic!"] {
        assert!(!body.contains(bad), "{}", bad);
    }
    assert!(lib.contains("Result<place_orderOutput, Fault>"));
    assert!(lib.contains("#![deny(unsafe_code, clippy::unwrap_used"));
}

// ---- 書く値が物理型・ltype の範囲に収まることを示す ----
#[test]
fn 合計が収まると示せなければ止まる_要素数の上限が要る() {
    let m = errs(&mutate(&code(), "Product → Quantity max 100", "Product → Quantity"));
    assert!(m.iter().any(|x| x.contains("注文.合計") && x.contains("収まると示せません")), "{:?}", m);
}

#[test]
fn 合計の項目の上限が小さいと止まる() {
    let m = errs(&mutate(&code(), "ltype OrderTotal = money(JPY) 0..1000000000000", "ltype OrderTotal = money(JPY) 0..1000"));
    assert!(m.iter().any(|x| x.contains("注文.合計") && x.contains("収まると示せません")), "{:?}", m);
}

#[test]
fn 単価の上限を書かないと合計が物理型に収まると示せない() {
    let m = errs(&mutate(&code(), "ltype NonNegMoney = money(JPY) 0..10000000", "ltype NonNegMoney = money(JPY) 0.."));
    assert!(m.iter().any(|x| x.contains("注文.合計") && x.contains("収まると示せません")), "{:?}", m);
}

const COUNTER: &str = r####"ltype Count = int 0..1000
entity Box {
  box_id: id unique
  n: Count required
}
result AddResult { Added, NoBox, Full }
deed add {
  input box_id: id required
  reads Box.n
  writes Box.n
  result AddResult
  effects none
  let b = Box[box_id] else { return NoBox }
  %s
  b.n += 5
  return Added
}
nexus 流れ {
  add: Added → reply, NoBox → reply, Full → reply
}
"####;

fn counter(guard: &str) -> String {
    COUNTER.replace("%s", guard)
}

#[test]
fn 加算は上限の確認が無いと止まり_確認すれば通る() {
    assert!(errs(&counter("")).iter().any(|m| m.contains("Box.n") && m.contains("収まると示せません")));
    okk(&counter("if b.n > 995 { return Full }"));
}

#[test]
fn 上限の確認が足りないと止まる() {
    assert!(any(&errs(&counter("if b.n > 996 { return Full }")), "収まると示せません"));
}

#[test]
fn 上限を書かない整数は物理型の最大までとして扱う() {
    okk(&code().replace("ltype StockCount = int 0..1000000", "ltype StockCount = int 0..")); // 減らすだけの更新は、物理型の最大までの範囲で収まる
    let src = counter("").replace("ltype Count = int 0..1000", "ltype Count = int 0..");
    assert!(any(&errs(&src), "収まると示せません"));
}

// ---- 生成規則の束縛：panic できる構文を clippy で強制する ----
fn has_clippy() -> bool {
    Command::new("cargo").args(["clippy", "--version"]).output().map(|o| o.status.success()).unwrap_or(false)
}

fn clippy(dir: &std::path::Path) -> std::process::Output {
    Command::new("cargo").args(["clippy", "--offline", "--lib", "--quiet"]).env("CARGO_TARGET_DIR", common::serve::target_dir().join("clippy")).current_dir(dir).output().unwrap()
}

#[test]
fn 生成した処理は_clippyのpanic禁止を通り_禁止を破ると落ちる() {
    if !has_clippy() {
        return;
    }
    let t = Tmp::new("clippy");
    let big = t.0.join("overseas");
    with_check_at(&overseas(), Some(&ov_path()), false, |ck| write_crate(ck, &generate(ck), "fn main() {}\n", &big, true).unwrap());
    let r = clippy(&big); // 画面・REST・par・待ち合わせ・合成型・部品まで使う大きい例
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 2500));
    let small = t.0.join("small");
    with_check(&code(), false, |ck| write_crate(ck, &generate(ck), "fn main() {}\n", &small, true).unwrap());
    let r = clippy(&small);
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 2500));
    let lib = small.join("src").join("lib.rs");
    let text = std::fs::read_to_string(&lib).unwrap().replacen("let mut writes: Vec<Write> = Vec::new();", "let mut writes: Vec<Write> = Vec::new(); let _boom: Option<u8> = None; let _ = _boom.unwrap();", 1);
    std::fs::write(&lib, text).unwrap();
    let bad = clippy(&small);
    assert!(!bad.status.success() && String::from_utf8_lossy(&bad.stderr).contains("unwrap"));
}

#[test]
fn 写像は_max_nを超える要素数を境界で止める() {
    let main = "use geas_app::*;\nuse std::collections::BTreeMap;\nfn main() {\n    let ok: BTreeMap<String, u8> = (0..100).map(|i| (format!(\"p{}\", i), 1u8)).collect();\n    let big: BTreeMap<String, u8> = (0..101).map(|i| (format!(\"p{}\", i), 1u8)).collect();\n    println!(\"{} {}\", place_orderInput { customer_id: \"c\".into(), items: ok }.validate().is_ok(), place_orderInput { customer_id: \"c\".into(), items: big }.validate().is_ok());\n}\n";
    assert_eq!(run_app(&code(), main), vec!["true false".to_string()]);
}

// ---- 割合の物理上限と、割合どうしの掛け算 ----
const RATIO_SRC: &str = r####"ltype NonNegMoney = money(JPY) 0..
ltype Rate = ratio 0..1
entity Item {
  item_id: id unique
  price: NonNegMoney required
}
result PutResult {
  Put(item_id: id)
}
deed put_item {
  input base: NonNegMoney required, tax_rate: Rate required, discount: Rate required
  writes Item
  result PutResult
  effects none

  let item = create Item { price: round(base * tax_rate * discount, half_up, JPY) }
  return Put(item.item_id)
}
nexus Flow {
  put_item: Put → reply
}
"####;

#[test]
fn 割合どうしの掛け算は丸めの対象で_途中の桁が収まると検査で示す() {
    ready(RATIO_SRC);
    // 範囲を書かない割合にも物理上限（分母 10^9・値 ±10^9）があり、掛けた途中の分子が i128 に収まらない組み合わせは検査で止まる
    assert!(any(&errs(&mutate(RATIO_SRC, "ltype Rate = ratio 0..1\n", "ltype Rate = ratio 0..1000000000\n")), "途中の分子"));
    // 丸めずに書くと問いになる
    let src = mutate(RATIO_SRC, "round(base * tax_rate * discount, half_up, JPY)", "base * tax_rate * discount");
    with_check(&src, false, |ck| assert!(ck.diags.iter().any(|d| d.code == "丸め")));
}

#[test]
fn 割合は分母の上限を境界で確かめ_割合どうしの掛け算が動く() {
    let main = "use geas_app::host::call;\nfn main() {\n    for (r1, r2) in [(\"1/2\", \"1/2\"), (\"1/2\", \"1000000001/1000000001\"), (\"1/0\", \"1/2\"), (\"2/1\", \"1/2\")] {\n        let req = format!(r#\"{{\"name\":\"put_item\",\"ctx\":{{\"new_Item_id\":\"A\"}},\"input\":{{\"base\":1000,\"tax_rate\":\"{}\",\"discount\":\"{}\"}},\"store\":{{}}}}\"#, r1, r2);\n        println!(\"{:?}\", call(&req));\n    }\n}\n";
    let got = run_app_host(RATIO_SRC, main);
    assert!(got[0].starts_with("Ok(") && got[0].contains("\\\"price\\\":250"), "{}", got[0]);
    assert!(got[1].starts_with("Err(") && got[1].contains("分母"), "{}", got[1]);
    assert!(got[2].starts_with("Err(") && got[2].contains("分母"), "{}", got[2]);
    assert!(got[3].starts_with("Err(") && got[3].contains("範囲"), "{}", got[3]);
}

// ---- 人の保証（trust）：入力の範囲の確認を、人が危険を承知で省く ----
fn trust_src(level: &str) -> String {
    RATIO_SRC.replace("tax_rate: Rate required,", &format!("tax_rate: Rate required{},", level))
}

fn gen(src: &str) -> String {
    with_check(src, false, |ck| generate(ck))
}

#[test]
fn trustは理由が必須で_段階は_verifyか_assume() {
    ready(&trust_src(" trust assume \"契約 §4\""));
    assert!(pe(&trust_src(" trust assume")).contains("理由"));
    assert!(pe(&trust_src(" trust check \"x\"")).contains("verify"));
}

#[test]
fn trustの段階に応じて境界の確認が変わり_仕様書に人の保証として出る() {
    let base = gen(&trust_src(""));
    let ver = gen(&trust_src(" trust verify \"仕入れ先が保証\""));
    let asm = gen(&trust_src(" trust assume \"契約 §4\""));
    let line = "check_ratio(\"tax_rate\", &self.tax_rate, Some(0), Some(1))?;";
    assert!(base.contains(line) && ver.contains(&format!("if cfg!(debug_assertions) {{ {} }}", line)));
    assert!(!asm.contains(line) && asm.contains("check_ratio(\"tax_rate\", &self.tax_rate, None, None)?;")); // 分母の物理上限は残る
    with_check(&trust_src(" trust assume \"契約 §4\""), false, |ck| {
        let sp = build(ck);
        assert_eq!(sp["人の保証"], json!([["put_item", "tax_rate", "割合 0..1", "確かめない", "契約 §4"]]));
        assert!(render(ck, &sp, "").contains("## 人の保証"));
    });
    with_check(&trust_src(""), false, |ck| assert_eq!(build(ck)["人の保証"], json!([])));
}

#[test]
fn 保証の強さは_人の保証の段階ごとに_弱い順に仕様書へ出る() {
    with_check(&trust_src(""), false, |ck| assert_eq!(build(ck)["保証の強さ"], json!([]))); // 何も緩めていなければ、人の保証の行は無い
    for (level, label) in [("verify", "検証ビルドだけ確かめる"), ("assume", "確かめない（人の保証）")] {
        with_check(&trust_src(&format!(" trust {} \"契約 §4\"", level)), false, |ck| {
            let sp = build(ck);
            let rows = sp["保証の強さ"].as_array().unwrap();
            assert_eq!(rows.iter().map(|r| (r[0].as_str().unwrap(), r[1].as_str().unwrap())).collect::<Vec<_>>(), vec![(label, "put_item tax_rate")]);
            assert!(rows[0][2].as_str().unwrap().contains("契約 §4"));
            assert!(render(ck, &sp, "").contains("## 保証の強さ"));
        });
    }
}

#[test]
fn 保証の強さは_検査器が示した不変条件と_人の保証を強い順に並べる() {
    with_check_at(&overseas(), Some(&ov_path()), false, |ck| {
        let sp = build(ck);
        let rows = sp["保証の強さ"].as_array().unwrap();
        assert!(!rows.is_empty() && rows[0][0] == "検査器が示した" && rows[0][1] == "不変条件");
        let order = ["検査器が示した", "実行時に毎回確かめる", "検証ビルドだけ確かめる", "範囲の端で止まる（人の保証）", "確かめない（人の保証）", "示せていない"];
        let idx: Vec<usize> = rows.iter().map(|r| order.iter().position(|o| *o == r[0].as_str().unwrap()).unwrap()).collect();
        let mut sorted = idx.clone();
        sorted.sort();
        assert_eq!(idx, sorted);
    });
}

#[test]
fn trust_assumeは範囲外を通し_既定は止める() {
    let main = "use geas_app::host::call;\nfn main() {\n    let req = r#\"{\"name\":\"put_item\",\"ctx\":{\"new_Item_id\":\"A\"},\"input\":{\"base\":1000,\"tax_rate\":\"3/2\",\"discount\":\"1/2\"},\"store\":{}}\"#;\n    println!(\"{:?}\", call(req));\n}\n";
    let a = run_app_host(&trust_src(""), main);
    let b = run_app_host(&trust_src(" trust assume \"契約 §4\""), main);
    assert!(a[0].starts_with("Err(") && a[0].contains("範囲外"), "{:?}", a);
    assert!(b[0].starts_with("Ok("), "{:?}", b); // 1000 × 3/2 × 1/2 = 750。範囲外の税率がそのまま通る
}

// ---- 文字列の書き方：行末の \ で次の行へ続け、改行そのものは \n ----
const REASON: &str = r####"ltype Qty = int 0..99
ltype Code = text 1..8 of "a\\-z"
entity Item {
  item_id: id unique
  stock: Qty required
  code: Code
}
result AddResult { Done, NoItem }
deed add {
  input item_id: id required
  reads Item.stock
  writes Item.stock
  result AddResult
  effects none
  let it = Item[item_id] else { return NoItem }
  it.stock += 1 saturating "在庫は棚の上限で止める。\
                            棚に入りきらないため\n（\"倉庫\" と相談済み）"
  return Done
}
"####;

#[test]
fn 文字列は行末のバックスラッシュで続け_改行はnで書き_仕様書の表を崩さない() {
    with_check(REASON, false, |ck| {
        assert!(ck.ok(), "{:?}", ck.diags);
        let why = "在庫は棚の上限で止める。棚に入りきらないため\n（\"倉庫\" と相談済み）"; // 続きの行の頭の空白は入らない
        assert_eq!(ck.infos["add"].guards, vec![("Item.stock".to_string(), "+=".to_string(), "saturating".to_string(), why.to_string())]);
        let ty = &ck.p.data[0].fields.iter().find(|f| f.name == "code").unwrap().r#type;
        assert_eq!(geas_core::ast::show_type(ty).unwrap(), "文字列 1..8 文字種 [\\-az]"); // \\- は文字種の - そのもの
        // コードの形に戻すと、書いたとおり
        assert_eq!(geas_core::ast::show(&geas_core::ast::Expr::StrLit { s: why.to_string() }), "\"在庫は棚の上限で止める。棚に入りきらないため\\n（\\\"倉庫\\\" と相談済み）\"");
        let md = render(ck, &build(ck), "");
        assert!(md.contains("棚に入りきらないため<br>（\"倉庫\" と相談済み）")); // 表の1行は1行のまま（改行は表の中の改行）
        assert!(md.lines().all(|l| !l.starts_with('（')));
        let html = geas_core::review::review(ck, None, &[], "", false).unwrap();
        assert!(html.contains("棚に入りきらないため<br>（&quot;倉庫&quot; と相談済み）"));
    });
    let e = pe(&mutate(REASON, "of \"a\\\\-z\"", "of \"\\d\""));
    assert!(e.contains("「\\d」は書けません") && e.contains("\\\\ と書きます"), "{}", e);
}

// ---- raw と、型に結んだ raw から変換する convert（入力の選別を通す境界）----
const RAW_SRC: &str = r####"ltype PartNo = text 8..8 of "0-9" read part_no_reader
ltype Plain = text 8..8 of "0-9"
entity Item {
  item_id: id unique
  part_no: PartNo required
}
result PutResult {
  Put(item_id: id)
}
convert part_no_reader(r: raw) -> PartNo {
  require len(r) == 8 "8桁にしてください"
  require only(r, "0-9") "数字だけにしてください"
  return text(r)
}
deed put_item {
  input part_no: PartNo required
  writes Item
  result PutResult
  effects none

  let item = create Item { part_no: part_no }
  return Put(item.item_id)
}
nexus Flow {
  put_item: Put → reply
}
"####;

#[test]
fn 型に結んだrawのconvertは入力と項目の両方に働く() {
    ready(RAW_SRC);
    let out = gen(RAW_SRC);
    assert!(out.contains("pub fn part_no_reader(r: &str) -> Result<String, String>") && out.contains("\"8桁にしてください\""));
    assert!(out.contains("part_no_reader(&self.part_no).map(|_| ())?;")); // 入力の取り込みで、型の変換が働く
    // raw は convert の引数にだけ書ける
    assert!(any(&errs(&mutate(RAW_SRC, "part_no: PartNo required\n}", "part_no: raw required\n}")), "raw"));
    // read は raw から変換する convert で、戻り値の型が同じこと
    assert!(any(&errs(&mutate(RAW_SRC, "read part_no_reader", "read missing_reader")), "read"));
    let other = mutate(RAW_SRC, "ltype Plain", "ltype Other = text 1..3\nltype Plain");
    assert!(any(&errs(&mutate(&other, "-> PartNo {", "-> Other {")), "戻り値の型"));
    // 変換を通っていない文字列は、変換つきの型の項目に書けない（型が事実を運ぶ）
    let other_input = mutate(&mutate(RAW_SRC, "input part_no: PartNo required", "input part_no: PartNo required, plain: Plain required"), "{ part_no: part_no }", "{ part_no: plain }");
    assert!(any(&errs(&other_input), "変換 part_no_reader を通った値と示せません"));
    // 式の中では呼べない／require は真偽の式／raw の操作は raw にだけ
    assert!(any(&errs(&mutate(RAW_SRC, "{ part_no: part_no }", "{ part_no: part_no_reader(part_no) }")), "式の中では呼べません"));
    assert!(any(&errs(&mutate(RAW_SRC, "require len(r) == 8", "require len(r)")), "真偽"));
    assert!(any(&errs(&mutate(RAW_SRC, "require len(r) == 8", "require len(1) == 8")), "raw です"));
    assert!(pe(&mutate(RAW_SRC, "\"8桁にしてください\"", "")).contains("理由"));
}

#[test]
fn 型に結んだ変換は拒否を入力不正にし_通った値だけが処理に入る() {
    let main = "use geas_app::host::call;\nfn main() {\n    for v in [\"12345678\", \"1234567\", \"1234567a\"] {\n        let req = format!(r#\"{{\"name\":\"put_item\",\"ctx\":{{\"new_Item_id\":\"A\"}},\"input\":{{\"part_no\":\"{}\"}},\"store\":{{}}}}\"#, v);\n        println!(\"{:?}\", call(&req));\n    }\n}\n";
    let got = run_app_host(RAW_SRC, main);
    assert!(got[0].starts_with("Ok(") && got[0].contains("12345678"), "{}", got[0]);
    assert!(got[1].starts_with("Err(") && got[1].contains("8桁"), "{}", got[1]);
    assert!(got[2].starts_with("Err(") && got[2].contains("数字だけ"), "{}", got[2]);
}

#[test]
fn 拒否の理由に引用符と改行と_バックスラッシュを書いても_そのまま返る() {
    let src = mutate(RAW_SRC, "\"8桁にしてください\"", "\"8桁に\\n\\\"してください\\\"\\\\\"");
    ready(&src);
    let main = "use geas_app::host::call;\nfn main() {\n    let req = r#\"{\"name\":\"put_item\",\"ctx\":{\"new_Item_id\":\"A\"},\"input\":{\"part_no\":\"1234567\"},\"store\":{}}\"#;\n    match call(req) { Ok(s) => println!(\"OK {}\", s), Err(e) => println!(\"ERR {}\", e) }\n}\n";
    let t = Tmp::new("reasonesc");
    build_crate(&src, main, &t.0, true);
    let r = Command::new("cargo").args(["run", "--quiet", "--offline"]).current_dir(&t.0).output().unwrap();
    assert!(r.status.success(), "{}", tail(&String::from_utf8_lossy(&r.stderr), 2500));
    assert_eq!(String::from_utf8_lossy(&r.stdout), "ERR 8桁に\n\"してください\"\\\n");
}

// ---- raw の文字位置を回る sum と、条件の式（検査数字を geas の式で書く）----
fn jan_src() -> String {
    RAW_SRC.replace("ltype PartNo = text 8..8 of \"0-9\" read part_no_reader", "ltype PartNo = text 13..13 of \"0-9\" read jan_reader").replace("part_no_reader", "jan_reader").replace(
        "  require len(r) == 8 \"8桁にしてください\"\n  require only(r, \"0-9\") \"数字だけにしてください\"",
        "  require len(r) == 13 \"13桁にしてください\"\n  require only(r, \"0-9\") \"数字だけにしてください\"\n  require sum { digit(r, i) * (if (len(r) - 1 - i) % 2 == 1 then 3 else 1) | i in positions(r) } % 10 == 0 \"検査数字が合いません\"")
}

#[test]
fn 検査数字は_convertの式で書け_型と引数の誤りは検査で止まる() {
    let j = jan_src();
    ready(&j);
    assert!(gen(&j).contains("checked_mul") && gen(&j).contains("rem_euclid(10i128)"));
    assert!(any(&errs(&j.replace("then 3 else 1", "then 3 else text(r)")), "then と else の型"));
    assert!(any(&errs(&j.replace("if (len(r) - 1 - i) % 2 == 1 then", "if len(r) then")), "真偽"));
    assert!(any(&errs(&j.replace("digit(r, i)", "digit(r, r)")), "digit( ) の位置は整数"));
}

#[test]
fn convertで書いた検査数字は_手本と同じ判定をする() {
    // 手本：右から 1 倍・3 倍・1 倍…の和が 10 の倍数
    let valid = |s: &str| s.chars().rev().enumerate().map(|(i, c)| c.to_digit(10).unwrap() * if i % 2 == 1 { 3 } else { 1 }).sum::<u32>() % 10 == 0;
    let cases = ["4901234567894", "4901234567895", "4006381333931", "4006381333932", "0000000000000", "0000000000001", "490123456789a", "490123456789", "49012345678945"];
    let main = format!("use geas_app::*;\nfn main() {{\n{}}}\n", cases.iter().map(|c| format!("    println!(\"{{}}\", jan_reader(\"{}\").is_ok());\n", c)).collect::<String>());
    let got = run_app_host(&jan_src(), &main);
    let want: Vec<String> = cases.iter().map(|c| (c.len() == 13 && c.chars().all(|x| x.is_ascii_digit()) && valid(c)).to_string()).collect();
    assert_eq!(got, want);
}

// ---- dangerous：未検査の raw を受けて、そのまま（または情報を付けて）外へ中継する ----
const RELAY_SRC: &str = r####"ltype Size = int 0..
ltype Tag = text 1..8
effect Partner. relay(body: raw) no_wait "中継の失敗は人が見る"
entity Note {
  note_id: id unique
  size: Size required
}
result Relayed {
  Done(note_id: id)
}
deed relay_note {
  input body: raw required trust dangerous "社外システムの本文を加工せずに中継する", tag: Tag required
  writes Note
  result Relayed
  effects Partner.relay

  let note = create Note { size: len(body) }
  request Partner.relay(raw_cat(to_raw(tag), body), idempotency_key = note.note_id)
  return Done(note.note_id)
}
nexus Flow {
  relay_note: Done → reply
}
"####;

#[test]
fn dangerousの入力だけが_rawを受けられ_中継の材料は決まった操作だけ() {
    ready(RELAY_SRC);
    // dangerous でない入力・項目・結果には raw を書けない
    assert!(any(&errs(&mutate(RELAY_SRC, " trust dangerous \"社外システムの本文を加工せずに中継する\"", "")), "raw"));
    assert!(any(&errs(&mutate(RELAY_SRC, "size: Size required\n}", "size: Size required\n  raw_copy: raw\n}")), "raw"));
    // 処理の中で raw に使えるのは len・to_raw・raw_cat だけ（中身を読む操作は convert の中でだけ）
    assert!(any(&errs(&mutate(RELAY_SRC, "len(body)", "len(text(body))")), "convert の中でだけ"));
    assert!(any(&errs(&mutate(RELAY_SRC, "raw_cat(to_raw(tag), body)", "raw_cat(tag, body)")), "raw 2つ"));
    // 人の保証の一覧に、dangerous が出る
    with_check(RELAY_SRC, false, |ck| {
        let sp = build(ck);
        let row = &sp["人の保証"][0];
        assert!(row[0] == "relay_note" && row[1] == "body" && row[3].as_str().unwrap().contains("危険"), "{}", row);
    });
}

#[test]
fn dangerousのrawは検査も変換もせず_そのまま外部作用へ中継される() {
    let main = "use geas_app::host::call;\nfn main() {\n    let body = r#\"<script>alert(\\\"x\\\")</script> 0x00 \\\\ 日本語\"#;\n    let req = format!(r#\"{{\"name\":\"relay_note\",\"ctx\":{{\"new_Note_id\":\"A\"}},\"input\":{{\"body\":\"{}\",\"tag\":\"T1\"}},\"store\":{{}}}}\"#, body);\n    println!(\"{:?}\", call(&req));\n}\n";
    let out = run_app_host(RELAY_SRC, main).join("\n");
    assert!(out.starts_with("Ok(") && out.contains("T1<script>") && out.contains("日本語"), "{}", out);
}
