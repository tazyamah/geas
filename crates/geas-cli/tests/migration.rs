// SPDX-License-Identifier: GPL-3.0-or-later
//! 保存の形の移行（migration 版 { add 実体.項目 = 値 ／ rename 実体.項目 to 新しい名前 ／ drop 実体.項目 ／ rename entity 実体 to 新しい名前 ／ drop entity 実体 }）。
//! 前の版の DB を、起動時に 1 回でまとめて今の形へ書き換える。途中で失敗したら、何も変えずに止まる。（Python 版 tests/test_migration.py の移し）
//! 組んで動かす試験は、サーバーを組む（初回は依存の crate を取るので通信が要る）。
mod common;
use common::*;
use geas_core::check::defs::check;
use geas_core::parser::parse;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_geas");
const M2: &str = "\nmigration 2 {\n  add Parcel.weight = 5\n  add Parcel.memo = null\n}\n";
const M3: &str = "\nmigration 3 {\n  rename Parcel.memo to note\n  drop Parcel.legacy\n}\n";
const M4: &str = "\nmigration 4 {\n  rename entity Parcel to Package\n  drop entity Coupon\n}\n";

fn v1() -> String {
    fixture("runtime_demo/runtime_demo.geas")
}

/// 見本の小包に、重さ（必ずある）とメモ（無いかもしれない。名前は memo）を足した版
fn app(memo: &str, migrations: &str) -> String {
    let mut t = v1().replacen("  refund_ref: id\n", &format!("  refund_ref: id\n  /// 重さ\n  weight: Weight required\n  /// メモ\n  {}: Kind\n", memo), 1);
    t = t.replacen("ltype Fee = money(JPY) 0..\n", "ltype Fee = money(JPY) 0..\n/// 重さ\nltype Weight = int 0..1000\n", 1);
    t = t.replacen("create Parcel { kind: kind, fee: fee, status: Received }", "create Parcel { kind: kind, fee: fee, status: Received, weight: 0 }", 1);
    assert_eq!(t.matches("weight").count(), 2);
    t + migrations
}

fn v2(m2: &str) -> String {
    app("memo", m2)
}

fn v3(m3: &str, m2: &str) -> String {
    app("note", &format!("{}{}", m2, m3))
}

fn v4(m4: &str, name: &str) -> String {
    format!("{}{}{}{}", app("note", "").replace("Parcel", name), M2, M3, m4)
}

fn has(text: &str, word: &str) -> bool {
    errors(text).iter().any(|m| m.contains(word))
}

fn parse_error(src: &str) -> String {
    match parse(src, None) {
        Ok(_) => panic!("構文の誤りになるはずです"),
        Err(e) => format!("{:?}", e),
    }
}

#[test]
fn addの検査() {
    assert!(errors(&v2(M2)).is_empty(), "{:?}", errors(&v2(M2)));
    assert!(has(&v2(&M2.replace("migration 2", "migration 3")), "版は 2 から 1 ずつ"));
    assert!(errors(&v2(&M2.replace("= 5", "= 1001"))).iter().any(|m| m.contains("型の範囲") && m.contains("の外")));
    assert!(has(&v2(&M2.replace("weight = 5", "weight = null")), "null にできません"));
    assert!(has(&v2(&M2.replace("= 5", "= \"5\"")), "値は数です"));
    assert!(has(&v2(&M2.replace("Parcel.memo", "Parcel.nowhere")), "今の実体の項目にありません"));
    assert!(has(&v2(&M2.replace("Parcel.memo = null", "Parcel.parcel_id = \"x\"")), "行のキー"));
    assert!(has(&v2(&M2.replace("add Parcel.memo = null", "add Parcel.weight = 6")), "add が 2 つ"));
    assert!(has(&v2(&M2.replace("add Parcel.memo = null", "add Parcel.status = Nowhere")), "状態 ParcelStatus の名前"));
    assert!(has(&v2(&M2.replace("Parcel.memo", "Package.memo")), "実体「Package」がありません"));
}

