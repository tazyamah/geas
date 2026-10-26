import shutil
import subprocess
import sys
from pathlib import Path

import pytest

from geas.ast import show_type
from geas.check import ERROR, QUESTION, check
from geas.examples import HOST_MOD, compare_examples, run_crate, runner_source, write_crate
from geas.hostgen import host_source
from geas.spec import render
from geas.parser import ParseError, parse
from geas.plan import A_ONLY, B_ONLY, DIFF, compare_static, read_plan
from geas.rustgen import generate
from geas.spec import build

ROOT = Path(__file__).parent.parent
EX = ROOT / "tests" / "fixtures" / "shop"
CODE = (EX / "shop.geas").read_text()
PLAN = (EX / "shop_plan.md").read_text()
needs_cargo = pytest.mark.skipif(shutil.which("cargo") is None, reason="cargo が無い")


def chk(src):
    return check(parse(src))


def msgs(ck, level):
    return [d.msg for d in ck.diags if d.level == level]


def mutate(src, old, new):
    assert old in src, old
    return src.replace(old, new, 1)


# ---- 正常系 ----
def test_お店の例は検査を通る():
    ck = chk(CODE)
    assert ck.ok, ck.diags
    assert not msgs(ck, QUESTION)


def test_不変条件は証明済みと出る():
    ck = chk(CODE)
    text = " ".join(s for _, rows in ck.invariant_status for _, s in rows)
    assert "証明できない" not in text and "減算の前に不足を確認" in text


def test_仕様書は読み書きと遷移をコードから取り出す():
    spec = build(chk(CODE))
    shop = {x["処理"]: x for x in spec["処理"]}
    assert shop["受注"]["書く"] == ["商品.在庫", "注文"]
    assert ("注文", "受付", "決済要求済", "決済要求") in spec["遷移"]
    assert shop["決済要求"]["外部"] == ["決済.請求(冪等キー=注文ID)"]


def test_物理数は範囲から選ばれる():
    spec = build(chk(CODE))
    assert dict(spec["実現"])["商品.在庫"] == "u64"
    assert ("明細.値", "u8") in next(x for x in spec["処理"] if x["処理"] == "受注")["物理数"]


# ---- 検査が止める例（構文のスケッチ §3 の表） ----
def test_丸めの指定が無いと問いになる():
    ck = chk(mutate(CODE, "let 合計 = 小計 + round(小計 * 税率, floor, JPY)", "let 合計 = 小計 + 小計 * 税率"))
    assert any("端数をどう丸めますか" in m for m in msgs(ck, QUESTION))


def test_結果に行き先が無いと止まる():
    ck = chk(mutate(CODE, ", 顧客なし → reply\n", "\n"))
    assert any("顧客なし" in m and "行き先" in m for m in msgs(ck, ERROR))


def test_冪等キーが無いと止まる():
    ck = chk(mutate(CODE, ", idempotency_key = 注文ID)", ")"))
    assert any("冪等キー" in m for m in msgs(ck, ERROR))


def test_宣言に無い遷移は止まる():
    ck = chk(mutate(CODE, "  注文.状態 = 決済要求済", "  注文.状態 = 完了"))
    assert any("遷移" in d.code and "完了" in d.msg for d in ck.diags if d.level == ERROR)


def test_元の状態の確認が無いと止まる():
    ck = chk(mutate(CODE, "  if 注文.状態 != 受付 { return 状態違い }\n", ""))
    assert any("確認" in m for m in msgs(ck, ERROR))


def test_在庫不足の判定を削ると不変条件が示せない():
    ck = chk(mutate(CODE, "  if 不足 is nonempty { return 在庫不足(不足) }\n", ""))
    assert any("反例" in m for m in msgs(ck, ERROR))


def test_書く項目の宣言漏れは止まる():
    ck = chk(mutate(CODE, "  writes 商品.在庫, 注文\n", "  writes 注文\n"))
    assert any("商品.在庫" in m for m in msgs(ck, ERROR))


def test_合計ゼロの確認を削ると不変条件が示せない():
    ck = chk(mutate(CODE, "  if 注文.合計 <= 0 { return 合計ゼロ }\n", ""))
    assert any("合計 > 0" in m for m in msgs(ck, ERROR))


def test_存在を確かめない参照は問いになる():
    ck = chk(mutate(CODE, "  let 顧客 = 顧客[顧客ID] else { return 顧客なし }", "  let 顧客 = 顧客[顧客ID]"))
    assert any("存在しない" in m for m in msgs(ck, QUESTION))


def test_構文エラーは行番号つき():
    with pytest.raises(ParseError, match="行目"):
        parse("entity 商品 {\n  在庫 int\n}\n")


# ---- Rust 生成 ----
def test_エラーのあるコードからは生成しない():
    with pytest.raises(AssertionError):
        generate(chk(mutate(CODE, ", idempotency_key = 注文ID)", ")")))


def test_生成は決定的で_コミットした生成物と一致する():
    ck = chk(CODE)
    assert generate(ck) == generate(chk(CODE))
    committed = EX / "shop_rust" / "src" / "lib.rs"
    assert committed.read_text() == generate(ck), "python -m geas rust で生成物を作り直してください"


# ---- 突合 ----
def static(plan_text):
    return compare_static(build(chk(CODE)), read_plan(plan_text))


def test_お店の例は計画と静的に一致する():
    assert [f for f in static(PLAN) if f.kind != "未規定"] == []


def test_計画にある遷移がコードに無いと検出する():
    plan = mutate(PLAN, "| TR2 |", "| TR9 | 注文 | 受付 | 完了 | 即時完了 | 仮 |\n| TR2 |")
    fs = static(plan)
    assert any(f.kind == A_ONLY and "受付→完了" in f.where for f in fs)


def test_コードが計画に無いものを書き込むと検出する():
    plan = mutate(PLAN, "| 商品.在庫, 注文 |", "| 商品.在庫 |")
    fs = static(plan)
    assert any(f.kind == DIFF and "書く" in f.where and "注文" in f.msg for f in fs)


def test_範囲の食い違いを検出する():
    fs = static(mutate(PLAN, "| 在庫 | 整数 0.. |", "| 在庫 | 整数 1.. |"))
    assert any(f.kind == DIFF and "商品.在庫" in f.where for f in fs)


def test_計画に無い項目がコードにあると検出する():
    fs = static(mutate(PLAN, "| DA3 | 商品 | 単価 | 金額(円) 0.. | ○ |  | 仮 |\n", ""))
    assert any(f.kind == B_ONLY and "商品.単価" in f.where for f in fs)


@needs_cargo
def test_具体例を実際に動かして計画と一致する(tmp_path):
    ck = chk(CODE)
    rows = read_plan(PLAN)["具体例"]
    src, problems = runner_source(ck, rows)
    assert [p for p in problems if p.kind != "未規定"] == []
    write_crate(ck, generate(ck), src, tmp_path)
    actual = run_crate(tmp_path)
    assert actual["EX1"][0] == "受付(N1)"
    assert compare_examples(rows, actual) == []


@needs_cargo
def test_具体例が計画と違えば検出する(tmp_path):
    ck = chk(CODE)
    rows = read_plan(mutate(PLAN, "| 在庫不足({A}) | なし |", "| 受付(N1) | なし |"))["具体例"]
    src, _ = runner_source(ck, rows)
    write_crate(ck, generate(ck), src, tmp_path)
    fs = compare_examples(rows, run_crate(tmp_path))
    assert any("EX2" in f.where and "結果" in f.where for f in fs)


def test_cli_compare(tmp_path):
    r = subprocess.run([sys.executable, "-m", "geas", "compare", str(EX / "shop.geas"), str(EX / "shop_plan.md"), "--no-run"],
                       capture_output=True, text=True, cwd=ROOT)
    assert r.returncode == 0, r.stdout + r.stderr


def test_計画と必須の入力が違うと突合で出る(tmp_path):
    plan = (EX / "shop_plan.md").read_text().replace("| 決済要求 | 注文ID（必須） |", "| 決済要求 | 注文ID |")
    (tmp_path / "plan.md").write_text(plan)
    r = subprocess.run([sys.executable, "-m", "geas", "compare", str(EX / "shop.geas"), str(tmp_path / "plan.md"), "--no-run"],
                       capture_output=True, text=True, cwd=ROOT)
    assert r.returncode != 0 and "注文ID" in r.stdout, r.stdout


# ---- トリガ ----
def test_トリガの要因が仕様書に出る():
    spec = build(chk(CODE))
    assert ("決済待ち", "応答 決済成功", "決済結果の反映") in spec["トリガ"]
    assert ("決済待ち", "期限 現在+30分", "決済の期限切れ") in spec["トリガ"]


def test_期限の無いトリガは止まる():
    ck = chk(mutate(CODE, "  deadline now + 30 min → 決済の期限切れ\n", ""))
    assert any("期限がありません" in m for m in msgs(ck, ERROR))


def test_待つ先のトリガが未定義だと止まる():
    ck = chk(mutate(CODE, "要求済 → wait 決済待ち", "要求済 → wait 承認待ち"))
    assert any("承認待ち" in m for m in msgs(ck, ERROR))


def test_トリガの行き先が未定義の処理だと止まる():
    ck = chk(mutate(CODE, "→ 決済の期限切れ\n}", "→ 期限切れ処理\n}"))
    assert any("期限切れ処理" in m for m in msgs(ck, ERROR))


def test_トリガの引数と処理の入力が合わないと止まる():
    ck = chk(mutate(CODE, "trigger 決済待ち(注文ID: id)", "trigger 決済待ち(注文: id)"))
    assert any("合いません" in m or "入力にもっていません" in m for m in msgs(ck, ERROR))


def test_計画にあるトリガの行き先がコードと違えば検出する():
    plan = mutate(PLAN, "| 期限 現在+30分 | 決済の期限切れ |", "| 期限 現在+30分 | 決済結果の反映 |")
    fs = compare_static(build(chk(CODE)), read_plan(plan))
    assert any(f.kind == DIFF and "トリガ" in f.where for f in fs)


def test_入力の範囲が項目より広いと止まる():
    src = mutate(CODE, "ltype ID集合 = set of id\n", "ltype ID集合 = set of id\nltype 広い数量 = int 1..200\nltype 広い商品数量 = 商品 → 広い数量\n")
    ck = chk(mutate(src, "顧客ID: id required, 明細: 商品数量 required", "顧客ID: id required, 明細: 広い商品数量 required"))
    assert any("範囲の広い入力" in m for m in msgs(ck, ERROR))


def test_トリガの型の食い違いと期限の誤りは止まる():
    assert any("型" in m for m in msgs(chk(mutate(CODE, "trigger 決済待ち(注文ID: id)", "trigger 決済待ち(注文ID: int)")), ERROR))
    assert any("期限が複数" in m for m in msgs(chk(mutate(CODE, "  deadline now + 30 min → 決済の期限切れ", "  deadline now + 30 min → 決済の期限切れ\n  deadline now + 1 hour → 決済の期限切れ")), ERROR))
    assert any("0より長く" in m for m in msgs(chk(mutate(CODE, "deadline now + 30 min", "deadline now + 0 min")), ERROR))


