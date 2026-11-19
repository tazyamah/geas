// SPDX-License-Identifier: GPL-3.0-or-later
//! 初回の対話で仕様を作るための台帳（Python 版 interview.py の移し。会話の進め方は skills/plan-interview/SKILL.md）。
use serde_json::{json, Map, Value};

pub const RUNTIME: [(&str, &str); 7] = [
    ("R1", "動作するプラットフォーム（スマホ／PC／ブラウザ／サーバ／組み込み）"),
    ("R2", "使う場所と接続（常時オンライン／ときどきオフライン／完全オフライン）"),
    ("R3", "データを残すか、どこに（残さない／端末内／サーバのDB／既存のDB）"),
    ("R4", "利用者の数と同時利用"),
    ("R5", "外部とのつながり（決済、メール、外部API、認証基盤など）"),
    ("R6", "配る方法（自分だけ／他の人へ配る／サーバに配備）"),
    ("R7", "画面の有無と種類"),
];
const LEVELS: [&str; 3] = ["要約", "利用者目標", "下位機能"];
const STATUSES: [&str; 3] = ["open", "unspec", "answered"];
const KINDS: [&str; 4] = ["runtime", "constraint", "quality", "other"];

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map_or(true, |f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn ps(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        o => o.to_string(),
    }
}

/// d.get(k, "") を文字列に
fn gs(d: &Value, k: &str) -> String {
    ps(d.get(k).unwrap_or(&Value::Null))
}

fn list<'a>(d: &'a Value, k: &str) -> &'a [Value] {
    d.get(k).and_then(|v| v.as_array()).map(|a| a.as_slice()).unwrap_or(&[])
}

pub fn runtime_status(r: &Value) -> String {
    let s = r.get("status").cloned().unwrap_or(Value::Null);
    if truthy(&s) {
        return ps(&s);
    }
    if truthy(r.get("value").unwrap_or(&Value::Null)) { "answered".into() } else { "open".into() }
}

pub fn new_ledger(title: &str) -> Value {
    json!({"title": title, "summary": "", "out_of_scope": [], "usecases": [], "questions": [], "extras": [], "runtime": {}, "screens": [], "judgments": []})
}

pub fn fill_defaults(led: &mut Value) {
    if let (Value::Object(m), Value::Object(d)) = (led, new_ledger("")) {
        for (k, v) in d {
            m.entry(k).or_insert(v);
        }
    }
}

fn usecase_gaps(u: &Value) -> Vec<String> {
    let mut g = vec![];
    if !truthy(u.get("main").unwrap_or(&Value::Null)) {
        g.push("主な流れ（成功の道）が未記入".to_string());
    }
    if !truthy(u.get("extensions").unwrap_or(&Value::Null)) {
        g.push("失敗・例外・分岐（拡張）が未記入".into());
    }
    if !truthy(u.get("examples").unwrap_or(&Value::Null)) {
        g.push("具体例が未記入".into());
    }
    if !LEVELS.contains(&u.get("level").and_then(|v| v.as_str()).unwrap_or("\u{0}")) {
        g.push(format!("ユースケースの粒度が未記入（{}）", LEVELS.join("／")));
    }
    g
}

