// SPDX-License-Identifier: GPL-3.0-or-later
//! 検査の指摘に、AI が直すための手がかりを付ける（hints.py の移し）。指摘の種類ごとに、見る節と直し方を返す。
use crate::check::{Diag, ERROR, QUESTION, WARN};
use crate::error::Error;
use serde_json::{json, Map, Value};

pub const REF: &str = "docs/language-reference.md";

/// 種類 → (見る節, 直し方)
pub fn table(code: &str) -> (&'static str, &'static str) {
    match code {
        "型" => ("§2 型", "式や項目の型をそろえる。範囲のある型（ltype）に入る根拠（上限の確認や範囲）が示せないときは、確認の文を足すか範囲を広げる"),
        "名前" => ("§1 全体の決まり", "名前は半角英数にし、重複や未定義を直す。日本語の表示名は直前の /// に書く"),
        "画面" => ("§9 画面（screen）", "画面の入力・遷移・表示する項目が、処理と entity に実在するか確かめる"),
        "リソース" => ("§10 REST（resource）", "処理と問い合わせをどれかの resource の7アクションに割り当てる（割り当てがないものがある）"),
        "トリガ" => ("§6 外とのやりとり", "トリガの種類・期限・冪等キーなど、宣言が足りない所を書き足す"),
        "計算" => ("§5 処理", "足す・引く・掛けるの途中の値が、計算に使う幅に収まると示せない。型に範囲を書くか、先に if で大きさを確かめる。人が引き受けるなら、文の末尾に `trust check|verify|assume \"理由\"`"),
        "範囲" => ("§10 REST（resource）", "scope 名前 on データ { where 条件 ／ filter 項目 equals caller } で範囲（行の集合）を宣言し、問い合わせの `from データ.範囲1.範囲2` で使う（並べると、すべてに入る行）。本人で絞る範囲を使う問い合わせを呼ぶ動作には、役割（for 役割）が要る"),
        "行き先" => ("§9 画面（screen）", "遷移先の画面・結果が実在するか、すべての結果に行き先があるか確かめる"),
        "遷移" => ("§3 データと状態", "状態の遷移の宣言（transition）と、処理の中の状態の書き換えを一致させる"),
        "単位" => ("§2 型", "金額の通貨や単位が合わない足し算・比較を直す（同じ通貨にそろえるか、変換を書く）"),
        "削除" => ("§3 データと状態", "delete の参照元の扱い（detach など）と、削除できる状態（removable）を宣言する"),
        "null" => ("§2 型", "無いかもしれない項目を使っている。required を付けるか、存在の確認（else の分岐）を書く"),
        "並列" => ("§7 フローと出来事", "par の枝どうしで同じものを書き換えない／合流の宣言を足す"),
        "丸め" => ("§2 型", "割り算・割合の丸め方（floor／ceil／half_up）を決めて書く。決めるのは人（問い）"),
        "宣言" => ("§4 処理（deed）", "reads・writes・touches などの宣言を、処理の中身と一致させる"),
        "合流" => ("§7 フローと出来事", "待ち合わせ（join）の条件と、合流する枝を揃える"),
        "出来事" => ("§7 フローと出来事", "出来事（event）の発生元と受け手を宣言する"),
        "冪等" => ("§6 外とのやりとり", "外への書き込みに冪等キーを付けるか、冪等でないことを宣言する"),
        "不変条件" => ("§3 データと状態", "処理が不変条件を壊さないと示せない。確認の文を足すか、不変条件を見直す（見直すのは人）"),
        "結果" => ("§4 処理（deed）", "処理の結果の種類と、呼び出し側が扱う種類を一致させる"),
        "応答" => ("§10 REST（resource）", "結果の種類に HTTP ステータスを割り当てる"),
        "失敗" => ("§4 処理（deed）", "失敗しうる式の結果を、else などで扱う"),
        "保証" => ("§6 外とのやりとり", "trust の段階と理由を書く。理由は人の保証として仕様書に出る"),
        "状態" => ("§3 データと状態", "状態の項目と遷移を揃える"),
        "項目" => ("§3 データと状態", "存在しない項目を参照している。entity の定義を確かめる"),
        "存在" => ("§4 処理（deed）", "見つからないときの扱いを決める（決めるのは人。問い）"),
        "書込" => ("§4 処理（deed）", "書き込みの適用順・上書きを確かめる"),
        "検査数字" => ("§2 型", "検査数字の桁の長さ・並びを確かめる"),
        "検査" => ("§2 型", "検査の形（check）の引数を確かめる"),
        "文字種" => ("§2 型", "文字種の指定を確かめる"),
        "割り算" => ("§2 型", "割る数が 1 以上と示せるか、`x / d or 0` で 0 のときの値を書く"),
        "並び" => ("§2 型", "並びの指定を確かめる"),
        "モジュール" => ("§1 全体の決まり", "use／module の参照先を確かめる（docs/modules.md）"),
        "データ" => ("§3 データと状態", "entity の定義を確かめる"),
        "直列" => ("§3 データと状態", "storage（保存先）の定義と、entity の store 行を揃える"),
        "rule" => ("§5 slot の値の決め方（rule）", "rule の書き方（条件と値）を確かめる"),
        "役割" => ("§10 REST（resource）", "actor で役割を宣言し、resource の外から呼べる動作すべてに `for 役割, …`（誰でもよい口は `for anyone`）を書く"),
        "移行" => ("§3 データと状態", "migration の版（2 から 1 ずつ）と手順（add・rename・drop）を、今の実体の項目に合わせる。add の値は、項目の型と範囲に合わせる"),
        "名前そのもの" => ("§1 全体の決まり", "名前を直す"),
        "表示名" => ("§1 全体の決まり", "直前の /// に日本語の表示名を書く"),
        "外部作用" => ("§6 外とのやりとり", "外への作用の宣言（trust・冪等）を足す"),
        "通信" => ("§6 外とのやりとり（effect の over http）", "通信先を https://ホスト名/パス にし、認証は needs secret で宣言し、返事の状態をすべて結果の種類に結ぶ（status other が必須）"),
        "データ用フォルダ" => ("§9 データ用フォルダ（data_folder）", "absolute の中の行は、その OS のパスの書き方の絶対パス。relative は相対パス（exe のあるフォルダが起点）。必ずあるとは限らない環境変数は needs env で宣言する"),
        "ファイル" => ("§6 外部作用の「ファイル操作（over file）」", "場所は path 型の引数の下の相対パス 1 つ。操作は write（新規だけ）・overwrite・read・delete のどれか 1 つ。ok と failed は必須。結果が gives を持つなら take で取り出す"),
        "常駐" => ("§9 画面（screen）の「常駐（resident）」", "メニューは、入口の画面（引数なし）を開く項目だけ。tooltip は 64 文字まで。終了の項目は言語が足す"),
        "環境" => ("§4 処理（deed）の「動かす前提 needs」", "needs の書き方（secret・exposure・instances と理由）を直す。環境の実際は書かない"),
        _ => ("§11 検査の結果の種類", "指摘の文言を読み、該当する節を language-reference.md で確かめる"),
    }
}

