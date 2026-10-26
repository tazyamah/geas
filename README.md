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
python -m geas check アプリ.geas                  # 検査
python -m geas spec アプリ.geas                   # 仕様書を生成
python -m geas rust アプリ.geas -o 出力先          # Rust を生成
python -m geas compare アプリ.geas 計画書.md       # 計画書と突合（具体例も実行）
python -m pytest tests                            # テスト
```

ひとり用つぶやきの例（会話から計画書を作り、コード、Rust/WASM、画面まで試したもの）は、[specgap の `lang_examples/solo_tweet`](https://github.com/tazyamah/specgap/tree/main/lang_examples/solo_tweet) にあります（試作の名前のままです）。

## 書き方（キーワードは英語、名前は日本語のまま）

```
data 注文 { 注文ID: id unique, 状態: 注文状態 ... }      // 型：id set of id text time bytes int ratio money(円) ref X
deed 決済要求 {
  input 注文ID: id                                      // 宣言：input slot reads writes transition result effects
  reads 注文.状態
  ...
  let 注文 = 注文[注文ID] else { return 注文なし }        // 本体は Rust 風（body も then もない）
  if 注文.状態 != 受付 { return 状態違い }
  for k in 注文.明細 { request 決済.請求(k, idempotency_key = k) }
  return 要求済
}
nexus 注文受付 { 受注: 受付 → 決済要求, 顧客なし → reply, ... }   // flow は nexus の別名。行き先は reply か wait トリガ名
trigger 決済待ち(注文ID: id) { reply 決済成功 → ... , deadline now + 30 min → ... }
```

`if` の中には、`return`・項目の更新・`update`・`request`・入れ子の `if` を書けます（`let` は外に書きます）。検査は、状態を変える文ごとに「その分岐で元の状態を確かめているか」を追います。

`query` の `where` は、1行が `or` でつないだ組、行どうしが `and` です（`where 状態 == 公開 or 状態 == 下書き`、`where 重さ >= 10`）。

他に `state`（initial middle final endless reject）、`result`、`effect`（failures no_wait）、`part`、`invariant`（implies）、`query`、`event`。全体の例は [tests/fixtures/shop/shop.geas](tests/fixtures/shop/shop.geas)。

## 状態

試作は [tazyamah/specgap](https://github.com/tazyamah/specgap) の `specgap_lang/` で進めてきました。ここへ移して、v0.1.0 を目指します。キーワードは英語にしました（名前と、構文木の中身は日本語のまま）。
