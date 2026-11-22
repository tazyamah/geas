// SPDX-License-Identifier: GPL-3.0-or-later
//! 検査の指摘と構文エラーの文言：AI が直しやすいこと（行番号・近い名前・他の言語の書き方への案内）と、
//! 書き間違えたコードで検査器が落ちないこと。元は Python 版の tests/test_messages.py（タグ python-final）。
mod common;
use common::*;
use geas_core::check::{defs::check, ERROR};
use geas_core::error::Error;
use geas_core::parser::parse;
use regex::Regex;
use serde_json::Value;
use std::process::Command;

// 小さな例：在庫を 1 足す（表示名つき）
const BASE: &str = "/// 数量
ltype Quantity = int 0..99
/// 商品
entity Product {
  /// 商品ID
  product_id: id unique
  /// 在庫
  stock: Quantity required
  /// 名前
  name: text
}
/// 追加結果
result AddResult {
  /// 済
  Done
  /// なし
  NoSuch
  /// 満杯
  Full
}
/// 在庫を足す
deed add_stock {
  /// 商品ID
  input product_id: id required
  reads Product.stock
  writes Product.stock
  result AddResult
  effects none
  let p = Product[product_id] else { return NoSuch }
  if p.stock > 98 { return Full }
  p.stock += 1
  return Done
}
";

fn edit(src: &str, old: &str, new: &str) -> String {
    assert!(src.contains(old), "{}", old);
    src.replacen(old, new, 1)
}

fn syntax_error(src: &str) -> Error {
    parse(src, None).err().unwrap_or_else(|| panic!("構文の誤りのはず:\n{}", src))
}

fn serr(src: &str) -> String {
    let s = format!("{}", syntax_error(src));
    assert!(!s.contains('\n'), "改行の字句を、そのまま文言に入れない: {:?}", s); // 改行の字句を、そのまま文言に入れない
    s
}

/// (for_code の文言, 表示名の文言, 行)
fn errors(src: &str) -> Vec<(String, String, i64)> {
    with_check(src, true, |ck| ck.diags.iter().filter(|d| d.level == ERROR).map(|d| (d.for_code(), d.to_string(), d.line)).collect())
}

fn codes(src: &str) -> Vec<String> {
    errors(src).into_iter().map(|e| e.0).collect()
}

fn ok(src: &str) -> bool {
    with_check(src, true, |ck| ck.ok())
}

fn shop() -> String {
    fixture("shop/shop.geas")
}

fn overseas() -> String {
    fixture("overseas_shop/overseas_shop.geas")
}

fn delete_demo() -> String {
    fixture("delete_demo/delete_demo.geas")
}

#[test]
fn 小さな例は通る() {
    assert!(ok(BASE));
}

// ---- 書き間違えたコードで、検査器が落ちない ----
#[test]
fn 未定義の名前があっても検査器は落ちずに指摘する() {
    let cases: Vec<(String, &str, &str)> = vec![
        (overseas(), "ltype PointCount = int 0..1000000000\n", ""), // 未定義の型の項目へ書く
        (overseas(), "entity Order {", "entity Orders {"), // 引いたデータが未定義のまま項目へ書く
        (overseas(), "  status: OrderStatus", "  status: Pending"), // 新規作成する項目の型が未定義
        (overseas(), "  order_id: id unique\n", ""), // キーの無いデータを、出来事の each で回す
        (delete_demo(), "  status: NoteStatus", "  status: Unknown"), // 消すデータの状態の型が未定義
        (BASE.to_string(), "let p = Product[product_id]", "let p = Products[product_id]"), // entity でない名前で引く
    ];
    for (src, old, new) in cases {
        let s = edit(&src, old, new);
        // 構文が通れば検査し、エラーが出ること（落ちないこと）
        if let Ok(mut prog) = parse(&s, None) {
            let ck = check(&mut prog, true);
            assert!(!ck.ok(), "{} → {}", old, new);
        }
    }
}

/// 決まった種の乱数（試験を毎回同じにする）
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as usize
    }
    fn below(&mut self, n: usize) -> usize {
        self.next() % n
    }
}

