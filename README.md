# Geas（ギアス）

日本語 | [English](README.en.md)

**AI がコードを書き、人はコードを一切読まずに、仕様書だけでレビューできる。** それを保証する言語です。

仕様書（人が読む）とコード（AI が書く）が直結合していて、仕様書に出ない動きはコードに書けません。言ったとおりに作れているかは、読んで確かめるのではなく、検査器が機械で示します。

名前の由来は、アイルランド伝承の geis（守らねばならない誓いと禁忌）です。制約を重くするほど、保証が強くなります。

## まず見る：生成された仕様書の見本

「海外にも売るお店」を題材にした、`.geas` から生成した仕様書です（人が読むのは、これだけ）。

- 仕様書の全体（[日本語](https://tazyamah.github.io/geas/overseas_shop.html) / [English](https://tazyamah.github.io/geas/en/overseas_shop.html)）：ユースケース、画面の遷移、具体例、状態、保存の形、人の保証、実行環境（コンテナと基盤の構成図）まで。構成図も表も、`.geas` の宣言から機械で生成しています（手で描いた図は 1 枚もありません）。
- 前の版からの変更と、答えてほしいこと（[日本語](https://tazyamah.github.io/geas/overseas_shop_changes.html) / [English](https://tazyamah.github.io/geas/en/overseas_shop_changes.html)）：更新のたびに、人が見るのはここだけ。

## 流れ

1. 会話で計画書（plan、Markdown）を作る（[skills/plan-interview](skills/plan-interview/SKILL.md)）。
2. 計画書から、AI がコード（`.geas`）を起こす。
3. 検査し、仕様書を生成して、計画書と突き合わせる。生成した Rust で具体例を動かして確かめる。人は仕様書をレビューする（[skills/geas-develop](skills/geas-develop/SKILL.md)）。

## 使い方

Rust（cargo）だけで動きます。本体も、生成物も、サーバーの実行部も Rust です。

```
cargo install --path crates/geas-cli               # 入れる（実行ファイル名は geas）
geas check アプリ.geas                             # 検査
geas review アプリ.geas --plan 計画書.md -o 仕様書.html   # 人が読む仕様書
geas rust アプリ.geas -o 出力先                    # Rust を生成
geas compare アプリ.geas 計画書.md                 # 計画書と突合（具体例も実行）
cargo test                                         # テスト
```

## 読むもの

- AI の方：[AGENTS.md](AGENTS.md) → ai-guide（[日本語](docs/ai-guide.md) / [English](docs/en/ai-guide.md)）
- 文書：文書サイト（[日本語](https://tazyamah.github.io/geas/docs/) / [English](https://tazyamah.github.io/geas/en/docs/)）、一覧（[日本語](docs/README.md) / [English](docs/en/README.md)）
- 動く全体の例：[tests/fixtures/overseas_shop/](tests/fixtures/overseas_shop/)
- 信頼してよい範囲（保証できること・できないこと）：[日本語](docs/why-spec-and-code-dont-drift.md) / [English](docs/en/why-spec-and-code-dont-drift.md)

## ライセンス

geas 本体は GPL-3.0-or-later です。生成された Rust とバイナリは制限なく自由に使えます。詳しくは license（[日本語](docs/license.md) / [English](docs/en/license.md)）、貢献の条件は [CONTRIBUTING.md](CONTRIBUTING.md)。
