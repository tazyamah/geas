# SPDX-License-Identifier: GPL-3.0-or-later
import argparse
import sys
import tempfile
from pathlib import Path

from .check import ERROR, check
from .servergen import write_server
from .extgen import ext_screens, write_ext
from .examples import ExampleError, compare_examples, run_crate, runner_source, write_crate
from .modules import ModuleError
from .parser import ParseError, parse
from .plan import EXAMPLE, UNSPEC, Finding, compare_static, read_plan
from .lexer import LexError
from .rustgen import generate
from .review import review, review_changes
from .spec import build, render
from . import interview


def load(path):
    try:
        return check(parse(Path(path).read_text(), path), strict_names=True)
    except (ParseError, LexError, ModuleError) as e:
        print(f"構文エラー: {e}", file=sys.stderr)
        sys.exit(2)


def load_text(text: str, path):
    """コードの文字列を、path の場所にあるものとして読む（use の相対参照のため）"""
    try:
        return check(parse(text, str(path)), strict_names=True)
    except (ParseError, LexError, ModuleError) as e:
        raise ValueError(str(e))


def read_base(a) -> tuple[str, str]:
    """前の版のコードの文字列と、その名前（--base のファイル、または --base-git の版）"""
    from . import mutate
    if a.base:
        return Path(a.base).read_text(), Path(a.base).name
    return mutate.read_git(Path(a.code), a.base_git), f"git {a.base_git}"


def report(findings) -> str:
    if not findings:
        return "突合の結果: 計画とコードに食い違いはありません。\n"
    out = ["突合の結果", "", "| 種別 | 場所 | 内容 |", "|---|---|---|"]
    out += [f"| {f.kind} | {f.where} | {f.msg} |" for f in findings]
    return "\n".join(out) + "\n"


def run_interview(a) -> int:
    if a.action == "init":
        if Path(a.ledger).exists() and not a.force:
            print(f"{a.ledger} は既にあります（上書きするなら --force）", file=sys.stderr)
            return 2
        interview.save(a.ledger, interview.new_ledger(a.title))
        print(f"台帳を作りました: {a.ledger}")
        return 0
    try:
        led = interview.load(a.ledger)
        for k in interview.new_ledger("").keys():
            led.setdefault(k, interview.new_ledger("")[k])
    except (OSError, ValueError) as e:
        print(f"台帳を読めません: {e}", file=sys.stderr)
        return 2
    if a.action == "status":
        todo, ready = interview.status(led)
        print("\n".join(f"- {t}" for t in todo) or "- 聞くことは残っていません")
        print("コードを書き始めてよい: " + ("はい" if ready else "いいえ（動作要件 R1〜R7 とユースケースが揃うまで）"))
        return 0 if ready else 1
    text = interview.render(led)
    Path(a.o).write_text(text) if a.o else print(text)
    return 0


def run_mutate(a) -> int:
    from . import mutate
    try:
        if a.base:
            base, scope = Path(a.base).read_text(), f"{Path(a.base).name} から変わった行だけ"
        elif a.base_git:
            base, scope = mutate.read_git(Path(a.code), a.base_git), f"git {a.base_git} から変わった行だけ"
        else:
            base, scope = None, "すべての行"
        ms, why = mutate.run(a.code, base)
        run_note = None
        if a.run and not why:
            if not a.plan:
                print("--run には --plan 計画書.md が要ります", file=sys.stderr)
                return 2
            run_note = mutate.run_examples(a.code, ms, Path(a.plan).read_text())
    except (ParseError, LexError, ModuleError, ValueError) as e:
        print(f"エラー: {e}", file=sys.stderr)
        return 2
    if why:
        print(why, file=sys.stderr)
        return 1
    text = mutate.report(ms, Path(a.code).name, scope + ("（具体例も動かして確認）" if a.run and not run_note else ""))
    if run_note:
        print(run_note, file=sys.stderr)
    Path(a.o).write_text(text) if a.o else print(text)
    return 0