#[test]
fn 崩したコードを何通り検査しても検査器は落ちない() {
    // 字句を消す・名前を入れ替える・行を消すなどで崩した版を、決まった種で作って検査する（落ちたら、その版を見る）
    let mut rnd = Lcg(20261108);
    let srcs = [shop(), delete_demo(), BASE.to_string()];
    let word = Regex::new(r"\b[A-Za-z_][A-Za-z0-9_]*\b").unwrap();
    for k in 0..120 {
        let base = &srcs[rnd.below(srcs.len())];
        let mut lines: Vec<String> = base.split('\n').map(String::from).collect();
        let mut names: Vec<String> = word.find_iter(base).map(|m| m.as_str().to_string()).collect();
        names.sort();
        names.dedup();
        let i = rnd.below(lines.len());
        match rnd.below(3) {
            0 => {
                lines.remove(i);
            }
            1 => {
                let ws: Vec<(usize, usize)> = word.find_iter(&lines[i]).map(|m| (m.start(), m.end())).collect();
                if !ws.is_empty() {
                    let (s, e) = ws[rnd.below(ws.len())];
                    let n = names[rnd.below(names.len())].clone();
                    lines[i] = format!("{}{}{}", &lines[i][..s], n, &lines[i][e..]);
                }
            }
            _ => {
                let l = lines[i].clone();
                lines.insert(i, l);
            }
        }
        let src = lines.join("\n");
        let r = std::panic::catch_unwind(|| {
            if let Ok(mut p) = parse(&src, None) {
                let _ = check(&mut p, true);
            }
        });
        assert!(r.is_ok(), "{} 番目の版で検査器が落ちました:\n{}", k, src);
    }
}

// ---- 構文エラー：位置と、他の言語の書き方への案内 ----
#[test]
fn 構文エラーは他の言語の書き方なら直し方を添える() {
    let cases: Vec<(&str, &str, &str)> = vec![
        ("if p.stock > 98 {", "if p.stock > 98 && p.stock < 200 {", "and と書きます"),
        ("if p.stock > 98 {", "if p.stock > 98 || p.stock < 0 {", "or と書きます"),
        ("if p.stock > 98 {", "if !(p.stock < 98) {", "not と書きます"),
        ("if p.stock > 98 { return Full }", "if p.stock > 98 { return Full }\n  else if p.stock > 97 { return Full }", "同じ行に続けて書きます"),
        ("if p.stock > 98 { return Full }", "if p.stock > 98 { return Full } elif p.stock > 97 { return Full }", "else if 条件 { … } と書きます"),
        ("if p.stock > 98 { return Full }", "if p.stock > 98 { return Full }\n  elsif p.stock > 97 { return Full }", "else if 条件 { … } と書きます"),
        ("if p.stock > 98 { return Full }", "if p.stock > 98 { return Full } else return Done", "else の後ろには { … } か if を書きます"),
        ("let p = Product[product_id] else { return NoSuch }", "let p = Product[product_id]\n  else { return NoSuch }", "同じ行に書きます"),
        ("p.stock += 1", "p.stock += 1;", "; は書きません"),
        ("p.stock += 1", "p.stock++", "+= 1"),
        ("let p =", "let mut p =", "mut は付けません"),
        ("return Done", "return Done()", "( ) は付けません"),
        ("return Done", "return", "結果の種類"),
        ("p.stock += 1", "while p.stock < 3 { p.stock += 1 }", "while はありません"),
        ("p.stock += 1", "match p.stock { }", "match・switch はありません"),
        ("p.stock += 1", "throw NoSuch", "return"),
        ("ltype Quantity", "type Quantity", "ltype です"),
        ("/// 数量\n", "enum Color { Red }\n/// 数量\n", "状態なら state、処理の結果なら result"),
        ("entity Product", "struct Product", "entity"),
        ("deed add_stock", "fn add_stock", "deed"),
        ("/// 数量\n", "import \"x.geas\"\n/// 数量\n", "use"),
        ("/// 数量\n", "/* メモ */\n/// 数量\n", "// から行末"),
        ("return Done\n}", "return Done\n", "行目の { が閉じていません"),
        ("return Done\n}", "return Done\n}\n}", "に対応する { がありません"),
        ("if p.stock > 98 {", "if p.stock > 98 ? {", "if 条件 then 式 else 式"),
        ("p.stock += 1", "p.stock += 1.5", "小数は書けません（「1.5」）"),
        ("p.stock += 1", "p.stock += ①", "数は半角の 0〜9 で書きます"),
        ("p.stock += 1", "p.stock += １", "数は半角の 0〜9 で書きます（「１」は半角の数字ではありません）"),
        ("ltype Quantity = int 0..99", "ltype Quantity = int ０..９９", "数は半角の 0〜9 で書きます"),
    ];
    for (old, new, expect) in cases {
        let e = serr(&edit(BASE, old, new));
        assert!(e.contains(expect), "{} → {}: {}", old, new, e);
    }
}

