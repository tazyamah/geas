"""検査の指摘と構文エラーの文言：AI が直しやすいこと（行番号・近い名前・他の言語の書き方への案内）と、
書き間違えたコードで検査器が落ちないこと"""
import json
import random
import re
import subprocess
import sys
from pathlib import Path

import pytest

from geas.check import ERROR, QUESTION, WARN, check
from geas.lexer import LexError
from geas.modules import ModuleError
from geas.parser import ParseError, parse

ROOT = Path(__file__).parent.parent
FIX = ROOT / "tests" / "fixtures"
SHOP = (FIX / "shop" / "shop.geas").read_text()
OVERSEAS = (FIX / "overseas_shop" / "overseas_shop.geas").read_text()
DELETE = (FIX / "delete_demo" / "delete_demo.geas").read_text()

# 小さな例：在庫を 1 足す（表示名つき）
BASE = '''/// 数量
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
'''


def edit(src, old, new):
    assert old in src, old
    return src.replace(old, new, 1)


def syntax_error(src):
    with pytest.raises((ParseError, LexError)) as e:
        parse(src)
    return e.value


def errors(src):
    ck = check(parse(src), strict_names=True)
    return [d for d in ck.diags if d.level == ERROR]


def test_小さな例は通る():
    assert check(parse(BASE), strict_names=True).ok


# ---- 書き間違えたコードで、検査器が落ちない ----

@pytest.mark.parametrize("src,old,new", [
    (OVERSEAS, "ltype PointCount = int 0..1000000000\n", ""),  # 未定義の型の項目へ書く
    (OVERSEAS, "entity Order {", "entity Orders {"),  # 引いたデータが未定義のまま項目へ書く
    (OVERSEAS, "  status: OrderStatus", "  status: Pending"),  # 新規作成する項目の型が未定義
    (OVERSEAS, "  order_id: id unique\n", ""),  # キーの無いデータを、出来事の each で回す
    (DELETE, "  status: NoteStatus", "  status: Unknown"),  # 消すデータの状態の型が未定義
    (BASE, "let p = Product[product_id]", "let p = Products[product_id]"),  # entity でない名前で引く
])
def test_未定義の名前があっても_検査器は落ちずに指摘する(src, old, new):
    ck = check(parse(edit(src, old, new)), strict_names=True)
    assert not ck.ok


def test_崩したコードを何通り検査しても_検査器は落ちない():
    """字句を消す・名前を入れ替える・行を消すなどで崩した版を、決まった種で作って検査する（落ちたら、その版を見る）"""
    rnd = random.Random(20261108)
    srcs = [SHOP, DELETE, BASE]
    for k in range(120):
        lines = rnd.choice(srcs).split("\n")
        names = sorted(set(re.findall(r"\b[A-Za-z_][A-Za-z0-9_]*\b", "\n".join(lines))))
        i = rnd.randrange(len(lines))
        op = rnd.randrange(3)
        if op == 0:
            del lines[i]
        elif op == 1:
            ws = list(re.finditer(r"\b[A-Za-z_][A-Za-z0-9_]*\b", lines[i]))
            if ws:
                m = rnd.choice(ws)
                lines[i] = lines[i][:m.start()] + rnd.choice(names) + lines[i][m.end():]
        else:
            lines.insert(i, lines[i])
        src = "\n".join(lines)
        try:
            check(parse(src), strict_names=True)
        except (ParseError, LexError, ModuleError):
            pass
        except Exception as e:  # noqa: BLE001
            pytest.fail(f"{k} 番目の版で検査器が落ちました: {type(e).__name__}: {e}\n{src}")


# ---- 構文エラー：位置と、他の言語の書き方への案内 ----

