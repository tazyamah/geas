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
    ck = chk(mutate(CODE, "合計 = 小計 + 丸め(小計 * 税率, 切り捨て, 円)", "合計 = 小計 + 小計 * 税率"))
    assert any("端数をどう丸めますか" in m for m in msgs(ck, QUESTION))


def test_結果に行き先が無いと止まる():
    ck = chk(mutate(CODE, ", 顧客なし → 応答\n", "\n"))
    assert any("顧客なし" in m and "行き先" in m for m in msgs(ck, ERROR))


def test_冪等キーが無いと止まる():
    ck = chk(mutate(CODE, ", 冪等キー = 注文ID)", ")"))
    assert any("冪等キー" in m for m in msgs(ck, ERROR))


def test_宣言に無い遷移は止まる():
    ck = chk(mutate(CODE, "    注文.状態 = 決済要求済", "    注文.状態 = 完了"))
    assert any("遷移" in d.code and "完了" in d.msg for d in ck.diags if d.level == ERROR)


def test_元の状態の確認が無いと止まる():
    ck = chk(mutate(CODE, "    もし 注文.状態 != 受付 なら 返す 状態違い\n", ""))
    assert any("確認" in m for m in msgs(ck, ERROR))


def test_在庫不足の判定を削ると不変条件が示せない():
    ck = chk(mutate(CODE, "    もし 不足 が 空でない なら 返す 在庫不足(不足)\n", ""))
    assert any("反例" in m for m in msgs(ck, ERROR))


def test_書く項目の宣言漏れは止まる():
    ck = chk(mutate(CODE, "  書く 商品.在庫, 注文\n", "  書く 注文\n"))
    assert any("商品.在庫" in m for m in msgs(ck, ERROR))


def test_合計ゼロの確認を削ると不変条件が示せない():
    ck = chk(mutate(CODE, "    もし 注文.合計 <= 0 なら 返す 合計ゼロ\n", ""))
    assert any("合計 > 0" in m for m in msgs(ck, ERROR))


def test_存在を確かめない参照は問いになる():
    ck = chk(mutate(CODE, "    顧客 = 顧客[顧客ID] なければ 返す 顧客なし", "    顧客 = 顧客[顧客ID]"))
    assert any("存在しない" in m for m in msgs(ck, QUESTION))


def test_構文エラーは行番号つき():
    with pytest.raises(ParseError, match="行目"):
        parse("データ 商品 {\n  在庫 整数\n}\n")


# ---- Rust 生成 ----
def test_エラーのあるコードからは生成しない():
    with pytest.raises(AssertionError):
        generate(chk(mutate(CODE, ", 冪等キー = 注文ID)", ")")))


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
    ck = chk(mutate(CODE, "  期限 現在 + 30 分 → 決済の期限切れ\n", ""))
    assert any("期限がありません" in m for m in msgs(ck, ERROR))


def test_待つ先のトリガが未定義だと止まる():
    ck = chk(mutate(CODE, "要求済 → 待つ 決済待ち", "要求済 → 待つ 承認待ち"))
    assert any("承認待ち" in m for m in msgs(ck, ERROR))


def test_トリガの行き先が未定義の処理だと止まる():
    ck = chk(mutate(CODE, "→ 決済の期限切れ\n}", "→ 期限切れ処理\n}"))
    assert any("期限切れ処理" in m for m in msgs(ck, ERROR))


def test_トリガの引数と処理の入力が合わないと止まる():
    ck = chk(mutate(CODE, "トリガ 決済待ち(注文ID: ID)", "トリガ 決済待ち(注文: ID)"))
    assert any("合いません" in m or "入力にもっていません" in m for m in msgs(ck, ERROR))


def test_計画にあるトリガの行き先がコードと違えば検出する():
    plan = mutate(PLAN, "| 期限 現在+30分 | 決済の期限切れ |", "| 期限 現在+30分 | 決済結果の反映 |")
    fs = compare_static(build(chk(CODE)), read_plan(plan))
    assert any(f.kind == DIFF and "トリガ" in f.where for f in fs)


def test_入力の範囲が項目より広いと止まる():
    ck = chk(mutate(CODE, "入力 顧客ID: ID, 明細: 商品 → 整数 1..99", "入力 顧客ID: ID, 明細: 商品 → 整数 1..200"))
    assert any("範囲の広い入力" in m for m in msgs(ck, ERROR))


def test_トリガの型の食い違いと期限の誤りは止まる():
    assert any("型" in m for m in msgs(chk(mutate(CODE, "トリガ 決済待ち(注文ID: ID)", "トリガ 決済待ち(注文ID: 整数)")), ERROR))
    assert any("期限が複数" in m for m in msgs(chk(mutate(CODE, "  期限 現在 + 30 分 → 決済の期限切れ", "  期限 現在 + 30 分 → 決済の期限切れ\n  期限 現在 + 1 時間 → 決済の期限切れ")), ERROR))
    assert any("0より長く" in m for m in msgs(chk(mutate(CODE, "期限 現在 + 30 分", "期限 現在 + 0 分")), ERROR))


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

