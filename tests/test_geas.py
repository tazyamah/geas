import shutil
import subprocess
import sys
from pathlib import Path

import pytest

from geas.ast import show_type
from geas.check import ERROR, QUESTION, check
from geas.examples import HOST_MOD, compare_examples, run_crate, runner_source, write_crate
from geas.hostgen import host_source
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
    ck = chk(mutate(CODE, "let 合計 = 小計 + 丸め(小計 * 税率, 切り捨て, 円)", "let 合計 = 小計 + 小計 * 税率"))
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
        parse("data 商品 {\n  在庫 int\n}\n")


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
    ck = chk(mutate(CODE, "input 顧客ID: id, 明細: 商品 → int 1..99", "input 顧客ID: id, 明細: 商品 → int 1..200"))
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
QUERY = """data メモ {
  メモID: id unique
  状態: メモ状態
  重さ: int 0..
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
    assert "if !((r.重さ as i128) >= 10) { return false; }" in src


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
MEMO = """data メモ {
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
    assert row["条件"] == ["本文 含む(大小無視,かな無視,幅無視) 単語"]
    plain = build(chk(MEMO.replace(" ignore_case ignore_kana ignore_width", "")))["問い合わせ"][0]
    assert plain["条件"] == ["本文 含む 単語"]


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
TWO = "deed 決済要求 {\n  input 注文ID: id, 別ID: id\n"


def two_orders(body):
    return mutate(mutate(CODE, "deed 決済要求 {\n  input 注文ID: id\n", TWO), PAY, body)


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
  input 他: set of id
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
  input 明細: 商品 → int 1..99, 注文ID: id
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
    src = mutate(CODE, "effect 決済.請求(注文ID: id, 金額: money(円))",
                 'effect 決済.請求(注文ID: id, 金額: money(円))\neffect 倉庫.引当(商品ID: id, 数: int 1..99) no_wait "試し"')
    return chk(src + FOR.replace("KEY", key))


def test_for_の中の冪等キーは回す値によらないと止まる():
    assert any("for で回す値によりません" in m for m in msgs(with_for("注文ID"), ERROR))
    assert not any("冪等" in m for m in msgs(with_for("k"), ERROR))


def test_冪等キーは_ID_か文字列():
    ck = chk(mutate(CODE, "idempotency_key = 注文ID)", "idempotency_key = 1)"))
    assert any("冪等キーは ID か文字列" in m for m in msgs(ck, ERROR))


def test_同じ結果に行き先を2つ書くと止まる():
    ck = chk(mutate(CODE, "在庫不足 → reply, 商品なし", "在庫不足 → reply, 在庫不足 → 決済要求, 商品なし"))
    assert any("行き先が2つ以上" in m for m in msgs(ck, ERROR))


def test_同じ処理を違う行き先で2つのネクサスに書くと止まる():
    ck = chk(CODE + "\nnexus 二重 {\n  受注: 受付 → reply, 在庫不足 → reply, 商品なし → reply, 顧客なし → reply\n}\n")
    assert any("違う行き先" in m for m in msgs(ck, ERROR))


def test_失敗の理由を宣言して待たないのは矛盾():
    ck = chk(mutate(CODE, "effect 決済.請求(注文ID: id, 金額: money(円))", 'effect 決済.請求(注文ID: id, 金額: money(円)) failures 残高不足 no_wait "試し"'))
    assert any("no_wait" in m for m in msgs(ck, ERROR))


# ---- 生成した Rust が必ずビルドできる ----
def test_定義の名前の重複は止まる():
    for old, new in [("data 顧客 {", "data 顧客 {\n  顧客ID: id unique\n}\n\ndata 顧客 {"),
                     ("  在庫: int 0..\n", "  在庫: int 0..\n  在庫: money(円) 0..\n"),
                     ("  input 注文ID: id\n  reads 注文.合計", "  input 注文ID: id\n  slot 注文ID: id\n  reads 注文.合計")]:
        assert any("2つ以上" in m for m in msgs(chk(mutate(CODE, old, new)), ERROR)), new


def test_未定義の型の名前は止まる():
    ck = chk(mutate(CODE, "  input 注文ID: id\n  reads 注文.合計", "  input 注文ID: id, 謎: 謎型\n  reads 注文.合計"))
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
