# ライセンスの考え方

この文書は方針の説明です。法的な文言の最終確認は、弁護士に依頼する前提の下書きです。

## 3つの層

| 層 | 対象 | ライセンス |
|---|---|---|
| geas 本体 | 構文解析・検査・生成器・CLI など `geas/` の Python のコード、docs、skills | GPL-3.0-or-later（[LICENSE](../LICENSE)） |
| 生成コードに埋め込まれる部品 | 生成される Rust に写される、範囲検査・有理数・丸め・割合検査・冪等キーの組・保証違反・JSON の入出力・WASM の入口などのコード片 | MIT OR Apache-2.0（[LICENSE-MIT](../LICENSE-MIT)・[LICENSE-APACHE](../LICENSE-APACHE)） |
| 生成された Rust とバイナリ | `.geas` から作られた Rust のコードと、そこからビルドしたもの | 利用者のもの。制限なし（埋め込まれた部品の MIT/Apache の表示だけ守る） |

## 意図

- 使うのは自由。geas を改変（改善）して配るなら、その改変も GPL で公開する。
- 利用者が geas で作ったアプリは、公開義務なく自由に使える。生成物へ GPL が及ぶ「汚染」を避けるため、生成コードへ写されるコード片は別の寛容なライセンスにする（GCC Runtime Library Exception と同じ問題への、別の解き方）。
- Rust の慣習に合わせ、埋め込み部品は MIT と Apache-2.0 の二重（どちらか選べる）。

## 埋め込み部品の範囲

次のものを埋め込み部品とする。ソース中では `SPDX-License-Identifier: MIT OR Apache-2.0` の印をつけ、生成コードの先頭にも同じ印を出す。

- `geas/rustgen.py` の `PRELUDE`
- `geas/hostgen.py` の `JSON_PRELUDE` と `CALL_WASM`
- 生成器が `.geas` の内容に応じて Rust の文字列として出力するコード片（検査の `if`、保証の確認、文脈の `検査()` など）全般

それ以外の `geas/` 配下は本体（GPL）。

## 将来 AGPL へ移る道

geas を Web サービスとして改変して提供する場合（配布しないので GPL の公開義務が及ばない）を閉じたくなったとき、AGPL へ移れるようにする。そのため、貢献は [CONTRIBUTING.md](../CONTRIBUTING.md) の条件で受ける（DCO の署名と、著作権者による再許諾の同意）。

## 未決

- 生成コードの先頭の表示の文面（短い SPDX の1行で足りるか）。
- 弁護士による文言の確認。
