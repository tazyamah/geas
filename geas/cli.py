# SPDX-License-Identifier: GPL-3.0-or-later
import argparse
import sys
import tempfile
from pathlib import Path

from .check import ERROR, QUESTION, WARN, check
from .servergen import write_server
from .windowgen import write_window
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


def syntax_error_text(e, path) -> str:
    """構文エラーの表示：位置と文言、その行の抜き出しと、字句の位置の印"""
    out = [f"構文エラー: {e}"]
    line, col, file = getattr(e, "line", 0), getattr(e, "col", 0), getattr(e, "file", "") or str(path)
    if line:
        try:
            src = Path(file).read_text(encoding="utf-8").split("\n")
        except (OSError, UnicodeDecodeError):
            src = []
        if 0 < line <= len(src) and src[line - 1].strip():
            text = src[line - 1].rstrip()
            out.append(f"  {line:>4} | {text}")
            if 0 < col <= len(text) + 1:
                import unicodedata
                pad = "".join(c if c == "\t" else " " * (2 if unicodedata.east_asian_width(c) in "WF" else 1) for c in text[:col - 1])
                out.append(f"  {'':>4} | {pad}^")
    return "\n".join(out)


def counts_line(ck) -> str:
    """check の最後の行。件数と、次にすること"""
    n = {lv: sum(1 for d in ck.diags if d.level == lv) for lv in (ERROR, QUESTION, WARN)}
    tally = f"エラー {n[ERROR]} 件・問い {n[QUESTION]} 件・警告 {n[WARN]} 件"
    if n[ERROR]:
        return f"検査: エラーがあります（{tally}）"
    if n[QUESTION]:
        return f"検査: 通りました（{tally}）。問いは人に1問ずつ聞いて決めてください（答えるまで rust と compare は進みません）"
    return "検査: 通りました" + (f"（{tally}）" if n[WARN] else "")


def print_diags(ck, file=None):
    """指摘を、コードに書いた名前のまま出す（AI はこの名前と行でコードを探して直す）"""
    for d in ck.diags:
        print(d.for_code(), file=file or sys.stdout)


def load(path):
    try:
        ck = check(parse(Path(path).read_text(encoding="utf-8"), path), strict_names=True)
    except (ParseError, LexError, ModuleError) as e:
        print(syntax_error_text(e, path), file=sys.stderr)
        sys.exit(2)
    ck.code_path = str(path)  # 仕様書が、コードの隣の台帳（部品の検査の結果）を読むため
    return ck


def load_text(text: str, path):
    """コードの文字列を、path の場所にあるものとして読む（use の相対参照のため）"""
    try:
        return check(parse(text, str(path)), strict_names=True)
    except (ParseError, LexError, ModuleError) as e:
        raise ValueError(str(e))


def run_parts(a) -> int:
    """部品（library）の境界値の検査。モジュールのファイルは、use で取り込んだ形にして全部の部品を確かめる"""
    import tempfile
    from .partcheck import check_parts, render
    from .stdref import reference
    path = Path(a.code)
    text = path.read_text(encoding="utf-8")
    first = next((ln.split() for ln in text.splitlines() if ln.strip() and not ln.strip().startswith("//")), [])
    is_module = bool(first) and first[0] == "module"
    try:
        ck = load_text(f'use "{path.resolve().as_posix()}"\n', path.resolve().parent / "_parts.geas") if is_module else load(a.code)
    except ValueError as e:
        print(e, file=sys.stderr)
        return 2
    parts = [pt for pt in ck.p.parts if pt.external and (is_module or a.all or pt.name in ck.used_libs)]
    if not parts:
        print("確かめる部品（library）がありません" + ("" if is_module or a.all else "（呼ばれていない部品も確かめるには --all）"))
        return 0
    with tempfile.TemporaryDirectory() as tmp:
        try:
            rep = check_parts(parts, Path(tmp), reference(parts))
        except RuntimeError as e:
            print(e, file=sys.stderr)
            return 2
    print(render(rep))
    if a.record:
        import json
        from .partcheck import ledger_of, ledger_path
        out = ledger_path(path)
        out.write_text(json.dumps(ledger_of(rep, parts), ensure_ascii=False, indent=1, sort_keys=True) + "\n", encoding="utf-8")
        print(f"台帳を書きました: {out}")
    return 0 if rep.ok else 1