#[test]
fn renameとdropの検査() {
    assert!(errors(&v3(M3, M2)).is_empty(), "{:?}", errors(&v3(M3, M2)));
    // 名前を変えたのに、実体の項目が前の名前のまま
    assert!(has(&app("memo", &format!("{}{}", M2, M3)), "名前を変えたなら、実体の項目も「note」にしてください"));
    // 消すと書いたのに、実体にまだある
    assert!(has(&v3(&M3.replace("drop Parcel.legacy", "drop Parcel.weight"), M2), "消すなら、実体の項目からも消してください"));
    // 変えた先が、今の実体に無い
    assert!(has(&v3(&M3.replace("to note", "to nota"), M2), "今の実体の項目にありません"));
    assert!(has(&v3(&M3.replace("rename Parcel.memo to note", "rename Parcel.note to note"), M2), "新しい名前が、前と同じです"));
    // 足してから名前を変えた項目の値は、変えた先の型で確かめる（note は 1〜10 文字）
    assert!(errors(&v3(M3, &M2.replace("memo = null", "memo = \"abc\""))).is_empty());
    assert!(has(&v3(M3, &M2.replace("memo = null", "memo = \"12345678901\"")), "値の長さ 11"));
    // 足して、あとで消す項目は、今の実体に無くてよい
    assert!(errors(&v3(&M3.replace("drop Parcel.legacy", "drop Parcel.tmp"), &M2.replace("add Parcel.memo", "add Parcel.tmp = 1\n  add Parcel.memo"))).is_empty());
    // 消したあとに、同じ名前をもう一度足せる
    assert!(errors(&v3(&format!("{}\nmigration 4 {{\n  drop Parcel.weight\n}}\n\nmigration 5 {{\n  add Parcel.weight = 7\n}}\n", M3), M2)).is_empty());
    assert!(parse_error(&v3(&M3.replace("to note", "→ note"), M2)).contains("to が要ります"));
    assert!(parse_error(&v3(&M3.replace("drop Parcel.legacy", "remove Parcel.legacy"), M2)).contains("rename 実体.項目 to 新しい名前"));
}

/// 前の版と比べた検査（check --base）の誤り
fn against(new_text: &str, base_text: &str) -> Vec<String> {
    let mut p_new = parse(new_text, None).unwrap();
    let mut p_base = parse(base_text, None).unwrap();
    let ck_base = check(&mut p_base, true);
    let mut ck = check(&mut p_new, true);
    ck.check_against(&ck_base);
    ck.diags.iter().filter(|d| d.level == "エラー").map(|d| d.msg.clone()).collect()
}

#[test]
fn 前の版と比べてmigrationに書いていない変更は止まる() {
    let base1 = v1();
    assert!(against(&base1, &base1).is_empty() && against(&v2(M2), &base1).is_empty() && against(&v3(M3, M2), &base1).is_empty() && against(&v3(M3, M2), &v2(M2)).is_empty());
    // 足した項目を書いていない
    let missing = against(&v2(""), &base1);
    assert!(missing.len() == 2 && missing.iter().all(|m| m.contains("増えましたが、migration に書いてありません")) && missing[0].contains("migration 2 { add Parcel.重さ = 値 }"), "{:?}", missing);
    // 版 2 から、名前を変えたのに書いていない（増えた note と、無くなった memo の両方を言う）
    let renamed = against(&app("note", M2), &v2(M2));
    assert!(renamed.len() == 3 && renamed.iter().any(|m| m.contains("「メモ」が増えました")), "{:?}", renamed); // 残りの 1 つは、版 2 の add の先（memo）が今の実体に無い、という検査
    assert!(renamed.iter().any(|m| m.contains("「Parcel.memo」は、今の実体の項目にありません")));
    assert!(renamed.iter().any(|m| m.contains("前の版にあった項目") && m.contains("行に、この項目の値が残ります") && m.contains("drop Parcel.")));
    // 前の migration を消した
    assert!(against(&app("memo", ""), &v2(M2)).iter().any(|m| m.contains("migration は消さずに残します")));
    // 実体の改名と削除も、書いていなければ止まる
    assert!(against(&v4(M4, "Package"), &v3(M3, M2)).is_empty() && against(&v4(M4, "Package"), &base1).is_empty());
    let gone = against(&v4("", "Package"), &v3(M3, M2));
    assert!(gone.iter().any(|m| m.contains("前の版にあった実体「Parcel」がありません") && m.contains("drop entity Parcel") && m.contains("rename entity Parcel to 新しい名前")), "{:?}", gone);
    let more = against(&v4(M4, "Package").replacen("  /// 重さ\n  weight: Weight required\n", "  /// 重さ\n  weight: Weight required\n  extra: Kind\n", 1), &v3(M3, M2));
    assert!(more.len() == 1 && more[0].contains("前の版に無い項目「extra」が増えました"), "{:?}", more); // 名前を変えた実体でも、項目の増減は前の版の同じ実体と比べる
}

