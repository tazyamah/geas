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


# ---- 集合：add／remove と、件数の上限 max N ----
SET_SRC = """ltype Tags = set of id {max}
ltype Digit = int 0..9

entity Tray {{
  tray_id: id unique
  tags: Tags {req}
  n: Digit required
}}
result TagResult {{
  Done
  NoTray
  TooMany
}}
deed tag_tray {{
  input tray_id: id required
  input tag_id: id required
  reads Tray.tags{reads}
  writes Tray.tags{writes}
  result TagResult
  effects none

  let t = Tray[tray_id] else {{ return NoTray }}
{body}  return Done
}}
nexus Tagging {{
  tag_tray: Done → reply, NoTray → reply, TooMany → reply
}}
"""


def set_errors(body: str, max: str = "max 3", req: str = "required", reads: str = "", writes: str = "") -> list:
    src = SET_SRC.format(max=max, req=req, body=body, reads=reads, writes=writes)
    return [d.msg for d in check(parse(src)).diags if d.level == "エラー"]


def test_集合に_上限を確かめてから足せる():
    assert set_errors("  if len(t.tags) >= 3 { return TooMany }\n  add tag_id to t.tags\n") == []
    assert set_errors("  if len(t.tags) > 2 { return TooMany }\n  add tag_id to t.tags\n") == []


def test_集合から外すのに_確かめは要らない():
    assert set_errors("  remove tag_id from t.tags\n") == []


def test_上限の確かめが無い追加はエラー():
    msgs = set_errors("  add tag_id to t.tags\n")
    assert any("まだ上限（3 件）に届いていないと示せません" in m and "if len(t.tags) >= 3" in m for m in msgs), msgs
    # 確かめが緩い（3 件のときも通る）のも、示せない
    msgs = set_errors("  if len(t.tags) > 3 { return TooMany }\n  add tag_id to t.tags\n")
    assert any("届いていないと示せません" in m for m in msgs), msgs


def test_足す先の集合には_件数の上限が要る():
    msgs = set_errors("  if len(t.tags) >= 3 { return TooMany }\n  add tag_id to t.tags\n", max="")
    assert any("型に件数の上限が要ります（set of id max N）" in m for m in msgs), msgs


def test_null_になりうる集合には足せない():
    msgs = set_errors("  if len(t.tags) >= 3 { return TooMany }\n  add tag_id to t.tags\n", req="")
    assert any("required を付けてください" in m for m in msgs), msgs


def test_集合でない項目には足せない():
    msgs = set_errors("  add tag_id to t.n\n", reads=", Tray.n", writes=", Tray.n")
    assert any("集合ではありません" in m for m in msgs), msgs


def test_集合に記号は使えず_直し方が出る():
    msgs = set_errors("  t.tags += tag_id\n")
    assert any("集合に += は使えません。add 要素 to t.tags と書きます" in m for m in msgs), msgs
    with pytest.raises(Exception, match="add 要素 to t.tags と書きます"):
        parse(SET_SRC.format(max="max 3", req="required", body="  t.tags << tag_id\n", reads="", writes=""))
    with pytest.raises(Exception, match="add 要素 to 変数.項目"):
        parse(SET_SRC.format(max="max 3", req="required", body="  add tag_id into t.tags\n", reads="", writes=""))


# ---- 定義した項目（保存しない）と、同じ行の 2 項目を比べる不変条件 ----
REL_SRC = """ltype Quantity = int 0..1000
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
"""
HELD = "  held: Total = stock + reserved\n"
ALL = "Product.stock, Product.reserved, Product.cap"


def rel_errors(body, inv="invariant Product: held <= cap", derived=HELD, reads=ALL, writes="Product.stock") -> list:
    src = REL_SRC.format(body=body, inv=inv, derived=derived, reads=reads, writes=writes)
    return [d.msg for d in check(parse(src)).diags if d.level == "エラー"]


def test_定義した項目を比べる不変条件は_確かめてから書けば通る():
    assert rel_errors("  if p.held + n > p.cap { return OverCap }\n  p.stock += n\n") == []


def test_確かめずに増やすと_不変条件を示せない():
    msgs = rel_errors("  if p.stock + n > 1000 { return OverCap }\n  p.stock += n\n", reads="Product.stock")
    assert any("不変条件 Product: held <= cap を示せません" in m for m in msgs), msgs


