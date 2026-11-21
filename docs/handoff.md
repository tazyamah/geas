# 引き継ぎ

次に geas を扱う人（AI を含む）が、最初に読むメモ。

## まず読むもの

1. [ai-guide.md](ai-guide.md)：入れ方、コマンド、作業の流れ、守ること、ソースの地図。
2. [why-spec-and-code-dont-drift.md](why-spec-and-code-dont-drift.md)：仕様書とコードがずれない仕組みと、信頼できる範囲・できない範囲。
3. [roadmap.md](roadmap.md)：未着手・保留の一覧。**何をやり残しているかはここ**。
4. [language-reference.md](language-reference.md)：言語の書き方。食い違ったら検査器が正本。
5. [language-test-map.md](language-test-map.md)：言語機能ごとの動作テストの対応表。

文書の一覧は [README.md](README.md)。決めた経緯・案・済みの記録は [history/](history/README.md) にあり、読まなくてよい。

## いまの状態

- 本体（検査器・生成器・CLI）も、生成物も、サーバーの実行部も Rust（cargo だけで動く）。生成物は `unsafe`・`unwrap`・`expect`・添字・素の算術を持たず、異常は `Fault` で返す（clippy の deny で強制）。
- 進め方：「大部分を geas の書き方で保証し、実行時の損失をゼロにする」。これを満たせないものを 1 件ずつ議論している。残りは [roadmap.md](roadmap.md)。

## 作業の約束

- 返答・コメント・説明文は日本語（`docs/en/` と `README.en.md` は英訳）。
- 小さい PR にして、マージしてから次へ。PR を積み重ねない。
- 質問や感想は修正依頼ではない。直す前に答えて確認する。
- 文書には、現在の仕様と使い方だけを書く。案・決めた経緯・済みの議論は [history/](history/README.md) に移す。
- テスト：`cargo test`。