#[test]
fn コマンドでもcheck_baseが同じように止める() {
    let t = Tmp::new("base");
    let (base, new) = (t.0.join("v1.geas"), t.0.join("new.geas"));
    std::fs::write(&base, v1()).unwrap();
    std::fs::write(&new, v2("")).unwrap();
    let r = Command::new(BIN).env("GEAS_LANG", "ja").arg("check").arg(&new).arg("--base").arg(&base).output().unwrap();
    assert_eq!(r.status.code(), Some(1));
    std::fs::write(&new, v3(M3, M2)).unwrap();
    let r = Command::new(BIN).env("GEAS_LANG", "ja").arg("check").arg(&new).arg("--base").arg(&base).output().unwrap();
    assert_eq!(r.status.code(), Some(0), "{}{}", String::from_utf8_lossy(&r.stdout), String::from_utf8_lossy(&r.stderr));
}

#[test]
fn 実体の改名と削除の検査() {
    assert!(errors(&v4(M4, "Package")).is_empty(), "{:?}", errors(&v4(M4, "Package")));
    // 定義の名前を変えていない・消していない
    assert!(has(&v4(M4, "Parcel"), "実体「Parcel」は、今も定義にあります。名前を変えたなら、実体の定義も「Package」にしてください"));
    assert!(has(&v4(&M4.replace("drop entity Coupon", "drop entity Package"), "Package"), "消すなら、実体の定義も消してください"));
    assert!(has(&v4(&M4.replace("to Package", "to Parcel"), "Parcel"), "新しい名前が、前と同じです"));
    assert!(has(&v4(&M4.replace("to Package", "to Packet"), "Package"), "実体「Packet」が、今の定義にありません"));
    // 実体の名前を変えたあとの版では、項目の手順も新しい名前で書く
    assert!(errors(&v4(&format!("{}\nmigration 5 {{\n  drop Package.old\n}}\n", M4), "Package")).is_empty());
    assert!(has(&v4(&format!("{}\nmigration 5 {{\n  drop Parcel.old\n}}\n", M4), "Package"), "実体「Parcel」がありません"));
    // entity を挟まない書き方は、項目の手順と取り違えないよう、直し方を出す
    assert!(parse_error(&v4(&M4.replace("rename entity Parcel", "rename Parcel"), "Package")).contains("rename entity Parcel to 新しい名前"));
    assert!(parse_error(&v4(&M4.replace("drop entity Coupon", "drop Coupon"), "Package")).contains("drop entity Coupon"));
}

// ---- 組んで動かす ----
fn target_dir() -> PathBuf {
    std::env::temp_dir().join("geas-migration-target")
}

