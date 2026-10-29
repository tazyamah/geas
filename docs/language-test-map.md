# 言語機能と動作テストの対応表

geas の保証は、「生成器が正しければ、どのアプリも `.geas` どおりに動く」ことに置きます。そのために、[language-reference.md](language-reference.md) の項目ごとに、**生成した Rust を実際に動かして確かめるテスト**があるかを、この表で管理します。新しい機能を足したら、この表に行を足し、動かすテストを書いてください。

「実行」の列の意味は次のとおりです。

- **◎**：小さな `.geas` から生成した Rust を動かし、結果・書き込み・外部作用を確かめている。
- **○**：検査器（静的）で誤りを止めるテストだけがある。実行時の動きは、別の項目のテストが間接的に通している。
- **✕**：動かす確認がまだ無い。

小さな `.geas` を書いて動かすテストは、[tests/test_behavior.py](../tests/test_behavior.py)（この表のために足したもの）と、`tests/test_geas.py`・`tests/test_delete.py` にあります。お店アプリ（`tests/fixtures/overseas_shop`）は、サーバーまで組んで動かす `tests/test_shop_app.py`（以下「お店の通し」）が確かめます。

| リファレンスの項目 | 実行 | 動かすテスト |
|---|---|---|
| 2. `int` の範囲・`text` の長さと文字・`check gtin`・`ratio`・`currency` を外から入れるときの確かめ | ◎ | `test_geas.py` の「ホストは入力の長さと文字を境界で確かめる」「check_gtin は正しい13桁だけ通す」「slot も境界で宣言の範囲を確かめる」、お店の通し「外から来る値は境界で確かめる」 |
| 2. `time`・`vtime`・合成型（`ltype X { … }`） | ◎ | `test_geas.py` の「ホストは時刻の計算をして…」「vtime の変換は暦どおりの秒になり…」「標準のタイムゾーンで暦の秒がずれ…」「合成型はホストの境界で成分ごとに確かめる」 |
| 2. 写像・集合・`ref` | ◎ | 「id の写像の値は合成型でも…」、`test_behavior.py`（数量の写像・集合の入力） |
| 3. 無いかもしれない（null）と、キーが無い（absent）の区別 | ◎ | `test_geas.py` の「ホストは null とキーが無いことを区別する」「…キー無しと null を同じに扱う」 |
| 3. 状態の遷移（宣言した順にしか進まない・順を飛ばす・戻す・繰り返すと断る） | ◎ | `test_behavior.py` の「状態は宣言した順にしか進まず…」（検査で止まる側は `test_geas.py` の「宣言に無い遷移は止まる」） |
| 3. `reject`（終わった状態からの遷移） | ○ | 検査で止める（`test_geas.py`）。実行時は、遷移の手前の確認が断る動きを上の行が確かめる |
| 3. `invariant`（在庫が 0 を下回らないなど） | ◎ | 在庫不足の注文を断り、受注後の在庫が 0 以上のままであることを `test_behavior.py` が確かめる。示せない書き方は検査が止める（`test_geas.py` の「在庫不足の判定を削ると不変条件が示せない」など） |
| 3. 削除：`detach` | ◎ | `test_delete.py` の「生成した削除は参照元を扱い…」 |
| 3. 削除：`cascade`・`leave` | ◎ | `test_behavior.py` の「削除は cascade で参照元も消し leave で残す」 |
| 3. 削除：`removable in`・`keep` | ◎／○ | `removable` は `test_delete.py`（消してよい状態だけ消す）。`keep` は検査で止める側のみ |
| 4. `create`・`x.f = …`・`x.f += …`・`update { … -= … }`・`delete` | ◎ | `test_behavior.py`（受注・在庫の加減算・状態の更新）、`test_delete.py` |
| 4. 内包表記・`sum`・`is nonempty`・`let … else`・`if`・`return` | ◎ | `test_behavior.py`（在庫不足の集合・小計・存在確認）、`test_geas.py` の null 関連 |
| 4. 金額の丸め `round(…, floor｜ceil｜half_up)`・割合×金額 | ◎ | `test_behavior.py` の「金額の丸めは指定の方向になり…」、`test_geas.py` の「組み込みの丸めは単位より近い値を返す」 |
| 4. 割り算と余り・`convert`・`part` の保証 | ◎ | `test_geas.py` の「割り算と余りは生成コードで動く」「convert は生成コードの中で計算され…」「部品の保証は呼ぶたびに確かめ…」 |
| 4. `request`（冪等キー・`for` の中の組） | ◎ | `test_behavior.py` の「…外部作用の冪等キーは回す値ごとに決まる」、お店の通し（同じ冪等キーで引き直す） |
| 5. `rule`（上から順に最初に当てはまる行） | ◎ | お店の通し（国内送料・海外送料・税率）。誤りは `test_rest.py` の「rule の誤りは止まる」 |
| 6. `effect` の `replies`・`joins`・`no_wait`・`failures` | ◎ | お店の通し（決済・倉庫の返事、返金）、`test_geas.py` の「外部作用の返事は joins で…」 |
| 6. `trigger` の `all`（待ち合わせ）・どれか1つ・`deadline` | ◎ | お店の通し（返事がそろって確定／30分で取り消し／拒否で取り消し） |
| 7. `nexus` の次の処理・`reply`・`wait`・`par` | ◎ | お店の通し（受注のあと並列の枝が動く）。枝の重なりの誤りは `test_geas.py` の「par の枝が同じ項目に書くか…」 |
| 7. `event … on online｜opened`・`each` | ◎ | お店の通し「決済の返事が落ちても、つながりが戻ると…」 |
| 8. `query` の `filter`（equals／at_least／at_most／contains ignore_case）・`where`（or）・`order`・`page`・`select` | ◎ | `test_behavior.py` の「問い合わせは絞り込み…」、`test_geas.py` の「null になりうる項目の問い合わせ…」「page は並びのうえで先頭の1ページを返す」 |
| 8. `filter … within`・`sum(…)`・`count(…)` の列 | ○ | `sum`・`count` は `test_geas.py`（null 関連）。`within` は動かす確認が無い（✕） |
| 9. `screen`（欄・行・操作・遷移） | ◎ | `test_screens.py`、お店の通し（計画書の「画面の具体例」から生成した Playwright。node と Playwright があるときだけ） |
| 9. ブラウザ拡張の `read`（url／element／row／lookup） | ◎ | `test_extgen.py`、お店の通し（拡張） |
| 10. `resource`（7 動作・結果ごとの HTTP の状態・入れ子・`key`） | ◎ | `test_rest.py`、お店の通し（計画書の「URL の具体例」から生成した HTTP のテスト） |
| サーバーの再起動で状態が残る | ◎ | お店の通し「再起動しても注文と返事待ちは残る」 |
| モジュール（`use`／`module`） | ◎ | `test_geas.py` の「モジュールを使ったコードがビルドでき動く」 |

## まだ無いもの（✕）

- `filter … within`（期間での絞り込み）を動かす確認。
- `ratio` 型の項目への書き込み、`bytes` 型の値の入出力（ホスト経由）。
- `endless` の状態。
- 同時に来た書き込み（サーバーの並行リクエスト）の直列化。サーバー実行部のテストとして別に足す。