fn pos(e: &Error) -> (i64, i64) {
    match e {
        Error::Parse(p) => (p.line, p.col),
        Error::Lex(l) => (l.line, l.col),
        Error::Module(m) => (m.line, 0),
    }
}

fn line_of(src: &str, text: &str) -> i64 {
    src.split('\n').position(|l| l == text).unwrap() as i64 + 1
}

#[test]
fn 構文エラーは行と列を持ち行末とファイルの終わりを言葉で出す() {
    let e = syntax_error(&edit(BASE, "if p.stock > 98 {", "if p.stock > 98 && p.stock < 200 {"));
    let line = line_of(BASE, "  if p.stock > 98 { return Full }");
    assert_eq!(pos(&e), (line, "  if p.stock > 98 ".chars().count() as i64 + 1));
    assert!(serr(&edit(BASE, "return Done", "return")).contains("「行末」"));
    let s = serr("entity Product {\n  product_id: id unique\n");
    assert!(s.contains("ファイルの終わり") && s.contains("1行目の { が閉じていません"), "{}", s);
}

#[test]
fn 閉じ忘れた文字列はその行で止まる() {
    // 文字列の中では行を変えられないので、" を閉じ忘れた行がそのままエラーの行になる（後ろの " と組にならない）
    let line = line_of(BASE, "  if p.stock > 98 { return Full }");
    for tail in ["\n  if p.name == \"x\" { return Full }", "\n  if p.name == \"x\" { return Full }\n  if p.name == \"y { return Full }", ""] {
        let src = edit(BASE, "  if p.stock > 98 { return Full }", &format!("  if p.name == \"abc {{ return Full }}{}", tail));
        let e = syntax_error(&src);
        assert_eq!(pos(&e), (line, "  if p.name == ".chars().count() as i64 + 1), "{}", e);
        let s = format!("{}", e);
        assert!(s.contains("終わりの \" を書き忘れたかもしれません") && s.contains("\\n と書きます"), "{}", s);
    }
    let s = serr(&(BASE.to_string() + "app \"geas"));
    assert!(s.contains("文字列が閉じていません"), "{}", s);
}

#[test]
fn 問い合わせとresourceの書き間違い() {
    let q = BASE.to_string() + "/// 一覧\nquery product_list {\n  from Product\n  where stock > 0\n  order stock desc\n  reads Product.stock\n}\n";
    assert!(ok(&q));
    let has = |src: String, needle: &str| {
        let s = serr(&src);
        assert!(s.contains(needle), "{}: {}", needle, s);
    };
    has(edit(&q, "where stock > 0", "where stock > 0 and stock < 50"), "where の行を分けて");
    has(edit(&q, "where stock > 0", "where stock > 0, stock < 50"), "where の行を分けて");
    has(edit(&q, "order stock desc", "order stock newest"), "「newest」は使えません");
    has(edit(&q, "order stock desc", "order by stock desc"), "by は書きません");
    has(edit(&q, "order stock desc", "order stock DESC"), "向きは小文字です");
    has(edit(&q, "order stock desc", "order stock"), "desc か asc");
    has(edit(&q, "  order stock desc", "  filter stock >= 3\n  order stock desc"), "at_least");
    let r = BASE.to_string() + "/// 商品たち\nresource Products {\n  update add_stock {\n    Done → 200\n    NoSuch → 404\n    Full → 409\n  }\n}\n";
    has(edit(&r, "update add_stock {", "put add_stock {"), "PUT は update");
    has(edit(&r, "update add_stock {", "patch add_stock {"), "PATCH はありません");
    has(edit(&r, "Done → 200", "Done => 200"), "→ か -> と書きます");
    has(edit(&r, "Done → 200", "Done: 200"), "→ か -> でつなぎます");
}