/// v3・v4 のアプリをサーバーまで組み、実行ファイルを返す（版ごとに 1 回）
fn build_exe(tag: &str, src: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("geas-migration-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let code = d.join(format!("{}.geas", tag));
    std::fs::write(&code, src).unwrap();
    let r = Command::new(BIN).env("GEAS_LANG", "ja").arg("rust").arg(&code).arg("-o").arg(d.join("app")).args(["--host", "--runtime"]).output().unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let b = Command::new("cargo").args(["build", "--release", "--quiet"]).env("CARGO_TARGET_DIR", target_dir()).current_dir(d.join("app").join("server")).output().unwrap();
    assert!(b.status.success(), "{}", tail(&String::from_utf8_lossy(&b.stderr), 2500));
    let exe = d.join("geas_server");
    std::fs::copy(target_dir().join("release").join("geas_server"), &exe).unwrap();
    exe
}

/// 組みは版ごとに 1 回で、同じ target を使うので、組みと取り出しを直列にする
fn built3() -> &'static Path {
    static B: OnceLock<PathBuf> = OnceLock::new();
    B.get_or_init(|| {
        let _g = BUILD.lock().unwrap_or_else(|e| e.into_inner());
        build_exe("v3", &v3(M3, M2))
    })
}

fn built4() -> &'static Path {
    static B: OnceLock<PathBuf> = OnceLock::new();
    B.get_or_init(|| {
        let _g = BUILD.lock().unwrap_or_else(|e| e.into_inner());
        build_exe("v4", &v4(M4, "Package"))
    })
}

static BUILD: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn old() -> Value {
    json!({"parcel_id": "p-1", "kind": "light", "fee": 300, "status": "Received", "refund_ref": null}) // 版 1 の行（重さとメモが無い）
}

fn with(base: &Value, extra: Value) -> Value {
    let mut b = base.clone();
    for (k, v) in extra.as_object().unwrap() {
        b[k] = v.clone();
    }
    b
}

fn now() -> Value {
    with(&old(), json!({"weight": 5, "note": null})) // 版 3 の行
}

/// 前の版のアプリが作った DB（小包が 1 行）。version を渡さなければ、版を覚えていない DB（版 1 として扱われる）
fn a_db(path: &Path, row: &Value, version: Option<i64>) {
    let c = rusqlite::Connection::open(path).unwrap();
    c.execute("CREATE TABLE \"Parcel\" (k TEXT PRIMARY KEY, v TEXT NOT NULL)", []).unwrap();
    c.execute("INSERT INTO \"Parcel\" VALUES (?, ?)", rusqlite::params![row["parcel_id"].as_str().unwrap(), row.to_string()]).unwrap();
    if let Some(v) = version {
        c.execute("CREATE TABLE geas_kv (k TEXT PRIMARY KEY, v INTEGER NOT NULL)", []).unwrap();
        c.execute("INSERT INTO geas_kv VALUES ('schema_version', ?)", [v]).unwrap();
    }
}

/// 起動して、すぐ止める（起動時の点検と移行までを行って待ち受けに入る。入ったら止める）。(終了コード, stderr)
fn start(exe: &Path, db: &Path) -> (i32, String) {
    let mut c = Command::new(exe).args(["--port", "0", "--db"]).arg(db).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
    let mut se = c.stderr.take().unwrap();
    let h = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut se, &mut s);
        s
    });
    let until = Instant::now() + Duration::from_secs(4);
    let code = loop {
        if let Some(st) = c.try_wait().unwrap() {
            break st.code().unwrap_or(-1);
        }
        if Instant::now() > until {
            let _ = c.kill();
            let _ = c.wait();
            break 0; // 待ち受けに入った
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    (code, h.join().unwrap())
}

fn state(db: &Path) -> (std::collections::BTreeMap<String, Value>, Option<i64>) {
    let c = rusqlite::Connection::open(db).unwrap();
    let rows = c.prepare("SELECT k, v FROM \"Parcel\"").unwrap().query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).unwrap().map(|x| x.unwrap()).map(|(k, v)| (k, serde_json::from_str(&v).unwrap())).collect();
    let has_kv: i64 = c.query_row("SELECT count(*) FROM sqlite_master WHERE name = 'geas_kv'", [], |r| r.get(0)).unwrap();
    let v = if has_kv > 0 { c.query_row("SELECT v FROM geas_kv WHERE k = 'schema_version'", [], |r| r.get(0)).ok() } else { None };
    (rows, v)
}