def read_base(a) -> tuple[str, str]:
    """前の版のコードの文字列と、その名前（--base のファイル、または --base-git の版）"""
    from . import mutate
    if a.base:
        return Path(a.base).read_text(encoding="utf-8"), Path(a.base).name
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
    Path(a.o).write_text(text, encoding="utf-8") if a.o else print(text)
    return 0


def run_mutate(a) -> int:
    from . import mutate
    try:
        if a.base:
            base, scope = Path(a.base).read_text(encoding="utf-8"), f"{Path(a.base).name} から変わった行だけ"
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
            run_note = mutate.run_examples(a.code, ms, Path(a.plan).read_text(encoding="utf-8"))
    except (ParseError, LexError, ModuleError, ValueError) as e:
        print(f"エラー: {e}", file=sys.stderr)
        return 2
    if why:
        print(why, file=sys.stderr)
        return 1
    text = mutate.report(ms, Path(a.code).name, scope + ("（具体例も動かして確認）" if a.run and not run_note else ""))
    if run_note:
        print(run_note, file=sys.stderr)
    Path(a.o).write_text(text, encoding="utf-8") if a.o else print(text)
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
    rows = read_plan(Path(a.plan).read_text(encoding="utf-8")).get("ユースケース", []) if a.plan else None
    text = to_markdown(build_receipt(ck, base_ck, a.code, base_text, label, rows))
    Path(a.o).write_text(text, encoding="utf-8") if a.o else print(text)
    return 0


def run_trace(a) -> int:
    from .trace import read_log, to_markdown, trace
    ck = load(a.code)
    try:
        text = sys.stdin.read() if a.log == "-" else Path(a.log).read_text(encoding="utf-8")
    except OSError as e:
        print(f"ログを読めません: {e}", file=sys.stderr)
        return 2
    entries = read_log(text)
    if not entries:
        print("geas のルートのログ（\"log\": \"geas.route\" の行）が見つかりません。", file=sys.stderr)
        return 2
    rows = read_plan(Path(a.plan).read_text(encoding="utf-8")).get("ユースケース", []) if a.plan else None
    t = trace(ck, entries, rows)
    out = to_markdown(t, Path(a.code).name)
    Path(a.o).write_text(out, encoding="utf-8") if a.o else print(out)
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
    base = json.loads(Path(a.baseline).read_text(encoding="utf-8")) if a.baseline else None
    if a.json:
        Path(a.json).write_text(bench.to_json(scores), encoding="utf-8")
    text = bench.to_markdown(scores, base)
    Path(a.o).write_text(text, encoding="utf-8") if a.o else print(text)
    return 0 if all(s.passed for s in scores) else 1


def _facts_from_plan(a):
    """--plan の JSON をアダプタで事実にする。(事実, メモ) か、エラーなら (None, メッセージ)"""
    import json
    from .adapters import ADAPTERS
    if a.adapter not in ADAPTERS:
        return None, f"アダプタ「{a.adapter}」はありません（ある：{'・'.join(ADAPTERS)}）"
    try:
        plan = json.loads(Path(a.plan).read_text(encoding="utf-8"))
    except (OSError, ValueError) as e:
        return None, f"plan を読めません: {e}"
    return ADAPTERS[a.adapter].facts_from_plan(plan, a.service), None


def run_env_facts(a) -> int:
    import json
    got, err = _facts_from_plan(a)
    if got is None:
        print(err, file=sys.stderr)
        return 2
    facts, notes = got
    print(json.dumps({"facts": facts, "notes": notes}, ensure_ascii=False, indent=2))
    return 0


