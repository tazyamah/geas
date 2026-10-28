# SPDX-License-Identifier: GPL-3.0-or-later
"""削除（delete／keep／removable／on delete）：検査（問いになる・エラー）と、生成した Rust の動き"""
import shutil
import subprocess
from pathlib import Path

import pytest

from geas.check import ERROR, QUESTION, check
from geas.examples import write_crate
from geas.parser import parse
from geas.rustgen import generate

SRC = (Path(__file__).parent / "fixtures" / "delete_demo" / "delete_demo.geas").read_text()


def chk(src):
    return check(parse(src), strict_names=True)


def msgs(ck, level):
    return [d.msg for d in ck.diags if d.level == level]


def mutate(src, old, new):
    assert old in src, old
    return src.replace(old, new, 1)


def test_削除の例は検査を通る():
    ck = chk(SRC)
    assert ck.ok and not msgs(ck, QUESTION), ck.diags


def test_参照元の扱いが決まっていないと問いになる():
    ck = chk(mutate(SRC, "  on delete Note.author → detach\n", ""))
    assert any("参照が残ります" in m for m in msgs(ck, QUESTION))


def test_状態を持つデータは消してよい状態が無いと問いになる():
    ck = chk(mutate(SRC, "  removable in Draft, Archived\n", ""))
    assert any("消してよい状態を決めてください" in m for m in msgs(ck, QUESTION))


def test_消してよい状態だと確かめていないと問いになる():
    ck = chk(mutate(SRC, "  if note.status != Archived { return NotArchived }\n", ""))
    assert any("確かめていません" in m for m in msgs(ck, QUESTION))


def test_keep_のデータは消せない():
    ck = chk(mutate(SRC, "  removable in Draft, Archived\n", "  keep\n"))
    assert any("keep" in m and "消せません" in m for m in msgs(ck, ERROR))
    assert any("両方は書けません" in m for m in msgs(chk(mutate(SRC, "  removable in Draft, Archived\n", "  keep\n  removable in Draft\n")), ERROR))


def test_detach_は無いかもしれない参照だけ():
    ck = chk(mutate(SRC, "  author: ref Customer\n", "  author: ref Customer required\n"))
    assert any("detach" in m for m in msgs(ck, ERROR))


def test_on_delete_の先が違うとエラー():
    assert any("参照する項目ではありません" in m for m in msgs(chk(mutate(SRC, "Note.author → detach", "Note.body → detach")), ERROR))
    assert any("どの状態にもありません" in m for m in msgs(chk(mutate(SRC, "removable in Draft, Archived", "removable in Draft, Nothing")), ERROR))


@pytest.mark.skipif(shutil.which("cargo") is None, reason="cargo が無い")
def test_生成した削除は参照元を扱い_消してよい状態だけ消す(tmp_path):
    main = """use geas_app::*;
fn main() {
    let mut s = Store::default();
    s.Customer.insert("c1".into(), Customer { customer_id: "c1".into() });
    s.Note.insert("n1".into(), Note { note_id: "n1".into(), author: Some("c1".into()), body: "x".into(), status: NoteStatus::Draft });
    s.Note.insert("n2".into(), Note { note_id: "n2".into(), author: Some("c1".into()), body: "y".into(), status: NoteStatus::Archived });
    // 下書きのメモは、保管していないので消せない。保管済みは消せる
    let o = delete_note(&delete_noteCtx {}, &s, &delete_noteInput { note_id: "n1".into() });
    assert_eq!(o.result, DeleteNoteResult::NotArchived);
    assert!(o.Write.is_empty());
    let o = delete_note(&delete_noteCtx {}, &s, &delete_noteInput { note_id: "n2".into() });
    assert_eq!(o.result, DeleteNoteResult::Deleted);
    s.apply(o.Write);
    assert!(!s.Note.contains_key("n2"));
    // ユーザーを消すと、メモは残り、書き手だけ空になる
    let o = delete_customer(&delete_customerCtx {}, &s, &delete_customerInput { customer_id: "c1".into() });
    s.apply(o.Write);
    assert!(s.Customer.is_empty());
    assert_eq!(s.Note["n1"].author, None);
    let o = delete_customer(&delete_customerCtx {}, &s, &delete_customerInput { customer_id: "c1".into() });
    assert_eq!(o.result, DeleteResult::NotFound);
    println!("ok");
}
"""
    ck = chk(SRC)
    write_crate(ck, generate(ck), main, tmp_path)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    assert r.stdout.strip() == "ok"
