# Geas（ギアス）

約束を守らせる言語。計画書（会話で作る仕様書）とコードを突き合わせて、言ったとおりに作れているかを機械で保証します。

名前の由来は、アイルランド伝承の geis（守らねばならない誓いと禁忌）です。制約を重くするほど、保証が強くなります。

## 用語

| 日本語 | 名前 |
|---|---|
| 処理 | Deed |
| 参照（外から埋まる枠） | Slot |
| 束（複数の外部作用を結ぶ1つの約束） | Bond |
| フロー（結果の行き先でつなぐ） | Nexus |
| データへの参照型 | ref |

選定理由と避けた候補は [docs/naming.md](docs/naming.md) にあります。コードの拡張子は `.geas` です。

## 流れ

1. 会話で計画書（plan、Markdown）を作る（[skills/plan-interview](skills/plan-interview/SKILL.md)）。
2. 計画書から、AI がコード（`.geas`）を起こす。
3. コードを検査し、仕様書を生成して、計画書と突き合わせる。具体例は生成した Rust で実際に動かして確かめる。

## 使い方（試作の Python 版。最終的にはすべて Rust にする）

```
python -m geas check examples/shop.geas                          # 検査
python -m geas spec examples/shop.geas                           # 仕様書を生成
python -m geas rust examples/shop.geas -o examples/shop_rust     # Rust を生成
python -m geas compare examples/shop.geas examples/shop_plan.md  # 計画書と突合（具体例も実行）
python -m pytest tests                                           # テスト
```

## 例

- [お店](examples/shop.geas)：短い見本です（計画書は [examples/shop_plan.md](examples/shop_plan.md)）。
- ひとり用つぶやき：会話から計画書を作り、コード、Rust/WASM、画面まで試した例です。[specgap の `lang_examples/solo_tweet`](https://github.com/tazyamah/specgap/tree/main/lang_examples/solo_tweet) にあります（試作の名前のままです）。

## 状態

試作は [tazyamah/specgap](https://github.com/tazyamah/specgap) の `specgap_lang/` で進めてきました。ここへ移して、v0.1.0 を目指します。言語のキーワードは、今は日本語の仮のままです（英語化と、処理→Deed などの置き換えは、別の決定として進めます）。