def run_env_verify(a, ck) -> int:
    import json
    from . import envcheck, envreq
    if not ck.ok:
        for d in ck.diags:
            print(d, file=sys.stderr)
        print("検査: エラーがあります（直してから照合してください）", file=sys.stderr)
        return 1
    notes: list = []
    if a.facts:
        try:
            facts = json.loads(Path(a.facts).read_text(encoding="utf-8"))
            facts = facts.get("facts", facts)
        except (OSError, ValueError) as e:
            print(f"事実を読めません: {e}", file=sys.stderr)
            return 2
    else:
        got, err = _facts_from_plan(a)
        if got is None:
            print(err, file=sys.stderr)
            return 2
        facts, notes = got
    results = envcheck.verify(envreq.items(ck.p), facts)
    if a.json:
        print(json.dumps({"results": results, "counts": envcheck.counts(results), "notes": notes}, ensure_ascii=False, indent=2))
    else:
        print(envcheck.to_text(results), end="")
        for n in notes:
            print(f"メモ：{n}")
    c = envcheck.counts(results)
    return 1 if c[envcheck.FAIL] or (a.strict and c[envcheck.UNKNOWN]) else 0


# 読むファイルを受け取る引数（無ければ、トレースバックでなく1行で知らせる）
INPUTS = ("code", "plan", "base", "seed", "facts", "baseline")
# 書き出すファイルの引数（置き場のフォルダが無ければ知らせる。rust の -o は作る）
OUTPUTS = ("o", "diff_o")


def check_paths(a) -> list[str]:
    """引数のファイルの確かめ。問題の文言の列（無ければ空）"""
    bad = []
    for k in INPUTS:
        v = getattr(a, k, None)
        if v and not Path(v).is_file():
            bad.append(f"ファイルがありません: {v}" if not Path(v).exists() else f"ファイルではありません: {v}")
    if getattr(a, "log", None) not in (None, "-") and not Path(a.log).is_file():
        bad.append(f"ログのファイルがありません: {a.log}")
    if a.cmd == "bench" and a.json and not Path(a.json).resolve().parent.is_dir():
        bad.append(f"書き出す先のフォルダがありません: {Path(a.json).parent}")
    for k in OUTPUTS:
        v = getattr(a, k, None)
        if v and a.cmd not in ("rust", "interview") and not Path(v).resolve().parent.is_dir():
            bad.append(f"書き出す先のフォルダがありません: {Path(v).parent}")
    if a.cmd == "rust":
        if a.runtime and not a.host:
            bad.append("--runtime は --host と一緒に使います（サーバーの実行部は、画面の窓口と組みます）")
        if a.standalone and not (a.host and a.runtime):
            bad.append("--standalone は --host --runtime と一緒に使います（窓つき exe は、サーバーの実行部と生成した画面を使います）")
        if a.seed and not a.runtime:
            bad.append("--seed は --host --runtime と一緒に使います（最初のデータはサーバーが入れます）")
    return bad


def utf8_stdio():
    """標準出力と標準エラーを UTF-8 にする。Windows では、リダイレクトした出力が既定で cp932 などになり、
    geas が書くファイル（UTF-8）と食い違ったり、cp932 に無い文字で落ちたりするため"""
    for s in (sys.stdout, sys.stderr):
        enc = (getattr(s, "encoding", None) or "").lower().replace("-", "").replace("_", "")
        if enc != "utf8" and hasattr(s, "reconfigure"):
            s.reconfigure(encoding="utf-8")


