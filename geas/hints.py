# SPDX-License-Identifier: GPL-3.0-or-later
"""検査の指摘（Diag）に、AI が直すための手がかりを付ける。

指摘の種類（code）ごとに、言語リファレンスのどこを見るか（ref）と、典型的な直し方（fix）を返す。
検査器が正本で、ここは「どこから直すか」の索引。指摘の文言そのものは変えない。
"""
import json

from .check import ERROR, QUESTION, WARN

REF = "docs/language-reference.md"

# 種類 → (見る節, 直し方)
TABLE = {
    "型": ("§2 型", "式や項目の型をそろえる。範囲のある型（ltype）に入る根拠（上限の確認や範囲）が示せないときは、確認の文を足すか範囲を広げる"),
    "名前": ("§1 全体の決まり", "名前は半角英数にし、重複や未定義を直す。日本語の表示名は直前の /// に書く"),
    "画面": ("§9 画面（screen）", "画面の入力・遷移・表示する項目が、処理と entity に実在するか確かめる"),
    "リソース": ("§10 REST（resource）", "処理と問い合わせをどれかの resource の7アクションに割り当てる（割り当てがないものがある）"),
    "トリガ": ("§6 外とのやりとり", "トリガの種類・期限・冪等キーなど、宣言が足りない所を書き足す"),
    "範囲": ("§2 型", "値が範囲に収まる根拠が示せない。上限の確認を足すか、`trust` で理由つきに緩める"),
    "行き先": ("§9 画面（screen）", "遷移先の画面・結果が実在するか、すべての結果に行き先があるか確かめる"),
    "遷移": ("§3 データと状態", "状態の遷移の宣言（transition）と、処理の中の状態の書き換えを一致させる"),
    "単位": ("§2 型", "金額の通貨や単位が合わない足し算・比較を直す（同じ通貨にそろえるか、変換を書く）"),
    "削除": ("§3 データと状態", "delete の参照元の扱い（detach など）と、削除できる状態（removable）を宣言する"),
    "null": ("§2 型", "無いかもしれない項目を使っている。required を付けるか、存在の確認（else の分岐）を書く"),
    "並列": ("§7 フローと出来事", "par の枝どうしで同じものを書き換えない／合流の宣言を足す"),
    "丸め": ("§2 型", "割り算・割合の丸め方（floor／ceil／half_up）を決めて書く。決めるのは人（問い）"),
    "宣言": ("§4 処理（deed）", "reads・writes・touches などの宣言を、処理の中身と一致させる"),
    "合流": ("§7 フローと出来事", "待ち合わせ（join）の条件と、合流する枝を揃える"),
    "出来事": ("§7 フローと出来事", "出来事（event）の発生元と受け手を宣言する"),
    "冪等": ("§6 外とのやりとり", "外への書き込みに冪等キーを付けるか、冪等でないことを宣言する"),
    "不変条件": ("§3 データと状態", "処理が不変条件を壊さないと示せない。確認の文を足すか、不変条件を見直す（見直すのは人）"),
    "結果": ("§4 処理（deed）", "処理の結果の種類と、呼び出し側が扱う種類を一致させる"),
    "応答": ("§10 REST（resource）", "結果の種類に HTTP ステータスを割り当てる"),
    "失敗": ("§4 処理（deed）", "失敗しうる式の結果を、else などで扱う"),
    "保証": ("§6 外とのやりとり", "trust の段階と理由を書く。理由は人の保証として仕様書に出る"),
    "状態": ("§3 データと状態", "状態の項目と遷移を揃える"),
    "項目": ("§3 データと状態", "存在しない項目を参照している。entity の定義を確かめる"),
    "存在": ("§4 処理（deed）", "見つからないときの扱いを決める（決めるのは人。問い）"),
    "書込": ("§4 処理（deed）", "書き込みの適用順・上書きを確かめる"),
    "検査数字": ("§2 型", "検査数字の桁の長さ・並びを確かめる"),
    "検査": ("§2 型", "検査の形（check）の引数を確かめる"),
    "文字種": ("§2 型", "文字種の指定を確かめる"),
    "割り算": ("§2 型", "割る数が 1 以上と示せるか、`x / d or 0` で 0 のときの値を書く"),
    "並び": ("§2 型", "並びの指定を確かめる"),
    "モジュール": ("§1 全体の決まり", "use／module の参照先を確かめる（docs/modules.md）"),
    "データ": ("§3 データと状態", "entity の定義を確かめる"),
    "直列": ("§3 データと状態", "storage（保存先）の定義と、entity の store 行を揃える"),
    "rule": ("§5 slot の値の決め方（rule）", "rule の書き方（条件と値）を確かめる"),
    "名前そのもの": ("§1 全体の決まり", "名前を直す"),
    "表示名": ("§1 全体の決まり", "直前の /// に日本語の表示名を書く"),
    "外部作用": ("§6 外とのやりとり", "外への作用の宣言（trust・冪等）を足す"),
    "通信": ("§6 外とのやりとり（effect の over http）", "通信先を https://ホスト名/パス にし、認証は needs secret で宣言し、返事の状態をすべて結果の種類に結ぶ（status other が必須）"),
    "環境": ("§4 処理（deed）の「動かす前提 needs」", "needs の書き方（secret・exposure・instances と理由）を直す。環境の実際は書かない"),
}
DEFAULT = ("§11 検査の結果の種類", "指摘の文言を読み、該当する節を language-reference.md で確かめる")


def hint_of(d) -> dict:
    sec, fix = TABLE.get(d.code, DEFAULT)
    return {"ref": f"{REF} {sec}", "fix": fix}


def as_dict(d) -> dict:
    """1件の指摘。where と message は、コードに書いた名前のまま（表示名に置き換えない）。line は分かるときだけ"""
    out = {"level": {ERROR: "error", WARN: "warning", QUESTION: "question"}.get(d.level, d.level),
           "code": d.code, "where": d.code_where or d.where, "message": d.code_msg or d.msg}
    if d.line:
        out["line"] = d.line
    if d.level != WARN or d.code not in ("表示名",):
        out["hint"] = hint_of(d)
    if d.level == QUESTION:
        out["hint"]["fix"] = "人に1問ずつ聞いて決める（skills/geas-develop）。答えをコードに反映して再検査する。" + out["hint"]["fix"]
    return out


def to_json(ck, path: str) -> str:
    return json.dumps({"file": path, "ok": ck.ok, "ready": ck.ready, "diagnostics": [as_dict(d) for d in ck.diags]},
                      ensure_ascii=False, indent=2) + "\n"


def syntax_json(path: str, e) -> str:
    """構文エラー（検査まで進めない）の JSON。e は例外（line・col・file・hint があれば載せる）か、文言"""
    fix = "メッセージの行を、近い例（tests/fixtures/overseas_shop/）と見比べて書き方を直す"
    diag = {"level": "syntax", "code": "構文", "where": getattr(e, "file", "") or path, "message": getattr(e, "msg", "") or str(e),
            "hint": {"ref": f"{REF} §1 全体の決まり", "fix": (getattr(e, "hint", "") or fix)}}
    for k in ("line", "col"):
        if getattr(e, k, 0):
            diag[k] = getattr(e, k)
    return json.dumps({"file": path, "ok": False, "ready": False, "diagnostics": [diag]}, ensure_ascii=False, indent=2) + "\n"
