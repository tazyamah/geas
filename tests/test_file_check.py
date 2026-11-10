"""ファイル操作（over file）の検査：場所は path 型の引数が指す場所の下の相対パスだけ、操作ごとに書ける結果が決まっている、結果が応答の結果の種類に漏れなく結ばれる。"""
from pathlib import Path

import pytest

from geas.check import check
from geas.parser import ParseError, parse

SRC = (Path(__file__).resolve().parent / "fixtures" / "file_demo" / "file_demo.geas").read_text(encoding="utf-8")
BLOCK = '''  write "{dir}/note.txt" from content
  ok → Done take size from bytes
  exists → Refused
  not_found → Refused
  denied → Refused
  failed → Refused
'''


def errors(old, new, src=SRC):
    assert old in src, old
    return [d.msg for d in check(parse(src.replace(old, new, 1))).diags if d.level == "エラー"]


def test_見本は通る():
    assert errors(BLOCK, BLOCK) == []


def test_操作は_write_overwrite_read_delete_の4つ_1つだけ():
    assert errors(BLOCK, BLOCK.replace("write ", "overwrite ").replace("  exists → Refused\n", "")) == []
    assert errors(BLOCK, '  delete "{dir}/note.txt"\n  ok → Refused\n  not_found → Refused\n  failed → Refused\n') == []
    assert errors(BLOCK, '  read "{dir}/note.txt"\n  ok → Refused\n  failed → Refused\n') == []
    with pytest.raises(ParseError, match="1 つだけ"):
        parse(SRC.replace(BLOCK, BLOCK + '  read "{dir}/note.txt"\n', 1))
    with pytest.raises(ParseError):
        parse(SRC.replace(BLOCK, BLOCK.replace("write ", "append "), 1))


def test_場所は_path_型の引数と_相対パスだけ():
    assert any("実行時に入力されるパスの型" in m for m in errors(BLOCK, BLOCK.replace("{dir}", "{note_id}")))
    assert any("引数にありません" in m for m in errors(BLOCK, BLOCK.replace("{dir}", "{nothing}")))
    with pytest.raises(ParseError, match="{引数}/ファイル名"):
        parse(SRC.replace(BLOCK, BLOCK.replace('"{dir}/note.txt"', '"/etc/passwd"'), 1))
    for bad in ("../x.txt", "a//b.txt", "a/./b", "a\\\\b", "x:y", "a?b"):
        assert any("相対パス" in m for m in errors(BLOCK, BLOCK.replace("note.txt", bad))), bad


def test_結果は_操作ごとに書けるものが決まり_ok_と_failed_は必須():
    assert any("failed → 種類 が要ります" in m for m in errors(BLOCK, BLOCK.replace("  failed → Refused\n", "")))
    assert any("ok → 種類 が要ります" in m for m in errors(BLOCK, BLOCK.replace("  ok → Done take size from bytes\n", "")))
    assert any("exists" in m and "write のときだけ" in m for m in errors(BLOCK, BLOCK.replace("write ", "overwrite ")))
    assert any("結果の種類" in m and "ありません" in m for m in errors(BLOCK, BLOCK.replace("→ Done", "→ Nope")))


def test_take_は_ok_だけ_content_は_read_だけ_中身が要る結果は取り出す():
    assert any("取り出せるのは" in m for m in errors(BLOCK, BLOCK.replace("take size from bytes", "take size from content")))
    assert any("take は、ok だけ" in m for m in errors(BLOCK, BLOCK.replace("not_found → Refused", "not_found → Done take size from bytes")))
    assert any("take {g}".replace("{g}", "size") in m or "取り出してください" in m for m in errors(BLOCK, BLOCK.replace(" take size from bytes", "")))
    assert any("取り出せるのは" in m for m in errors(BLOCK, '  delete "{dir}/note.txt"\n  ok → Done take size from bytes\n  failed → Refused\n'))
    with pytest.raises(ParseError, match="from"):
        parse(SRC.replace("take size from bytes", "take size", 1))


def test_書く内容は文字列の引数_replies_が要る():
    assert any("別にしてください" in m for m in errors(BLOCK, BLOCK.replace("from content", "from dir")))
    assert any("引数にありません" in m for m in errors(BLOCK, BLOCK.replace("from content", "from nothing")))
    assert any("replies" in m for m in errors("replies ExportSettled over file", "over file"))


def test_生成物の定義に_操作と場所と結果が出る():
    from geas.hostgen import definitions
    for op, block in (("write", BLOCK), ("overwrite", BLOCK.replace("write ", "overwrite ").replace("  exists → Refused\n", "")),
                      ("delete", '  delete "{dir}/note.txt"\n  ok → Refused\n  failed → Refused\n'), ("read", '  read "{dir}/note.txt"\n  ok → Refused\n  failed → Refused\n')):
        d = definitions(check(parse(SRC.replace(BLOCK, block, 1))))["effects"]["Files.export"]["file"]
        assert d["op"] == op and d["dir"] == "dir" and d["rest"] == "note.txt", d
        assert d["rows"]["failed"] == {"outcome": "Refused", "take": {}}
    assert definitions(check(parse(SRC)))["effects"]["Files.export"]["file"]["rows"]["ok"] == {"outcome": "Done", "take": {"size": "bytes"}}