# ---- 初回の対話の台帳 ----
def test_台帳は次に聞くことを示し_揃うまで開始不可():
    from geas import interview as iv

    led = iv.new_ledger("メモ")
    todo, ready = iv.status(led)
    assert not ready and any("一言" in t for t in todo)
    led.update(summary="メモを残す", out_of_scope=["共有"])
    led["usecases"].append({"id": "UC1", "actor": "本人", "goal": "メモを書く", "level": "利用者目標",
                            "main": ["書く", "保存する"], "extensions": [{"at": "2", "cond": "容量不足", "handling": "エラー表示"}],
                            "examples": [{"given": "空", "when": "書く", "then": "1件"}]})
    todo, ready = iv.status(led)
    assert not ready and any("R1" in t for t in todo)
    led["runtime"] = {k: {"value": "x"} for k in iv.RUNTIME}
    assert iv.status(led) == ([], True)


def test_未解決の問いと未仕分けの付加情報は開始を止める():
    from geas import interview as iv

    led = iv.new_ledger("メモ")
    led.update(summary="a", out_of_scope=["b"], runtime={k: {"value": "x"} for k in iv.RUNTIME})
    led["usecases"].append({"id": "UC1", "level": "利用者目標", "main": ["a"], "extensions": [{"at": "1", "cond": "c", "handling": "h"}], "examples": [{"given": "g", "when": "w", "then": "t"}]})
    led["questions"].append({"id": "Q1", "text": "空のとき", "status": "open"})
    assert not iv.status(led)[1]
    led["questions"][0]["status"] = "unspec"
    led["extras"].append({"id": "X1", "text": "スマホで使いたい"})
    todo, ready = iv.status(led)
    assert any("仕分け" in t for t in todo) and not ready


def test_台帳の状態の打ち間違いと_R1の未規定は止まる():
    from geas import interview as iv

    led = iv.new_ledger("メモ")
    led.update(summary="a", out_of_scope=["b"])
    led["usecases"].append({"id": "UC1", "level": "利用者目標", "main": ["a"], "extensions": [{"at": "1", "cond": "c", "handling": "h"}], "examples": [{"given": "g", "when": "w", "then": "t"}]})
    led["questions"].append({"id": "Q1", "text": "x", "status": "Open"})
    assert any("不正" in t for t in iv.status(led)[0])
    led["questions"][0].update(status="unspec", answer="結果に影響しないため")
    led["runtime"] = {k: {"status": "answered", "value": "x"} for k in iv.RUNTIME}
    led["runtime"]["R1"] = {"status": "unspec", "value": "決めない"}
    todo, ready = iv.status(led)
    assert not ready and any("R1" in t and "未規定にできません" in t for t in todo)
    led["runtime"]["R1"] = {"status": "answered", "value": "ブラウザ"}
    led["runtime"]["R4"] = {"status": "unspec", "value": "決めない"}
    assert iv.status(led) == ([], True)


def test_台帳から出したplanの表は突合の対象にならない():
    from geas import interview as iv

    led = iv.new_ledger("メモ")
    led["usecases"].append({"id": "UC1", "goal": "書く", "main": ["書く"]})
    assert read_plan(iv.render(led)) == {}



# ---- 構文（英語キーワード・Rust 風の本体） ----
def test_flowはnexusの別名():
    ck = chk(mutate(CODE, "nexus 注文受付", "flow 注文受付"))
    assert not msgs(ck, ERROR)


def test_日本語の古いキーワードや_body_は受け付けない():
    with pytest.raises(ParseError):
        parse("データ 商品 {\n  在庫: int\n}\n")
    with pytest.raises(ParseError):
        parse(mutate(CODE, "  let 顧客 = 顧客[顧客ID] else", "  body {\n  let 顧客 = 顧客[顧客ID] else"))


def test_if_の本体は改行して書いてもよい():
    ck = chk(mutate(CODE, "  if 注文.合計 <= 0 { return 合計ゼロ }\n", "  if 注文.合計 <= 0 {\n    return 合計ゼロ\n  }\n"))
    assert not msgs(ck, ERROR)


# ---- 条件つきの状態変更 ----
GUARD = "  if 注文.状態 != 受付 { return 状態違い }\n  if 注文.合計 <= 0 { return 合計ゼロ }\n  注文.状態 = 決済要求済\n"
COND = "  if 注文.合計 <= 0 { return 合計ゼロ }\n  if 注文.状態 == 受付 {\n    注文.状態 = 決済要求済\n  }\n"


def test_if_の中で元の状態を確かめてから変えるのは通る():
    ck = chk(mutate(CODE, GUARD, COND))
    assert not [d for d in ck.diags if d.level == ERROR and d.code == "遷移"]


def test_if_の中でも元の状態を確かめていないと止まる():
    ck = chk(mutate(CODE, GUARD, "  if 注文.合計 > 0 {\n    注文.状態 = 決済要求済\n  }\n"))
    assert any("遷移" in d.code and "受付" in d.msg for d in ck.diags if d.level == ERROR)


def test_if_の中の確認は外へ持ち出さない():
    ck = chk(mutate(CODE, GUARD, "  if 注文.状態 == 受付 {\n    request 決済.請求(注文ID, 注文.合計, idempotency_key = 注文ID)\n  }\n  注文.状態 = 決済要求済\n"))
    assert any("遷移" in d.code and "受付" in d.msg for d in ck.diags if d.level == ERROR)


def test_if_の中に_let_は書けない():
    ck = chk(mutate(CODE, GUARD, "  if 注文.状態 == 受付 {\n    let x = 注文[注文ID] else { return 注文なし }\n  }\n"))
    assert any("if の中" in m for m in msgs(ck, ERROR))


def test_条件つきの変更は_Rust_の_if_の中に出る():
    prog = parse(mutate(CODE, GUARD, COND))
    src = generate(check(prog))
    assert "    if (注文.状態 == 注文状態::受付) {\n        書込列.push(書込::注文_状態(注文.注文ID.clone(), 注文状態::決済要求済));\n    }" in src


# ---- 問い合わせの条件（または・数との比較） ----
QUERY = """ltype 重さ値 = int 0..

entity メモ {
  メモID: id unique
  状態: メモ状態
  重さ: 重さ値
}
state メモ状態 {
  initial 下書き
  middle 公開
  final 削除
}
query 探す {
  from メモ
  where 状態 == 公開 or 状態 == 下書き
  where 重さ >= 10
  reads メモ.状態, メモ.重さ
}
"""


def test_問い合わせの条件に_or_と数の比較を書ける():
    ck = chk(QUERY)
    assert not msgs(ck, ERROR)
    assert build(ck)["問い合わせ"][0]["条件"] == ["状態 == 公開 または 状態 == 下書き", "重さ >= 10"]


def test_数との比較は数の項目にだけ使える():
    ck = chk(QUERY.replace("重さ >= 10", "状態 >= 10"))
    assert any("数との比較" in m for m in msgs(ck, ERROR))


def test_問い合わせの条件は_Rust_で_or_になる():
    src = generate(chk(QUERY))
    assert "if !(r.状態 == メモ状態::公開 || r.状態 == メモ状態::下書き) { return false; }" in src
    assert "if !(r.重さ.map_or(false, |v| (v as i128) >= 10)) { return false; }" in src  # 重さは required でないので、null は満たさない


# ---- 問い合わせの select（結合と集計） ----
def test_selectで参照先の項目と集計を返せる():
    spec = build(chk(CODE))
    row = next(q for q in spec["問い合わせ"] if q["問い合わせ"] == "注文一覧")
    assert row["列"] == ["注文ID", "顧客", "点数"]


def test_selectの参照先が参照でないと止まる():
    ck = chk(mutate(CODE, "顧客.顧客ID as 顧客", "合計.顧客ID as 顧客"))
    assert any("参照（ref）ではありません" in m for m in msgs(ck, ERROR))


def test_sumはキーつきの集まりにだけ使える():
    ck = chk(mutate(CODE, "sum(明細) as 点数", "sum(合計) as 点数"))
    assert any("集計できません" in m for m in msgs(ck, ERROR))


def test_selectに書いた参照先は読むの宣言に要る():
    ck = chk(mutate(CODE, "注文.顧客, 顧客.顧客ID, 注文.明細\n}", "注文.顧客, 注文.明細\n}"))
    assert any("顧客.顧客ID" in m for m in msgs(ck, ERROR))


def test_selectのある問い合わせは_each_に使えない():
    ck = chk(mutate(CODE, "invariant 商品: 在庫 >= 0", "event 開いた {\n  each 注文一覧 → 決済要求\n}\n\ninvariant 商品: 在庫 >= 0"))
    assert any("select" in m for m in msgs(ck, ERROR))


# ---- 文字列検索の同一視（contains の軸） ----
MEMO = """entity メモ {
  メモID: id unique
  本文: text
}
query 探す {
  input 単語: text
  from メモ
  filter 本文 contains 単語 ignore_case ignore_kana ignore_width
  reads メモ.本文
}
"""


def test_containsの同一視する軸は書いたものだけ():
    ck = chk(MEMO)
    assert not msgs(ck, ERROR)
    row = build(ck)["問い合わせ"][0]
    assert row["条件"] == ["本文 含む(大小無視,かな無視,幅無視) 単語（単語が無ければ絞らない）"]
    plain = build(chk(MEMO.replace(" ignore_case ignore_kana ignore_width", "")))["問い合わせ"][0]
    assert plain["条件"] == ["本文 含む 単語（単語が無ければ絞らない）"]
    req = build(chk(MEMO.replace("input 単語: text", "input 単語: text required")))["問い合わせ"][0]
    assert req["条件"] == ["本文 含む(大小無視,かな無視,幅無視) 単語"]  # 必須の入力なら、無いときの動きは無い


def test_生成した照合は大小_かな_幅をそろえる(tmp_path):
    if shutil.which("rustc") is None:
        pytest.skip("rustc が無い")
    src = generate(chk(MEMO))
    start = src.index("pub fn 照合")
    end = src.index("pub fn 集合表示")
    main = src[start:end] + '''
fn main() {
    assert_eq!(照合("ＡＢＣ　abc", true, false, true), "abc　abc");
    assert_eq!(照合("カタカナ", false, true, false), "かたかな");
    assert_eq!(照合("Ａｂ", false, false, false), "Ａｂ");
}
'''
    f = tmp_path / "t.rs"
    f.write_text(main)
    r = subprocess.run(["rustc", "--edition", "2021", "-o", str(tmp_path / "t"), str(f)], capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]
    assert subprocess.run([str(tmp_path / "t")]).returncode == 0


