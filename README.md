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
  input 注文ID: id required                             // 宣言：input slot reads writes transition result effects
  reads 注文.状態
  ...
  let 注文 = 注文[注文ID] else { return 注文なし }        // 本体は Rust 風（body も then もない）
  if 注文.状態 != 受付 { return 状態違い }
  for k in 注文.明細 { request 決済.請求(k, idempotency_key = (注文ID, k)) }
  return 要求済
}
nexus 注文受付 { 受注: 受付 → 決済要求, 顧客なし → reply, ... }   // flow は nexus の別名。行き先は reply か wait トリガ名
trigger 決済待ち(注文ID: id) { reply 決済成功 → ... , deadline now + 30 min → ... }
```

データの項目は、原則としてすべて「無いかもしれない」（null になりうる）ものとして扱います。必ずある約束は `required` で書き、仕様書の「必須」に出ます（キーと状態の項目は必須）。null になりうる項目は、使う前に `let 題 = メモ.題 else { return 題なし }` か `if メモ.題 is null { return 題なし }` で確かめます（確かめずに使うと検査で止まるので、生成したコードは null で壊れません）。新規作成では書かなかった項目が null になり、`メモ.題 = null` で null に戻せます。1件や項目そのものが無いこと（undefined。`D[k] is absent` で確かめる）と、値が null であることは別のものです。問い合わせでは、null はどの条件・絞り込みも満たさず、並びでは最後になります。

処理と問い合わせの入力も同じで、書かなければ「無いかもしれない」入力です。必ず要る入力には `required` を書き（`input 注文ID: id required`）、仕様書の入力欄に「（必須）」と出て計画書と突き合わせます。無いかもしれない入力は、使う前に `let x = 入力 else { return … }` か `if 入力 is null { return … }` で確かめます（null になりうる項目へそのまま入れるだけなら、確かめは要りません）。外から受け取るときは、キーが無いのも null も同じ「無い」として扱い、空文字は「ある」です。空文字と未入力を区別したいときは、処理の手前の入力の境界で不正として弾きます。必ず要る入力が無い・null なら入力不正です。`trigger` の引数には、必ず要る入力しか渡せません。

整数と金額は違う型で、そのままでは混ぜられません。許すのは、同じ型どうしの計算、金額 × 整数（数量ぶん）、金額 × 割合（端数は `丸め` で整数にする）です。違う型にするときは `money(個数)`・`int(金額)` で変えてから計算します。数字をそのまま書いたもの（`0` など）は、どちらの型としても使えます。

結果の中身には名前を付けます（`result 受注結果 { 受付(注文ID: id), 在庫不足(不足: set of id), 顧客なし }`）。ネクサスで次の処理へ進むときは、結果の中身を次の処理の入力へ名前と型で渡します。過不足や型の違いはエラーです（`trigger` の引数と同じルール）。

外から足す部品は、保証を geas の式で書きます（`part 税込(x: money(円) 0..) -> money(円) 0.. guarantee result >= x`）。保証が無い部品や、戻り値（`result`）を使わない保証は、検査でエラーです。呼ぶたびに、戻り値の範囲と保証を確かめる Rust の `if` を生成し、違反したときは処理を止めます（書込は適用されず、保存は変わりません）。部品の実装は、生成する `trait 部品群` をアプリが書きます。引数と戻り値の型（金額か整数か）は、検査で確かめます。`丸め` のような組み込みの部品は宣言しません。

外部作用（`request`）には冪等キー（`idempotency_key`）が必須で、ID か文字列、またはその組 `(注文ID, k)` で書きます。`for` の中では、回すたびに違う値と、`for` の外で決まる値の両方を組に含めます（回す値だけだと、別の注文で同じ商品を回したときにキーがぶつかり、外のサービスに重複として捨てられるため）。組は、区切りがあいまいにならない形（`["O1","P1"]`）で1つの文字列にして外へ渡します。

`if` の中には、`return`・項目の更新・`update`・`request`・入れ子の `if` を書けます（`let` は外に書きます）。検査は、状態を変える文ごとに「その分岐で元の状態を確かめているか」を追います。

問い合わせの `filter` と `limit` に使う入力が無いときは、絞り込まず（`limit` なら全件を）返します。この動きは、仕様書の条件欄に「（最小が無ければ絞らない）」のように出ます。

`query` の `where` は、1行が `or` でつないだ組、行どうしが `and` です（`where 状態 == 公開 or 状態 == 下書き`、`where 重さ >= 10`）。

`query` に `select 注文ID, 顧客.顧客ID as 顧客, sum(明細) as 点数` と書くと、IDの列でなく列つきの行を返します（参照の先の項目は結合、`sum` と `count` はキーつきの集まりの集計）。

文字列の検索は `filter 本文 contains 単語` が完全一致の部分文字列で、同一視する軸は書いたものだけです（`ignore_case`＝大小、`ignore_kana`＝ひらがなとカタカナ、`ignore_width`＝全角と半角の英数記号）。正規化（NFC）は入力の境界で済ませる前提で、検索には持ち込みません。

`invariant` に書けるのは「項目 >= 数」と「状態の項目 == 状態 implies 項目 > 数」の形です。検査は、項目へ書き込むたびに、その場で値の範囲から約束が守れることを示し、示せなければエラーにします（項目の型の範囲 `int 0..` なども同じように示します）。値の範囲は、入力と項目の範囲、`if x.項目 <= 0 { return … }` のような確認、減らす前の不足の確認から分かります。

他に `state`（initial middle final endless reject）、`result`、`effect`（failures no_wait）、`part`、`invariant`（implies）、`query`、`event`。全体の例は [tests/fixtures/shop/shop.geas](tests/fixtures/shop/shop.geas)。

## 状態

試作は [tazyamah/specgap](https://github.com/tazyamah/specgap) の `specgap_lang/` で進めてきました。ここへ移して、v0.1.0 を目指します。キーワードは英語にしました（名前と、構文木の中身は日本語のまま）。