pub fn status(led: &Value) -> (Vec<String>, bool) {
    let mut todo: Vec<String> = vec![];
    if !truthy(&led["summary"]) {
        todo.push("作るものの一言（summary）".into());
    }
    if !truthy(&led["out_of_scope"]) {
        todo.push("範囲外（out_of_scope）。無ければ「なし」と記入".into());
    }
    let ucs = list(led, "usecases");
    if ucs.is_empty() {
        todo.push("アクターとユースケースの一覧（usecases）。「他に使う人は？ 他にやりたいことは？」と聞く".into());
    }
    for u in ucs {
        for g in usecase_gaps(u) {
            todo.push(format!("{} {}: {}", gs(u, "id"), gs(u, "goal"), g));
        }
    }
    for q in list(led, "questions") {
        let st = q.get("status").map(ps).unwrap_or_else(|| "open".into());
        if !STATUSES.contains(&st.as_str()) {
            todo.push(format!("問い {} の status「{}」が不正です（{}）", gs(q, "id"), st, STATUSES.join("／")));
        } else if st == "open" {
            todo.push(format!("未解決の問い {}: {}", gs(q, "id"), gs(q, "text")));
        } else if !truthy(q.get("answer").unwrap_or(&Value::Null)) {
            todo.push(format!("問い {}: {} なのに答え（未規定なら理由）が空です", gs(q, "id"), st));
        }
    }
    for x in list(led, "extras") {
        match x.get("kind") {
            None | Some(Value::Null) => todo.push(format!("付加情報 {} の仕分け（人に確かめる）: {}", gs(x, "id"), gs(x, "text"))),
            Some(k) if !KINDS.contains(&ps(k).as_str()) => todo.push(format!("付加情報 {} の kind「{}」が不正です（{}）", gs(x, "id"), ps(k), KINDS.join("／"))),
            _ => {}
        }
    }
    if !ucs.is_empty() && todo.is_empty() {
        for (k, text) in RUNTIME {
            let r = led["runtime"].get(k).cloned().unwrap_or_else(|| json!({}));
            let st = runtime_status(&r);
            if !STATUSES.contains(&st.as_str()) {
                todo.push(format!("動作要件 {} の status「{}」が不正です", k, st));
            } else if st == "open" {
                todo.push(format!("動作要件 {}: {}", k, text));
            } else if k == "R1" && st == "unspec" {
                todo.push("動作要件 R1（動作プラットフォーム）は未規定にできません。確定させてください".into());
            } else if st == "answered" && !truthy(r.get("value").unwrap_or(&Value::Null)) {
                todo.push(format!("動作要件 {}: answered なのに値が空です", k));
            }
        }
    }
    if !ucs.is_empty() && todo.is_empty() {
        let r7 = led["runtime"].get("R7").cloned().unwrap_or_else(|| json!({}));
        if runtime_status(&r7) == "answered" && gs(&r7, "value").contains("画面") && !truthy(led.get("screens").unwrap_or(&Value::Null)) {
            todo.push("画面の動き（screens）：見せるもの、状態ごとの表示、押したらどうなるか、開いた・つながったなどの出来事で起こること".into());
        }
        for sc in list(led, "screens") {
            for (key, label) in [("parts", "見せるもの"), ("states", "状態ごとの表示"), ("actions", "押したらどうなるか")] {
                if !truthy(sc.get(key).unwrap_or(&Value::Null)) {
                    todo.push(format!("画面 {} {}: {}が未記入", gs(sc, "id"), gs(sc, "name"), label));
                }
            }
        }
    }
    let ready = !ucs.is_empty() && todo.is_empty();
    (todo, ready)
}