@pytest.mark.parametrize("old,new,expect", [
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
    ("/// 数量\n", 'import "x.geas"\n/// 数量\n', "use"),
    ("/// 数量\n", "/* メモ */\n/// 数量\n", "// から行末"),
    ("return Done\n}", "return Done\n", "行目の { が閉じていません"),
    ("return Done\n}", "return Done\n}\n}", "に対応する { がありません"),
    ("if p.stock > 98 {", "if p.stock > 98 ? {", "if 条件 then 式 else 式"),
    ("p.stock += 1", "p.stock += 1.5", "小数は書けません（「1.5」）"),
    ("p.stock += 1", "p.stock += ①", "数は半角の 0〜9 で書きます"),
    ("p.stock += 1", "p.stock += １", "数は半角の 0〜9 で書きます（「１」は半角の数字ではありません）"),
    ("ltype Quantity = int 0..99", "ltype Quantity = int ０..９９", "数は半角の 0〜9 で書きます"),
])
def test_構文エラーは_他の言語の書き方なら直し方を添える(old, new, expect):
    e = syntax_error(edit(BASE, old, new))
    assert expect in str(e), str(e)
    assert "\n" not in str(e)  # 改行の字句を、そのまま文言に入れない


def test_構文エラーは_行と列を持ち_行末とファイルの終わりを言葉で出す():
    e = syntax_error(edit(BASE, "if p.stock > 98 {", "if p.stock > 98 && p.stock < 200 {"))
    line = BASE.split("\n").index("  if p.stock > 98 { return Full }") + 1
    assert (e.line, e.col) == (line, len("  if p.stock > 98 ") + 1)
    assert "「行末」" in str(syntax_error(edit(BASE, "return Done", "return")))
    e = syntax_error("entity Product {\n  product_id: id unique\n")
    assert "ファイルの終わり" in str(e) and "1行目の { が閉じていません" in str(e)


def test_閉じ忘れた文字列は_その行で止まる():
    """文字列の中では行を変えられないので、" を閉じ忘れた行がそのままエラーの行になる（後ろの " と組にならない）"""
    line = BASE.split("\n").index("  if p.stock > 98 { return Full }") + 1
    for tail in ('\n  if p.name == "x" { return Full }', '\n  if p.name == "x" { return Full }\n  if p.name == "y { return Full }', ""):
        e = syntax_error(edit(BASE, "  if p.stock > 98 { return Full }", '  if p.name == "abc { return Full }' + tail))
        assert (e.line, e.col) == (line, len("  if p.name == ") + 1), str(e)
        assert '終わりの " を書き忘れたかもしれません' in str(e) and "\\n と書きます" in str(e)
    e = syntax_error(BASE + 'app "geas')
    assert "文字列が閉じていません" in str(e)


def test_問い合わせと_resource_の書き間違い():
    q = BASE + '''/// 一覧
query product_list {
  from Product
  where stock > 0
  order stock desc
  reads Product.stock
}
'''
    assert check(parse(q), strict_names=True).ok
    assert "where の行を分けて" in str(syntax_error(edit(q, "where stock > 0", "where stock > 0 and stock < 50")))
    assert "where の行を分けて" in str(syntax_error(edit(q, "where stock > 0", "where stock > 0, stock < 50")))
    assert "「newest」は使えません" in str(syntax_error(edit(q, "order stock desc", "order stock newest")))
    assert "by は書きません" in str(syntax_error(edit(q, "order stock desc", "order by stock desc")))
    assert "向きは小文字です" in str(syntax_error(edit(q, "order stock desc", "order stock DESC")))
    assert "desc か asc" in str(syntax_error(edit(q, "order stock desc", "order stock")))
    assert "at_least" in str(syntax_error(edit(q, "  order stock desc", "  filter stock >= 3\n  order stock desc")))
    r = BASE + "/// 商品たち\nresource Products {\n  update add_stock {\n    Done → 200\n    NoSuch → 404\n    Full → 409\n  }\n}\n"
    assert "PUT は update" in str(syntax_error(edit(r, "update add_stock {", "put add_stock {")))
    assert "PATCH はありません" in str(syntax_error(edit(r, "update add_stock {", "patch add_stock {")))
    assert "→ か -> と書きます" in str(syntax_error(edit(r, "Done → 200", "Done => 200")))
    assert "→ か -> でつなぎます" in str(syntax_error(edit(r, "Done → 200", "Done: 200")))


