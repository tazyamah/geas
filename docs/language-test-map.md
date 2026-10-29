# 言語機能と動作テストの対応表

geas の保証は、「生成器が正しければ、どのアプリも `.geas` どおりに動く」ことに置きます。そのために、[language-reference.md](language-reference.md) の項目ごとに、**生成した Rust を実際に動かして確かめるテスト**があるかを、この表で管理します。新しい機能を足したら、この表に行を足し、動かすテストを書いてください。

「実行」の列の意味は次のとおりです。

- **◎**：小さな `.geas` から生成した Rust を動かし、結果・書き込み・外部作用を確かめている。
- **○**：検査器（静的）で誤りを止めるテストだけがある。実行時の動きは、別の項目のテストが間接的に通している。
- **✕**：動かす確認がまだ無い。

小さな `.geas` を書いて動かすテストは、次の 4 か所にあります（お店アプリを使わずに、機能ごとに確かめます）。

- [tests/test_behavior.py](../tests/test_behavior.py)：生成した Rust（処理・問い合わせ・削除・金額）を直接動かす。
- [tests/test_runtime.py](../tests/test_runtime.py)：サーバーの実行部まで組み、並列・待ち合わせ・期限・出来事・rule・冪等キー・REST の状態・再起動・画面・拡張を動かす（題材は `tests/fixtures/runtime_demo`）。
- `tests/test_geas.py`・`tests/test_delete.py`：ホストの境界・型・部品・モジュールなど。

お店アプリ（`tests/fixtures/overseas_shop`）の `tests/test_shop_app.py`（以下「お店の通し」）は、機能ごとの確認ではなく、実アプリの通しの確認（E2E）です。

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
| 5. `rule`（上から順に最初に当てはまる行） | ◎ | `test_runtime.py` の「rule は上から順に…」。誤りは `test_rest.py` の「rule の誤りは止まる」 |
| 6. `effect` の `replies`・`joins`・`no_wait` | ◎ | `test_runtime.py` の「par の枝は両方動き、待ち合わせは…」「返事がそろうと行き先へ進み…」「拒否の返事は…」 |
| 6. `trigger` の `all`（待ち合わせ）・`deadline` | ◎ | `test_runtime.py` の「par の枝は両方動き…」「期限が来るまでは待ち…」。応答の中身（承認の与信）が行き先へ渡ることも同じ |
| 6. `trigger` の「どれか1つ」（`reply X → …`） | ◎ | `test_runtime.py` の「どれか1つの返事で進むトリガは…」（1つ届けば進む／届かなければ期限で進む） |
| 6. `effect` の `failures` | ○ | 検査のみ（`test_geas.py`）。実行時には動きを持たない宣言（✕ではなく対象外） |
| 7. `nexus` の次の処理・`reply`・`wait`・`par` | ◎ | `test_runtime.py`（受付のあと並列の枝が動く、結果の種類ごとに行き先が分かれる）。枝の重なりの誤りは `test_geas.py` の「par の枝が同じ項目に書くか…」 |
| 7. `event … on online`・`each` | ◎ | `test_runtime.py` の「つながりが戻る出来事は…」。`opened` は「アプリを開いた出来事も…」。外から渡す入力で動く形（`external → 処理`）は動かす確認が無い（✕） |
| 8. `query` の `filter`（equals／at_least／at_most／contains ignore_case）・`where`（or）・`order`・`page`・`select` | ◎ | `test_behavior.py` の「問い合わせは絞り込み…」、`test_geas.py` の「null になりうる項目の問い合わせ…」「page は並びのうえで先頭の1ページを返す」 |
| 8. `sum(…)`・`count(…)` の列 | ◎ | `test_geas.py` の「null になりうる項目の問い合わせ…」 |
| 8. `filter … within`（期間の絞り込み） | ✕ | **不具合**：必要な参照の名前が「現在」固定で、名前の規則（英小文字）に合わず、規則どおりには書けない。`test_behavior.py` に、直すまで xfail のテストを置いた |
| 9. `screen`（欄・行・操作・通知・遷移・design） | ◎ | `test_screens.py`、`test_runtime.py` の「計画書の画面の具体例から…」（実際の画面で通す。node と Playwright があるときだけ）。固定通貨の金額列は `test_screens.py` の「固定の通貨の金額の列は…」 |
| 9. ブラウザ拡張の `read`（url／element／row／lookup） | ◎ | `test_extgen.py`、`test_runtime.py` の「計画書の画面の具体例から…」（拡張の 3 通り：ページから読む・ページの表・読めない） |
| 9. 画面の行き先 `back`・`open`（行を選んで進む） | ◎ | `test_runtime.py` の「計画書の画面の具体例から…」（一覧で行を選んで詳細へ進み、取り下げて一覧へ戻る） |
| 10. `resource`（結果ごとの HTTP の状態・`key`・create の出来事） | ◎ | `test_rest.py`、`test_runtime.py`（状態の返り方と、境界の 400） |
| 10. `resource` の `update`・`destroy`・入れ子（`under`） | ◎ | `test_runtime.py` の「update・destroy・入れ子は…」と、計画書の URL の具体例から生成したテスト |
| サーバーの再起動で、待ち・時計の先・ID の採番が続く | ◎ | `test_runtime.py` の「再起動しても…」 |
| モジュール（`use`／`module`） | ◎ | `test_geas.py` の「モジュールを使ったコードがビルドでき動く」 |

## まだ無いもの（✕）

- `filter … within`（上の不具合を直してから、通常のテストにする）。
- `event` の `external → 処理`（外から渡す入力で動く形）。
- 同時に来た書き込み（サーバーへの並行リクエスト）の直列化。
- `now`・`tz_offset` を使う処理（時刻の計算は `test_geas.py` の時刻の項目で動かしている）。