def run_receipt(a) -> int:
    from .receipt import build_receipt, to_markdown
    if not (a.base or a.base_git):
        print("前の版が要ります（--base 前のコード.geas、または --base-git REV）。", file=sys.stderr)
        return 2
    try:
        base_text, label = read_base(a)
        ck = load(a.code)
        base_ck = load_text(base_text, a.base or a.code)
    except (ValueError, OSError) as e:
        print(f"前の版を読めません: {e}", file=sys.stderr)
        return 2
    rows = read_plan(Path(a.plan).read_text()).get("ユースケース", []) if a.plan else None
    text = to_markdown(build_receipt(ck, base_ck, a.code, base_text, label, rows))
    Path(a.o).write_text(text) if a.o else print(text)
    return 0


def run_trace(a) -> int:
    from .trace import read_log, to_markdown, trace
    ck = load(a.code)
    try:
        text = sys.stdin.read() if a.log == "-" else Path(a.log).read_text()
    except OSError as e:
        print(f"ログを読めません: {e}", file=sys.stderr)
        return 2
    entries = read_log(text)
    if not entries:
        print("geas のルートのログ（\"log\": \"geas.route\" の行）が見つかりません。", file=sys.stderr)
        return 2
    rows = read_plan(Path(a.plan).read_text()).get("ユースケース", []) if a.plan else None
    t = trace(ck, entries, rows)
    out = to_markdown(t, Path(a.code).name)
    Path(a.o).write_text(out) if a.o else print(out)
    return 1 if t["unknown"] else 0


