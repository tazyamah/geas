// SPDX-License-Identifier: GPL-3.0-or-later
//! 言語機能ごとの動作テスト（削除）：delete／keep／removable／on delete の検査（問い・エラー）と、生成した Rust の動き。
//! どの機能にどのテストがあるかは docs/language-test-map.md。（Python 版 tests/test_delete.py の移し）
mod common;
use common::*;

fn src() -> String {
    fixture("delete_demo/delete_demo.geas")
}

fn mutate(s: &str, old: &str, new: &str) -> String {
    assert!(s.contains(old), "{}", old);
    s.replacen(old, new, 1)
}

#[test]
fn 削除の例は検査を通る() {
    let d = diags(&src());
    assert!(d.iter().all(|(l, _)| l != "エラー" && l != "問い"), "{:?}", d);
}

#[test]
fn 参照元の扱いが決まっていないと問いになる() {
    assert!(questions(&mutate(&src(), "  on delete Note.author → detach\n", "")).iter().any(|m| m.contains("参照が残ります")));
}

#[test]
fn 状態を持つデータは消してよい状態が無いと問いになる() {
    assert!(questions(&mutate(&src(), "  removable in Draft, Archived\n", "")).iter().any(|m| m.contains("消してよい状態を決めてください")));
}

#[test]
fn 消してよい状態だと確かめていないと問いになる() {
    assert!(questions(&mutate(&src(), "  if note.status != Archived { return NotArchived }\n", "")).iter().any(|m| m.contains("確かめていません")));
}

#[test]
fn keepのデータは消せない() {
    let s = src();
    assert!(errors(&mutate(&s, "  removable in Draft, Archived\n", "  keep\n")).iter().any(|m| m.contains("keep") && m.contains("消せません")));
    assert!(errors(&mutate(&s, "  removable in Draft, Archived\n", "  keep\n  removable in Draft\n")).iter().any(|m| m.contains("両方は書けません")));
}

#[test]
fn detachは無いかもしれない参照だけ() {
    assert!(errors(&mutate(&src(), "  author: ref Customer\n", "  author: ref Customer required\n")).iter().any(|m| m.contains("detach")));
}

#[test]
fn on_deleteの先が違うとエラー() {
    let s = src();
    assert!(errors(&mutate(&s, "Note.author → detach", "Note.body → detach")).iter().any(|m| m.contains("参照する項目ではありません")));
    assert!(errors(&mutate(&s, "removable in Draft, Archived", "removable in Draft, Nothing")).iter().any(|m| m.contains("どの状態にもありません")));
}

#[test]
fn 生成した削除は参照元を扱い消してよい状態だけ消す() {
    let main = r#"use geas_app::*;
fn main() {
    let mut s = Store::default();
    s.Customer.insert("c1".into(), Customer { customer_id: "c1".into() });
    s.Note.insert("n1".into(), Note { note_id: "n1".into(), author: Some("c1".into()), body: "x".into(), status: NoteStatus::Draft });
    s.Note.insert("n2".into(), Note { note_id: "n2".into(), author: Some("c1".into()), body: "y".into(), status: NoteStatus::Archived });
    // 下書きのメモは、保管していないので消せない。保管済みは消せる
    let o = delete_note(&delete_noteCtx {}, &s, &delete_noteInput { note_id: "n1".into() }).unwrap();
    assert_eq!(o.result, DeleteNoteResult::NotArchived);
    assert!(o.Write.is_empty());
    let o = delete_note(&delete_noteCtx {}, &s, &delete_noteInput { note_id: "n2".into() }).unwrap();
    assert_eq!(o.result, DeleteNoteResult::Deleted);
    s.apply(o.Write).unwrap();
    assert!(!s.Note.contains_key("n2"));
    // ユーザーを消すと、メモは残り、書き手だけ空になる
    let o = delete_customer(&delete_customerCtx {}, &s, &delete_customerInput { customer_id: "c1".into() }).unwrap();
    s.apply(o.Write).unwrap();
    assert!(s.Customer.is_empty());
    assert_eq!(s.Note["n1"].author, None);
    let o = delete_customer(&delete_customerCtx {}, &s, &delete_customerInput { customer_id: "c1".into() }).unwrap();
    assert_eq!(o.result, DeleteResult::NotFound);
    println!("ok");
}
"#;
    assert_eq!(run_app(&src(), main), strs(&["ok"]));
}