def test_外部作用の無いアプリでも生成した_crate_がビルドできる(tmp_path):
    if shutil.which("cargo") is None:
        pytest.skip("cargo が無い")
    ck = chk(MEMO)
    write_crate(ck, generate(ck), "fn main() {}\n", tmp_path, host=True)
    r = subprocess.run(["cargo", "build", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]


# ---- 確認は「どのキーで引いたものか」に効く（データ名には効かない） ----
PAY = "  let 注文 = 注文[注文ID] else { return 注文なし }\n  if 注文.状態 != 受付 { return 状態違い }\n"
TWO = "deed 決済要求 {\n  input 注文ID: id required, 別ID: id required\n"


def two_orders(body):
    return mutate(mutate(CODE, "deed 決済要求 {\n  input 注文ID: id required\n", TWO), PAY, body)


def test_別の注文の状態を確かめても_こちらは遷移できない():
    ck = chk(two_orders("  let 別 = 注文[別ID] else { return 注文なし }\n  let 注文 = 注文[注文ID] else { return 注文なし }\n  if 別.状態 != 受付 { return 状態違い }\n"))
    assert any(d.code == "遷移" and "受付" in d.msg for d in ck.diags if d.level == ERROR)


def test_同じ名前で引き直すと確認は消える():
    ck = chk(two_orders(PAY + "  let 注文 = 注文[別ID] else { return 注文なし }\n"))
    assert any(d.code == "遷移" and "受付" in d.msg for d in ck.diags if d.level == ERROR)


def test_存在の確認は同じキーにだけ効く():
    ck = chk(two_orders(PAY + "  if 注文[別ID].合計 <= 0 { return 合計ゼロ }\n"))
    assert any("注文[別ID]" in d.msg for d in ck.diags if d.level == QUESTION)
    ck = chk(two_orders(PAY + "  if 注文[別ID] is absent { return 注文なし }\n  if 注文[別ID].合計 <= 0 { return 合計ゼロ }\n"))
    assert not any("注文[別ID]" in d.msg for d in ck.diags if d.level == QUESTION)


BULK = """
deed 一括完了 {
  input 他: ID集合
  writes 注文.状態
  result 待ち解除結果
  effects none

  update { 注文[k].状態 = 完了 | k in 他 }
  return 反映済
}
"""


def test_updateで状態の項目は変えられない():
    assert any("update で状態の項目" in m for m in msgs(chk(CODE + BULK), ERROR))


def test_状態は状態名で変える():
    ck = chk(two_orders(PAY + "  let 別 = 注文[別ID] else { return 注文なし }\n").replace("  注文.状態 = 決済要求済\n", "  注文.状態 = 別.状態\n", 1))
    assert any("状態名で書いてください" in m for m in msgs(ck, ERROR))


def test_新規作成の状態は状態名で書く():
    ck = chk(mutate(CODE, "状態: 受付 }", "状態: 顧客ID }"))
    assert any("状態名で書いてください" in m for m in msgs(ck, ERROR))


def test_空の_if_はエラーで止まる():
    ck = chk(mutate(CODE, "  if 注文.合計 <= 0 { return 合計ゼロ }\n", "  if 注文.合計 <= 0 { }\n  if 注文.合計 <= 0 { return 合計ゼロ }\n"))
    assert any("if の中が空" in m for m in msgs(ck, ERROR))


# ---- 不変条件と項目の範囲は、書き込むその場で、値の範囲から示す ----
REFLECT = ("  writes 注文.状態\n  transition 注文: 決済要求済 → 完了", "  writes 注文.状態, 注文.合計\n  transition 注文: 決済要求済 → 完了")


def test_項目の更新で在庫を減らすと範囲も不変条件も示せない():
    ck = chk(mutate(mutate(CODE, "  reads 注文.合計, 注文.状態\n  writes 注文.状態\n",
                           "  reads 注文.合計, 注文.状態, 商品.在庫\n  writes 注文.状態, 商品.在庫\n"),
                    "  注文.状態 = 決済要求済\n",
                    "  注文.状態 = 決済要求済\n  let 品 = 注文[注文ID] else { return 注文なし }\n  let 商 = 商品[注文ID] else { return 注文なし }\n  商.在庫 = 商.在庫 - 1\n"))
    assert any("反例" in m and "商品.在庫" in m for m in msgs(ck, ERROR))


def test_同じ減算を2回すると不足の確認は効かない():
    up = "  update { 商品[k].在庫 -= v | (k, v) in 明細 }\n"
    assert any("反例" in m for m in msgs(chk(mutate(CODE, up, up + up)), ERROR))


def test_状態がSでないと分かっていれば値を下げてよい():
    ck = chk(mutate(mutate(CODE, *REFLECT), "  注文.状態 = 完了\n", "  注文.状態 = 完了\n  注文.合計 = 0\n"))
    assert ck.ok, ck.diags
    ck = chk(mutate(mutate(CODE, *REFLECT), "  注文.状態 = 完了\n", "  注文.合計 = 0\n  注文.状態 = 完了\n"))
    assert any("合計 > 0" in m for m in msgs(ck, ERROR))


def test_Sで新規作成するなら値の確認が要る():
    ck = chk(mutate(mutate(CODE, "transition 注文: (new) → 受付", "transition 注文: (new) → 決済要求済"), "状態: 受付 }", "状態: 決済要求済 }"))
    assert any("合計 > 0" in m for m in msgs(ck, ERROR))


def test_分岐の中の書き込みで確認は崩れる():
    ck = chk(mutate(mutate(CODE, "  reads 注文.合計, 注文.状態\n  writes 注文.状態\n", "  reads 注文.合計, 注文.状態\n  writes 注文.状態, 注文.合計\n"),
                    "  注文.状態 = 決済要求済\n", "  if 注文.合計 > 100 {\n    注文.合計 = 0\n  }\n  注文.状態 = 決済要求済\n"))
    assert any("合計 > 0" in m for m in msgs(ck, ERROR))


def test_遷移した後は元の状態の確認が消える():
    ck = chk(mutate(mutate(CODE, "transition 注文: 受付 → 決済要求済", "transition 注文: 受付 → 決済要求済\n  transition 注文: 受付 → キャンセル"),
                    "  注文.状態 = 決済要求済\n", "  注文.状態 = 決済要求済\n  注文.状態 = キャンセル\n"))
    assert any(d.code == "遷移" and "受付" in d.msg for d in ck.diags if d.level == ERROR)


def test_同じデータの別の束縛の確認も遷移で消える():
    ck = chk(mutate(two_orders(PAY + "  let 別 = 注文[別ID] else { return 注文なし }\n  if 別.状態 != 受付 { return 状態違い }\n"),
                    "  注文.状態 = 決済要求済\n", "  注文.状態 = 決済要求済\n  別.状態 = 決済要求済\n"))
    assert any(d.code == "遷移" and "受付" in d.msg for d in ck.diags if d.level == ERROR)


def test_書けない形の不変条件はエラーになる():
    ck = chk(CODE + "\ninvariant 商品: 在庫 <= 100\n")
    assert any("まだ検査できません" in m for m in msgs(ck, ERROR))


def test_項目の更新は型を確かめる():
    ck = chk(mutate(mutate(CODE, *REFLECT), "  注文.状態 = 完了\n", "  注文.状態 = 完了\n  注文.合計 = 注文ID\n"))
    assert any("型の違う値" in m for m in msgs(ck, ERROR))


@needs_cargo
def test_項目へ数を書くと物理数へ変換される(tmp_path):
    ck = chk(mutate(mutate(CODE, *REFLECT), "  注文.状態 = 完了\n", "  注文.状態 = 完了\n  注文.合計 = 0\n"))
    write_crate(ck, generate(ck), "fn main() {}\n", tmp_path)
    r = subprocess.run(["cargo", "build", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]


def test_減らした後に確かめた不足の集まりは古い():
    chk_line = "  if 不足 is nonempty { return 在庫不足(不足) }\n"
    up = "  update { 商品[k].在庫 -= v | (k, v) in 明細 }\n"
    src = mutate(CODE, up, up + chk_line + up)
    assert chk(mutate(CODE, up, up)).ok
    assert any("反例" in m for m in msgs(chk(src), ERROR))


# ---- 外部作用とフローの重複・矛盾 ----
FOR = """
deed 引当 {
  input 明細: 商品数量 required, 注文ID: id required
  result 待ち解除結果
  effects 倉庫.引当

  for (k, v) in 明細 {
    request 倉庫.引当(k, v, idempotency_key = KEY)
  }
  return 反映済
}

nexus 引当の流れ {
  引当: 反映済 → reply, 注文なし → reply, 状態違い → reply
}
"""


def with_for(key):
    src = mutate(CODE, "effect 決済.請求(注文ID: id, 金額: money(JPY))",
                 'effect 決済.請求(注文ID: id, 金額: money(JPY))\neffect 倉庫.引当(商品ID: id, 数: 数量) no_wait "試し"')
    return chk(src + FOR.replace("KEY", key))


def test_for_の中の冪等キーは回す値と外の値の組にする():
    assert any("for で回す値によりません" in m for m in msgs(with_for("注文ID"), ERROR))
    assert any("for で回す値だけです" in m for m in msgs(with_for("k"), ERROR))  # 別の注文で同じ商品を回すとぶつかる
    assert not any("冪等" in m for m in msgs(with_for("(注文ID, k)"), ERROR))
    assert not any("冪等" in m for m in msgs(with_for("(k, 注文ID)"), ERROR))
    assert any("冪等キーは ID か文字列" in m for m in msgs(with_for("(注文ID, v)"), ERROR))


@needs_cargo
def test_冪等キーの組は区切りがあいまいにならない(tmp_path):
    ck = with_for("(注文ID, k)")
    assert ck.ready, ck.diags
    main = """fn main() {
    println!("{}", geas_app::冪等キーの組(&["a,b".to_string(), "c".to_string()]));
    println!("{}", geas_app::冪等キーの組(&["a".to_string(), "b\\",\\"c".to_string()]));
    println!("{}", geas_app::冪等キーの組(&["O1".to_string(), "P1".to_string()]));
}
"""
    write_crate(ck, generate(ck), main, tmp_path)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    a, b, c = r.stdout.splitlines()
    assert a != b and c == '["O1","P1"]', (a, b, c)


def test_冪等キーは_ID_か文字列():
    ck = chk(mutate(CODE, "idempotency_key = 注文ID)", "idempotency_key = 1)"))
    assert any("冪等キーは ID か文字列" in m for m in msgs(ck, ERROR))


def test_応答と処理を同じ結果に並べると止まる():
    ck = chk(mutate(CODE, "在庫不足 → reply, 商品なし", "在庫不足 → par { reply, 決済要求 }, 商品なし"))
    assert any("par に入れられません" in m for m in msgs(ck, ERROR))
    with pytest.raises(ParseError, match="par"):
        parse(mutate(CODE, "在庫不足 → reply, 商品なし", "在庫不足 → reply, 在庫不足 → 決済要求, 商品なし"))


NOTIFY = """
effect 通知.メール(注文ID: id) no_wait "通知は結果を待たない"
effect 通知.プッシュ(注文ID: id) no_wait "通知は結果を待たない"
result 通知結果 {
  要求済
}
deed メール通知 {
  input 注文ID: id required
  result 通知結果
  effects 通知.メール

  request 通知.メール(注文ID, idempotency_key = 注文ID)
  return 要求済
}
deed プッシュ通知 {
  input 注文ID: id required
  result 通知結果
  effects 通知.プッシュ

  request 通知.プッシュ(注文ID, idempotency_key = 注文ID)
  return 要求済
}
nexus メール {
  メール通知: 要求済 → reply
}
nexus プッシュ {
  プッシュ通知: 要求済 → reply
}
"""
FANOUT = ("受注: 受付 → 決済要求, 在庫不足", "受注: 受付 → par { メール通知, プッシュ通知 }, 在庫不足")


def fanout(*edits):
    return chk(mutate(CODE + NOTIFY, *edits))


def test_同じ結果に処理を続けて書くと_書いた順に起こす行き先になる():
    ck = fanout(*FANOUT)
    assert ck.ready, ck.diags
    assert build(ck)["処理"][0]["結果"][0] == ("受付", "並列［メール通知 ＋ プッシュ通知］")
    assert any("2回書かれています" in m for m in msgs(fanout(FANOUT[0], "受注: 受付 → par { メール通知, メール通知 }, 在庫不足"), ERROR))
    src = generate(ck)
    assert "vec![行き先::処理(\"メール通知\"), 行き先::処理(\"プッシュ通知\")]" in src


def test_par_の枝がデータを書くと止まる():
    ck = fanout(FANOUT[0], "受注: 受付 → par { メール通知, 決済要求 }, 在庫不足")
    assert any("par の枝「決済要求」は、データを書いています" in m for m in msgs(ck, ERROR))


@needs_cargo
def test_行き先が複数のフローも生成コードがビルドできる(tmp_path):
    ck = fanout(*FANOUT)
    write_crate(ck, generate(ck), "fn main() {}\n", tmp_path)
    r = subprocess.run(["cargo", "build", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]


def test_同じ処理を違う行き先で2つのネクサスに書くと止まる():
    ck = chk(CODE + "\nnexus 二重 {\n  受注: 受付 → reply, 在庫不足 → reply, 商品なし → reply, 顧客なし → reply\n}\n")
    assert any("違う行き先" in m for m in msgs(ck, ERROR))


def test_失敗の理由を宣言して待たないのは矛盾():
    ck = chk(mutate(CODE, "effect 決済.請求(注文ID: id, 金額: money(JPY))", 'effect 決済.請求(注文ID: id, 金額: money(JPY)) failures 残高不足 no_wait "試し"'))
    assert any("no_wait" in m for m in msgs(ck, ERROR))


# ---- 生成した Rust が必ずビルドできる ----
def test_定義の名前の重複は止まる():
    for old, new in [("entity 顧客 {", "entity 顧客 {\n  顧客ID: id unique\n}\n\nentity 顧客 {"),
                     ("  在庫: 在庫数 required\n", "  在庫: 在庫数 required\n  在庫: 非負金額\n"),
                     ("  input 注文ID: id required\n  reads 注文.合計", "  input 注文ID: id required\n  slot 注文ID: id\n  reads 注文.合計")]:
        assert any("2つ以上" in m for m in msgs(chk(mutate(CODE, old, new)), ERROR)), new


def test_未定義の型の名前は止まる():
    ck = chk(mutate(CODE, "  input 注文ID: id required\n  reads 注文.合計", "  input 注文ID: id required, 謎: 謎型\n  reads 注文.合計"))
    assert any("謎型" in m for m in msgs(ck, ERROR))


def test_最後に_return_が無いと止まる():
    ck = chk(mutate(CODE, "  request 決済.請求(注文ID, 注文.合計, idempotency_key = 注文ID)\n  return 要求済\n",
                    "  request 決済.請求(注文ID, 注文.合計, idempotency_key = 注文ID)\n"))
    assert any("return で終えて" in m for m in msgs(ck, ERROR))


def test_if_の条件は真偽():
    ck = chk(mutate(CODE, "  if 注文.合計 <= 0 { return 合計ゼロ }\n", "  if 注文.合計 <= 0 { return 合計ゼロ }\n  if 注文.合計 { return 合計ゼロ }\n"))
    assert any("if の条件" in m for m in msgs(ck, ERROR))


def test_update_の値の型を確かめる():
    src = CODE + BULK.replace("一括完了", "一括ゼロ").replace("writes 注文.状態", "writes 注文.合計").replace("注文[k].状態 = 完了", "注文[k].合計 = 完了")
    assert any("型の違う値" in m for m in msgs(chk(src), ERROR))


@needs_cargo
def test_割合の参照があってもホストつきでビルドできる(tmp_path):
    ck = chk(CODE)
    write_crate(ck, generate(ck), "fn main() {\n    let r = geas_app::host::有理を読む(\"1/10\", Some(0), Some(1)).unwrap();\n"
                "    assert_eq!(r.to_string(), \"1/10\");\n    assert!(geas_app::host::有理を読む(\"3/2\", Some(0), Some(1)).is_err());\n}\n", tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]


# ---- 誤検出と警告の漏れ ----
def test_update_の書き込みは_ほかで読んだ項目を消さない():
    src = CODE + BULK.replace("一括完了", "一括ゼロ").replace("writes 注文.状態", "reads 注文.合計\n  writes 注文.合計").replace(
        "  update { 注文[k].状態 = 完了 | k in 他 }\n", "  update { 注文[k].合計 = 1 | k in 他, 注文[k].合計 > 0 }\n")
    ck = chk(src)
    assert not any("注文.合計" in m and "使っていません" in m for m in msgs(ck, ERROR)), ck.diags


def test_使わない外部作用と届かない状態は警告():
    ck = chk(mutate(CODE, "effect 決済.請求(注文ID: id, 金額: money(JPY))", "effect 決済.請求(注文ID: id, 金額: money(JPY))\neffect 倉庫.引当(商品ID: id) no_wait \"試し\""))
    assert any("どの処理からも要求されていません" in m for m in msgs(ck, "警告"))
    ck = chk(mutate(CODE, "transition 注文: 受付 → 決済要求済", "transition 注文: 受付 → 受付"))
    assert any("決済要求済" in m and "進む処理" in m for m in msgs(ck, "警告"))


# ---- ネクサスで次の処理へは、結果の中身を名前と型で渡す ----
def test_結果の中身には名前が要る():
    with pytest.raises(ParseError, match="名前を付けて"):
        parse(mutate(CODE, "  受付(注文ID: id)\n", "  受付(id)\n"))


def test_結果の中身と次の処理の入力は名前と型で突き合わせる():
    assert any("中身「不足」を、処理 決済要求 は入力にもっていません" in m
               for m in msgs(chk(mutate(CODE, "在庫不足 → reply, 商品なし", "在庫不足 → 決済要求, 商品なし")), ERROR))
    ck = chk(mutate(CODE, "  受付(注文ID: id)\n", "  受付(番号: id)\n"))
    assert any("入力「注文ID」が、結果「受付」の中身にありません" in m for m in msgs(ck, ERROR))
    ck = chk(mutate(CODE, "  受付(注文ID: id)\n", "  受付(注文ID: ID集合)\n"))
    assert any("型" in m and "受付" in m for m in msgs(ck, ERROR))


@needs_cargo
def test_ホストは結果の中身を名前つきで返す(tmp_path):
    ck = chk(CODE)
    req = ('{"名前":"受注","文脈":{"税率":"1/10","新注文ID":"N1"},"入力":{"顧客ID":"C1","明細":{"A":2}},'
           '"保存":{"商品":{"A":{"商品ID":"A","在庫":5,"単価":100}},"顧客":{"C1":{"顧客ID":"C1"}}}}')
    write_crate(ck, generate(ck), f'fn main() {{\n    println!("{{}}", geas_app::host::呼ぶ(r#"{req}"#).unwrap());\n}}\n', tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]
    assert '"中身":{"注文ID":"N1"}' in r.stdout, r.stdout


# ---- 整数と金額は違う型。そのままでは混ぜず、money( )・int( ) で変えてから計算する ----
SUB = "  let 合計 = 小計 + round(小計 * 税率, floor, JPY)\n"


def test_整数と金額はそのままでは足し引きできない():
    ck = chk(mutate(CODE, SUB, SUB.replace("小計 + round", "小計 + 合計数 + 丸め").replace("  let 合計", "  let 合計数 = sum { v | (k, v) in 明細 }\n  let 合計")))
    assert any("整数と金額は、そのままでは足し引きできません" in m for m in msgs(ck, ERROR))


def test_金額どうしは掛けられない():
    ck = chk(mutate(CODE, "商品[k].単価 * v |", "商品[k].単価 * 商品[k].単価 |"))
    assert any("金額どうしは掛けられません" in m for m in msgs(ck, ERROR))


def test_整数の変数は金額の項目へそのまま入れられない():
    src = mutate(CODE, SUB, "  let 合計数 = sum { v | (k, v) in 明細 }\n" + SUB)
    ck = chk(mutate(src, "合計: 合計, 状態: 受付 }", "合計: 合計数, 状態: 受付 }"))
    assert any("型の違う値（Int）" in m for m in msgs(ck, ERROR))
    ck = chk(mutate(src, "合計: 合計, 状態: 受付 }", "合計: money(合計数), 状態: 受付 }"))
    assert ck.ok, ck.diags


def test_数字はどちらの型としても使える():
    assert chk(CODE).ok  # 合計 <= 0、在庫 < v など
    ck = chk(mutate(CODE, SUB, "  let 合計 = 小計 + 100\n"))
    assert ck.ok, ck.diags


def test_変換の向きを取り違えると止まる():
    ck = chk(mutate(CODE, SUB, "  let 合計 = money(小計)\n"))
    assert any("money( ) で変えられるのは整数だけ" in m for m in msgs(ck, ERROR))


@needs_cargo
def test_型の変換を使っても_Rust_がビルドできる(tmp_path):
    src = mutate(CODE, SUB, "  let 合計数 = sum { v | (k, v) in 明細 }\n  let 合計 = 小計 + money(合計数) + round(小計 * 税率, floor, JPY)\n")
    ck = chk(src)
    assert ck.ok, ck.diags
    write_crate(ck, generate(ck), "fn main() {}\n", tmp_path)
    r = subprocess.run(["cargo", "build", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]


# ---- すべては原則「無いかもしれない」。必須の約束は required ----
NOTE = """ltype 重さ値 = int 0..

entity メモ {
  メモID: id unique
  本文: text required
  題: text
  重さ: 重さ値
}
result 書いた結果 {
  書いた(メモID: id)
}
result 題の結果 {
  題あり(題: text)
  無し
  題なし
}
deed 書く {
  input 本文: text required
  writes メモ
  result 書いた結果
  effects none

  let メモ = create メモ { 本文: 本文 }
  return 書いた(メモ.メモID)
}
deed 題を見る {
  input メモID: id required
  reads メモ.題
  result 題の結果
  effects none

  let メモ = メモ[メモID] else { return 無し }
  let 題 = メモ.題 else { return 題なし }
  return 題あり(題)
}
nexus 流れ {
  書く: 書いた → 題を見る
  題を見る: 題あり → reply, 無し → reply, 題なし → reply
}
"""


def test_データの項目に_optional_は書かない():
    with pytest.raises(ParseError, match="required"):
        parse(NOTE.replace("  題: text\n", "  題: text optional\n"))


def test_無いかもしれない項目は確かめてから使う():
    assert chk(NOTE).ok, chk(NOTE).diags
    ck = chk(NOTE.replace("  let 題 = メモ.題 else { return 題なし }\n  return 題あり(題)", "  return 題あり(メモ.題)"))
    assert any("null の" in m for m in msgs(ck, ERROR))
    ck = chk(NOTE.replace("  let 題 = メモ.題 else { return 題なし }\n  return 題あり(題)",
                          "  if メモ.題 is null { return 題なし }\n  return 題あり(メモ.題)"))
    assert ck.ok, ck.diags


def test_必須の項目は新規作成で省けず_null_も入れられない():
    assert any("必須の項目「本文」" in m for m in msgs(chk(NOTE.replace("create メモ { 本文: 本文 }", "create メモ { 題: 本文 }")), ERROR))
    assert any("null は入れられません" in m for m in msgs(chk(NOTE.replace("create メモ { 本文: 本文 }", "create メモ { 本文: null }")), ERROR))
    assert any("is null" in m for m in msgs(chk(NOTE.replace("if メモ.題 is null", "if メモ.本文 is null").replace(
        "  let 題 = メモ.題 else { return 題なし }\n", "  if メモ.本文 is null { return 題なし }\n  let 題 = メモ.題 else { return 題なし }\n")), ERROR))


def test_null_を書いた後は確かめ直す():
    src = NOTE.replace("  reads メモ.題\n", "  reads メモ.題\n  writes メモ.題\n").replace(
        "  let 題 = メモ.題 else { return 題なし }\n  return 題あり(題)", "  let 題 = メモ.題 else { return 題なし }\n  メモ.題 = null\n  return 題あり(メモ.題)")
    assert any("null の" in m for m in msgs(chk(src), ERROR))


def test_null_は下限の不変条件を満たさない():
    ck = chk(NOTE + "\ninvariant メモ: 重さ >= 0\n")
    assert any("null は満たしません" in m for m in msgs(ck, ERROR))


def test_仕様書の必須は_required_とキーと状態():
    spec = build(chk(NOTE))
    req = {(d, f): r for d, f, _, r, _ in spec["データ"]}
    assert req[("メモ", "本文")] == "○" and req[("メモ", "メモID")] == "○" and req[("メモ", "題")] == ""


@needs_cargo
def test_ホストは_null_とキーが無いことを区別する(tmp_path):
    ck = chk(NOTE)
    call = lambda store: '{"名前":"題を見る","文脈":{},"入力":{"メモID":"M1"},"保存":{"メモ":{"M1":' + store + '}}}'
    main = "fn main() {\n" + "".join(
        f'    println!("{{:?}}", geas_app::host::呼ぶ(r#"{call(s)}"#));\n'
        for s in ['{"メモID":"M1","本文":"a","題":"t","重さ":null}', '{"メモID":"M1","本文":"a","題":null,"重さ":null}',
                  '{"メモID":"M1","本文":"a","重さ":null}']) + "}\n"
    write_crate(ck, generate(ck), main, tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]
    got = r.stdout.splitlines()
    assert '\\"結果\\":\\"題あり\\"' in got[0] and '\\"中身\\":{\\"題\\":\\"t\\"}' in got[0], got[0]
    assert '\\"結果\\":\\"題なし\\"' in got[1], got[1]
    assert got[2].startswith("Err(") and "題" in got[2], got[2]


NULLIN = """ltype 重さ値 = int 0..
ltype 軽い重さ = int 0..9

entity メモ {
  メモID: id unique
  本文: text required
  題: text
  重さ: 重さ値
}
result 書いた結果 {
  書いた(メモID: id)
  無題
}
deed 書く {
  input 本文: text required, 題: text, 重さ: 軽い重さ
  writes メモ
  result 書いた結果
  effects none

  let t = 題 else { return 無題 }
  if 重さ is null { return 無題 }
  if 重さ > 5 { return 無題 }
  let メモ = create メモ { 本文: 本文, 題: t, 重さ: 重さ }
  return 書いた(メモ.メモID)
}
deed 下書き {
  input 本文: text required, 題: text
  writes メモ
  result 書いた結果
  effects none

  let メモ = create メモ { 本文: 本文, 題: 題 }
  return 書いた(メモ.メモID)
}
nexus 流れ {
  書く: 書いた → reply, 無題 → reply
  下書き: 書いた → reply, 無題 → reply
}
"""


def test_入力は既定で無いかもしれず_確かめてから使う():
    assert chk(NULLIN).ok, chk(NULLIN).diags
    ck = chk(NULLIN.replace("  if 重さ is null { return 無題 }\n", ""))
    assert any("入力「重さ」は無いことがあります" in m for m in msgs(ck, ERROR))
    with pytest.raises(ParseError, match="required"):
        parse(NULLIN.replace("題: text, 重さ", "題: text optional, 重さ"))


def test_トリガの引数には必ず要る入力を渡す():
    ck = chk(mutate(CODE, "deed 決済要求 {\n  input 注文ID: id required\n", "deed 決済要求 {\n  input 注文ID: id\n"))
    assert any("required の入力を渡してください" in m for m in msgs(ck, ERROR))


@needs_cargo
def test_ホストは無いかもしれない入力のキー無しと_null_を同じに扱う(tmp_path):
    ck = chk(NULLIN)
    calls = [("書く", '{"本文":"a","題":"t","重さ":3}'), ("書く", '{"本文":"a","重さ":3}'), ("書く", '{"本文":"a","題":null,"重さ":3}'),
             ("書く", '{"本文":"a","題":"","重さ":3}'), ("書く", '{"題":"t","重さ":3}'), ("書く", '{"本文":null,"題":"t","重さ":3}'),
             ("書く", '{"本文":"a","題":"t","重さ":10}'), ("下書き", '{"本文":"a"}')]
    main = "fn main() {\n" + "".join(
        f'    println!("{{:?}}", geas_app::host::呼ぶ(r#"{{"名前":"{n}","文脈":{{"新メモID":"M1"}},"入力":{i},"保存":{{}}}}"#));\n'
        for n, i in calls) + "}\n"
    write_crate(ck, generate(ck), main, tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    got = r.stdout.splitlines()
    ok = lambda x, v: f'\\"結果\\":\\"{v}\\"' in x
    assert ok(got[0], "書いた") and ok(got[1], "無題") and ok(got[2], "無題"), got[:3]
    assert ok(got[3], "書いた"), got[3]  # 空文字は「ある」
    assert all(g.startswith("Err(") for g in got[4:7]), got[4:7]  # 必須の入力が無い・null、範囲の外は入力不正
    assert ok(got[7], "書いた") and '\\"題\\":null' in got[7], got[7]  # 無い入力は、そのまま null の項目へ


def test_ホストは宣言にない入力の項目を無いものとして扱う(tmp_path):
    ck = chk(NULLIN)
    calls = ['{"本文":"a","重さ":3}', '{"本文":"a","重さ":3,"余分":"x","題名":"typo"}']
    main = "fn main() {\n" + "".join(
        f'    println!("{{:?}}", geas_app::host::呼ぶ(r#"{{"名前":"書く","文脈":{{"新メモID":"M1"}},"入力":{i},"保存":{{}}}}"#));\n'
        for i in calls) + "}\n"
    write_crate(ck, generate(ck), main, tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    got = r.stdout.splitlines()
    assert got[0] == got[1] and '\\"結果\\":\\"無題\\"' in got[0], got  # 宣言にない項目は、来ていないのと同じ


TEXT = """ltype 利用者名 = text 1..20 of "a-z0-9_"

entity 利用者 {
  利用者ID: id unique
  名: 利用者名 required
}
result 登録結果 {
  登録した(利用者ID: id)
}
deed 登録する {
  input 名: 利用者名 required
  writes 利用者
  result 登録結果
  effects none

  let 利用者 = create 利用者 { 名: 名 }
  return 登録した(利用者.利用者ID)
}
nexus 流れ {
  登録する: 登録した → reply
}
"""


def test_文字列の長さと使える文字は仕様書の型に出る():
    from geas.ast import show_type
    ck = chk(TEXT)
    assert ck.ok, ck.diags
    t = ck.data["利用者"].fields[1].type
    assert show_type(t) == "文字列 1..20 文字種 [0-9_a-z]"


def 入力だけ型を変える(decl):
    """項目の型は利用者名のまま、入力の名前の型だけを、新しい論理型（decl）に変える"""
    src = mutate(TEXT, 'ltype 利用者名 = text 1..20 of "a-z0-9_"\n', 'ltype 利用者名 = text 1..20 of "a-z0-9_"\n' + decl + "\n")
    return mutate(src, "input 名: 利用者名", "input 名: 入力名")


def test_長さや文字が広い入力を項目へ入れると止まる():
    assert not msgs(chk(入力だけ型を変える('ltype 入力名 = text 1..10 of "a-z0-9_"')), ERROR)  # 狭いのはよい
    ck = chk(入力だけ型を変える('ltype 入力名 = text 1..30 of "a-z0-9_"'))
    assert any("長さ" in m and "収まる" in m for m in msgs(ck, ERROR)), ck.diags
    ck = chk(mutate(TEXT, "input 名: 利用者名", "input 名: text"))
    assert any("長さ" in m for m in msgs(ck, ERROR)), ck.diags
    ck = chk(入力だけ型を変える('ltype 入力名 = text 1..20 of "a-z0-9_A-Z"'))
    assert any("文字" in m and "収まる" in m for m in msgs(ck, ERROR)), ck.diags
    ck = chk(入力だけ型を変える("ltype 入力名 = text 1..20"))
    assert any("文字" in m for m in msgs(ck, ERROR)), ck.diags


def test_文字種の書き方の誤りは止まる():
    for bad in ('of "z-a"', 'of ""', 'of "a\\"'):
        with pytest.raises(ParseError):
            chk(mutate(TEXT, 'of "a-z0-9_"\n', bad + "\n"))


def test_ホストは入力の長さと文字を境界で確かめる(tmp_path):
    ck = chk(TEXT)
    calls = ['{"名":"abc_09"}', '{"名":""}', '{"名":"' + "a" * 21 + '"}', '{"名":"ABC"}', '{"名":"あ"}', '{"名":"' + "a" * 20 + '"}']
    main = "fn main() {\n" + "".join(
        f'    println!("{{:?}}", geas_app::host::呼ぶ(r#"{{"名前":"登録する","文脈":{{"新利用者ID":"U1"}},"入力":{i},"保存":{{}}}}"#));\n'
        for i in calls) + "}\n"
    write_crate(ck, generate(ck), main, tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    got = r.stdout.splitlines()
    assert '登録した' in got[0] and '登録した' in got[5], (got[0], got[5])
    assert all(g.startswith("Err(") for g in got[1:5]), got[1:5]  # 空・長すぎ・使えない文字は入力不正


TIMEC = """entity 約束 {
  約束ID: id unique
  期限: time
  待ち秒: int
}
result 約束結果 {
  約束した(約束ID: id)
}
deed 約束する {
  input 開始: time required
  slot 現在: time
  writes 約束
  result 約束結果
  effects none

  let 約束 = create 約束 { 期限: 現在 + 24 hour, 待ち秒: 現在 - 開始 }
  return 約束した(約束.約束ID)
}
nexus 流れ {
  約束する: 約束した → reply
}
"""


def test_時刻に期間を足し引きでき_時刻の差は秒数になる():
    assert chk(TIMEC).ok, chk(TIMEC).diags
    assert chk(mutate(TIMEC, "現在 + 24 hour", "現在 - 90 min")).ok
    assert chk(mutate(TIMEC, "現在 + 24 hour", "24 hour + 現在")).ok
    for bad in ("現在 + 開始", "現在 + 5", "24 hour", "現在 * 2"):
        ck = chk(mutate(TIMEC, "期限: 現在 + 24 hour", f"期限: {bad}"))
        assert msgs(ck, ERROR), bad
    ck = chk(mutate(TIMEC, "待ち秒: 現在 - 開始", "待ち秒: 24 hour - 現在"))
    assert msgs(ck, ERROR)
    for unit in ("day", "week", "month", "year"):  # 暦で数え方が変わる単位は、まだ書けない
        with pytest.raises(ParseError, match="暦で数え方が変わる"):
            chk(mutate(TIMEC, "24 hour", f"1 {unit}"))
    with pytest.raises(ParseError, match="暦で数え方が変わる"):  # 絞り込み・期限の単位も同じ
        chk(mutate(CODE, "deadline now + 30 min", "deadline now + 1 day"))
    with pytest.raises(ParseError, match="長すぎます"):
        chk(mutate(TIMEC, "24 hour", "99999999999 sec"))


def test_ホストは時刻の計算をして_範囲外の時刻は境界で止める(tmp_path):
    ck = chk(TIMEC)
    calls = [(1000, 400), (1000, 10**16), (10**16, 400)]
    main = "fn main() {\n" + "".join(
        f'    println!("{{:?}}", geas_app::host::呼ぶ(r#"{{"名前":"約束する","文脈":{{"新約束ID":"K1","現在":{n}}},"入力":{{"開始":{b}}},"保存":{{}}}}"#));\n'
        for n, b in calls) + "}\n"
    write_crate(ck, generate(ck), main, tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    got = r.stdout.splitlines()
    assert "87400" in got[0] and "600" in got[0], got[0]  # 期限 = 1000 + 86400、待ち = 1000 - 400
    assert got[1].startswith("Err(") and got[2].startswith("Err("), got[1:]


SLOT = """ltype 千以下の数 = int 0..1000
ltype 十以下の数 = int 0..10
ltype 倍率値 = int 1..100

entity 品 {
  品ID: id unique
  数: 千以下の数 required
}
result 置いた結果 {
  置いた(品ID: id)
}
deed 置く {
  input 数: 十以下の数 required
  slot 倍率: 倍率値
  writes 品
  result 置いた結果
  effects none

  let 品 = create 品 { 数: 数 * 倍率 }
  return 置いた(品.品ID)
}
nexus 流れ {
  置く: 置いた → reply
}
"""


@needs_cargo
def test_slot_も境界で宣言の範囲を確かめる(tmp_path):
    ck = chk(SLOT)
    assert ck.ready, ck.diags
    call = lambda k: f'{{"名前":"置く","文脈":{{"倍率":{k},"新品ID":"A"}},"入力":{{"数":10}},"保存":{{}}}}'
    main = "fn main() {\n" + "".join(f'    println!("{{:?}}", geas_app::host::呼ぶ(r#"{call(k)}"#));\n' for k in (100, 101, 0)) + "}\n"
    write_crate(ck, generate(ck), main, tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    got = r.stdout.splitlines()
    assert got[0].startswith("Ok(") and '\\"数\\":1000' in got[0], got[0]  # 範囲の端は通る
    assert got[1].startswith("Err(") and "倍率" in got[1], got[1]  # 宣言の外は処理を走らせない
    assert got[2].startswith("Err(") and "倍率" in got[2], got[2]


PART = """ltype 非負金額 = money(JPY) 0..
ltype 税抜の限り = money(JPY) 0..1000000

entity 品 {
  品ID: id unique
  価格: 非負金額 required
}
result 置いた結果 {
  置いた(品ID: id)
}
part 税込(x: 非負金額) -> 非負金額 guarantee result >= x
deed 置く {
  input 税抜: 税抜の限り required
  writes 品
  result 置いた結果
  effects none

  let 品 = create 品 { 価格: 税込(税抜) }
  return 置いた(品.品ID)
}
nexus 流れ {
  置く: 置いた → reply
}
"""


def test_部品の保証は_geas_の式で書き_引数と_result_だけを使う():
    ck = chk(PART)
    assert ck.ready, ck.diags
    assert build(ck)["部品"] == [("税込", "x: 金額(円) 0..", "金額(円) 0..", "result >= x")]
    with pytest.raises(ParseError, match="保証がありません"):
        parse(PART.replace(" guarantee result >= x", ""))
    assert any("戻り値（result）を使っていません" in m for m in msgs(chk(PART.replace("result >= x", "x >= 0")), ERROR))
    assert any("真偽の式" in m for m in msgs(chk(PART.replace("result >= x", "result + x")), ERROR))
    assert any("定義されていません" in m for m in msgs(chk(PART.replace("result >= x", "result >= y")), ERROR))


def test_部品の呼び出しは宣言と型が合わないと止まる():
    assert any("引数は 1 個" in m for m in msgs(chk(PART.replace("税込(税抜)", "税込(税抜, 税抜)")), ERROR))
    assert any("型が違います" in m for m in msgs(chk(PART.replace("税込(税抜)", "税込(int(税抜))")), ERROR))
    assert any("組み込みの部品と同じ名前" in m for m in msgs(chk(PART.replace("税込", "round")), ERROR))


@needs_cargo
def test_部品の保証は呼ぶたびに確かめ_違反は保存を変えずに止まる(tmp_path):
    ck = chk(PART)
    main = """use geas_app::*;
use geas_app::host::呼ぶ;
struct 良い;
impl 部品群 for 良い { fn 税込(&self, x: i64) -> i64 { x + x / 10 } }
struct 悪い;
impl 部品群 for 悪い { fn 税込(&self, x: i64) -> i64 { x - 1 } }
fn main() {
    let req = r#"{"名前":"置く","文脈":{"新品ID":"A"},"入力":{"税抜":1000},"保存":{}}"#;
    println!("{:?}", 呼ぶ(req, &良い));
    println!("{:?}", 呼ぶ(req, &悪い));
}
"""
    write_crate(ck, generate(ck), main, tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    ok, ng = r.stdout.splitlines()
    assert ok.startswith("Ok(") and '\\"価格\\":1100' in ok, ok
    assert ng.startswith("Err(") and "税込" in ng and "result >= x" in ng, ng


@needs_cargo
def test_組み込みの丸めは単位より近い値を返す(tmp_path):
    """組み込みの部品の保証は、宣言せず、ここで確かめる（切り捨て・切り上げは差が 1 未満、四捨五入は 1/2 以下）"""
    ck = chk(PART)
    main = """use geas_app::*;
fn main() {
    let mut bad = 0;
    for a in -50i128..50 {
        for b in 1i128..13 {
            let x = 有理 { 分子: a, 分母: b };
            let (lo, up, rn) = (floor(x), ceil(x), half_up(x));
            if !(lo * b <= a && a < (lo + 1) * b) { bad += 1; }
            if !((up - 1) * b < a && a <= up * b) { bad += 1; }
            if !(2 * (rn * b - a).abs() <= b) { bad += 1; }
        }
    }
    println!("{}", bad);
}
"""
    write_crate(ck, generate(ck), main, tmp_path)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    assert r.stdout.strip() == "0"


NULLQ = """ltype 重さ値 = int 0..
ltype 札の数 = int 1..9
ltype 札束 = 人 → 札の数

entity 人 {
  人ID: id unique
  名前: text
}
entity メモ {
  メモID: id unique
  書いた人: ref 人
  重さ: 重さ値
  札: 札束
}
query 一覧 {
  input 最小: int
  from メモ
  filter 重さ at_least 最小
  order 重さ newest
  select メモID, 書いた人.名前 as 名前, sum(札) as 札数, count(札) as 札の種類
  reads メモ.メモID, メモ.書いた人, 人.名前, メモ.重さ, メモ.札
}
"""


@needs_cargo
def test_null_になりうる項目の問い合わせも_Rust_がビルドできる(tmp_path):
    ck = chk(NULLQ)
    assert ck.ok, ck.diags
    main = """use geas_app::*;
fn main() {
    let mut s = 保存::default();
    s.人.insert("P".into(), 人 { 人ID: "P".into(), 名前: None });
    s.メモ.insert("A".into(), メモ { メモID: "A".into(), 書いた人: Some("P".into()), 重さ: None, 札: None });
    s.メモ.insert("B".into(), メモ { メモID: "B".into(), 書いた人: None, 重さ: Some(3), 札: Some([("P".to_string(), 2u8)].into()) });
    let all = 一覧(&一覧文脈 {}, &s, &一覧入力 { 最小: None });
    println!("{}", all.iter().map(|x| x.表示()).collect::<Vec<_>>().join(";"));
    println!("{}", 一覧(&一覧文脈 {}, &s, &一覧入力 { 最小: Some(1) }).len());
}
"""
    write_crate(ck, generate(ck), main, tmp_path)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    rows, n = r.stdout.splitlines()
    assert rows == "メモID=B/名前=null/札数=2/札の種類=1;メモID=A/名前=null/札数=0/札の種類=0", rows  # null は並びの最後
    assert n == "1"  # 重さが null のものは絞り込みを満たさない


PAGEQ = """ltype 重さ値 = int 0..
ltype 件数値 = int 1..100

entity メモ {
  メモID: id unique
  重さ: 重さ値
}
query 一覧 {
  input 件数: 件数値
  from メモ
  order 重さ newest
  page 件数
  reads メモ.重さ
}
query 先頭 {
  from メモ
  order 重さ oldest
  page 2
  reads メモ.重さ
}
"""


def test_page_は_order_が無いとエラーで_limit_は使えない():
    assert chk(PAGEQ).ok, chk(PAGEQ).diags
    ck = chk(mutate(PAGEQ, "  order 重さ oldest\n  page 2", "  page 2"))
    assert any("order が要ります" in m for m in msgs(ck, ERROR)), ck.diags
    with pytest.raises(ParseError, match="page に替わりました"):
        chk(mutate(PAGEQ, "page 2", "limit 2"))
    with pytest.raises(ParseError):
        chk(mutate(PAGEQ, "page 2", "page 0"))


def test_page_の件数は仕様書に出る():
    from geas.spec import query_rows
    rows = {q.name: query_rows(q) for q in chk(PAGEQ).p.queries}
    assert rows["先頭"]["ページ"] == "2件" and rows["一覧"]["ページ"] == "件数（件数が無ければ全件）"


@needs_cargo
def test_page_は並びのうえで先頭の1ページを返す(tmp_path):
    ck = chk(PAGEQ)
    main = """use geas_app::*;
fn main() {
    let mut s = 保存::default();
    for (id, w) in [("A", 1i64), ("B", 5), ("C", 3)] { s.メモ.insert(id.into(), メモ { メモID: id.into(), 重さ: Some(w as _) }); }
    println!("{:?}", 先頭(&先頭文脈 {}, &s, &先頭入力 {}));
    println!("{:?}", 一覧(&一覧文脈 {}, &s, &一覧入力 { 件数: Some(2) }));
}
"""
    write_crate(ck, generate(ck), main, tmp_path)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    a, b = r.stdout.splitlines()
    assert a == '["A", "C"]' and b == '["B", "C"]', (a, b)  # oldest の先頭2件／newest の先頭2件


def test_問いが残っているうちは生成しない():
    ck = chk(mutate(CODE, "let 合計 = 小計 + round(小計 * 税率, floor, JPY)", "let 合計 = 小計 + 小計 * 税率"))
    assert ck.ok and not ck.ready
    with pytest.raises(AssertionError, match="問い"):
        generate(ck)


# ---- 論理型（ltype）と entity ----
LTYPE = """ltype 数量 = int 1..99
ltype 商品数量 = 商品 → 数量

entity 商品 {
  商品ID: id unique
  在庫: 数量 required
}

entity 注文 {
  注文ID: id unique
  明細: 商品数量 required
}
"""


def test_ltype_は使った場所に定義が写される():
    p = parse(LTYPE)
    stock = next(f for d in p.data if d.name == "商品" for f in d.fields if f.name == "在庫")
    assert stock.type.kind == "整数" and (stock.type.range.lo, stock.type.range.hi) == (1, 99)
    lines = next(f for d in p.data if d.name == "注文" for f in d.fields if f.name == "明細")
    assert lines.type.kind == "写像" and lines.type.key == "商品" and lines.type.value.range.hi == 99


def test_ltype_の名前の重複と組み込み名は止まる():
    with pytest.raises(ParseError, match="2つ以上"):
        parse("ltype 数量 = int 1..99\nltype 数量 = int 1..9\n")
    with pytest.raises(ParseError, match="組み込み"):
        parse("ltype int = int 1..99\n")


def test_data_は_entity_に改名した():
    with pytest.raises(ParseError, match="entity"):
        parse("data 商品 {\n  商品ID: id unique\n}\n")


def test_範囲や組み合わせの直書きは止まり_名前を付けるよう案内する():
    for ty in ("int 1..99", "text 1..20", "money(JPY) 0..", "set of id", "商品 → 数量"):
        with pytest.raises(ParseError, match="ltype"):
            parse(f"entity 商品 {{\n  商品ID: id unique\n  項目: {ty}\n}}\n")
    parse("entity 商品 {\n  商品ID: id unique\n  項目: int\n  名: text\n  金: money(JPY)\n}\n")  # 約束のない組み込みはそのまま書ける


# ---- store（保存先）は entity の外に書く ----
STORE = """entity メモ {
  メモID: id unique
  本文: text required
}
store メモ in Dropbox
"""


def test_store_を書くと_未同期の状態が足される():
    p = parse(STORE)
    assert p.data[0].store == "Dropbox"
    assert any(s.name == "記録状態" for s in p.states)
    assert any(f.name == "記録" for f in p.data[0].fields)


def test_store_が無ければ_保存の状態は足されない():
    p = parse(STORE.replace("store メモ in Dropbox\n", ""))
    assert p.data[0].store is None
    assert not any(s.name == "記録状態" for s in p.states)


def test_store_の誤りは止まる():
    with pytest.raises(ParseError, match="entity がありません"):
        parse(STORE.replace("store メモ", "store 謎"))
    with pytest.raises(ParseError, match="2つ以上"):
        parse(STORE + "store メモ in 端末\n")
    with pytest.raises(ParseError, match="別の行"):
        parse(STORE.replace("entity メモ {", "entity メモ store Dropbox {").replace("store メモ in Dropbox\n", ""))


# ---- 合成型 ----
COMP = """ltype 横 = int 0..100
ltype 点 { x: 横, y: 横 }
ltype 線 {
  始点: 点
  終点: 点
}

entity 地図 {
  地図ID: id unique
  位置: 点 required
  道: 線 required
}
result 置いた結果 {
  置いた(地図ID: id)
}
result 読んだ結果 {
  読んだ(地図ID: id)
  なし
  右寄り
}
deed 置く {
  input 位置: 点 required
  writes 地図
  result 置いた結果
  effects none

  let 地図 = create 地図 { 位置: 位置, 道: 線 { 始点: 位置, 終点: 点 { x: 位置.x, y: 5 } } }
  return 置いた(地図.地図ID)
}
deed 横を見る {
  input 地図ID: id required
  reads 地図.道
  result 読んだ結果
  effects none

  let 地図 = 地図[地図ID] else { return なし }
  if 地図.道.終点.x > 50 { return 右寄り }
  return 読んだ(地図ID)
}
nexus 流れ {
  置く: 置いた → reply
  横を見る: 読んだ → reply, なし → reply, 右寄り → reply
}
"""


def test_合成型は項目と入力に使え_成分は平らに読める():
    ck = chk(COMP)
    assert ck.ok and ck.ready, ck.diags
    sp = build(ck)
    assert ("地図", "位置", "点（x: 整数 0..100、y: 整数 0..100）", "○", "") in sp["データ"]


def test_合成型の誤りは止まる():
    # 成分の書き漏れ・余計な成分・型の違い
    ck = chk(mutate(COMP, "点 { x: 位置.x, y: 5 }", "点 { x: 位置.x }"))
    assert any("成分「y」がありません" in m for m in msgs(ck, ERROR))
    ck = chk(mutate(COMP, "点 { x: 位置.x, y: 5 }", "点 { x: 位置.x, y: 5, z: 1 }"))
    assert any("成分「z」がありません" in m for m in msgs(ck, ERROR))
    ck = chk(mutate(COMP, "点 { x: 位置.x, y: 5 }", "点 { x: 位置.x, y: 位置 }"))
    assert any("型の違う値" in m for m in msgs(ck, ERROR)), ck.diags
    # 範囲が収まると示せない（入力は 0..1000 で、成分は 0..100）
    wide = mutate(COMP, "ltype 点 {", "ltype 広い横 = int 0..1000\nltype 点 {")
    ck = chk(mutate(mutate(wide, "deed 置く {\n  input 位置: 点 required", "deed 置く {\n  input 位置: 点 required, 幅: 広い横 required"), "点 { x: 位置.x, y: 5 }", "点 { x: 位置.x, y: 幅 }"))
    assert any("成分の範囲に収まると示せません" in m for m in msgs(ck, ERROR)), ck.diags
    # 存在しない成分を読む
    ck = chk(mutate(COMP, "地図.道.終点.x > 50", "地図.道.終点.z > 50"))
    assert any("成分「z」がありません" in m for m in msgs(ck, ERROR))


def test_合成型は結果の中身には使えない():
    ck = chk(mutate(COMP, "読んだ(地図ID: id)", "読んだ(地図ID: id, 場所: 点)"))
    assert any("いまは項目と処理の入力・slot にだけ" in m for m in msgs(ck, ERROR)), ck.diags


def test_合成型の宣言の誤りは止まる():
    for src, msg in [("ltype 点 {\n  x: int\n  x: int\n}\n", "2つ以上"),
                     ("ltype 点 {\n}\n", "成分がありません"),
                     ("ltype 横 = int 0..9\nltype 横束 = 商品 → 横\nltype 点 {\n  束: 横束\n}\n", "使えません")]:
        with pytest.raises(ParseError, match=msg):
            parse(src)


@needs_cargo
def test_合成型はホストの境界で成分ごとに確かめる(tmp_path):
    ck = chk(COMP)
    call = lambda n, ctx, i, store: ('    println!("{:?}", geas_app::host::呼ぶ(r#"{"名前":"' + n + '","文脈":' + ctx + ',"入力":' + i + ',"保存":' + store + '}"#));\n')
    rec = lambda x: '{"地図":{"G1":{"地図ID":"G1","位置":{"x":1,"y":2},"道":{"始点":{"x":1,"y":2},"終点":{"x":' + str(x) + ',"y":5}}}}}'
    main = "fn main() {\n" + "".join([
        call("置く", '{"新地図ID":"G1"}', '{"位置":{"x":10,"y":20}}', "{}"),
        call("置く", '{"新地図ID":"G1"}', '{"位置":{"x":10,"y":200}}', "{}"),
        call("置く", '{"新地図ID":"G1"}', '{"位置":{"x":10}}', "{}"),
        call("置く", '{"新地図ID":"G1"}', '{"位置":{"x":10,"y":20,"z":1}}', "{}"),
        call("横を見る", "{}", '{"地図ID":"G1"}', rec(5)),
        call("横を見る", "{}", '{"地図ID":"G1"}', rec(70)),
    ]) + "}\n"
    write_crate(ck, generate(ck), main, tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    got = r.stdout.splitlines()
    assert '\\"位置\\":{\\"x\\":10,\\"y\\":20}' in got[0] and '\\"終点\\":{\\"x\\":10,\\"y\\":5}' in got[0], got[0]
    assert "範囲外" in got[1] and "位置.y" in got[1], got[1]
    assert got[2].startswith("Err(") and "y" in got[2], got[2]
    assert got[3].startswith("Ok(") and "置いた" in got[3]  # 宣言にない成分は来ていないものとして扱う
    assert "読んだ" in got[4] and "右寄り" in got[5], got[4:6]


# ---- vtime（成分は未指定にできる）----
VDT = """entity 約束 {
  約束ID: id unique
  期日: vtime required
}
result 決めた結果 {
  決めた(約束ID: id)
}
result 月結果 {
  未指定
  前半
  後半
  なし
}
deed 決める {
  input 期日: vtime required
  writes 約束
  result 決めた結果
  effects none

  let 約束 = create 約束 { 期日: 期日 }
  return 決めた(約束.約束ID)
}
deed 誕生日にする {
  writes 約束
  result 決めた結果
  effects none

  let 約束 = create 約束 { 期日: vtime { month: 10, day: 26 } }
  return 決めた(約束.約束ID)
}
deed 月を見る {
  input 約束ID: id required
  reads 約束.期日
  result 月結果
  effects none

  let 約束 = 約束[約束ID] else { return なし }
  if 約束.期日.month is null { return 未指定 }
  if 約束.期日.month > 6 { return 後半 }
  return 前半
}
nexus 流れ {
  決める: 決めた → reply
  誕生日にする: 決めた → reply
  月を見る: 前半 → reply, 後半 → reply, 未指定 → reply, なし → reply
}
"""


def test_vtimeの成分は未指定にでき_確かめずに読むと止まる():
    ck = chk(VDT)
    assert ck.ok and ck.ready, ck.diags
    sp = build(ck)
    assert any(r[1] == "期日" and r[2].startswith("vtime（year: 整数 1..9999") for r in sp["データ"]), sp["データ"]
    ck = chk(mutate(VDT, "  if 約束.期日.month is null { return 未指定 }\n", ""))
    assert any("未指定かもしれない成分" in m for m in msgs(ck, ERROR)), ck.diags
    # 必須の成分を持つ合成型では、書き漏れがエラー。null を書くのも止まる
    ck = chk("ltype 点 {\n  x: int\n  y: int optional\n}\n" + mutate(VDT, "vtime { month: 10, day: 26 }", "点 { y: 1 }").replace("期日: vtime required", "期日: 点 required").replace("input 期日: vtime required", "input 期日: 点 required").replace("約束.期日.month", "約束.期日.y"))
    assert any("成分「x」がありません" in m for m in msgs(ck, ERROR)), ck.diags


def test_vtimeの成分の範囲は構造の上限だけ():
    ck = chk(mutate(VDT, "vtime { month: 10, day: 26 }", "vtime { month: 13, day: 26 }"))
    assert any("成分の範囲に収まると示せません" in m for m in msgs(ck, ERROR)), ck.diags


@needs_cargo
def test_vtimeはホストの境界で成分ごとに確かめ_未指定はnull(tmp_path):
    ck = chk(VDT)
    call = lambda n, ctx, i, store: ('    println!("{:?}", geas_app::host::呼ぶ(r#"{"名前":"' + n + '","文脈":' + ctx + ',"入力":' + i + ',"保存":' + store + '}"#));\n')
    rec = lambda body: '{"約束":{"A1":{"約束ID":"A1","期日":' + body + '}}}'
    main = "fn main() {\n" + "".join([
        call("決める", '{"新約束ID":"A1"}', '{"期日":{"year":2026,"month":10}}', "{}"),
        call("決める", '{"新約束ID":"A1"}', '{"期日":{"month":13}}', "{}"),
        call("誕生日にする", '{"新約束ID":"A1"}', "{}", "{}"),
        call("月を見る", "{}", '{"約束ID":"A1"}', rec('{"month":null}')),
        call("月を見る", "{}", '{"約束ID":"A1"}', rec('{"month":3}')),
        call("月を見る", "{}", '{"約束ID":"A1"}', rec('{"month":9}')),
    ]) + "}\n"
    write_crate(ck, generate(ck), main, tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    got = r.stdout.splitlines()
    assert '\\"month\\":10' in got[0] and '\\"day\\":null' in got[0] and '\\"year\\":2026' in got[0], got[0]
    assert "範囲外" in got[1] and "期日.month" in got[1], got[1]
    assert '\\"month\\":10' in got[2] and '\\"day\\":26' in got[2] and '\\"year\\":null' in got[2], got[2]
    assert "未指定" in got[3] and "前半" in got[4] and "後半" in got[5], got[3:6]


CONVERT = PART.replace("part 税込(x: 非負金額) -> 非負金額 guarantee result >= x", "convert 税込(x: 非負金額) -> 非負金額 = x + x")


def test_convert_は式が定義で_外の部品も保証も要らない():
    ck = chk(CONVERT)
    assert ck.ready, ck.diags
    assert build(ck)["部品"] == [("税込", "x: 金額(円) 0..", "金額(円) 0..", "= x + x")]
    assert not ck.infos["置く"].parts
    assert "trait 部品群" not in generate(ck)


def test_convert_の式の型が戻り値と違うと止まる():
    bad = CONVERT.replace("= x + x", "= int(x)")
    assert any("戻り値の型と違います" in m or "戻り値の型（" in m for m in msgs(chk(bad), ERROR))


@needs_cargo
def test_convert_は生成コードの中で計算され_実行時の範囲検査は出さない(tmp_path):
    ck = chk(CONVERT)
    write_crate(ck, generate(ck), "fn main() {}\n", tmp_path)
    r = subprocess.run(["cargo", "build", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]


def test_convert_の結果が範囲に収まると示せなければ検査で止まる():
    bad = CONVERT.replace("convert 税込(x: 非負金額) -> 非負金額", "convert 税込(x: 非負金額) -> 税抜の限り")
    assert any("収まると示せません" in m for m in msgs(chk(bad), ERROR))
    ok = CONVERT.replace("convert 税込(x: 非負金額) -> 非負金額 = x + x", "convert 税込(x: 税抜の限り) -> 非負金額 = x + x")
    assert chk(ok).ready
    assert "保証違反(\"税込\"" not in generate(chk(ok))


DIVIDE = """ltype 日数 = int 0..
ltype 週数 = int 0..
ltype 余り日 = int 0..6

convert 週(d: 日数) -> 週数 = d / 7
convert 端(d: 日数) -> 余り日 = d % 7
"""


def test_整数の割り算は正の定数で切り捨て_範囲は端を割って示す():
    ck = chk(DIVIDE.replace("convert 端(d: 日数) -> 余り日 = d % 7\n", ""))
    assert ck.ready, ck.diags
    with pytest.raises(ParseError, match="正の整数の定数"):
        parse(DIVIDE.replace("d / 7", "d / 0", 1))
    with pytest.raises(ParseError, match="正の整数の定数"):
        parse(DIVIDE.replace("d / 7", "d / d", 1))
    assert any("割り算できるのは整数だけ" in m for m in msgs(chk(DIVIDE.replace("ltype 日数 = int 0..", "ltype 日数 = money(JPY) 0..").replace("convert 端(d: 日数) -> 余り日 = d % 7\n", "")), ERROR))


def test_余りの範囲は静的に示せる():
    assert chk(DIVIDE).ready
    bad = DIVIDE.replace("ltype 余り日 = int 0..6", "ltype 余り日 = int 0..5")
    assert any("収まると示せません" in m for m in msgs(chk(bad), ERROR))


@needs_cargo
def test_割り算と余りは生成コードで動く(tmp_path):
    ck = chk(DIVIDE)
    write_crate(ck, generate(ck), "fn main() {}\n", tmp_path)
    r = subprocess.run(["cargo", "build", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]


UTC = """convert 実在日時(v: vtime fill { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 }) -> time = time(
  (v.day + (153 * (v.month + 12 * ((14 - v.month) / 12) - 3) + 2) / 5 + 365 * (v.year + 4800 - (14 - v.month) / 12)
   + (v.year + 4800 - (14 - v.month) / 12) / 4 - (v.year + 4800 - (14 - v.month) / 12) / 100
   + (v.year + 4800 - (14 - v.month) / 12) / 400 - 32045 - 2440588) * 86400 + v.hour * 3600 + v.minute * 60 + v.second)

entity 予定 {
  予定ID: id unique
  時刻: time required
}
result 予約結果 {
  予約済(予定ID: id)
}
deed 予約 {
  input 日時: vtime required
  writes 予定
  result 予約結果
  effects none

  let 予定 = create 予定 { 時刻: 実在日時(日時) }
  return 予約済(予定.予定ID)
}
nexus 流れ {
  予約: 予約済 → reply
}
"""


def test_vtimeを補うルールつきで実在日時へ変える():
    ck = chk(UTC)
    assert ck.ready, ck.diags


def test_補うルールが無い成分を読むと違反になる():
    bad = UTC.replace("fill { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 }", "fill { year: 1970, month: 1, day: 1, hour: 0, minute: 0 }")
    assert any("未指定かもしれない成分" in m for m in msgs(chk(bad), ERROR))
    bad2 = UTC.replace("hour: 0,", "hour: 99,")
    assert any("収まりません" in m for m in msgs(chk(bad2), ERROR))


@needs_cargo
def test_vtimeの変換は生成コードで動く(tmp_path):
    ck = chk(UTC)
    write_crate(ck, generate(ck), "fn main() {}\n", tmp_path)
    r = subprocess.run(["cargo", "build", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-1500:]


@needs_cargo
def test_vtimeの変換は暦どおりの秒になり_未指定は補うルールの値になる(tmp_path):
    ck = chk(UTC)
    main = """use geas_app::*;
fn t(日時: vtime) -> i64 {
    let ctx = 予約文脈 { 新予定ID: "e1".to_string() };
    let o = 予約(&ctx, &保存::default(), &予約入力 { 日時 });
    match &o.書込[0] { 書込::予定_新規(r) => r.時刻 }
}
fn main() {
    println!("{}", t(vtime { year: Some(2026), month: Some(10), day: Some(26), hour: Some(3), minute: Some(4), second: Some(5) }));
    println!("{}", t(vtime { year: Some(2000), month: None, day: None, hour: None, minute: None, second: None }));
    println!("{}", t(vtime { year: None, month: None, day: None, hour: None, minute: None, second: None }));
}
"""
    write_crate(ck, generate(ck), main, tmp_path)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    assert r.stdout.split() == ["1792983845", "946684800", "0"], r.stdout


def test_仕様書にフロー図が出る_並列は_プラスのゲートウェイ():
    ck = fanout(*FANOUT)
    md = render(ck, build(ck))
    assert "```mermaid" in md and "flowchart LR" in md
    assert md.count('{"＋"}') == 1
    assert '-- "受付" -->' in md and "応答（reply）" in md
    plain = render(chk(CODE), build(chk(CODE)))
    assert '{"＋"}' not in plain and '{"×"}' in plain and "待つ 決済待ち" in plain


def test_説明コメントは意味を変えず_仕様書の名前と表示名の表になる():
    ck = chk(mutate(CODE, "entity 顧客 {", "/// 買う人\n/// @en Customer\nentity 顧客 {"))
    assert ck.ready, ck.diags
    assert ("顧客", "既定: 買う人 / en: Customer") in build(ck)["名前"]
    assert "## 名前と表示名" in render(ck, build(ck))
    assert generate(ck) == generate(chk(CODE))  # 説明コメントがあってもなくても、生成されるコードは同じ
    assert "名前と表示名" not in render(chk(CODE), build(chk(CODE)))