def test_取り込んだモジュールの構文エラーは_そのファイルの名前を出す(tmp_path):
    (tmp_path / "geo.geas").write_text("module Geo\nltype Lat = int -90..90\nconvert half(x: Lat) -> Lat = x &&\n")
    app = tmp_path / "app.geas"
    app.write_text('use "geo.geas"\n')
    with pytest.raises(ParseError) as e:
        parse(app.read_text(), str(app))
    assert str(e.value).startswith("geo.geas の 3行目") and e.value.file.endswith("geo.geas")
    (tmp_path / "geo.geas").write_text('module Geo\nltype Lat = int -90..90 "\n')
    with pytest.raises(LexError) as e:
        parse(app.read_text(), str(app))
    assert str(e.value).startswith("geo.geas の 2行目")


# ---- 検査の指摘：行番号・近い名前・直し方 ----

@pytest.mark.parametrize("typ,expect", [
    ("string", "text"), ("String", "text"), ("boolean", "bool"), ("float", "小数の型はありません"), ("decimal", "money"),
    ("uuid", "id です"), ("datetime", "time"), ("i32", "int"), ("text?", "required"), ("Quantty", "近い名前：Quantity"),
])
def test_他の言語の型名には_geasの型を案内する(typ, expect):
    errs = errors(edit(BASE, "name: text", f"name: {typ}"))
    assert any(expect in d.for_code() for d in errs), [d.for_code() for d in errs]


@pytest.mark.parametrize("old,new,expect", [
    ("if p.stock > 98", "if p.stok > 98", "近い名前：stock"),
    ("let p = Product[product_id]", "let p = Product[productId]", "近い名前：product_id"),
    ("let p = Product[product_id]", "let p = Products[product_id]", "近い名前：Product"),
    ("if p.stock > 98", "if True", "true／false と小文字"),
    ("if p.stock > 98", "if p.name is null or None", "null です"),
    ("if p.stock > 98 { return Full }", "if p.stock > 98 { return Ful }", "近い名前：Full"),
    ("if p.stock > 98 { return Full }", "if p.stock > 98 { return Ok }", "あるもの：Done・NoSuch・Full"),
    ("if p.stock > 98", "if p.name == null", "p.name is null"),
    ("if p.stock > 98", "if p.name == 'x'", '"…" で囲みます'),
    ("  if p.stock > 98 { return Full }\n", "", "if p.stock > 98 { return … }"),
    ("  result AddResult\n", "", "結果の形の宣言がありません"),
])
def test_検査の指摘は_直し方を添える(old, new, expect):
    errs = errors(edit(BASE, old, new))
    assert any(expect in d.for_code() for d in errs), [d.for_code() for d in errs]


def test_足し引きの範囲の指摘は_引くときも境目を示す():
    src = edit(edit(BASE, "  if p.stock > 98 { return Full }\n", ""), "p.stock += 1", "p.stock -= 3")
    assert any("if p.stock < 3 { return … }" in d.for_code() for d in errors(src))
    ok = edit(edit(BASE, "if p.stock > 98 { return Full }", "if p.stock < 3 { return Full }"), "p.stock += 1", "p.stock -= 3")
    assert check(parse(ok), strict_names=True).ok  # 示した境目で確かめれば通る


def test_足し引きの範囲の指摘は_書ける境目だけを示す():
    no_guard = edit(BASE, "  if p.stock > 98 { return Full }\n", "")
    msgs = [d.for_code() for d in errors(edit(no_guard, "ltype Quantity = int 0..99", "ltype Quantity = int"))]
    assert any("項目と足す値の型に ltype で範囲を付けて" in m for m in msgs), msgs  # 範囲の無い int の端の数は示さない
    yen = edit(edit(no_guard, "ltype Quantity = int 0..99", "ltype Quantity = money(JPY) 0..100000"), "p.stock += 1", "p.stock += 100")
    assert any("if p.stock > 99900 { return … }" in d.for_code() for d in errors(yen))


def test_宣言と本体の食い違いは_直す宣言の名前で言う():
    msgs = [d.for_code() for d in errors(edit(BASE, "reads Product.stock", "reads Product.stock, Product.name"))]
    assert any("宣言の reads に「Product.name」がありますが、本体では読んでいません" in m for m in msgs), msgs
    msgs = [d.for_code() for d in errors(edit(BASE, "  writes Product.stock\n", ""))]
    assert any("宣言の writes にありません" in m for m in msgs), msgs
    assert not any("ていません" in m and ("読むて" in m or "書くて" in m) for m in msgs)


