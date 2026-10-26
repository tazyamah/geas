"""初回の対話で仕様を作るための台帳（出典：Cockburn が提唱したユースケース手法。Writing Effective Use Cases, 2000）（ledger）。会話の進め方は skills/plan-interview/SKILL.md。

台帳は JSON。AI が一問ごとに答えを書き込み、`status` が次に聞くことを示し、`render` が plan の仕様書（①）の会話の部分を Markdown にする。
台帳は plan（①）の元で、コードから作らない（突合の独立性を保つ）。暫定の Python 実装（最終は Rust）。

台帳の形:
  title, summary, out_of_scope[]            概要と範囲外
  usecases[]  id, actor, goal, level, main[], extensions[{at, cond, handling}], rules[], examples[{given, when, then}]
  questions[] id, text, usecase(任意), status(open|unspec|answered), answer   未解決の問い（unspec＝意図的に未規定）
  extras[]    id, text, from_usecase, kind(runtime|constraint|quality|other)    返答に混ざっていた付加情報の置き場
  runtime{}   R1..R7 の答え（値と来歴）
  screens[]   id, name, device(共通|スマホ|PC), usecases[], parts[], states[{name, shows}], actions[{when, then, judgment}], events[{when, then, judgment}]   画面の動き（見せるもの・状態ごとの表示・押したらどうなるか・開いた／つながったなどの出来事で起こること）
  judgments[] id, rule, sources[], who, when    判断（J）。AI が解釈した**ルール1つにつき1件**。sources は元の発言（複数可。1つの発言から複数のJ、複数の発言から1つのJもある）
"""
import json
from pathlib import Path

# 必ず聞く動作要件（notes/spec-runtime-requirements-checklist.md の R1〜R7）
RUNTIME = {
    "R1": "動作するプラットフォーム（スマホ／PC／ブラウザ／サーバ／組み込み）",
    "R2": "使う場所と接続（常時オンライン／ときどきオフライン／完全オフライン）",
    "R3": "データを残すか、どこに（残さない／端末内／サーバのDB／既存のDB）",
    "R4": "利用者の数と同時利用",
    "R5": "外部とのつながり（決済、メール、外部API、認証基盤など）",
    "R6": "配る方法（自分だけ／他の人へ配る／サーバに配備）",
    "R7": "画面の有無と種類",
}
LEVELS = ("要約", "利用者目標", "下位機能")
STATUSES = ("open", "unspec", "answered")  # 未解決／意図的に未規定／答え済み
KINDS = ("runtime", "constraint", "quality", "other")


def runtime_status(r: dict) -> str:
    """動作要件の状態。status が無く value だけあれば答え済み（古い書き方）。"""
    return r.get("status") or ("answered" if r.get("value") else "open")


def new_ledger(title: str) -> dict:
    return {"title": title, "summary": "", "out_of_scope": [], "usecases": [], "questions": [], "extras": [], "runtime": {}, "screens": [], "judgments": []}


def load(path) -> dict:
    return json.loads(Path(path).read_text())


def save(path, led: dict):
    Path(path).write_text(json.dumps(led, ensure_ascii=False, indent=2) + "\n")


def usecase_gaps(u: dict) -> list[str]:
    gaps = []
    if not u.get("main"):
        gaps.append("主な流れ（成功の道）が未記入")
    if not u.get("extensions"):
        gaps.append("失敗・例外・分岐（拡張）が未記入")
    if not u.get("examples"):
        gaps.append("具体例が未記入")
    if u.get("level") not in LEVELS:
        gaps.append(f"ユースケースの粒度が未記入（{'／'.join(LEVELS)}）")
    return gaps


def open_questions(led: dict) -> list[dict]:
    return [q for q in led["questions"] if q.get("status", "open") == "open"]


def status(led: dict) -> tuple[list[str], bool]:
    """次に聞くことの一覧と、コードを書き始めてよいか。"""
    todo: list[str] = []
    if not led["summary"]:
        todo.append("作るものの一言（summary）")
    if not led["out_of_scope"]:
        todo.append("範囲外（out_of_scope）。無ければ「なし」と記入")
    if not led["usecases"]:
        todo.append("アクターとユースケースの一覧（usecases）。「他に使う人は？ 他にやりたいことは？」と聞く")
    for u in led["usecases"]:
        for g in usecase_gaps(u):
            todo.append(f"{u['id']} {u.get('goal', '')}: {g}")
    for q in led["questions"]:
        st = q.get("status", "open")
        if st not in STATUSES:
            todo.append(f"問い {q['id']} の status「{st}」が不正です（{'／'.join(STATUSES)}）")
        elif st == "open":
            todo.append(f"未解決の問い {q['id']}: {q['text']}")
        elif not q.get("answer"):
            todo.append(f"問い {q['id']}: {st} なのに答え（未規定なら理由）が空です")
    for x in led["extras"]:
        if x.get("kind") is None:
            todo.append(f"付加情報 {x['id']} の仕分け（人に確かめる）: {x['text']}")
        elif x["kind"] not in KINDS:
            todo.append(f"付加情報 {x['id']} の kind「{x['kind']}」が不正です（{'／'.join(KINDS)}）")
    if led["usecases"] and not todo:
        for k, text in RUNTIME.items():
            r = led["runtime"].get(k, {})
            st = runtime_status(r)
            if st not in STATUSES:
                todo.append(f"動作要件 {k} の status「{st}」が不正です")
            elif st == "open":
                todo.append(f"動作要件 {k}: {text}")
            elif k == "R1" and st == "unspec":
                todo.append("動作要件 R1（動作プラットフォーム）は未規定にできません。確定させてください")
            elif st == "answered" and not r.get("value"):
                todo.append(f"動作要件 {k}: answered なのに値が空です")
    if led["usecases"] and not todo:
        r7 = led["runtime"].get("R7", {})
        if runtime_status(r7) == "answered" and "画面" in (r7.get("value") or "") and not led.get("screens"):
            todo.append("画面の動き（screens）：見せるもの、状態ごとの表示、押したらどうなるか、開いた・つながったなどの出来事で起こること")
        for sc in led.get("screens", []):
            for key, label in (("parts", "見せるもの"), ("states", "状態ごとの表示"), ("actions", "押したらどうなるか")):
                if not sc.get(key):
                    todo.append(f"画面 {sc['id']} {sc.get('name', '')}: {label}が未記入")
    return todo, bool(led["usecases"]) and not todo


