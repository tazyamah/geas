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
        ("トリガ", {"トリガ", "要因", "行き先"}), ("外部作用", {"外部作用", "出す内容"}), ("出来事", {"出来事", "起こし方", "処理"}), ("指定", {"項目", "指定"}), ("具体例", {"期待する結果"}),
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
                           "並び": n(r.get("並び", "")), "先頭": n(r.get("先頭", ""))} for r in sect("問い合わせ")}
    b = {x["問い合わせ"]: {"入力": items(",".join(x["入力"])), "読む": items(",".join(x["読む"])), "条件": items(",".join(x["条件"])),
                           "並び": n(x["並び"]), "先頭": n(x["先頭"])} for x in spec["問い合わせ"]}
    cmp_map("問い合わせ", a, b, out, str)
    # トリガ（要因ごとの行き先）
    a = {(r["トリガ"], n(r["要因"])): {"行き先": n(r["行き先"])} for r in sect("トリガ")}
    b = {(t, n(c)): {"行き先": n(d)} for t, c, d in spec["トリガ"]}
    cmp_map("トリガ", a, b, out, lambda k: f"{k[0]}: {k[1]}")
    # 外部作用（外へ出す内容）
    a = {r["外部作用"]: {"出す内容": n(r["出す内容"])} for r in sect("外部作用")}
    b = {e: {"出す内容": n(c)} for e, c in spec["外部作用"]}
    cmp_map("外部作用", a, b, out, lambda k: k)
    # 出来事（外の出来事が何を起こすか）
    a = {(r["出来事"], n(r["起こし方"]), n(r["処理"])): {} for r in sect("出来事")}
    b = {(e, n(h), n(d)): {} for e, h, d in spec["出来事"]}
    cmp_map("出来事", a, b, out, lambda k: f"{k[0]}: {k[1]} → {k[2]}")
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
