# SPDX-License-Identifier: GPL-3.0-or-later
"""2026-11-16 に決めた、信頼性まわりの言語の決まりの試験"""
import pytest

from geas.check import check
from geas.parser import parse

HEAD = """ltype Count = int 0..100
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

"""
TAIL = """}
nexus Filling {
  put_tray: Done → reply, Refused → reply
}
"""


def errors(body: str, head: str = HEAD, tail: str = TAIL) -> list:
    return [d.msg for d in check(parse(head + body + tail)).diags if d.level == "エラー"]


# ---- if の中の let ----
def test_if_の中に_let_を書ける():
    assert errors("""  if n > 50 {
    let twice = n * 2
    let t = create Tray { n: twice }
    return Done(t.tray_id)
  }
  return Refused
""") == []


def test_枝の中の名前は_枝を抜けたら消える():
    msgs = errors("""  if n > 50 {
    let twice = n * 2
  }
  let t = create Tray { n: twice }
  return Done(t.tray_id)
""")
    assert any("「twice」は定義されていません" in m for m in msgs), msgs


def test_重ならない別の枝なら_同じ名前を使える():
    assert errors("""  if n > 50 {
    let twice = n * 2
    let t = create Tray { n: twice }
    return Done(t.tray_id)
  } else {
    let twice = m + m
    if twice > 10 {
      let t = create Tray { n: twice }
      return Done(t.tray_id)
    }
  }
  let twice = n + m
  let t = create Tray { n: twice }
  return Done(t.tray_id)
""") == []


def test_見えている名前は_枝の中でも付け直せない():
    msgs = errors("""  let twice = n * 2
  if n > 50 {
    let twice = m
  }
  return Refused
""")
    assert any("「twice」は、すでにある名前です" in m for m in msgs), msgs


def test_枝で確かめたことは_同じ名前の別の値に残らない():
    # 枝の中で d が 1 以上と確かめても、枝を抜けたあとの別の d（0 かもしれない）で割れてはいけない
    msgs = errors("""  if n > 50 {
    let d = m
    if d == 0 { return Refused }
    let t = create Tray { n: n / d }
    return Done(t.tray_id)
  }
  let d = m
  let t = create Tray { n: n / d }
  return Done(t.tray_id)
""")
    assert len([m for m in msgs if "割る数" in m]) == 1, msgs


# ---- 文字列：定数との比較と、つなぐ + ----
TEXT_HEAD = """ltype Count = int 0..100
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

"""


def text_errors(body: str) -> list:
    return errors(body, TEXT_HEAD)


MAKE = "  let t = create Tray { n: n, label: w }\n  return Done(t.tray_id)\n"


def test_処理の中で_文字列を定数と比べられる():
    assert text_errors('  if w == "gift" { return Refused }\n' + MAKE) == []
    assert text_errors('  if "gift" != w { return Refused }\n' + MAKE) == []


def test_型に合わない定数との比較はエラー():
    msgs = text_errors('  if w == "abcdefghijkl" { return Refused }\n' + MAKE)
    assert any("長さは 1..10 なので、12 文字" in m and "同じになることはありません" in m for m in msgs), msgs
    msgs = text_errors('  if w == "Gift" { return Refused }\n' + MAKE)
    assert any("使える文字は [a-z]" in m and "「G」を含む" in m for m in msgs), msgs


def test_文字列の大小と_数との比較は書けない():
    assert any("大小は比べられません" in m for m in text_errors('  if w < "m" { return Refused }\n' + MAKE))
    assert any("文字列だけです" in m for m in text_errors('  if n == "5" { return Refused }\n' + MAKE))


def test_文字列を_足し算でつなげる():
    assert text_errors('  let t = create Tray { n: n, label: w + "x" }\n  return Done(t.tray_id)\n') == []
    assert text_errors('  let both = w + "x"\n  let t = create Tray { n: n, label: both }\n  return Done(t.tray_id)\n') == []


def test_つないだ長さが入れ先を超えうるならエラー():
    msgs = text_errors('  let t = create Tray { n: n, label: w + "xy" }\n  return Done(t.tray_id)\n')
    assert any("長さ（3..12）" in m and "収まると示せません" in m for m in msgs), msgs
    msgs = text_errors('  let both = w + w\n  let t = create Tray { n: n, label: both }\n  return Done(t.tray_id)\n')
    assert any("長さ（2..20）" in m for m in msgs), msgs


def test_つないだ文字が入れ先の文字種を出るならエラー():
    msgs = text_errors('  let t = create Tray { n: n, label: w + "X" }\n  return Done(t.tray_id)\n')
    assert any("文字が、項目の使える文字に収まると示せません" in m for m in msgs), msgs


def test_数と文字列は_足し算でつなげない():
    msgs = text_errors('  let t = create Tray { n: n, label: w + n }\n  return Done(t.tray_id)\n')
    assert any("同じ種類の数どうし" in m or "文字列だけ" in m for m in msgs), msgs