pub fn hint_of(d: &Diag) -> Map<String, Value> {
    let (sec, fix) = table(&d.code);
    let mut m = Map::new();
    m.insert("ref".into(), json!(format!("{} {}", REF, sec)));
    m.insert("fix".into(), json!(fix));
    m
}

/// 1件の指摘。where と message は、コードに書いた名前のまま。line は分かるときだけ
pub fn as_dict(d: &Diag) -> Value {
    let level = if d.level == ERROR {
        "error"
    } else if d.level == WARN {
        "warning"
    } else if d.level == QUESTION {
        "question"
    } else {
        d.level.as_str()
    };
    let mut out = Map::new();
    out.insert("level".into(), json!(level));
    out.insert("code".into(), json!(d.code));
    out.insert("where".into(), json!(if d.code_where.is_empty() { &d.where_ } else { &d.code_where }));
    out.insert("message".into(), json!(if d.code_msg.is_empty() { &d.msg } else { &d.code_msg }));
    if d.line != 0 {
        out.insert("line".into(), json!(d.line));
    }
    if d.level != WARN || d.code != "表示名" {
        let mut h = hint_of(d);
        if d.level == QUESTION {
            let fix = h["fix"].as_str().unwrap_or("").to_string();
            h.insert("fix".into(), json!(format!("人に1問ずつ聞いて決める（skills/geas-develop）。答えをコードに反映して再検査する。{}", fix)));
        }
        out.insert("hint".into(), Value::Object(h));
    }
    Value::Object(out)
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap() + "\n"
}

pub fn to_json(ok: bool, ready: bool, diags: &[Diag], path: &str) -> String {
    pretty(&json!({"file": path, "ok": ok, "ready": ready, "diagnostics": diags.iter().map(as_dict).collect::<Vec<_>>()}))
}

/// 構文エラー（検査まで進めない）の JSON
pub fn syntax_json(path: &str, e: &Error) -> String {
    let (msg, line, col, file, hint) = match e {
        Error::Lex(x) => (x.msg.clone(), x.line, x.col, x.file.clone(), String::new()),
        Error::Parse(x) => (x.msg.clone(), x.line, x.col, x.file.clone(), x.hint.clone()),
        Error::Module(x) => (x.msg.clone(), x.line, 0, x.file.clone(), String::new()),
    };
    let fix = if hint.is_empty() { "メッセージの行を、近い例（tests/fixtures/overseas_shop/）と見比べて書き方を直す".to_string() } else { hint };
    let mut diag = Map::new();
    diag.insert("level".into(), json!("syntax"));
    diag.insert("code".into(), json!("構文"));
    diag.insert("where".into(), json!(if file.is_empty() { path.to_string() } else { file }));
    diag.insert("message".into(), json!(if msg.is_empty() { e.to_string() } else { msg }));
    diag.insert("hint".into(), json!({"ref": format!("{} §1 全体の決まり", REF), "fix": fix}));
    if line != 0 {
        diag.insert("line".into(), json!(line));
    }
    if col != 0 {
        diag.insert("col".into(), json!(col));
    }
    pretty(&json!({"file": path, "ok": false, "ready": false, "diagnostics": [Value::Object(diag)]}))
}