def main(argv=None) -> int:
    utf8_stdio()
    ap = argparse.ArgumentParser(prog="geas", description="計画書と .geas を突き合わせ、検査・仕様書・Rust の生成をする（試作の Python 版）",
                                 epilog="ふだんの流れ：check（検査）→ review（人が読む仕様書）→ compare（計画書と突合）→ rust（生成）。"
                                        "詳しくは docs/ai-guide.md")
    sub = ap.add_subparsers(dest="cmd")
    iv = sub.add_parser("interview", help="初回の対話の台帳（init／status／render）。Cockburn のユースケース手法に基づく")
    iv.add_argument("action", choices=["init", "status", "render"])
    iv.add_argument("ledger", help="台帳のファイル")
    iv.add_argument("--title", default="（題名）")
    iv.add_argument("-o", help="render の出力先")
    iv.add_argument("--force", action="store_true", help="init で既存の台帳を上書きする")
    c = sub.add_parser("check", help="コードを検査する")
    c.add_argument("code", help=".geas ファイル")
    c.add_argument("--json", action="store_true", help="結果を JSON で出す（AI が直すための手がかり hint つき）")
    pc = sub.add_parser("parts", help="部品（library）を、引数の範囲の端の値で実際に呼んで、宣言（範囲・guarantee・panic しない）どおりかを確かめる。食い違いがあれば終了コード 1")
    pc.add_argument("code", help=".geas ファイル（module で始まるモジュールのファイルでもよい）")
    pc.add_argument("--all", action="store_true", help="呼ばれていない部品も確かめる（モジュールのファイルは、いつも全部）")
    pc.add_argument("--record", action="store_true", help="結果を、コードの隣の 名前.parts.json（台帳）に書く。仕様書の「人の保証」に、宣言を実際に呼んで確かめたことが出る")
    s = sub.add_parser("spec", help="コードから仕様書（③）を生成する")
    s.add_argument("code", help=".geas ファイル")
    s.add_argument("--plan", help="計画書（①）。渡すと、内部 URL ごとの確認の網羅（具体例・ユースケース）も載る")
    s.add_argument("-o", help="仕様書（Markdown）の出力先。無ければ標準出力に出す")
    ev = sub.add_parser("env-checks", help="動かす前提（アプリが実行環境に要求すること）を、ツールに依存しない検査項目の一覧にして出す")
    ev.add_argument("code")
    ev.add_argument("--json", action="store_true", help="JSON で出す（plan の照合器や、検査ツールへの変換器が読む）")
    ef = sub.add_parser("env-facts", help="plan（terraform show -json の出力）から、環境の事実（プラットフォームに依存しない形）を作る")
    ef.add_argument("plan", help="terraform show -json の出力")
    ef.add_argument("--adapter", default="aws-ecs", help="プラットフォームのアダプタ（いまは aws-ecs）")
    ef.add_argument("--service", default="", help="アプリの ECS サービスのアドレス（サービスが複数あるとき）")
    ev2 = sub.add_parser("env-verify", help="動かす前提（env-checks の一覧）と、環境の事実を照合する。不合格があれば終了コード 1")
    ev2.add_argument("code")
    src = ev2.add_mutually_exclusive_group(required=True)
    src.add_argument("--facts", help="環境の事実の JSON（env-facts の出力）")
    src.add_argument("--plan", help="terraform show -json の出力（アダプタで事実にしてから照合する）")
    ev2.add_argument("--adapter", default="aws-ecs")
    ev2.add_argument("--service", default="")
    ev2.add_argument("--json", action="store_true", help="結果を JSON で出す")
    ev2.add_argument("--strict", action="store_true", help="「確かめられなかった」も、終了コード 1 にする")
    v = sub.add_parser("review", help="人が読む仕様書（HTML）を生成する。計画書を渡すと、突合の結果とユースケースも載る")
    v.add_argument("code", help=".geas ファイル")
    v.add_argument("--plan", help="計画書（①）")
    v.add_argument("--diff-o", help="答えてほしいことと前の版からの変更だけの HTML の出力先（仕様書とは別のファイル）")
    v.add_argument("--base", help="比べる前の版のコード。--diff-o の HTML に「前の版からの変更」が載る")
    v.add_argument("--base-git", metavar="REV", help="比べる前の版を、git の REV（例: HEAD~1）にある同じコードにする")
    v.add_argument("-o", required=True, help="仕様書（HTML）の出力先")
    r = sub.add_parser("rust", help="Rust の crate を生成する")
    r.add_argument("code", help=".geas ファイル")
    r.add_argument("-o", required=True, help="crate を書き出すフォルダ")
    r.add_argument("--standalone", action="store_true", help="窓つき exe（geas_window の crate。画面と処理を 1 つの実行ファイルにし、通信路を使わない）も、生成物の window/ に置く（--host --runtime と一緒に）")
    r.add_argument("--target", choices=("windows", "macos", "linux"), help="窓つき exe を組む OS（--standalone のとき。省けば、いま動いている OS）。path 名前 = absolute { … } を宣言したアプリは、すべてにこの OS の行が要る")
    r.add_argument("--plan", help="具体例の実行用 bin も生成する（--host --runtime と一緒なら、URL の具体例の HTTP テストも）")
    r.add_argument("--host", action="store_true", help="画面（WASM）から呼ぶ窓口 host.rs と cdylib 設定も生成する")
    r.add_argument("--runtime", action="store_true", help="サーバーの実行部（geas_server の crate）も、生成物の server/ に置く（--host と一緒に）")
    r.add_argument("--seed", help="サーバーが、DB が空のときだけ入れる最初のデータ（実体名→キー→行の JSON）。無ければ何も入れない")
    r.add_argument("--server", default="http://localhost:8787", help="拡張と MCP の橋渡しが問い合わせるサーバーの既定の URL")
    m = sub.add_parser("compare", help="plan の仕様書（①）とコードの突合。具体例も実行する")
    m.add_argument("code", help=".geas ファイル")
    m.add_argument("plan", help="計画書（①）")
    m.add_argument("--no-run", action="store_true", help="具体例を実行しない（静的な突合だけ）")
    m.add_argument("-o", help="結果を Markdown で書き出す")
    mu = sub.add_parser("mutate", help="変異検査：約束を 1 つ崩した版を作り、検査器が止めるかを見る（普段は走らせない。レビューの前に）")
    mu.add_argument("code", help=".geas ファイル")
    mu.add_argument("--base", help="前の版のコード。変わった行だけを崩す")
    mu.add_argument("--base-git", metavar="REV", help="前の版を、git の REV（例: HEAD~1）にある同じコードにする。変わった行だけを崩す")
    mu.add_argument("--run", action="store_true", help="検査器が止めなかった崩し方を、計画書の具体例を動かして確かめる（--plan が要る。Rust をビルドするので遅い）")
    mu.add_argument("--plan", help="--run で使う計画書（①）")
    mu.add_argument("-o", help="結果を Markdown で書き出す")
    rc = sub.add_parser("receipt", help="変更ごとのレシート：前の版と比べた、信頼に関わる変化（PR の本文に貼る Markdown）")
    rc.add_argument("code", help=".geas ファイル")
    rc.add_argument("--base", help="前の版のコード")
    rc.add_argument("--base-git", metavar="REV", help="前の版を、git の REV（例: HEAD~1）にある同じコードにする")
    rc.add_argument("--plan", help="計画書（①）。渡すと、動きが変わるユースケースも出す")
    rc.add_argument("-o", help="結果を Markdown で書き出す")
    tr = sub.add_parser("trace", help="運用のログ（生成したサーバーの標準エラー）を、仕様書のルート・処理・結果・ユースケースに結ぶ。結べないものがあれば終了コード 1")
    tr.add_argument("code", help=".geas ファイル")
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
    if a.cmd is None:
        ap.print_help(sys.stderr)
        return 2
    bad = check_paths(a)
    if bad:
        for b in bad:
            print(b, file=sys.stderr)
        return 2

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
    if a.cmd == "env-facts":
        return run_env_facts(a)
    if a.cmd == "parts":
        return run_parts(a)
    if a.cmd == "check" and a.json:
        from .hints import syntax_json
        try:
            ck = check(parse(Path(a.code).read_text(encoding="utf-8"), a.code), strict_names=True)
        except (ParseError, LexError, ModuleError) as e:
            print(syntax_json(a.code, e), end="")
            return 2
    else:
        ck = load(a.code)
    if a.cmd == "check":
        if a.json:
            from .hints import to_json
            print(to_json(ck, a.code), end="")
            return 0 if ck.ok else 1
        print_diags(ck)
        print(counts_line(ck))
        return 0 if ck.ok else 1
    if a.cmd == "env-verify":
        return run_env_verify(a, ck)
    if a.cmd == "env-checks":
        from . import envreq
        if not ck.ok:
            print_diags(ck, sys.stderr)
            print("検査: エラーがあります（直してから一覧を出してください）", file=sys.stderr)
            return 1
        print(envreq.to_json(ck.p) if a.json else envreq.to_text(ck.p), end="")
        return 0
    if a.cmd == "spec":
        text = render(ck, build(ck), f"（{Path(a.code).name}）")
        if a.plan:
            from .plan import coverage_markdown
            text += "\n".join(coverage_markdown(build(ck), read_plan(Path(a.plan).read_text(encoding="utf-8")))) + "\n"
        Path(a.o).write_text(text, encoding="utf-8") if a.o else print(text)
        return 0 if ck.ok else 1
    if a.cmd == "review":
        plan_md = Path(a.plan).read_text(encoding="utf-8") if a.plan else None
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
        Path(a.o).write_text(review(ck, plan_md, findings, f"（{Path(a.code).name}）"), encoding="utf-8")
        if a.diff_o:
            Path(a.diff_o).write_text(review_changes(ck, findings, f"（{Path(a.code).name}）", base_ck=base_ck, base_label=base_label, receipt=receipt), encoding="utf-8")
            print(f"生成しました: {a.diff_o}")
        elif base_ck is not None:
            print("前の版は、--diff-o で出力先を渡したときだけ使います（仕様書には載せません）。", file=sys.stderr)
        print(f"生成しました: {a.o}")
        return 0 if ck.ok else 1
    if not ck.ready:
        print_diags(ck)
        print(counts_line(ck), file=sys.stderr)
        print("検査にエラーか問いが残っているので、続けません（問いに答えて、コードか計画書に反映してください）。", file=sys.stderr)
        return 1
    if a.cmd == "rust" and a.standalone and ck.p.paths:
        # 組む OS では、すべての絶対パス（absolute）が、その OS の行を持つこと
        target = a.target or {"win32": "windows", "darwin": "macos"}.get(sys.platform, "linux")
        lack = [n for n, k, es, _ in ck.p.paths if k == "absolute" and not any(o == target for o, _, _ in es)]
        if lack:
            print(f'パス {"・".join(lack)} に、組む OS（{target}）の行がありません。`{target} "…"` を書くか、--target で組む OS を指定してください', file=sys.stderr)
            return 1
    lib = generate(ck)
    if a.cmd == "rust":
        rows = read_plan(Path(a.plan).read_text(encoding="utf-8")).get("具体例", []) if a.plan else []
        src, problems = runner_source(ck, rows)
        for p in problems:
            print(f"{p.kind}: {p.where}: {p.msg}")
        write_crate(ck, lib, src, Path(a.o), host=a.host)
        if a.host and a.runtime:
            write_server(Path(a.o), Path(a.seed) if a.seed else None, net=any(e.http for e in ck.p.effects))
            if a.standalone:
                write_window(Path(a.o), resident=ck.p.resident is not None)
            if a.plan:
                from .testgen import write_url_tests
                try:
                    f = write_url_tests(ck, read_plan(Path(a.plan).read_text(encoding="utf-8")), a.o)
                except ValueError as e:
                    print(f"URL の具体例: {e}", file=sys.stderr)
                    return 1
                if f:
                    print(f"URL の具体例のテストを生成しました: {f}")
                from .testgen import write_screen_tests
                try:
                    f = write_screen_tests(ck, read_plan(Path(a.plan).read_text(encoding="utf-8")), a.o)
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
    plan = read_plan(Path(a.plan).read_text(encoding="utf-8"))
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
        Path(a.o).write_text(text, encoding="utf-8")
    return 1 if any(f.kind != UNSPEC for f in findings) else 0
