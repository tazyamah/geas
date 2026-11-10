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
| 4. 書いた後に同じ項目を読む（書いた値が見える）・1件をまるごと比べる `x == y` | ◎ | `test_behavior.py` の「書いた後に同じ項目を読むと、書いた値が見える」「書いた後に1件をまるごと比べると、書いた値で比べる」（create で作った1件との比較も） |
| 4. 文の `if` の `else`・`else if`（通った道の書き込みと結果だけが出る） | ◎ | `test_behavior.py` の「if と else if と else は、通った道の書き込みと結果だけを出す」。検査（else の中で条件が成り立たないことを使う、同じ道で 2 回書くと止まる、両方 return した後ろには届かない）は `test_geas.py` の else の節 |
| 1. 文字列の `\n`・`\"`・`\\` と、行末の `\`（次の行へ続ける） | ◎ | `test_geas.py` の「拒否の理由に引用符と改行と、バックスラッシュを書いても、そのまま返る」（生成した Rust を動かす）と「文字列は行末のバックスラッシュで続け…仕様書の表を崩さない」 |
| 4. 負の数と符号の反転（`-5`・`-x`）・`min`・`max`・`abs`・結果の中身の数・`x in 集まり`・`len(x)` | ◎ | `test_numbers.py` の「負の数と min max abs は 生成した Rust でも式どおりに動く」「in と len は 生成した Rust でも動く」。範囲の証明は同じファイルの検査の試験（`or`・`not`・`and` をまたぐ読み取り、条件の式の範囲、`let` の付け直しの禁止、結果の中身の範囲） |
| 4. `library` の宣言が実際の動きと合うこと（部品の境界値の検査 `geas parts`） | ◎ | `test_parts.py`：標準部品 248 個の全部を端の値で呼び、panic・範囲・手本（`geas/stdref.py`）との一致を確かめる。わざと違えた宣言（あふれ・範囲外・保証違反・組めない・0 で割る）を、種類ごとに見つける |
| 4. 金額の丸め `round(…, floor｜ceil｜half_up)`・割合×金額 | ◎ | `test_behavior.py` の「金額の丸めは指定の方向になり…」、`test_geas.py` の「組み込みの丸めは単位より近い値を返す」 |
| 4. 割り算と余り・`convert`・`library` の保証 | ◎ | `test_geas.py` の「割り算と余りは生成コードで動く」「convert は生成コードの中で計算され…」「ライブラリの戻り値は呼ぶたびに確かめ…」「サーバーとして組んでも、ライブラリの計算の溢れは panic として止まる」「ライブラリは版を固定した依存として…」 |
| 4. `request`（冪等キー・`for` の中の組） | ◎ | `test_behavior.py` の「…外部作用の冪等キーは回す値ごとに決まる」、お店の通し（同じ冪等キーで引き直す） |
| 5. `rule`（上から順に最初に当てはまる行） | ◎ | `test_runtime.py` の「rule は上から順に…」。誤りは `test_rest.py` の「rule の誤りは止まる」 |
| 5. 同時リクエストの直列化（全体の Mutex と 1 リクエスト 1 トランザクション）・汚れた Mutex | ◎ | `test_runtime.py` の「同時に来たリクエストは直列に並べた場合と同じ結果になる」「サーバーの単体テストが通る…」 |
| 6. `effect` の `replies`・`joins`・`no_wait` | ◎ | `test_runtime.py` の「par の枝は両方動き、待ち合わせは…」「返事がそろうと行き先へ進み…」「拒否の返事は…」 |
| 6. `trigger` の `all`（待ち合わせ）・`deadline` | ◎ | `test_runtime.py` の「par の枝は両方動き…」「期限が来るまでは待ち…」。応答の中身（承認の与信）が行き先へ渡ることも同じ |
| 6. `trigger` の「どれか1つ」（`reply X → …`） | ◎ | `test_runtime.py` の「どれか1つの返事で進むトリガは…」（1つ届けば進む／届かなければ期限で進む） |
| 6. `effect` の `failures` | ○ | 検査のみ（`test_geas.py`）。実行時には動きを持たない宣言（✕ではなく対象外） |
| 7. `nexus` の次の処理・`reply`・`wait`・`par` | ◎ | `test_runtime.py`（受付のあと並列の枝が動く、結果の種類ごとに行き先が分かれる）。枝の重なりの誤りは `test_geas.py` の「par の枝が同じ項目に書くか…」 |
| 7. `event … on online`・`each` | ◎ | `test_runtime.py` の「つながりが戻る出来事は…」。`opened` は「アプリを開いた出来事も…」。外から渡す入力で動く形（`external → 処理`）は動かす確認が無い（✕） |
| 8. `query` の `filter`（equals／at_least／at_most／contains ignore_case）・`where`（or）・`order`・`page`・`select` | ◎ | `test_behavior.py` の「問い合わせは絞り込み…」、`test_geas.py` の「null になりうる項目の問い合わせ…」「page は並びのうえで先頭の1ページを返す」 |
| 8. `sum(…)`・`count(…)` の列 | ◎ | `test_geas.py` の「null になりうる項目の問い合わせ…」 |
| 8. `filter … within`（期間の絞り込み） | ◎ | `test_behavior.py` の「within は current_time から1時間以内のものだけ返す」（時刻の slot `current_time` と比べる） |
| 9. `screen`（欄・行・操作・通知・遷移・design） | ◎ | `test_screens.py`、`test_runtime.py` の「計画書の画面の具体例から…」（実際の画面で通す。node と Playwright があるときだけ）。固定通貨の金額列は `test_screens.py` の「固定の通貨の金額の列は…」 |
| 9. ブラウザ拡張の `read`（url／element／row／lookup） | ◎ | `test_extgen.py`、`test_runtime.py` の「計画書の画面の具体例から…」（拡張の 3 通り：ページから読む・ページの表・読めない） |
| 9. 画面の行き先 `back`・`open`（行を選んで進む） | ◎ | `test_runtime.py` の「計画書の画面の具体例から…」（一覧で行を選んで詳細へ進み、取り下げて一覧へ戻る） |
| 10. `resource`（結果ごとの HTTP の状態・`key`・create の出来事） | ◎ | `test_rest.py`、`test_runtime.py`（状態の返り方と、境界の 400） |
| 10. `resource` の `update`・`destroy`・入れ子（`under`） | ◎ | `test_runtime.py` の「update・destroy・入れ子は…」と、計画書の URL の具体例から生成したテスト |
| 10. 流れの途中の処理の URL（「内部」） | ◎ | `test_rest.py` の「流れの途中の処理の URL は内部として…」「画面の操作に流れの途中の処理は使えない」「計画書のルートの表に外からの列があれば…」「…レシートの弱くなったものに出る」、`test_runtime.py` の「流れの途中の処理の URL は --test なしでは答えず…」、`test_screens.py` の MCP のツール定義（内部の URL はツールにしない） |
| 12. 生成物の守り（CSP・名前のエスケープ） | ◎ | `test_runtime.py` の「画面と API の応答には CSP が付き…」（応答ヘッダ、生成した HTML にインラインのスクリプト・イベントハンドラ属性・style 属性が無いこと、拡張の manifest の CSP）。画面の通しは CSP 違反を誤りとして数える |
| 12. 生成物の守り（生成コードへの差し込み） | ◎ | `test_security.py`：表示名に生の文字列の終わり（`"####`）を書いても `DEFS_JSON` は 1 つの文字列のままで、差し込んだ関数はビルドしたアプリに無い。計画書の具体例の値・ID は生成した Rust の文字列から出ず、実行してもコードとして動かない。`--base-git` の前の版は `-` で始まると git を呼ばない |
| 12. サーバーの口の開け方（`--test`・CSRF・Host） | ◎ | `test_runtime.py` の「テスト用の口と宣言を通らない口は…」「--test を付けると…」「POST は JSON と名乗るものだけを受け…」「Host が手元の名前でも…」、`http.rs` の単体テスト（パスの `+`・`%` の読み戻し、Host の名前） |
| 6. 期限・返事の処理の切り離し（1 件の失敗でほかを止めない） | ◎ | `test_runtime.py` の「期限の待ちが1件失敗し続けても…」 |
| 11. MCP（AI から使う口。ツール定義・REST への中継） | ◎／○ | `test_screens.py` の「MCP のツール定義は URL の定義から機械的に作られ…」「MCP の橋渡しはツールの呼び出しを REST に中継する」（中継先は模擬のサーバー）。実サーバーに中継して動かす確認は無い（✕） |
| 6. 外部作用の実通信（`over http`：宣言した先だけ・認証・状態の結び方・リダイレクト不従・シークレット無しは送らない） | ◎ | `test_http_effect.py`（手元の模擬の X に向けて、つぶやきの見本を動かす）、`test_http_check.py`（検査） |
| 6. 外部作用のファイル操作（`over file`：write・overwrite・read・delete。宣言した場所・ファイルだけ・すでにあれば書かない・無い場所は失敗として結ぶ・OS の書式でない場所は入力不正） | ○ | `test_file_effect.py`（実際のフォルダに、ノートの見本を動かす。write・既存・無い場所・不正な場所。overwrite・delete・read は fileio の単体試験で、サーバーを通す試験は write だけ）、`test_file_check.py`（検査）、`test_user_path.py`（実行時入力のパス） |
| 窓つき exe（画面から処理への呼び出しは IPC、IP のソケットを持たない） | ◎ | `test_window.py`（xvfb と WebKitGTK が無い環境ではスキップ） |
| 常駐（`resident`：検査・仕様書、トレイのアイコン作成、重複起動の断り） | ◎／○ | `test_resident_check.py`、`test_window.py`。実トレイのクリック・隠れる動きは未確認（○） |
| サーバーの再起動で、待ち・時計の先・ID の採番が続く | ◎ | `test_runtime.py` の「再起動しても…」 |
| サーバーが保存をメモリに持つこと（別のサーバー・DB の直接の書き換えが次のリクエストで見える、巻き戻しでメモリの保存も捨てる）・1KB を超える応答が待たずに返ること | ◎ | `test_runtime.py` の「保存はメモリに持つが…」「1KBを超える応答も…」、サーバーの単体テスト（`db.rs` の data_version、`engine.rs` の巻き戻し） |
| モジュール（`use`／`module`） | ◎ | `test_geas.py` の「モジュールを使ったコードがビルドでき動く」 |

## まだ無いもの（✕）

- `event` の `external → 処理`（外から渡す入力で動く形）。
- `now`・`tz_offset` を使う処理（時刻の計算は `test_geas.py` の時刻の項目で動かしている）。