fn one(k: &str, v: Value) -> std::collections::BTreeMap<String, Value> {
    [(k.to_string(), v)].into_iter().collect()
}

#[test]
fn 版1のdbは起動時に今の形になり2回目は何もしない() {
    let t = Tmp::new("m1");
    let db = t.0.join("v1.sqlite");
    a_db(&db, &with(&old(), json!({"legacy": 1})), None);
    assert_eq!(start(built3(), &db).0, 0);
    assert_eq!(state(&db), (one("p-1", now()), Some(3))); // 重さが入り、メモは note で空、legacy は消えた
    rusqlite::Connection::open(&db).unwrap().execute("UPDATE \"Parcel\" SET v = replace(v, '\"weight\":5', '\"weight\":7')", []).unwrap(); // 移行のあとに変えた値は、次の起動で初期値に戻らない
    assert_eq!(start(built3(), &db).0, 0);
    assert_eq!(state(&db), (one("p-1", with(&now(), json!({"weight": 7}))), Some(3)));
}

#[test]
fn 版2のdbは足りない版だけが当たる() {
    let t = Tmp::new("m2");
    let db = t.0.join("v2.sqlite");
    a_db(&db, &with(&old(), json!({"weight": 9, "memo": "hi", "legacy": 1})), Some(2));
    assert_eq!(start(built3(), &db).0, 0);
    assert_eq!(state(&db), (one("p-1", with(&old(), json!({"weight": 9, "note": "hi"}))), Some(3))); // 版 2 の add は当たらない（重さは 9 のまま）。メモの値は note に移る
}

#[test]
fn 新しいdbは今の版として始まる() {
    let t = Tmp::new("m3");
    let db = t.0.join("new.sqlite");
    assert_eq!(start(built3(), &db).0, 0);
    assert_eq!(state(&db), (Default::default(), Some(3)));
}

#[test]
fn 合わない行があれば何も変えずに止まる() {
    let t = Tmp::new("m4");
    let bad = with(&old(), json!({"fee": -1, "legacy": 1}));
    let db = t.0.join("bad.sqlite");
    a_db(&db, &bad, None);
    let (code, err) = start(built3(), &db);
    assert!(code != 0 && err.contains("今の保存の形に合わせられません") && err.contains("Parcel.fee=-1 は範囲外") && err.contains("DB は変えていません"), "{}", tail(&err, 800));
    assert_eq!(state(&db), (one("p-1", bad), None));
    // 前の名前と新しい名前の両方がある行は、どちらの値を残すか決まらない
    let both = with(&old(), json!({"weight": 1, "memo": "a", "note": "b"}));
    let db = t.0.join("both.sqlite");
    a_db(&db, &both, Some(2));
    let (code, err) = start(built3(), &db);
    assert!(code != 0 && err.contains("前の名前「memo」と新しい名前「note」の両方があります"), "{}", tail(&err, 800));
    assert_eq!(state(&db), (one("p-1", both), Some(2)));
}

#[test]
fn このアプリより新しい版のdbは開かない() {
    let t = Tmp::new("m5");
    let db = t.0.join("newer.sqlite");
    a_db(&db, &now(), Some(4));
    let (code, err) = start(built3(), &db);
    assert!(code != 0 && err.contains("版 4") && err.contains("より新しい"), "{}", tail(&err, 800));
    assert_eq!(state(&db).1, Some(4));
}

