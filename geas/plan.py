# SPDX-License-Identifier: GPL-3.0-or-later
"""plan の仕様書（①）の読み取りと、生成した仕様書（③）との突合。
①は固定の見出しと表を持つ Markdown。◆の欄だけを機械が突合し、それ以外（文章・来歴）は読み飛ばす。"""
import re
from dataclasses import dataclass


@dataclass
class Finding:
    kind: str  # 計画にあってコードにない／コードにあって計画にない／値が違う／具体例の不一致／未規定
    where: str
    msg: str


A_ONLY, B_ONLY, DIFF, EXAMPLE, UNSPEC = (
    "計画にあってコードにない", "コードにあって計画にない", "値が違う", "具体例の不一致", "未規定",
)


def n(s: str) -> str:
    """比べるための正規化：空白を除き、矢印とカンマの書き方をそろえる。"""
    s = re.sub(r"[ \t　]", "", s or "")
    return s.replace("->", "→").replace("、", ",").replace("，", ",").replace("（", "(").replace("）", ")")


def split_top(s: str, sep: str) -> list[str]:
    out, depth, cur = [], 0, ""
    for c in s:
        if c in "({[":
            depth += 1
        elif c in ")}]":
            depth -= 1
        if c == sep and depth == 0:
            out.append(cur)
            cur = ""
        else:
            cur += c
    if cur.strip() or out:
        out.append(cur)
    return [x.strip() for x in out if x.strip()]


def tables(md: str) -> list[tuple[list[str], list[dict]]]:
    """Markdown の表を (見出し, 行) の列にする。見出しの「◆」は外す。"""
    out, cur = [], None
    for line in md.splitlines():
        if line.lstrip().startswith("|"):
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            if cur is None:
                cur = [[c.replace("◆", "").strip() for c in cells], []]
            elif all(re.fullmatch(r":?-{2,}:?", c) for c in cells if c):
                continue
            else:
                cells += [""] * (len(cur[0]) - len(cells))
                cur[1].append(dict(zip(cur[0], cells)))
        else:
            if cur is not None:
                out.append((cur[0], cur[1]))
            cur = None
    if cur is not None:
        out.append((cur[0], cur[1]))
    return out


def classify(headers: list[str]) -> str | None:
    h = set(headers)
    for kind, need in (
        ("データ", {"項目", "型・範囲"}), ("状態", {"状態", "区分"}), ("遷移", {"元", "先", "起こす処理"}),
        ("拒否", {"元", "先", "拒否の理由"}), ("問い合わせ", {"問い合わせ", "条件", "並び"}), ("処理", {"入力", "読む", "書く"}), ("不変条件", {"内容"}),
        ("トリガ", {"トリガ", "要因", "行き先"}), ("外部作用", {"外部作用", "出す内容"}), ("合流", {"外部作用", "合流先のトリガ"}), ("出来事", {"出来事", "起こし方", "処理"}), ("ルート", {"メソッド", "経路", "動作", "呼ぶもの"}), ("画面", {"画面", "見せる", "入力欄", "入口"}), ("画面遷移", {"画面", "操作", "結果", "行き先"}), ("ユースケース", {"ユースケース", "手順", "画面", "操作"}), ("指定", {"項目", "指定"}), ("具体例", {"期待する結果"}),
    ):
        if need <= h:
            return kind
    return None


def read_plan(md: str) -> dict[str, list[dict]]:
    plan: dict[str, list[dict]] = {}
    for headers, rows in tables(md):
        kind = classify(headers)
        if kind:
            plan.setdefault(kind, []).extend(rows)
    return plan


def items(cell: str) -> set[str]:
    return {x for x in split_top(n(cell).replace("/", ","), ",") if x and x != "なし"}


def cmp_map(kind, a: dict, b: dict, out: list[Finding], label):
    """a＝計画（①）、b＝コード（③）。"""
    for k in a:
        if k not in b:
            out.append(Finding(A_ONLY, f"{kind} {label(k)}", "計画にあるが、コードから生成した仕様書に無い"))
    for k in b:
        if k not in a:
            out.append(Finding(B_ONLY, f"{kind} {label(k)}", "コードにあるが、計画に無い（会話で決めていない振る舞いなら、問いにする）"))
    for k in a:
        if k in b and a[k] != b[k]:
            for field in a[k]:
                if a[k][field] != b[k][field]:
                    out.append(Finding(DIFF, f"{kind} {label(k)} の{field}", f"計画: {fmt(a[k][field])} ／ コード: {fmt(b[k][field])}"))