def test_減らすだけなら_前から成り立つ約束で示せる():
    assert rel_errors("  if p.stock < n { return Short }\n  p.stock -= n\n", reads="Product.stock") == []


def test_上限を下げる側にも_確かめが要る():
    assert any("不変条件 Product: held <= cap を示せません" in m
               for m in rel_errors("  p.cap = n\n", reads="Product.cap", writes="Product.cap"))
    assert rel_errors("  if n < p.held { return OverCap }\n  p.cap = n\n", reads="Product.stock, Product.reserved", writes="Product.cap") == []


def test_2_つの項目を直に比べる不変条件():
    inv = "invariant Product: reserved <= stock"
    assert rel_errors("  if p.reserved + n > p.stock { return OverCap }\n  p.reserved += n\n", inv=inv, derived="",
                      reads="Product.reserved, Product.stock", writes="Product.reserved") == []
    msgs = rel_errors("  if p.stock < n { return Short }\n  p.stock -= n\n", inv=inv, derived="", reads="Product.stock")
    assert any("不変条件 Product: reserved <= stock を示せません" in m for m in msgs), msgs


def test_不変条件に式は直に書けず_定義してから比べる():
    msgs = rel_errors("", inv="invariant Product: stock + reserved <= cap", derived="", reads="Product.stock", writes="Product.stock")
    assert any("式は項目として定義してから比べます" in m for m in msgs), msgs


def test_定義した項目は_入れ子にできない():
    msgs = rel_errors("", inv="", derived=HELD + "  more: Total = held + stock\n")
    assert any("定義した項目「held」は使えません（入れ子にはしません" in m for m in msgs), msgs


def test_定義した項目の式は_足し算と引き算だけ():
    msgs = rel_errors("", inv="", derived="  twice: Total = stock * 2\n")
    assert any("足し算と引き算だけです" in m for m in msgs), msgs


def test_定義した項目には書き込めない():
    msgs = rel_errors("  p.held = n\n", inv="")
    assert any("項目「held」がありません" in m or "定義した項目" in m for m in msgs), msgs


def test_定義した項目の型の範囲も_書き込みのたびに示す():
    # held は 0..1500 と約束した。stock と reserved はそれぞれ 0..1000 なので、足すと超えうる
    derived = "  held: Held = stock + reserved\n"
    src = lambda body: REL_SRC.replace("ltype Total = int 0..2000", "ltype Held = int 0..1500").format(
        body=body, inv="", derived=derived, reads="Product.stock, Product.reserved", writes="Product.stock")
    errs = lambda body: [d.msg for d in check(parse(src(body))).diags if d.level == "エラー"]
    assert any("held（= stock + reserved）が範囲 0..1500に収まる を示せません" in m for m in errs("  if p.stock + n > 1000 { return OverCap }\n  p.stock += n\n"))
    assert errs("  if p.held + n > 1500 or p.stock + n > 1000 { return OverCap }\n  p.stock += n\n") == []


def test_仕様書に_定義した項目と証明の結果が出る():
    from geas.spec import build, render
    ck = check(parse(REL_SRC.format(body="  if p.held + n > p.cap { return OverCap }\n  p.stock += n\n", inv="invariant Product: held <= cap",
                                    derived=HELD, reads=ALL, writes="Product.stock")))
    md = render(ck, build(ck))
    assert "（保存しない。= stock + reserved）" in md
    assert "| Product: held <= cap | change_stock: 証明済み（書く前に確かめている） |" in md


# ---- 途中の計算が収まると示す（trust prove / check / verify / assume） ----
CALC_HEAD = """ltype Wide = int 0..9000000000000000000
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

"""
CALC_MAKE = "  let t = create Tray { n: 1 }\n  return Done(t.tray_id)\n"


def calc_diags(body: str, level: str = "エラー") -> list:
    return [d.msg for d in check(parse(CALC_HEAD + body + CALC_MAKE + TAIL)).diags if d.level == level]


def test_収まると示せる計算には_何も書かなくてよい():
    # 2 つの掛け算は 9e18 × 9e18 < 2^127 で、計算に使う幅に収まる
    assert calc_diags("  let x = a * b\n  if x > 5 { return Refused }\n") == []


def test_収まると示せない途中の計算はエラー():
    msgs = calc_diags("  let x = a * b * c\n  if x > 5 { return Refused }\n")
    assert any("途中の計算が収まると示せません" in m and "trust check" in m for m in msgs), msgs