def render(led: dict) -> str:
    out = [f"# 仕様書（plan）：{led['title']}", "", "会話で作ったレビュー対象の仕様書（①）の会話の部分（節番号 C0〜C5）。plan 雛形の節1〜7（データ・状態・処理・指定・具体例の表）は、この内容から、コードより前に会話で書き起こす。台帳から生成した。決まっていないことは「**未規定**」と書く。", ""]
    out += ["## C0. 概要（文章。突合しない）", "", led["summary"] or "**未規定**", ""]
    out += ["範囲外: " + ("、".join(led["out_of_scope"]) or "**未規定**"), ""]
    out += ["## C1. ユースケース（Cockburn。文章。コードの起こしの元）", ""]
    for u in led["usecases"]:
        out += [f"### {u['id']} {u.get('goal', '')}（アクター: {u.get('actor', '')}／粒度: {u.get('level') or '**未規定**'}）", ""]
        out += ["主な流れ:"] + [f"{i}. {s}" for i, s in enumerate(u.get("main", []), 1)] + [""]
        if u.get("extensions"):
            out += ["拡張（失敗・例外・分岐）:"] + [f"- {e['at']}: {e['cond']} → {e['handling']}" for e in u["extensions"]] + [""]
        if u.get("rules"):
            out += ["ルール:"] + [f"- {r}" for r in u["rules"]] + [""]
        if u.get("examples"):
            out += ["具体例:"] + [f"- 前提 {e['given']} ／ 操作 {e['when']} ／ 期待 {e['then']}" for e in u["examples"]] + [""]
    out += ["## C2. 動かす環境（動作要件）", "", "| ID | 聞くこと | 答え | 来歴 |", "|---|---|---|---|"]
    for k, text in RUNTIME.items():
        r = led["runtime"].get(k, {})
        st = runtime_status(r)
        shown = r.get("value") or ("**未規定**（意図的）" if st == "unspec" else "（まだ聞いていない）")
        out.append(f"| {k} | {text} | {shown} | {r.get('from', '')} |")
    out += [""]
    if led["extras"]:
        out += ["## C3. 返答に混ざっていた付加情報（仕分け済みの置き場）", "", "| ID | 内容 | 出どころ | 種別 |", "|---|---|---|---|"]
        out += [f"| {x['id']} | {x['text']} | {x.get('from_usecase', '')} | {x.get('kind') or '未仕分け'} |" for x in led["extras"]]
        out += [""]
    out += ["## C4. 未解決の問い", "", "| ID | 問い | 状態 | 答え |", "|---|---|---|---|"]
    out += [f"| {q['id']} | {q['text']} | {q.get('status', 'open')} | {q.get('answer', '')} |" for q in led["questions"]] or []
    out += ["", "## C6. 画面の動き（見せるもの・状態ごとの表示・押したら・出来事で）", ""]
    for sc in led.get("screens", []):
        out += [f"### {sc['id']} {sc.get('name', '')}（{sc.get('device', '共通')}／ユースケース: {'、'.join(sc.get('usecases', []))}）", "", "見せるもの: " + "、".join(sc.get("parts", [])), ""]
        out += ["| 状態 | 見せるもの |", "|---|---|"] + [f"| {x['name']} | {x['shows']} |" for x in sc.get("states", [])] + [""]
        out += ["| きっかけ | 起こること | 判断 |", "|---|---|---|"]
        out += [f"| 押す：{x['when']} | {x['then']} | {x.get('judgment', '')} |" for x in sc.get("actions", [])]
        out += [f"| 出来事：{x['when']} | {x['then']} | {x.get('judgment', '')} |" for x in sc.get("events", [])] + [""]
    out += ["", "## C5. 判断の記録", "", "| ID | ルール（AIの解釈） | 元の発言 | 誰が | いつ |", "|---|---|---|---|---|"]
    out += [f"| {j['id']} | {j['rule']} | {' ／ '.join(j.get('sources', []))} | {j.get('who', '')} | {j.get('when', '')} |" for j in led["judgments"]]
    return "\n".join(out) + "\n"