def fmt(v):
    if isinstance(v, (set, frozenset)):
        return "{" + ", ".join(sorted(v)) + "}" if v else "なし"
    return v if v != "" else "（空）"


def compare_static(spec: dict, plan: dict) -> list[Finding]:
    out: list[Finding] = []
    j = ".".join

    def sect(name):
        return plan.get(name, [])

    # データ
    a = {(r["データ"], r["項目"]): {"型・範囲": n(r["型・範囲"]), "必須": r.get("必須", "").strip(), "一意": r.get("一意", "").strip()}
         for r in sect("データ")}
    b = {(d, f): {"型・範囲": n(t), "必須": req, "一意": uq} for d, f, t, req, uq in spec["データ"]}
    cmp_map("データ", a, b, out, j)
    # 状態・遷移・拒否
    a = {(r["データ"], r["状態"]): {"区分": r["区分"].strip()} for r in sect("状態")}
    b = {(d, s): {"区分": k} for d, s, k in spec["状態"]}
    cmp_map("状態", a, b, out, j)
    # 同じ遷移を複数の処理が起こせる（別端末の操作の取り込みなど）。処理名の集合として比べる
    a = {(r["データ"], r["元"], r["先"]): {"起こす処理": ",".join(sorted(items(r["起こす処理"])))} for r in sect("遷移")}
    b = {}
    for d, x, y, p in spec["遷移"]:
        b.setdefault((d, x, y), set()).add(p)
    b = {k: {"起こす処理": ",".join(sorted(v))} for k, v in b.items()}
    cmp_map("遷移", a, b, out, lambda k: f"{k[0]}: {k[1]}→{k[2]}")
    a = {(r["データ"], r["元"], r["先"]): {} for r in sect("拒否")}
    b = {(d, x, y): {} for d, x, y, _ in spec["拒否"]}
    cmp_map("拒否", a, b, out, lambda k: f"{k[0]}: {k[1]}→{k[2]}")
    # 処理
    a = {r["処理"]: {"入力": items(r["入力"]), "読む": items(r["読む"]), "書く": items(r["書く"]),
                      "結果→行き先": items(r["結果 → 行き先"] if "結果 → 行き先" in r else r.get("結果→行き先", "")),
                      "外部作用": items(r.get("外部作用", ""))} for r in sect("処理")}
    b = {x["処理"]: {"入力": items(",".join(x["入力"])), "読む": items(",".join(x["読む"])), "書く": items(",".join(x["書く"])),
                      "結果→行き先": items(",".join(f"{v}→{d}" for v, d in x["結果"])),
                      "外部作用": items(",".join(x["外部"]))} for x in spec["処理"]}
    cmp_map("処理", a, b, out, str)
    # 問い合わせ（読むだけの処理）
    a = {r["問い合わせ"]: {"入力": items(r.get("入力", "")), "読む": items(r.get("読む", "")), "条件": items(r.get("条件", "")),
                           "並び": n(r.get("並び", "")), "ページ": n(r.get("ページ", "")), "列": items(r.get("列", ""))} for r in sect("問い合わせ")}
    b = {x["問い合わせ"]: {"入力": items(",".join(x["入力"])), "読む": items(",".join(x["読む"])), "条件": items(",".join(x["条件"])),
                           "並び": n(x["並び"]), "ページ": n(x["ページ"]), "列": items(",".join(x["列"]))} for x in spec["問い合わせ"]}
    cmp_map("問い合わせ", a, b, out, str)
    # トリガ（要因ごとの行き先）
    a = {(r["トリガ"], n(r["要因"])): {"行き先": n(r["行き先"])} for r in sect("トリガ")}
    b = {(t, n(c)): {"行き先": n(d)} for t, c, d in spec["トリガ"]}
    cmp_map("トリガ", a, b, out, lambda k: f"{k[0]}: {k[1]}")
    # 外部作用（外へ出す内容）
    a = {r["外部作用"]: {"出す内容": n(r["出す内容"])} for r in sect("外部作用")}
    b = {e: {"出す内容": n(c)} for e, c in spec["外部作用"]}
    cmp_map("外部作用", a, b, out, lambda k: k)
    # 返事の合流先（外部作用の返事が入るトリガ）
    a = {r["外部作用"]: {"合流先": n(r["合流先のトリガ"])} for r in sect("合流")}
    b = {e: {"合流先": n(t)} for e, t in spec["合流"]}
    cmp_map("合流", a, b, out, lambda k: k)
    # 出来事（外の出来事が何を起こすか）
    a = {(r["出来事"], n(r["起こし方"]), n(r["処理"])): {} for r in sect("出来事")}
    b = {(e, n(h), n(d)): {} for e, h, d in spec["出来事"]}
    cmp_map("出来事", a, b, out, lambda k: f"{k[0]}: {k[1]} → {k[2]}")
    # 画面と遷移（画面デザインは突合に含めない）
    a = {r["画面"]: {"見せる": items(r["見せる"]), "入力欄": items(r["入力欄"]), "受け取る値": items(r.get("受け取る値", "")), "入口": r["入口"].strip(),
                     "選ばせる slot": items(r.get("選ばせる slot", ""))} for r in sect("画面")}
    b = {x: {"見せる": items(q), "入力欄": items(f), "受け取る値": items(pr), "入口": e, "選ばせる slot": items(sl)} for x, q, f, pr, e, sl, _ in spec["画面"]}
    if any("URL" in r for r in sect("画面")):  # 計画の画面の表に URL の欄があれば、それも突合する
        for r in sect("画面"):
            a[r["画面"]]["URL"] = n(r.get("URL", ""))
        for x, *_, u in spec["画面"]:
            b[x]["URL"] = n(u)
    cmp_map("画面", a, b, out, str)
    a = {(r["画面"], n(r["操作"]), n(r["結果"])): {"行き先": n(r["行き先"])} for r in sect("画面遷移")}
    b = {(x, n(o), n(v)): {"行き先": n(d)} for x, o, v, d in spec["画面遷移"]}
    cmp_map("画面遷移", a, b, out, lambda k: f"{k[0]}: {k[1]} {k[2]}".rstrip())
    # ルート（URL。経路ごとの動作・呼ぶもの・結果の種類ごとの HTTP の状態）
    a = {(r["メソッド"].strip(), r["経路"].strip()): {"動作": r["動作"].strip(), "呼ぶもの": n(r["呼ぶもの"]), "結果→状態": items(r.get("結果→状態", ""))} for r in sect("ルート")}
    b = {(m, path): {"動作": kind, "呼ぶもの": n(tgt), "結果→状態": items(st)} for m, path, kind, tgt, st in spec["ルート"]}
    if a or not b:
        cmp_map("ルート", a, b, out, lambda k: f"{k[0]} {k[1]}")
    out += check_usecases(spec, sect("ユースケース"))
    # 不変条件（内容の文字列で突合）
    a = {n(r["内容"]): {} for r in sect("不変条件")}
    b = {n(t): {} for t in spec["不変条件"]}
    cmp_map("不変条件", a, b, out, lambda k: k)
    # 指定事項：未規定は問いに回し、それ以外は突合
    a = {}
    for r in sect("指定"):
        if "未規定" in r["指定"]:
            out.append(Finding(UNSPEC, f"指定事項 {r['項目']}", "計画で意図的に未規定（コードは既定に任せる）。問いとして残る"))
        else:
            a[r["項目"]] = {"指定": n(r["指定"])}
    b = {k: {"指定": n(v)} for k, v in spec["指定"]}
    cmp_map("指定", a, b, out, str)
    return out