def test_trust_prove_は_書かないのと同じ():
    assert calc_diags('  let x = a * b trust prove "2 つまでなら収まる"\n  if x > 5 { return Refused }\n') == []
    msgs = calc_diags('  let x = a * b * c trust prove "収まるはず"\n  if x > 5 { return Refused }\n')
    assert any("途中の計算が収まると示せません" in m for m in msgs), msgs


def test_trust_check_は_エラーを警告に下げる():
    body = '  let x = a * b * c trust check "実際の値は小さい"\n  if x > 5 { return Refused }\n'
    assert calc_diags(body) == []
    assert any("trust check" in m for m in calc_diags(body, "警告"))


@pytest.mark.parametrize("level", ["verify", "assume"])
def test_trust_verify_と_assume_は_警告も出さない(level):
    body = f'  let x = a * b * c trust {level} "実際の値は小さい"\n  if x > 5 {{ return Refused }}\n'
    assert calc_diags(body) == [] and calc_diags(body, "警告") == []


def test_示せている文に_trust_を書くとエラー():
    msgs = calc_diags('  let x = a * b trust assume "念のため"\n  if x > 5 { return Refused }\n')
    assert any("trust assume は要りません" in m for m in msgs), msgs


def test_trust_の段階の名前を間違えると_候補が出る():
    with pytest.raises(Exception) as e:
        parse(CALC_HEAD + '  let x = a * b * c trust warn "x"\n' + CALC_MAKE + TAIL)
    assert "prove" in str(e.value) and "assume" in str(e.value)


def test_人が引き受けた計算は_仕様書の人の保証に出る():
    from geas.spec import build as build_spec
    ck = check(parse(CALC_HEAD + '  let x = a * b * c trust verify "実際の値は小さい"\n  if x > 5 { return Refused }\n' + CALC_MAKE + TAIL))
    rows = [r for r in build_spec(ck)["人の保証"] if "途中の計算" in r[1]]
    assert rows and "検証ビルドだけ" in rows[0][3] and rows[0][4] == "実際の値は小さい"


def test_生成した_Rust_は_示せた計算から溢れの確かめを外す():
    from geas.rustgen import generate
    def rust(body):
        return generate(check(parse(CALC_HEAD + body + CALC_MAKE + TAIL)))
    cond = "  if x > 5 { return Refused }\n"
    assert ".wrapping_mul(" in rust("  let x = a * b\n" + cond) and ".checked_mul(" not in rust("  let x = a * b\n" + cond).split("pub fn put_tray")[1]
    assert ".checked_mul(" in rust('  let x = a * b * c trust check "小さい"\n' + cond).split("pub fn put_tray")[1]
    assert "cfg!(debug_assertions)" in rust('  let x = a * b * c trust verify "小さい"\n' + cond)


@pytest.mark.skipif(__import__("shutil").which("cargo") is None, reason="cargo が無い")
def test_途中の計算は_段階どおりに動く(tmp_path):
    """9e18 を 3 つ掛けると、計算に使う幅（i128）を超える。check は異常で止まり、assume は確かめずに先へ進む。
    verify は、検証ビルド（cargo run の既定）では check と同じに止まる"""
    import subprocess
    from geas.examples import write_crate
    from geas.rustgen import generate
    req = ('r#"{"name":"put_tray","ctx":{"new_Tray_id":"T"},'
           '"input":{"a":9000000000000000000,"b":9000000000000000000,"c":9000000000000000000},"store":{}}"#')
    main = "use geas_app::host::call;\nfn main() {\n    match call(" + req + ') { Ok(s) => println!("OK {}", s), Err(e) => println!("ERR {}", e) }\n}\n'
    out = {}
    for level in ("check", "verify", "assume"):
        ck = check(parse(CALC_HEAD + f'  let x = a * b * c trust {level} "試験"\n  if x > 5 {{ return Refused }}\n' + CALC_MAKE + TAIL))
        d = tmp_path / level
        d.mkdir()
        write_crate(ck, generate(ck), main, d, host=True)
        r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=d, capture_output=True, text=True)
        assert r.returncode == 0, r.stderr[-2500:]
        out[level] = r.stdout.strip()
    assert out["check"].startswith("ERR") and "あふれ" in out["check"], out
    assert out["verify"] == out["check"], out
    assert out["assume"].startswith("OK"), out
