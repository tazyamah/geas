# Geas（ギアス）

**AI がコードを書き、人はコードを一切読まずに、仕様書だけでレビューできる。** それを保証する言語です。

仕様書（人が読む）とコード（AI が書く）が直結合していて、仕様書に出ない動きはコードに書けません。言ったとおりに作れているかは、読んで確かめるのではなく、検査器が機械で示します。

名前の由来は、アイルランド伝承の geis（守らねばならない誓いと禁忌）です。制約を重くするほど、保証が強くなります。

## まず見る：生成された仕様書の見本

「海外にも売るお店」を題材にした、`.geas` から生成した仕様書です（人が読むのは、これだけ）。

- [仕様書の全体](https://tazyamah.github.io/geas/overseas_shop.html)：ユースケース、画面の遷移、具体例、状態、保存の形、人の保証まで。
- [前の版からの変更と、答えてほしいこと](https://tazyamah.github.io/geas/overseas_shop_changes.html)：更新のたびに、人が見るのはここだけ。

## 流れ

1. 会話で計画書（plan、Markdown）を作る（[skills/plan-interview](skills/plan-interview/SKILL.md)）。
2. 計画書から、AI がコード（`.geas`）を起こす。
3. 検査し、仕様書を生成して、計画書と突き合わせる。生成した Rust で具体例を動かして確かめる。人は仕様書をレビューする（[skills/geas-develop](skills/geas-develop/SKILL.md)）。

## 使い方

Python 3.10 以上、外部ライブラリなし（試作の Python 版。生成物と実行部は Rust）。

```
python -m geas check アプリ.geas                  # 検査
python -m geas review アプリ.geas --plan 計画書.md -o 仕様書.html   # 人が読む仕様書
python -m geas rust アプリ.geas -o 出力先          # Rust を生成
python -m geas compare アプリ.geas 計画書.md       # 計画書と突合（具体例も実行）
python -m pytest tests                            # テスト
```

## 読むもの

- AI の方：[AGENTS.md](AGENTS.md) → [docs/ai-guide.md](docs/ai-guide.md)
- 文書：[文書サイト](https://tazyamah.github.io/geas/docs/)（一覧は [docs/README.md](docs/README.md)）
- 動く全体の例：[tests/fixtures/overseas_shop/](tests/fixtures/overseas_shop/)
- 信頼してよい範囲（保証できること・できないこと）：[docs/why-spec-and-code-dont-drift.md](docs/why-spec-and-code-dont-drift.md)

## ライセンス

geas 本体は GPL-3.0-or-later です。生成された Rust とバイナリは制限なく自由に使えます。詳しくは [docs/license.md](docs/license.md)、貢献の条件は [CONTRIBUTING.md](CONTRIBUTING.md)。