pub fn render(led: &Value) -> String {
    let mut out: Vec<String> = vec![];
    let mut push = |s: &str| out.push(s.to_string());
    push(&format!("# 仕様書（plan）：{}", gs(led, "title")));
    push("");
    push("会話で作ったレビュー対象の仕様書（①）の会話の部分（節番号 C0〜C5）。plan 雛形の節1〜7（データ・状態・処理・指定・具体例の表）は、この内容から、コードより前に会話で書き起こす。台帳から生成した。決まっていないことは「**未規定**」と書く。");
    push("");
    push("## C0. 概要（文章。突合しない）");
    push("");
    push(&if truthy(&led["summary"]) { gs(led, "summary") } else { "**未規定**".into() });
    push("");
    let oos: Vec<String> = list(led, "out_of_scope").iter().map(ps).collect();
    push(&format!("範囲外: {}", if oos.is_empty() { "**未規定**".to_string() } else { oos.join("、") }));
    push("");
    push("## C1. ユースケース（Cockburn。文章。コードの起こしの元）");
    push("");
    for u in list(led, "usecases") {
        let lv = if truthy(u.get("level").unwrap_or(&Value::Null)) { gs(u, "level") } else { "**未規定**".into() };
        push(&format!("### {} {}（アクター: {}／粒度: {}）", gs(u, "id"), gs(u, "goal"), gs(u, "actor"), lv));
        push("");
        push("主な流れ:");
        for (i, s) in list(u, "main").iter().enumerate() {
            push(&format!("{}. {}", i + 1, ps(s)));
        }
        push("");
        if truthy(u.get("extensions").unwrap_or(&Value::Null)) {
            push("拡張（失敗・例外・分岐）:");
            for e in list(u, "extensions") {
                push(&format!("- {}: {} → {}", gs(e, "at"), gs(e, "cond"), gs(e, "handling")));
            }
            push("");
        }
        if truthy(u.get("rules").unwrap_or(&Value::Null)) {
            push("ルール:");
            for r in list(u, "rules") {
                push(&format!("- {}", ps(r)));
            }
            push("");
        }
        if truthy(u.get("examples").unwrap_or(&Value::Null)) {
            push("具体例:");
            for e in list(u, "examples") {
                push(&format!("- 前提 {} ／ 操作 {} ／ 期待 {}", gs(e, "given"), gs(e, "when"), gs(e, "then")));
            }
            push("");
        }
    }
    push("## C2. 動かす環境（動作要件）");
    push("");
    push("| ID | 聞くこと | 答え | 来歴 |");
    push("|---|---|---|---|");
    for (k, text) in RUNTIME {
        let r = led["runtime"].get(k).cloned().unwrap_or_else(|| json!({}));
        let st = runtime_status(&r);
        let shown = if truthy(r.get("value").unwrap_or(&Value::Null)) { gs(&r, "value") } else if st == "unspec" { "**未規定**（意図的）".into() } else { "（まだ聞いていない）".into() };
        push(&format!("| {} | {} | {} | {} |", k, text, shown, gs(&r, "from")));
    }
    push("");
    if truthy(&led["extras"]) {
        push("## C3. 返答に混ざっていた付加情報（仕分け済みの置き場）");
        push("");
        push("| ID | 内容 | 出どころ | 種別 |");
        push("|---|---|---|---|");
        for x in list(led, "extras") {
            let kind = if truthy(x.get("kind").unwrap_or(&Value::Null)) { gs(x, "kind") } else { "未仕分け".into() };
            push(&format!("| {} | {} | {} | {} |", gs(x, "id"), gs(x, "text"), gs(x, "from_usecase"), kind));
        }
        push("");
    }
    push("## C4. 未解決の問い");
    push("");
    push("| ID | 問い | 状態 | 答え |");
    push("|---|---|---|---|");
    for q in list(led, "questions") {
        let st = q.get("status").map(ps).unwrap_or_else(|| "open".into());
        push(&format!("| {} | {} | {} | {} |", gs(q, "id"), gs(q, "text"), st, gs(q, "answer")));
    }
    push("");
    push("## C6. 画面の動き（見せるもの・状態ごとの表示・押したら・出来事で）");
    push("");
    for sc in list(led, "screens") {
        let dev = if sc.get("device").is_some() { gs(sc, "device") } else { "共通".into() };
        let ucs: Vec<String> = list(sc, "usecases").iter().map(ps).collect();
        push(&format!("### {} {}（{}／ユースケース: {}）", gs(sc, "id"), gs(sc, "name"), dev, ucs.join("、")));
        push("");
        let parts: Vec<String> = list(sc, "parts").iter().map(ps).collect();
        push(&format!("見せるもの: {}", parts.join("、")));
        push("");
        push("| 状態 | 見せるもの |");
        push("|---|---|");
        for x in list(sc, "states") {
            push(&format!("| {} | {} |", gs(x, "name"), gs(x, "shows")));
        }
        push("");
        push("| きっかけ | 起こること | 判断 |");
        push("|---|---|---|");
        for x in list(sc, "actions") {
            push(&format!("| 押す：{} | {} | {} |", gs(x, "when"), gs(x, "then"), gs(x, "judgment")));
        }
        for x in list(sc, "events") {
            push(&format!("| 出来事：{} | {} | {} |", gs(x, "when"), gs(x, "then"), gs(x, "judgment")));
        }
        push("");
    }
    push("");
    push("## C5. 判断の記録");
    push("");
    push("| ID | ルール（AIの解釈） | 元の発言 | 誰が | いつ |");
    push("|---|---|---|---|---|");
    for j in list(led, "judgments") {
        let src: Vec<String> = list(j, "sources").iter().map(ps).collect();
        push(&format!("| {} | {} | {} | {} | {} |", gs(j, "id"), gs(j, "rule"), src.join(" ／ "), gs(j, "who"), gs(j, "when")));
    }
    out.join("\n") + "\n"
}

pub fn empty_map() -> Map<String, Value> {
    Map::new()
}