def run_bench_cmd(a) -> int:
    import json
    from . import bench
    cases, runs = Path(a.cases), Path(a.runs)
    if not cases.is_dir() or not runs.is_dir():
        print("ケース集と結果の置き場は、どちらもフォルダで渡してください。", file=sys.stderr)
        return 2
    scores = bench.run_bench(cases, runs, run=not a.no_run)
    if not scores:
        print("ケース集に plan.md を持つフォルダがありません。", file=sys.stderr)
        return 2
    base = json.loads(Path(a.baseline).read_text()) if a.baseline else None
    if a.json:
        Path(a.json).write_text(bench.to_json(scores))
    text = bench.to_markdown(scores, base)
    Path(a.o).write_text(text) if a.o else print(text)
    return 0 if all(s.passed for s in scores) else 1


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(prog="geas", description="独自構文の検査・Rust生成・plan突合（試作）")
    sub = ap.add_subparsers(dest="cmd", required=True)
    iv = sub.add_parser("interview", help="初回の対話の台帳（init／status／render）。Cockburn のユースケース手法に基づく")
    iv.add_argument("action", choices=["init", "status", "render"])
    iv.add_argument("ledger")
    iv.add_argument("--title", default="（題名）")
    iv.add_argument("-o", help="render の出力先")
    iv.add_argument("--force", action="store_true", help="init で既存の台帳を上書きする")
    c = sub.add_parser("check", help="コードを検査する")
    c.add_argument("code")
    c.add_argument("--json", action="store_true", help="結果を JSON で出す（AI が直すための手がかり hint つき）")
    s = sub.add_parser("spec", help="コードから仕様書（③）を生成する")
    s.add_argument("code")
    s.add_argument("--plan", help="計画書（①）。渡すと、内部 URL ごとの確認の網羅（具体例・ユースケース）も載る")
    s.add_argument("-o")
    v = sub.add_parser("review", help="人が読む仕様書（HTML）を生成する。計画書を渡すと、突合の結果とユースケースも載る")
    v.add_argument("code")
    v.add_argument("--plan", help="計画書（①）")
    v.add_argument("--diff-o", help="答えてほしいことと前の版からの変更だけの HTML の出力先（仕様書とは別のファイル）")
    v.add_argument("--base", help="比べる前の版のコード。--diff-o の HTML に「前の版からの変更」が載る")
    v.add_argument("--base-git", metavar="REV", help="比べる前の版を、git の REV（例: HEAD~1）にある同じコードにする")
    v.add_argument("-o", required=True)
    r = sub.add_parser("rust", help="Rust の crate を生成する")
    r.add_argument("code")
    r.add_argument("-o", required=True)
    r.add_argument("--plan", help="具体例の実行用 bin も生成する（--host --runtime と一緒なら、URL の具体例の HTTP テストも）")
    r.add_argument("--host", action="store_true", help="画面（WASM）から呼ぶ窓口 host.rs と cdylib 設定も生成する")
    r.add_argument("--runtime", action="store_true", help="サーバーの実行部（geas_server の crate）も、生成物の server/ に置く（--host と一緒に）")
    r.add_argument("--seed", help="サーバーが、DB が空のときだけ入れる最初のデータ（実体名→キー→行の JSON）。無ければ何も入れない")
    r.add_argument("--server", default="http://localhost:8787", help="拡張と MCP の橋渡しが問い合わせるサーバーの既定の URL")
    m = sub.add_parser("compare", help="plan の仕様書（①）とコードの突合。具体例も実行する")
    m.add_argument("code")
    m.add_argument("plan")
    m.add_argument("--no-run", action="store_true", help="具体例を実行しない（静的な突合だけ）")
    m.add_argument("-o", help="結果を Markdown で書き出す")
    mu = sub.add_parser("mutate", help="変異検査：約束を 1 つ崩した版を作り、検査器が止めるかを見る（普段は走らせない。レビューの前に）")
    mu.add_argument("code")
    mu.add_argument("--base", help="前の版のコード。変わった行だけを崩す")
    mu.add_argument("--base-git", metavar="REV", help="前の版を、git の REV（例: HEAD~1）にある同じコードにする。変わった行だけを崩す")
    mu.add_argument("--run", action="store_true", help="検査器が止めなかった崩し方を、計画書の具体例を動かして確かめる（--plan が要る。Rust をビルドするので遅い）")
    mu.add_argument("--plan", help="--run で使う計画書（①）")
    mu.add_argument("-o", help="結果を Markdown で書き出す")
    rc = sub.add_parser("receipt", help="変更ごとのレシート：前の版と比べた、信頼に関わる変化（PR の本文に貼る Markdown）")
    rc.add_argument("code")
    rc.add_argument("--base", help="前の版のコード")
    rc.add_argument("--base-git", metavar="REV", help="前の版を、git の REV（例: HEAD~1）にある同じコードにする")
    rc.add_argument("--plan", help="計画書（①）。渡すと、動きが変わるユースケースも出す")
    rc.add_argument("-o", help="結果を Markdown で書き出す")
    tr = sub.add_parser("trace", help="運用のログ（生成したサーバーの標準エラー）を、仕様書のルート・処理・結果・ユースケースに結ぶ。結べないものがあれば終了コード 1")
    tr.add_argument("code")
    tr.add_argument("log", help="ログのファイル（- なら標準入力）")
    tr.add_argument("--plan", help="計画書（①）。渡すと、ユースケースと画面も出す")
    tr.add_argument("-o", help="結果を Markdown で書き出す")
    bn = sub.add_parser("bench", help="AI が計画書から書いた .geas の採点（PR の直前・ビルドの直前に 1 回）。AI は呼ばず、書かれたものを採点するだけ")
    bn.add_argument("cases", help="ケース集のフォルダ（<名前>/plan.md）")
    bn.add_argument("runs", help="AI が書いた結果の置き場（<名前>/app.geas、<名前>/rounds.txt）")
    bn.add_argument("--no-run", action="store_true", help="具体例を動かさない（Rust をビルドしない）")
    bn.add_argument("--baseline", help="前回の --json の出力。悪くなったケースを示す")
    bn.add_argument("--json", help="結果を JSON で書き出す（次回の --baseline 用）")
    bn.add_argument("-o", help="結果を Markdown で書き出す")
    a = ap.parse_args(argv)

    if a.cmd == "interview":
        return run_interview(a)
    if a.cmd == "mutate":
        return run_mutate(a)
    if a.cmd == "receipt":
        return run_receipt(a)
    if a.cmd == "trace":
        return run_trace(a)
    if a.cmd == "bench":
        return run_bench_cmd(a)
    if a.cmd == "check" and a.json:
        from .hints import syntax_json
        try:
            ck = check(parse(Path(a.code).read_text(), a.code), strict_names=True)
        except (ParseError, LexError, ModuleError) as e:
            print(syntax_json(a.code, str(e)), end="")
            return 2
    else:
        ck = load(a.code)
    if a.cmd == "check":
        if a.json:
            from .hints import to_json
            print(to_json(ck, a.code), end="")
            return 0 if ck.ok else 1
        for d in ck.diags:
            print(d)
        print("検査: " + ("通りました" if ck.ok else "エラーがあります"))
        return 0 if ck.ok else 1
    if a.cmd == "spec":
        text = render(ck, build(ck), f"（{Path(a.code).name}）")
        if a.plan:
            from .plan import coverage_markdown
            text += "\n".join(coverage_markdown(build(ck), read_plan(Path(a.plan).read_text()))) + "\n"
        Path(a.o).write_text(text) if a.o else print(text)
        return 0 if ck.ok else 1
    if a.cmd == "review":
        plan_md = Path(a.plan).read_text() if a.plan else None
        findings = compare_static(build(ck), read_plan(plan_md)) if plan_md else []
        base_ck, base_label, base_text = None, "", None
        try:
            if a.base or a.base_git:
                base_text, base_label = read_base(a)
                base_ck = load_text(base_text, a.base or a.code)
        except ValueError as e:
            print(f"前の版を読めません: {e}", file=sys.stderr)
            return 2
        receipt = None
        if a.diff_o and base_ck is not None:
            from .receipt import build_receipt
            receipt = build_receipt(ck, base_ck, a.code, base_text, base_label, read_plan(plan_md).get("ユースケース", []) if plan_md else None)
        Path(a.o).write_text(review(ck, plan_md, findings, f"（{Path(a.code).name}）"))
        if a.diff_o:
            Path(a.diff_o).write_text(review_changes(ck, findings, f"（{Path(a.code).name}）", base_ck=base_ck, base_label=base_label, receipt=receipt))
            print(f"生成しました: {a.diff_o}")
        elif base_ck is not None:
            print("前の版は、--diff-o で出力先を渡したときだけ使います（仕様書には載せません）。", file=sys.stderr)
        print(f"生成しました: {a.o}")
        return 0 if ck.ok else 1
    if not ck.ready:
        for d in ck.diags:
            print(d)
        print("検査にエラーか問いが残っているので、続けません（問いに答えて、コードか計画書に反映してください）。", file=sys.stderr)
        return 1
    lib = generate(ck)
    if a.cmd == "rust":
        rows = read_plan(Path(a.plan).read_text()).get("具体例", []) if a.plan else []
        src, problems = runner_source(ck, rows)
        for p in problems:
            print(f"{p.kind}: {p.where}: {p.msg}")
        write_crate(ck, lib, src, Path(a.o), host=a.host)
        if a.host and a.runtime:
            write_server(Path(a.o), Path(a.seed) if a.seed else None)
            if a.plan:
                from .testgen import write_url_tests
                try:
                    f = write_url_tests(ck, read_plan(Path(a.plan).read_text()), a.o)
                except ValueError as e:
                    print(f"URL の具体例: {e}", file=sys.stderr)
                    return 1
                if f:
                    print(f"URL の具体例のテストを生成しました: {f}")
                from .testgen import write_screen_tests
                try:
                    f = write_screen_tests(ck, read_plan(Path(a.plan).read_text()), a.o)
                except ValueError as e:
                    print(f"画面の具体例: {e}", file=sys.stderr)
                    return 1
                if f:
                    print(f"画面の具体例のテストを生成しました: {f}")
        if a.host:
            from .mcp import write_mcp
            write_mcp(ck, a.o, a.server)
        if a.host and ext_screens(ck):
            write_ext(ck, a.o, a.server)
        print(f"生成しました: {a.o}")
        return 0
    plan = read_plan(Path(a.plan).read_text())
    findings = compare_static(build(ck), plan)
    rows = plan.get("具体例", [])
    src, problems = runner_source(ck, rows)
    findings += problems
    if not a.no_run and rows:
        with tempfile.TemporaryDirectory() as tmp:
            write_crate(ck, lib, src, Path(tmp))
            try:
                findings += compare_examples(rows, run_crate(Path(tmp), ck.p.show_names))
            except ExampleError as e:
                findings.append(Finding(EXAMPLE, "具体例の実行", str(e)))
    text = report(findings)
    print(text)
    if a.o:
        Path(a.o).write_text(text)
    return 1 if any(f.kind != UNSPEC for f in findings) else 0