#[test]
fn 取り込んだモジュールの構文エラーはそのファイルの名前を出す() {
    let t = Tmp::new("modmsg");
    let geo = t.0.join("geo.geas");
    std::fs::write(&geo, "module Geo\nltype Lat = int -90..90\nconvert half(x: Lat) -> Lat = x &&\n").unwrap();
    let app = t.0.join("app.geas");
    std::fs::write(&app, "use \"geo.geas\"\n").unwrap();
    let text = std::fs::read_to_string(&app).unwrap();
    let e = parse(&text, Some(&app)).err().expect("構文の誤りのはず");
    let s = format!("{}", e);
    assert!(s.starts_with("geo.geas の 3行目"), "{}", s);
    match &e {
        Error::Parse(p) => assert!(p.file.ends_with("geo.geas")),
        other => panic!("{:?}", other),
    }
    std::fs::write(&geo, "module Geo\nltype Lat = int -90..90 \"\n").unwrap();
    let e = parse(&text, Some(&app)).err().expect("字句の誤りのはず");
    assert!(matches!(e, Error::Lex(_)), "{:?}", e);
    assert!(format!("{}", e).starts_with("geo.geas の 2行目"), "{}", e);
}

// ---- 検査の指摘：行番号・近い名前・直し方 ----
#[test]
fn 他の言語の型名にはgeasの型を案内する() {
    for (typ, expect) in [
        ("string", "text"), ("String", "text"), ("boolean", "bool"), ("float", "小数の型はありません"), ("decimal", "money"),
        ("uuid", "id です"), ("datetime", "time"), ("i32", "int"), ("text?", "required"), ("Quantty", "近い名前：Quantity"),
    ] {
        let c = codes(&edit(BASE, "name: text", &format!("name: {}", typ)));
        assert!(c.iter().any(|m| m.contains(expect)), "{}: {:?}", typ, c);
    }
}

#[test]
fn 検査の指摘は直し方を添える() {
    let cases: Vec<(&str, &str, &str)> = vec![
        ("if p.stock > 98", "if p.stok > 98", "近い名前：stock"),
        ("let p = Product[product_id]", "let p = Product[productId]", "近い名前：product_id"),
        ("let p = Product[product_id]", "let p = Products[product_id]", "近い名前：Product"),
        ("if p.stock > 98", "if True", "true／false と小文字"),
        ("if p.stock > 98", "if p.name is null or None", "null です"),
        ("if p.stock > 98 { return Full }", "if p.stock > 98 { return Ful }", "近い名前：Full"),
        ("if p.stock > 98 { return Full }", "if p.stock > 98 { return Ok }", "あるもの：Done・NoSuch・Full"),
        ("if p.stock > 98", "if p.name == null", "p.name is null"),
        ("if p.stock > 98", "if p.name == 'x'", "\"…\" で囲みます"),
        ("  if p.stock > 98 { return Full }\n", "", "if p.stock > 98 { return … }"),
        ("  result AddResult\n", "", "結果の形の宣言がありません"),
    ];
    for (old, new, expect) in cases {
        let c = codes(&edit(BASE, old, new));
        assert!(c.iter().any(|m| m.contains(expect)), "{} → {}: {:?}", old, new, c);
    }
}