def test_指摘には行番号が付き_AI向けはコードの名前_人向けは表示名で出す():
    src = edit(BASE, "if p.stock > 98", "if p.stok > 98")
    d = next(x for x in errors(src) if "stok" in x.msg)
    assert d.line == src.split("\n").index("  if p.stok > 98 { return Full }") + 1
    assert "データ Product に項目「stok」がありません（近い名前：stock）" in d.for_code() and f"（{d.line}行目）" in d.for_code()
    assert "データ 商品 に項目「stok」がありません（近い名前：在庫）" in str(d)  # 仕様書に載るほうは表示名
    # 定義そのものの指摘も、定義の行が付く
    d = next(x for x in errors(edit(BASE, "name: text", "name: string")))
    assert d.line == BASE.split("\n").index("  name: text") + 1
    ck = check(parse(SHOP + "\nltype Bad = int 0..9\n"), strict_names=True)
    assert all(x.line for x in ck.diags if x.code == "表示名"), [str(x) for x in ck.diags if not x.line]


# ---- CLI ----

def run(*args, cwd=ROOT):
    return subprocess.run([sys.executable, "-m", "geas", *args], capture_output=True, text=True, cwd=cwd)


def test_checkは_コードの名前と行番号で出し_最後に件数を出す(tmp_path):
    f = tmp_path / "app.geas"
    f.write_text(edit(BASE, "if p.stock > 98", "if p.stok > 98"))
    r = run("check", str(f))
    assert r.returncode == 1
    assert "[エラー] 処理 add_stock（30行目）: データ Product に項目「stok」がありません（近い名前：stock）（名前）" in r.stdout, r.stdout
    assert r.stdout.strip().split("\n")[-1].startswith("検査: エラーがあります（エラー ")
    d = json.loads(run("check", str(f), "--json").stdout)
    e = next(x for x in d["diagnostics"] if "stok" in x["message"])
    assert e["line"] == 30 and e["where"] == "処理 add_stock" and "近い名前：stock" in e["message"]


def test_checkは_問いが残れば_人に聞くことを最後の行で言う(tmp_path):
    f = tmp_path / "app.geas"
    f.write_text(edit(SHOP, "round(subtotal * tax_rate, floor, JPY)", "subtotal * tax_rate"))
    r = run("check", str(f))
    assert r.returncode == 0 and "問い 1 件" in r.stdout and "人に1問ずつ聞いて" in r.stdout, r.stdout


def test_構文エラーは_その行と位置の印を出す(tmp_path):
    f = tmp_path / "app.geas"
    f.write_text(edit(BASE, "if p.stock > 98 {", "if p.stock > 98 && p.stock < 200 {"))
    r = run("check", str(f))
    assert r.returncode == 2
    lines = r.stderr.split("\n")
    assert lines[0].startswith("構文エラー: 30行目: ") and "and と書きます" in lines[0]
    assert lines[1].endswith("|   if p.stock > 98 && p.stock < 200 { return Full }")
    assert lines[2].index("^") == lines[1].index("&&")
    d = json.loads(run("check", str(f), "--json").stdout)["diagnostics"][0]
    assert (d["line"], d["col"]) == (30, len("  if p.stock > 98 ") + 1) and "and と書きます" in d["hint"]["fix"]


def test_ファイルが無い_組み合わせが違うときは_トレースバックでなく1行で断る(tmp_path):
    r = run("check", str(tmp_path / "nothing.geas"))
    assert r.returncode == 2 and "ファイルがありません" in r.stderr and "Traceback" not in r.stderr
    r = run("spec", str(FIX / "shop" / "shop.geas"), "--plan", str(tmp_path / "nothing.md"))
    assert r.returncode == 2 and "nothing.md" in r.stderr and "Traceback" not in r.stderr
    r = run("spec", str(FIX / "shop" / "shop.geas"), "-o", str(tmp_path / "no" / "dir" / "spec.md"))
    assert r.returncode == 2 and "フォルダがありません" in r.stderr
    r = run("rust", str(FIX / "shop" / "shop.geas"), "-o", str(tmp_path / "out"), "--runtime")
    assert r.returncode == 2 and "--host" in r.stderr and not (tmp_path / "out").exists()
    r = run()
    assert r.returncode == 2 and "usage" in r.stderr and "check" in r.stderr
