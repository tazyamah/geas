// SPDX-License-Identifier: GPL-3.0-or-later
//! 言語機能ごとの動作テスト（信頼性）。途中の計算の trust の段階と、定義した項目を問い合わせの列に出すことを、生成した Rust で動かす（Python 版 tests/test_reliability.py の移し）。
//! どの機能にどのテストがあるかは docs/language-test-map.md。
mod common;
use common::*;

const TAIL: &str = r####"}
nexus Filling {
  put_tray: Done → reply, Refused → reply
}
"####;

const REL_SRC: &str = r####"ltype Quantity = int 0..1000
ltype Total = int 0..2000

entity Product {{
  product_id: id unique
  stock: Quantity required
  reserved: Quantity required
  cap: Quantity required
{derived}}}
{inv}
result StockResult {{
  Done
  NoProduct
  OverCap
  Short
}}
deed change_stock {{
  input product_id: id required
  input n: Quantity required
  reads {reads}
  writes {writes}
  result StockResult
  effects none

  let p = Product[product_id] else {{ return NoProduct }}
{body}  return Done
}}
nexus Changing {{
  change_stock: Done → reply, NoProduct → reply, OverCap → reply, Short → reply
}}
"####;

const HELD: &str = r####"  held: Total = stock + reserved
"####;

const ALL: &str = r####"Product.stock, Product.reserved, Product.cap"####;

const CALC_HEAD: &str = r####"ltype Wide = int 0..9000000000000000000
ltype Out = int 0..