#[test]
fn 足し引きの範囲の指摘は引くときも境目を示す() {
    let src = edit(&edit(BASE, "  if p.stock > 98 { return Full }\n", ""), "p.stock += 1", "p.stock -= 3");
    assert!(codes(&src).iter().any(|m| m.contains("if p.stock < 3 { return … }")));
    let good = edit(&edit(BASE, "if p.stock > 98 { return Full }", "if p.stock < 3 { return Full }"), "p.stock += 1", "p.stock -= 3");
    assert!(ok(&good)); // 示した境目で確かめれば通る
}

#[test]
fn 足し引きの範囲の指摘は書ける境目だけを示す() {
    let no_guard = edit(BASE, "  if p.stock > 98 { return Full }\n", "");
    let msgs = codes(&edit(&no_guard, "ltype Quantity = int 0..99", "ltype Quantity = int"));
    assert!(msgs.iter().any(|m| m.contains("項目と足す値の型に ltype で範囲を付けて")), "{:?}", msgs); // 範囲の無い int の端の数は示さない
    let yen = edit(&edit(&no_guard, "ltype Quantity = int 0..99", "ltype Quantity = money(JPY) 0..100000"), "p.stock += 1", "p.stock += 100");
    assert!(codes(&yen).iter().any(|m| m.contains("if p.stock > 99900 { return … }")));
}

#[test]
fn 宣言と本体の食い違いは直す宣言の名前で言う() {
    let msgs = codes(&edit(BASE, "reads Product.stock", "reads Product.stock, Product.name"));
    assert!(msgs.iter().any(|m| m.contains("宣言の reads に「Product.name」がありますが、本体では読んでいません")), "{:?}", msgs);
    let msgs = codes(&edit(BASE, "  writes Product.stock\n", ""));
    assert!(msgs.iter().any(|m| m.contains("宣言の writes にありません")), "{:?}", msgs);
    assert!(!msgs.iter().any(|m| m.contains("ていません") && (m.contains("読むて") || m.contains("書くて"))));
}

#[test]
fn 指摘には行番号が付きai向けはコードの名前人向けは表示名で出す() {
    let src = edit(BASE, "if p.stock > 98", "if p.stok > 98");
    let d = errors(&src).into_iter().find(|x| x.0.contains("stok")).unwrap();
    assert_eq!(d.2, line_of(&src, "  if p.stok > 98 { return Full }"));
    assert!(d.0.contains("データ Product に項目「stok」がありません（近い名前：stock）") && d.0.contains(&format!("（{}行目）", d.2)), "{}", d.0);
    assert!(d.1.contains("データ 商品 に項目「stok」がありません（近い名前：在庫）"), "{}", d.1); // 仕様書に載るほうは表示名
    // 定義そのものの指摘も、定義の行が付く
    let d = errors(&edit(BASE, "name: text", "name: string")).into_iter().next().unwrap();
    assert_eq!(d.2, line_of(BASE, "  name: text"));
    with_check(&(shop() + "\nltype Bad = int 0..9\n"), true, |ck| {
        let bad: Vec<String> = ck.diags.iter().filter(|x| x.code == "表示名" && x.line == 0).map(|x| x.to_string()).collect();
        assert!(bad.is_empty(), "{:?}", bad);
    });
}

// ---- CLI ----
fn run(args: &[&str]) -> (i32, String, String) {
    let r = Command::new(env!("CARGO_BIN_EXE_geas")).env("GEAS_LANG", "ja").args(args).current_dir(root()).output().unwrap();
    (r.status.code().unwrap_or(-1), String::from_utf8_lossy(&r.stdout).to_string(), String::from_utf8_lossy(&r.stderr).to_string())
}

