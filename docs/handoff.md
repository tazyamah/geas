# 引き継ぎ

次に geas を扱う人（AI を含む）が、最初に読むメモ。最終更新：2026-10-30。

## まず読むもの

1. [ai-guide.md](ai-guide.md)：入れ方、コマンド、作業の流れ、守ること、ソースの地図。
2. [why-spec-and-code-dont-drift.md](why-spec-and-code-dont-drift.md)：仕様書とコードがずれない仕組みと、信頼できる範囲・できない範囲。
3. [roadmap.md](roadmap.md)：未着手・保留の一覧。**何をやり残しているかはここ**。
4. [language-reference.md](language-reference.md)：言語の書き方。食い違ったら検査器が正本。
5. [language-test-map.md](language-test-map.md)：言語機能ごとの動作テストの対応表。
6. [open-questions.md](open-questions.md)：後で検証すること。

## いまの状態

- 検査器・生成器・CLI は Python（試作）、生成物と実行部は Rust。生成物は `unsafe`・`unwrap`・`expect`・添字・素の算術を持たず、異常は `Fault` で返す（clippy の deny で強制）。
- 値が項目の範囲と物理型に収まることを、検査器が区間で示す（#76）。写像の要素数は `max N`（#77 で境界でも確かめる）。割合は分母 1〜10⁹ の物理上限があり、割合どうしの掛け算も途中の桁を示す（#78）。
- 進め方：「大部分を geas の書き方で保証し、実行時の損失をゼロにする」。これを満たせないものを 1 件ずつ議論している。続きは roadmap.md の表の 3 から。

## 作業の約束

- 返答・コメント・説明文は日本語。
- 小さい PR にして、マージしてから次へ。PR を積み重ねない。
- 質問や感想は修正依頼ではない。直す前に答えて確認する。
- テスト：`python -m pytest tests`。`rusqlite` を取得できない環境では `runtime` と `shop_app` のテストが動かない（PR に書く）。生成物（`tests/fixtures/shop/shop_rust`）は、生成器を変えたら `python -m geas rust tests/fixtures/shop/shop.geas -o tests/fixtures/shop/shop_rust` で作り直す。