VIEW_OP = "見る"  # 操作のない手順：その画面が入口として見えている


def usecases(rows: list[dict]) -> dict[str, list[dict]]:
    """計画書のユースケースの表を、ユースケースごとの手順の列（手順の番号順）にする"""
    out: dict[str, list[dict]] = {}
    for r in rows:
        out.setdefault(r["ユースケース"], []).append(r)
    return {k: sorted(v, key=lambda r: int(r["手順"]) if r["手順"].strip().isdigit() else 0) for k, v in out.items()}


def check_usecases(spec: dict, rows: list[dict]) -> list[Finding]:
    """ユースケースが画面で実現できているか：各手順が画面遷移として実在し、前の手順の行き先が次の手順の画面になっていること。
    どのユースケースにも使われない遷移は、会話で決めていない動きの可能性があるので知らせる"""
    if not rows:
        return []
    out: list[Finding] = []
    trans = {(n(a), n(o), n(v)): n(d) for a, o, v, d in spec["画面遷移"]}
    entries = {n(x[0]) for x in spec["画面"] if x[4]}
    urls = {n(x[0]): n(x[-1]) for x in spec["画面"]}
    used = set()
    for uc, steps in usecases(rows).items():
        here = None  # 直前の手順の後に見えている画面
        for st in steps:
            scr, op, res = n(st["画面"]), n(st["操作"]), n(st.get("結果", ""))
            where = f"ユースケース {uc} 手順 {st['手順']}"
            if "URL" in st and scr in urls and n(st["URL"]) != urls[scr]:  # 手順の URL は、その画面の URL（計画書に URL の列があるときだけ突合する）
                out.append(Finding(DIFF, where, f"URL は計画: {st['URL'].strip()} ／ コード: {urls[scr]}（画面「{st['画面']}」の URL）"))
            if here is not None and scr != here:
                out.append(Finding(DIFF, where, f"前の手順のあとに見えている画面は「{here}」ですが、この手順は「{st['画面']}」です"))
            if op == n(VIEW_OP):
                if here is None and scr not in entries:
                    out.append(Finding(DIFF, where, f"「{st['画面']}」は画面の入口ではありません"))
                here = scr
                continue
            dest = trans.get((scr, op, res))
            if dest is None:
                out.append(Finding(A_ONLY, where, f"画面「{st['画面']}」の操作「{st['操作']}」（結果「{st.get('結果', '')}」）は、コードの画面遷移にありません"))
                here = None
                continue
            used.add((scr, op, res))
            here = scr if dest == "そのまま" else None if dest == "戻る" else dest
    for k in trans:
        if k not in used:
            out.append(Finding(B_ONLY, f"画面遷移 {k[0]}: {k[1]} {k[2]}".rstrip(), "どのユースケースの手順にも使われていません（ユースケースが足りないか、不要な遷移）"))
    return out