#[test]
fn checkはコードの名前と行番号で出し最後に件数を出す() {
    let t = Tmp::new("msgcheck");
    let f = t.0.join("app.geas");
    let fs = f.to_string_lossy().to_string();
    std::fs::write(&f, edit(BASE, "if p.stock > 98", "if p.stok > 98")).unwrap();
    let (rc, out, _) = run(&["check", &fs]);
    assert_eq!(rc, 1);
    assert!(out.contains("[エラー] 処理 add_stock（30行目）: データ Product に項目「stok」がありません（近い名前：stock）（名前）"), "{}", out);
    assert!(out.trim().split('\n').last().unwrap().starts_with("検査: エラーがあります（エラー "));
    let (_, out, _) = run(&["check", &fs, "--json"]);
    let d: Value = serde_json::from_str(&out).unwrap();
    let e = d["diagnostics"].as_array().unwrap().iter().find(|x| x["message"].as_str().unwrap().contains("stok")).unwrap();
    assert!(e["line"] == 30 && e["where"] == "処理 add_stock" && e["message"].as_str().unwrap().contains("近い名前：stock"), "{}", e);
}

#[test]
fn checkは問いが残れば人に聞くことを最後の行で言う() {
    let t = Tmp::new("msgq");
    let f = t.0.join("app.geas");
    std::fs::write(&f, edit(&shop(), "round(subtotal * tax_rate, floor, JPY)", "subtotal * tax_rate")).unwrap();
    let (rc, out, _) = run(&["check", &f.to_string_lossy()]);
    assert!(rc == 0 && out.contains("問い 1 件") && out.contains("人に1問ずつ聞いて"), "{}", out);
}

#[test]
fn 構文エラーはその行と位置の印を出す() {
    let t = Tmp::new("msgsyn");
    let f = t.0.join("app.geas");
    let fs = f.to_string_lossy().to_string();
    std::fs::write(&f, edit(BASE, "if p.stock > 98 {", "if p.stock > 98 && p.stock < 200 {")).unwrap();
    let (rc, _, err) = run(&["check", &fs]);
    assert_eq!(rc, 2);
    let lines: Vec<&str> = err.split('\n').collect();
    assert!(lines[0].starts_with("構文エラー: 30行目: ") && lines[0].contains("and と書きます"), "{}", err);
    assert!(lines[1].ends_with("|   if p.stock > 98 && p.stock < 200 { return Full }"), "{}", err);
    let col = |s: &str, pat: &str| s.find(pat).map(|b| s[..b].chars().count());
    assert_eq!(col(lines[2], "^"), col(lines[1], "&&"));
    let (_, out, _) = run(&["check", &fs, "--json"]);
    let d: Value = serde_json::from_str(&out).unwrap();
    let d = &d["diagnostics"][0];
    assert!(d["line"] == 30 && d["col"] == "  if p.stock > 98 ".chars().count() as i64 + 1 && d["hint"]["fix"].as_str().unwrap().contains("and と書きます"), "{}", d);
}

#[test]
fn ファイルが無い組み合わせが違うときは1行で断る() {
    let t = Tmp::new("msgcli");
    let shop_path = root().join("tests").join("fixtures").join("shop").join("shop.geas");
    let sp = shop_path.to_string_lossy().to_string();
    let none = t.0.join("nothing.geas");
    let (rc, _, err) = run(&["check", &none.to_string_lossy()]);
    assert!(rc == 2 && err.contains("ファイルがありません") && !err.contains("panicked"), "{}", err);
    let np = t.0.join("nothing.md");
    let (rc, _, err) = run(&["spec", &sp, "--plan", &np.to_string_lossy()]);
    assert!(rc == 2 && err.contains("nothing.md") && !err.contains("panicked"), "{}", err);
    let nd = t.0.join("no").join("dir").join("spec.md");
    let (rc, _, err) = run(&["spec", &sp, "-o", &nd.to_string_lossy()]);
    assert!(rc == 2 && err.contains("フォルダがありません"), "{}", err);
    let out = t.0.join("out");
    let (rc, _, err) = run(&["rust", &sp, "-o", &out.to_string_lossy(), "--runtime"]);
    assert!(rc == 2 && err.contains("--host") && !out.exists(), "{}", err);
    let (rc, _, err) = run(&[]);
    assert!(rc == 2 && err.contains("usage") && err.contains("check"), "{}", err);
}