entity Tray {
  tray_id: id unique
  n: Out required
}
result PutResult {
  Done(tray_id: id)
  Refused
}
deed put_tray {
  input a: Wide required
  input b: Wide required
  input c: Wide required
  writes Tray
  result PutResult
  effects none

"####;

const CALC_MAKE: &str = r####"  let t = create Tray { n: 1 }
  return Done(t.tray_id)
"####;

const HELD_QUERY: &str = r####"
query stock_list {
  from Product
  select product_id, stock, held
  order stock desc
  reads Product.product_id, Product.stock, Product.reserved
}
"####;

/// REL_SRC は Python の format 用（{body} など。波かっこは {{ }}）。同じ置き換えをする
fn rel(body: &str, inv: &str, derived: &str, reads: &str, writes: &str) -> String {
    REL_SRC.replace("{body}", body).replace("{inv}", inv).replace("{derived}", derived).replace("{reads}", reads).replace("{writes}", writes).replace("{{", "{").replace("}}", "}")
}

fn calc(level: &str) -> String {
    format!("{}  let x = a * b * c trust {} \"試験\"\n  if x > 5 {{ return Refused }}\n{}{}", CALC_HEAD, level, CALC_MAKE, TAIL)
}

#[test]
fn 途中の計算は段階どおりに動く() {
    // 9e18 を 3 つ掛けると、計算に使う幅（i128）を超える。check は異常で止まり、assume は確かめずに先へ進む。
    // verify は、検証ビルド（cargo run の既定）では check と同じに止まる
    let req = "r#\"{\"name\":\"put_tray\",\"ctx\":{\"new_Tray_id\":\"T\"},\"input\":{\"a\":9000000000000000000,\"b\":9000000000000000000,\"c\":9000000000000000000},\"store\":{}}\"#";
    let main = format!("use geas_app::host::call;\nfn main() {{\n    match call({}) {{ Ok(s) => println!(\"OK {{}}\", s), Err(e) => println!(\"ERR {{}}\", e) }}\n}}\n", req);
    let mut out = std::collections::HashMap::new();
    let mut dirs = vec![];
    for level in ["check", "verify", "assume"] {
        let t = Tmp::new(level);
        build_crate(&calc(level), &main, &t.0, true);
        out.insert(level, cargo_run(&t.0, false).join("\n"));
        dirs.push((level, t));
    }
    assert!(out["check"].starts_with("ERR") && out["check"].contains("あふれ"), "{:?}", out);
    assert_eq!(out["verify"], out["check"]);
    assert!(out["assume"].starts_with("OK"), "{:?}", out);
    // 通常ビルド（--release）の verify は確かめない：溢れた値（桁が回った値）のまま先へ進み、異常にも panic にもならない。check は通常ビルドでも止まる
    for (level, want) in [("verify", "OK"), ("check", "ERR")] {
        let t = &dirs.iter().find(|(l, _)| *l == level).unwrap().1;
        let got = cargo_run(&t.0, true).join("\n");
        assert!(got.starts_with(want), "{} {}", level, got);
    }
}

fn held_src() -> String {
    format!("{}{}", rel("  if p.held + n > p.cap { return OverCap }\n  p.stock += n\n", "invariant Product: held <= cap", HELD, ALL, "Product.stock"), HELD_QUERY)
}

#[test]
fn 定義した項目は問い合わせの列に出せて読むのは元の項目() {
    assert!(errors(&held_src()).is_empty(), "{:?}", errors(&held_src()));
    // 読む宣言には、定義の式の元の項目（stock と reserved）が要る
    let errs = errors(&held_src().replace(", Product.reserved", ""));
    assert!(errs.iter().any(|m| m.contains("「Product.reserved」を読むのに、宣言（読む）にありません")), "{:?}", errs);
}

#[test]
fn 定義した項目は条件や並びには書けない() {
    let errs = errors(&held_src().replace("order stock desc", "order held desc"));
    assert!(errs.iter().any(|m| m.contains("定義した項目「held」は、問い合わせでは select の列にだけ書けます")), "{:?}", errs);
}

#[test]
fn 問い合わせの答えに定義した項目の値が出る() {
    let store = "{\"Product\":{\"a\":{\"product_id\":\"a\",\"stock\":3,\"reserved\":4,\"cap\":10},\"b\":{\"product_id\":\"b\",\"stock\":600,\"reserved\":300,\"cap\":1000}}}";
    let r = format!("{{\"name\":\"stock_list\",\"ctx\":{{}},\"input\":{{}},\"store\":{}}}", store);
    let main = format!("use geas_app::host::call;\nfn main() {{\n    match call(r#\"{}\"#) {{ Ok(s) => println!(\"{{}}\", s), Err(e) => println!(\"ERR {{}}\", e) }}\n}}\n", r);
    let got = run_app_host(&held_src(), &main);
    let v: serde_json::Value = serde_json::from_str(&got[0]).unwrap_or_else(|e| panic!("{} {}", e, got[0]));
    let want: serde_json::Value = serde_json::json!([{"product_id": "b", "stock": 600, "held": 900}, {"product_id": "a", "stock": 3, "held": 7}]);
    assert_eq!(v["body"]["rows"], want, "{}", got[0]);
}

// ---- 検査の誤り・通る形（Python 版の静的な試験の移し）----
use geas_core::check::ERROR as ERR;
use geas_core::spec::{build, render};

const HEAD: &str = r####"ltype Count = int 0..100
ltype Big = int 0..1000

entity Tray {
  tray_id: id unique
  n: Big required
}
result PutResult {
  Done(tray_id: id)
  Refused
}
deed put_tray {
  input n: Count required
  input m: Count required
  writes Tray
  result PutResult
  effects none

"####;

fn errs_of(src: &str) -> Vec<String> {
    with_check(src, false, |ck| ck.diags.iter().filter(|d| d.level == ERR).map(|d| d.msg.clone()).collect())
}

fn errs_in(head: &str, body: &str) -> Vec<String> {
    errs_of(&format!("{}{}{}", head, body, TAIL))
}

fn has(v: &[String], needles: &[&str]) -> bool {
    v.iter().any(|m| needles.iter().all(|n| m.contains(n)))
}

// ---- if の中の let ----
#[test]
fn ifの中にletを書ける() {
    assert!(errs_in(HEAD, "  if n > 50 {\n    let twice = n * 2\n    let t = create Tray { n: twice }\n    return Done(t.tray_id)\n  }\n  return Refused\n").is_empty());
}

#[test]
fn 枝の中の名前は_枝を抜けたら消える() {
    let m = errs_in(HEAD, "  if n > 50 {\n    let twice = n * 2\n  }\n  let t = create Tray { n: twice }\n  return Done(t.tray_id)\n");
    assert!(has(&m, &["「twice」は定義されていません"]), "{:?}", m);
}

#[test]
fn 重ならない別の枝なら_同じ名前を使える() {
    let m = errs_in(HEAD, "  if n > 50 {\n    let twice = n * 2\n    let t = create Tray { n: twice }\n    return Done(t.tray_id)\n  } else {\n    let twice = m + m\n    if twice > 10 {\n      let t = create Tray { n: twice }\n      return Done(t.tray_id)\n    }\n  }\n  let twice = n + m\n  let t = create Tray { n: twice }\n  return Done(t.tray_id)\n");
    assert!(m.is_empty(), "{:?}", m);
}

#[test]
fn 見えている名前は_枝の中でも付け直せない() {
    let m = errs_in(HEAD, "  let twice = n * 2\n  if n > 50 {\n    let twice = m\n  }\n  return Refused\n");
    assert!(has(&m, &["「twice」は、すでにある名前です"]), "{:?}", m);
}

#[test]
fn 枝で確かめたことは_同じ名前の別の値に残らない() {
    // 枝の中で d が 1 以上と確かめても、枝を抜けたあとの別の d（0 かもしれない）で割れてはいけない
    let m = errs_in(HEAD, "  if n > 50 {\n    let d = m\n    if d == 0 { return Refused }\n    let t = create Tray { n: n / d }\n    return Done(t.tray_id)\n  }\n  let d = m\n  let t = create Tray { n: n / d }\n  return Done(t.tray_id)\n");
    assert_eq!(m.iter().filter(|x| x.contains("割る数")).count(), 1, "{:?}", m);
}

// ---- 文字列：定数との比較と、つなぐ + ----
const TEXT_HEAD: &str = r####"ltype Count = int 0..100
ltype Word = text 1..10 of "a-z"
ltype Label = text 1..11 of "a-z"

entity Tray {
  tray_id: id unique
  n: Count required
  label: Label required
}
result PutResult {
  Done(tray_id: id)
  Refused
}
deed put_tray {
  input n: Count required
  input w: Word required
  writes Tray
  result PutResult
  effects none

"####;

const MAKE: &str = "  let t = create Tray { n: n, label: w }\n  return Done(t.tray_id)\n";

fn text_errors(body: &str) -> Vec<String> {
    errs_in(TEXT_HEAD, body)
}

#[test]
fn 処理の中で_文字列を定数と比べられる() {
    assert!(text_errors(&format!("  if w == \"gift\" {{ return Refused }}\n{}", MAKE)).is_empty());
    assert!(text_errors(&format!("  if \"gift\" != w {{ return Refused }}\n{}", MAKE)).is_empty());
}

#[test]
fn 型に合わない定数との比較はエラー() {
    let m = text_errors(&format!("  if w == \"abcdefghijkl\" {{ return Refused }}\n{}", MAKE));
    assert!(has(&m, &["長さは 1..10 なので、12 文字", "同じになることはありません"]), "{:?}", m);
    let m = text_errors(&format!("  if w == \"Gift\" {{ return Refused }}\n{}", MAKE));
    assert!(has(&m, &["使える文字は [a-z]", "「G」を含む"]), "{:?}", m);
}

#[test]
fn 文字列の大小と_数との比較は書けない() {
    assert!(has(&text_errors(&format!("  if w < \"m\" {{ return Refused }}\n{}", MAKE)), &["大小は比べられません"]));
    assert!(has(&text_errors(&format!("  if n == \"5\" {{ return Refused }}\n{}", MAKE)), &["文字列だけです"]));
}

#[test]
fn 文字列を_足し算でつなげる() {
    assert!(text_errors("  let t = create Tray { n: n, label: w + \"x\" }\n  return Done(t.tray_id)\n").is_empty());
    assert!(text_errors("  let both = w + \"x\"\n  let t = create Tray { n: n, label: both }\n  return Done(t.tray_id)\n").is_empty());
}

#[test]
fn つないだ長さが入れ先を超えうるならエラー() {
    let m = text_errors("  let t = create Tray { n: n, label: w + \"xy\" }\n  return Done(t.tray_id)\n");
    assert!(has(&m, &["長さ（3..12）", "収まると示せません"]), "{:?}", m);
    let m = text_errors("  let both = w + w\n  let t = create Tray { n: n, label: both }\n  return Done(t.tray_id)\n");
    assert!(has(&m, &["長さ（2..20）"]), "{:?}", m);
}

#[test]
fn つないだ文字が入れ先の文字種を出るならエラー() {
    let m = text_errors("  let t = create Tray { n: n, label: w + \"X\" }\n  return Done(t.tray_id)\n");
    assert!(has(&m, &["文字が、項目の使える文字に収まると示せません"]), "{:?}", m);
}

#[test]
fn 数と文字列は_足し算でつなげない() {
    let m = text_errors("  let t = create Tray { n: n, label: w + n }\n  return Done(t.tray_id)\n");
    assert!(m.iter().any(|x| x.contains("同じ種類の数どうし") || x.contains("文字列だけ")), "{:?}", m);
}

// ---- 集合：add／remove と、件数の上限 max N ----
const SET_SRC: &str = r####"ltype Tags = set of id {max}
ltype Digit = int 0..9

entity Tray {
  tray_id: id unique
  tags: Tags {req}
  n: Digit required
}
result TagResult {
  Done
  NoTray
  TooMany
}
deed tag_tray {
  input tray_id: id required
  input tag_id: id required
  reads Tray.tags{reads}
  writes Tray.tags{writes}
  result TagResult
  effects none

  let t = Tray[tray_id] else { return NoTray }
{body}  return Done
}
nexus Tagging {
  tag_tray: Done → reply, NoTray → reply, TooMany → reply
}
"####;

fn set_src(body: &str, max: &str, req: &str, reads: &str, writes: &str) -> String {
    SET_SRC.replace("{max}", max).replace("{req}", req).replace("{body}", body).replace("{reads}", reads).replace("{writes}", writes)
}

fn set_errors(body: &str) -> Vec<String> {
    errs_of(&set_src(body, "max 3", "required", "", ""))
}

#[test]
fn 集合に_上限を確かめてから足せる() {
    assert!(set_errors("  if len(t.tags) >= 3 { return TooMany }\n  add tag_id to t.tags\n").is_empty());
    assert!(set_errors("  if len(t.tags) > 2 { return TooMany }\n  add tag_id to t.tags\n").is_empty());
}

#[test]
fn 集合から外すのに_確かめは要らない() {
    assert!(set_errors("  remove tag_id from t.tags\n").is_empty());
}

#[test]
fn 上限の確かめが無い追加はエラー() {
    let m = set_errors("  add tag_id to t.tags\n");
    assert!(has(&m, &["まだ上限（3 件）に届いていないと示せません", "if len(t.tags) >= 3"]), "{:?}", m);
    // 確かめが緩い（3 件のときも通る）のも、示せない
    let m = set_errors("  if len(t.tags) > 3 { return TooMany }\n  add tag_id to t.tags\n");
    assert!(has(&m, &["届いていないと示せません"]), "{:?}", m);
}

#[test]
fn 足す先の集合には_件数の上限が要る() {
    let m = errs_of(&set_src("  if len(t.tags) >= 3 { return TooMany }\n  add tag_id to t.tags\n", "", "required", "", ""));
    assert!(has(&m, &["型に件数の上限が要ります（set of id max N）"]), "{:?}", m);
}

#[test]
fn nullになりうる集合には足せない() {
    let m = errs_of(&set_src("  if len(t.tags) >= 3 { return TooMany }\n  add tag_id to t.tags\n", "max 3", "", "", ""));
    assert!(has(&m, &["required を付けてください"]), "{:?}", m);
}

#[test]
fn 集合でない項目には足せない() {
    let m = errs_of(&set_src("  add tag_id to t.n\n", "max 3", "required", ", Tray.n", ", Tray.n"));
    assert!(has(&m, &["集合ではありません"]), "{:?}", m);
}

#[test]
fn 集合に記号は使えず_直し方が出る() {
    let m = set_errors("  t.tags += tag_id\n");
    assert!(has(&m, &["集合に += は使えません。add 要素 to t.tags と書きます"]), "{:?}", m);
    let e = parse_err(&set_src("  t.tags << tag_id\n", "max 3", "required", "", "")).unwrap();
    assert!(e.contains("add 要素 to t.tags と書きます"), "{}", e);
    let e = parse_err(&set_src("  add tag_id into t.tags\n", "max 3", "required", "", "")).unwrap();
    assert!(e.contains("add 要素 to 変数.項目"), "{}", e);
}

// ---- 定義した項目（保存しない）と、同じ行の 2 項目を比べる不変条件 ----
fn rel_errors(body: &str, inv: &str, derived: &str, reads: &str, writes: &str) -> Vec<String> {
    errs_of(&rel(body, inv, derived, reads, writes))
}

fn rel_d(body: &str) -> Vec<String> {
    rel_errors(body, "invariant Product: held <= cap", HELD, ALL, "Product.stock")
}

#[test]
fn 定義した項目を比べる不変条件は_確かめてから書けば通る() {
    assert!(rel_d("  if p.held + n > p.cap { return OverCap }\n  p.stock += n\n").is_empty());
}

#[test]
fn 確かめずに増やすと_不変条件を示せない() {
    let m = rel_errors("  if p.stock + n > 1000 { return OverCap }\n  p.stock += n\n", "invariant Product: held <= cap", HELD, "Product.stock", "Product.stock");
    assert!(has(&m, &["不変条件 Product: held <= cap を示せません"]), "{:?}", m);
}

#[test]
fn 減らすだけなら_前から成り立つ約束で示せる() {
    assert!(rel_errors("  if p.stock < n { return Short }\n  p.stock -= n\n", "invariant Product: held <= cap", HELD, "Product.stock", "Product.stock").is_empty());
}

#[test]
fn 上限を下げる側にも_確かめが要る() {
    let inv = "invariant Product: held <= cap";
    assert!(has(&rel_errors("  p.cap = n\n", inv, HELD, "Product.cap", "Product.cap"), &["不変条件 Product: held <= cap を示せません"]));
    assert!(rel_errors("  if n < p.held { return OverCap }\n  p.cap = n\n", inv, HELD, "Product.stock, Product.reserved", "Product.cap").is_empty());
}

#[test]
fn 二つの項目を直に比べる不変条件() {
    let inv = "invariant Product: reserved <= stock";
    assert!(rel_errors("  if p.reserved + n > p.stock { return OverCap }\n  p.reserved += n\n", inv, "", "Product.reserved, Product.stock", "Product.reserved").is_empty());
    let m = rel_errors("  if p.stock < n { return Short }\n  p.stock -= n\n", inv, "", "Product.stock", "Product.stock");
    assert!(has(&m, &["不変条件 Product: reserved <= stock を示せません"]), "{:?}", m);
}

#[test]
fn 不変条件に式は直に書けず_定義してから比べる() {
    let m = rel_errors("", "invariant Product: stock + reserved <= cap", "", "Product.stock", "Product.stock");
    assert!(has(&m, &["式は項目として定義してから比べます"]), "{:?}", m);
}

#[test]
fn 定義した項目は_入れ子にできない() {
    let m = rel_errors("", "", &format!("{}  more: Total = held + stock\n", HELD), ALL, "Product.stock");
    assert!(has(&m, &["定義した項目「held」は使えません（入れ子にはしません"]), "{:?}", m);
}

#[test]
fn 定義した項目の式は_足し算と引き算だけ() {
    let m = rel_errors("", "", "  twice: Total = stock * 2\n", ALL, "Product.stock");
    assert!(has(&m, &["足し算と引き算だけです"]), "{:?}", m);
}

#[test]
fn 定義した項目には書き込めない() {
    let m = rel_errors("  p.held = n\n", "", HELD, ALL, "Product.stock");
    assert!(m.iter().any(|x| x.contains("項目「held」がありません") || x.contains("定義した項目")), "{:?}", m);
}

#[test]
fn 定義した項目の型の範囲も_書き込みのたびに示す() {
    // held は 0..1500 と約束した。stock と reserved はそれぞれ 0..1000 なので、足すと超えうる
    let src = |body: &str| rel(body, "", "  held: Held = stock + reserved\n", "Product.stock, Product.reserved", "Product.stock").replacen("ltype Total = int 0..2000", "ltype Held = int 0..1500", 1);
    assert!(has(&errs_of(&src("  if p.stock + n > 1000 { return OverCap }\n  p.stock += n\n")), &["held（= stock + reserved）が範囲 0..1500に収まる を示せません"]));
    assert!(errs_of(&src("  if p.held + n > 1500 or p.stock + n > 1000 { return OverCap }\n  p.stock += n\n")).is_empty());
}

#[test]
fn 仕様書に_定義した項目と証明の結果が出る() {
    let src = rel("  if p.held + n > p.cap { return OverCap }\n  p.stock += n\n", "invariant Product: held <= cap", HELD, ALL, "Product.stock");
    with_check(&src, false, |ck| {
        let md = render(ck, &build(ck), "");
        assert!(md.contains("（保存しない。= stock + reserved）"), "{}", md);
        assert!(md.contains("| Product: held <= cap | change_stock: 証明済み（書く前に確かめている） |"), "{}", md);
    });
}

// ---- 途中の計算が収まると示す（trust prove / check / verify / assume）----
fn calc_diags(body: &str, level: &str) -> Vec<String> {
    with_check(&format!("{}{}{}{}", CALC_HEAD, body, CALC_MAKE, TAIL), false, |ck| ck.diags.iter().filter(|d| d.level == level).map(|d| d.msg.clone()).collect())
}

fn calc_err(body: &str) -> Vec<String> {
    calc_diags(body, ERR)
}

#[test]
fn 収まると示せる計算には_何も書かなくてよい() {
    // 2 つの掛け算は 9e18 × 9e18 < 2^127 で、計算に使う幅に収まる
    assert!(calc_err("  let x = a * b\n  if x > 5 { return Refused }\n").is_empty());
}

#[test]
fn 収まると示せない途中の計算はエラー() {
    let m = calc_err("  let x = a * b * c\n  if x > 5 { return Refused }\n");
    assert!(has(&m, &["途中の計算が収まると示せません", "trust check"]), "{:?}", m);
}

#[test]
fn trust_proveは_書かないのと同じ() {
    assert!(calc_err("  let x = a * b trust prove \"2 つまでなら収まる\"\n  if x > 5 { return Refused }\n").is_empty());
    let m = calc_err("  let x = a * b * c trust prove \"収まるはず\"\n  if x > 5 { return Refused }\n");
    assert!(has(&m, &["途中の計算が収まると示せません"]), "{:?}", m);
}

#[test]
fn trust_checkは_エラーを警告に下げる() {
    let body = "  let x = a * b * c trust check \"実際の値は小さい\"\n  if x > 5 { return Refused }\n";
    assert!(calc_err(body).is_empty());
    assert!(calc_diags(body, geas_core::check::WARN).iter().any(|m| m.contains("trust check")));
}

#[test]
fn trust_verifyとassumeは_警告も出さない() {
    for level in ["verify", "assume"] {
        let body = format!("  let x = a * b * c trust {} \"実際の値は小さい\"\n  if x > 5 {{ return Refused }}\n", level);
        assert!(calc_err(&body).is_empty() && calc_diags(&body, geas_core::check::WARN).is_empty(), "{}", level);
    }
}

#[test]
fn 示せている文に_trustを書くとエラー() {
    let m = calc_err("  let x = a * b trust assume \"念のため\"\n  if x > 5 { return Refused }\n");
    assert!(has(&m, &["trust assume は要りません"]), "{:?}", m);
}

#[test]
fn trustの段階の名前を間違えると_候補が出る() {
    let e = parse_err(&format!("{}  let x = a * b * c trust warn \"x\"\n{}{}", CALC_HEAD, CALC_MAKE, TAIL)).unwrap();
    assert!(e.contains("prove") && e.contains("assume"), "{}", e);
}

#[test]
fn 人が引き受けた計算は_仕様書の人の保証に出る() {
    let src = format!("{}  let x = a * b * c trust verify \"実際の値は小さい\"\n  if x > 5 {{ return Refused }}\n{}{}", CALC_HEAD, CALC_MAKE, TAIL);
    with_check(&src, false, |ck| {
        let spec = build(ck);
        let rows: Vec<_> = spec["人の保証"].as_array().unwrap().iter().filter(|r| r[1].as_str().unwrap().contains("途中の計算")).collect();
        assert!(!rows.is_empty() && rows[0][3].as_str().unwrap().contains("検証ビルドだけ") && rows[0][4] == "実際の値は小さい", "{:?}", rows);
    });
}

#[test]
fn 生成したrustは_示せた計算から溢れの確かめを外す() {
    let rust = |body: &str| with_check(&format!("{}{}{}{}", CALC_HEAD, body, CALC_MAKE, TAIL), false, |ck| geas_core::rustgen::generate(ck));
    let cond = "  if x > 5 { return Refused }\n";
    let a = rust(&format!("  let x = a * b\n{}", cond));
    assert!(a.contains(".wrapping_mul(") && !a.split("pub fn put_tray").nth(1).unwrap().contains(".checked_mul("));
    assert!(rust(&format!("  let x = a * b * c trust check \"小さい\"\n{}", cond)).split("pub fn put_tray").nth(1).unwrap().contains(".checked_mul("));
    assert!(rust(&format!("  let x = a * b * c trust verify \"小さい\"\n{}", cond)).contains("cfg!(debug_assertions)"));
}