COVERAGE_HEAD = ["メソッド", "経路", "結果", "状態", "具体例", "ユースケース", "確認"]


def coverage(spec: dict, plan: dict) -> tuple[list[list[str]], int, int]:
    """内部 URL（ルート）ごと、結果の種類（HTTP の状態）ごとに、計画書の具体例とユースケースが当たっているかを数える。
    具体例：処理の名前と期待する結果が合うもの。ユースケース：画面の操作（処理の名前と結果）が合う手順。
    GET の問い合わせ：その問い合わせを見せる（入力欄の選ぶ元にする）画面を通る手順。戻り値は（行, 確認できた数, 全体の数）"""
    ex: dict[tuple[str, str], list[str]] = {}
    for r in plan.get("具体例", []):
        res = n(re.split(r"[（(]", r["期待する結果"])[0])
        if "未規定" not in r["期待する結果"]:
            ex.setdefault((n(r["処理"]), res), []).append(r["ID"].strip())
    shows: dict[str, set[str]] = {}  # 画面 → 画面を開くと読む問い合わせ
    for x, q, fields, *_ in spec["画面"]:
        shows[n(x)] = set(items(q)) | {n(f.split("←")[1]) for f in split_top(fields, ",") if "←" in f}
    uc_act: dict[tuple[str, str], set[str]] = {}
    uc_get: dict[str, set[str]] = {}
    for uc, steps in usecases(plan.get("ユースケース", [])).items():
        for st in steps:
            scr, op, res = n(st["画面"]), n(st["操作"]), n(st.get("結果", ""))
            for q in shows.get(scr, ()):
                uc_get.setdefault(q, set()).add(uc)
            if op != n(VIEW_OP):
                uc_act.setdefault((op, res), set()).add(uc)
    rows, got = [], 0
    for m, path, _kind, tgt, st in spec["ルート"]:
        t = n(tgt)
        variants = [tuple(x.split("→")) for x in st.split(", ") if "→" in x] or [("", "200" if m == "GET" else "－")]
        for v, code in variants:
            e = ex.get((t, n(v)), []) if v else []
            u = sorted(uc_act.get((t, n(v)), set()) if v else uc_get.get(t, set()))
            ok = bool(e or u)
            got += ok
            rows.append([m, path, v, code, ", ".join(e), "、".join(u), "○" if ok else "未"])
    return rows, got, len(rows)


def coverage_markdown(spec: dict, plan: dict) -> list[str]:
    rows, got, total = coverage(spec, plan)
    out = ["", f"## 内部 URL ごとの確認の網羅（確認できた {got} / {total}、{got * 100 // max(total, 1)}%。具体例とユースケースが当たっているか）", "",
           "| " + " | ".join(COVERAGE_HEAD) + " |", "|" + "---|" * len(COVERAGE_HEAD)]
    return out + ["| " + " | ".join(c for c in r) + " |" for r in rows]