#[test]
fn dropを書かずに消した項目は行に残ったままになる() {
    // drop が要る理由。知らない項目のある行も読めるので、migration に書かずに項目を消すと、起動は通り、値は行に残る。
    // だから check --base は、書いていない削除をエラーにする
    let t = Tmp::new("m6");
    let row = with(&now(), json!({"old_field": "残る値"}));
    let db = t.0.join("left.sqlite");
    a_db(&db, &row, Some(3));
    assert_eq!(start(built3(), &db).0, 0);
    assert_eq!(state(&db), (one("p-1", row), Some(3)));
}

fn tables(db: &Path) -> std::collections::BTreeMap<String, Value> {
    let c = rusqlite::Connection::open(db).unwrap();
    let names: Vec<String> = c.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'geas_%' AND name NOT LIKE 'sqlite_%'").unwrap().query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect();
    let mut out = std::collections::BTreeMap::new();
    for n in names {
        let rows: serde_json::Map<String, Value> = c.prepare(&format!("SELECT k, v FROM \"{}\"", n)).unwrap().query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).unwrap().map(|x| x.unwrap()).map(|(k, v)| (k, serde_json::from_str(&v).unwrap())).collect();
        out.insert(n, Value::Object(rows));
    }
    let kv: serde_json::Map<String, Value> = c.prepare("SELECT k, v FROM geas_kv").unwrap().query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).unwrap().map(|x| x.unwrap()).map(|(k, v)| (k, json!(v))).collect();
    out.insert("#kv".into(), Value::Object(kv));
    out
}

#[test]
fn 実体の改名と削除は起動時に行ごと移り消える() {
    let t = Tmp::new("m7");
    let db = t.0.join("v3.sqlite");
    a_db(&db, &now(), Some(3));
    {
        let c = rusqlite::Connection::open(&db).unwrap();
        c.execute("CREATE TABLE \"Coupon\" (k TEXT PRIMARY KEY, v TEXT NOT NULL)", []).unwrap();
        c.execute("INSERT INTO \"Coupon\" VALUES ('c-1', '{\"coupon_id\": \"c-1\"}')", []).unwrap();
        c.execute("INSERT INTO geas_kv VALUES ('id:Parcel', 7), ('id:Coupon', 2)", []).unwrap();
    }
    assert_eq!(start(built4(), &db).0, 0);
    let tb = tables(&db);
    assert!(tb["Package"] == json!({"p-1": now()}) && !tb.contains_key("Parcel") && !tb.contains_key("Coupon")); // 行は新しい名前の表へ移り、前の表は消える
    assert_eq!(tb["#kv"], json!({"schema_version": 4, "id:Package": 7})); // 採番の続きも引き継ぐ（消した実体のぶんは消える）
    assert!(start(built4(), &db).0 == 0 && tables(&db) == tb); // 2 回目は何もしない
}

#[test]
fn 版を覚えていない古いdbも前の名前の表から移る() {
    let t = Tmp::new("m8");
    let db = t.0.join("v1.sqlite");
    a_db(&db, &with(&old(), json!({"legacy": 1})), None);
    assert_eq!(start(built4(), &db).0, 0);
    let tb = tables(&db);
    assert!(tb["Package"] == json!({"p-1": now()}) && !tb.contains_key("Parcel") && tb["#kv"]["schema_version"] == 4);
}

#[test]
fn 実体の移行も合わない行があれば何も変えずに止まる() {
    let t = Tmp::new("m9");
    let bad = with(&now(), json!({"fee": -1}));
    let db = t.0.join("bad.sqlite");
    a_db(&db, &bad, Some(3));
    let (code, err) = start(built4(), &db);
    assert!(code != 0 && err.contains("今の保存の形に合わせられません") && err.contains("DB は変えていません"), "{}", tail(&err, 800));
    let c = rusqlite::Connection::open(&db).unwrap();
    let v: String = c.query_row("SELECT v FROM \"Parcel\" WHERE k = 'p-1'", [], |r| r.get(0)).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&v).unwrap(), bad);
    let ver: i64 = c.query_row("SELECT v FROM geas_kv WHERE k = 'schema_version'", [], |r| r.get(0)).unwrap();
    assert_eq!(ver, 3);
}
