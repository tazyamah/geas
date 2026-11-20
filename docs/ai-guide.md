# AI 向けの入口：geas を入れて使うまで

このファイルは、AI（Claude Code など）が geas を使う作業を始めるときに、最初に読む入口です。人向けの説明は [README.md](../README.md) にあります。

geas は、会話で作る計画書（plan）と、AI が書く言語 `.geas` のコードを突き合わせて、言ったとおりに作れているかを機械で保証する道具です。**`.geas` を書くのは AI で、人は読みません。** 人が読むのは、コードから生成した仕様書（Markdown／HTML）です。

## 読む順番

| 知りたいこと | 読むもの |
|---|---|
| 言語の書き方（宣言・文・式の一覧） | [language-reference.md](language-reference.md) |
| 他の言語のノリで書き間違えやすいこと（`&&`・`else`・`string`・`;` など） | [language-reference.md の 12 節](language-reference.md#12-他の言語から来たときに間違えやすいこと) |
| 動く全体の例（海外にも売るお店：画面・REST・拡張・サーバーまで） | [tests/fixtures/overseas_shop/overseas_shop.geas](../tests/fixtures/overseas_shop/overseas_shop.geas) と同じフォルダの `overseas_shop_plan.md`（計画書）・`overseas_shop_spec.md`（生成した仕様書） |
| 小さな例 | [tests/fixtures/shop/shop.geas](../tests/fixtures/shop/shop.geas)、削除の例 `tests/fixtures/delete_demo/` |
| 計画書の作り方（会話で聞く手順） | [skills/plan-interview/SKILL.md](../skills/plan-interview/SKILL.md) |
| `.geas` を書いて検査し、人に仕様書を見せるまでの聞き方（問い・レビュアー目線の質問・変異検査の生き残り） | [skills/geas-develop/SKILL.md](../skills/geas-develop/SKILL.md) |
| 言語機能ごとの動作テスト（どの機能が実際に動かして確かめられているか） | [language-test-map.md](language-test-map.md) |
| geas の基本の考えと、仕様書とコードがずれない仕組み（どこまで保証しているか） | [why-spec-and-code-dont-drift.md](why-spec-and-code-dont-drift.md) |
| 名前の付け方 | [naming.md](naming.md)、[v0.1.0-scope.md](v0.1.0-scope.md) の「名前の書き方」 |
| 決まった設計の経緯（par・待ち合わせ・金額の単位・検査数字など） | [v0.1.0-scope.md](v0.1.0-scope.md) |
| モジュール（`use`／`module`） | [modules.md](modules.md) |
| 日時の考え方 | [datetime.md](datetime.md) |
| 文書の一覧 | [README.md](README.md) |
| 未決の事柄 | [open-questions.md](open-questions.md) |

迷ったら、**検査器（`geas check`）が正本**です。書き方の説明と検査器が食い違ったら、検査器の動きを信じて、説明のほうを直してください。

## 入れる

geas 本体（検査器・生成器・CLI）は Rust です（以前の Python 版は、git のタグ `python-final` に残してあります）。

| 道具 | 要るか | 用途 |
|---|---|---|
| Rust（cargo） | 必須 | `cargo install --path crates/geas-cli` で `geas` を入れる。`rust` で作ったコードの実行・具体例の実行・テスト（`cargo test`）にも使う |
| Python 3 と pytest | 生成した通しテスト（`tests/test_url_examples.py`・`tests/test_screen_examples.py`）を回すときだけ | geas 本体には要らない。生成したアプリのテストの書き方が pytest |
| node と Playwright | 画面・拡張の通しの確認をするとき（無ければその確認は飛ばされる） | 生成した `tests/test_screen_examples.py`（実行部は生成物の `tests/screen_runner.js`） |

```sh
git clone https://github.com/tazyamah/geas.git
cd geas
cargo install --path crates/geas-cli                          # geas を入れる
geas check tests/fixtures/overseas_shop/overseas_shop.geas   # 動作確認
cargo test --release                                         # テスト（コーパス 3,894 件の突き合わせと、具体例の通し）
cargo test --release -- --ignored                            # 生成したサーバーを組んで起動する確認（遅い。依存の crate を取る通信が要る）
```

インストールを避けるなら、`cargo run --release -p geas-cli -- check アプリ.geas` のように、リポジトリの直下から動かせます。

geas が読み書きするファイルと、標準出力・標準エラーは、OS によらず UTF-8 です（Windows でも、リダイレクトした仕様書や生成した Rust が cp932 になりません）。

## コマンド

```sh
geas check アプリ.geas [--json]               # 検査。エラー・警告・問いを、コードに書いた名前と行番号で出し、最後の行に件数。終了コードは、エラーがあれば 1、構文エラーとファイルが無いときは 2。--json は、指摘ごとに line と hint（見る節と直し方）を付けた JSON
geas spec アプリ.geas [--plan 計画書.md] [-o 出力.md]   # 仕様書（Markdown）を生成。--plan で、内部 URL ごとの確認の網羅も載る
geas env-checks アプリ.geas [--json]              # 動かす前提（アプリが実行環境に要求すること）を、ツールに依存しない検査項目の一覧にして出す。照合は env-verify
geas env-facts plan.json [--adapter aws-ecs] [--service アドレス]   # terraform show -json の出力から、環境の事実（クラウドに依存しない形）を作る。読めなかった事実は null
geas env-verify アプリ.geas (--plan plan.json | --facts 事実.json) [--strict] [--json]   # 動かす前提と環境の事実を照合。不合格があれば終了コード 1（--strict は「確かめられなかった」も 1）
geas env-probe [--db 保存先ファイル] [--egress ホスト:口,…]   # 動いている機械の上で環境の事実を読む（serverspec 的な収集器。Linux の /proc を読む）。開いている口、保存先のファイルが載るマウントの種類（消えるか）、宛先への TCP 到達。読めないものは null。出力は env-facts と同じ形なので、`> 事実.json` にして env-verify --facts で照合する
geas container アプリ.geas [-o 出力フォルダ]   # container・infra vm_docker の宣言から Dockerfile・compose.yaml・.dockerignore・secrets.env.example を生成する（出力フォルダは geas rust --host --runtime と同じ場所。-o が無ければ標準出力）
geas doctor アプリ.geas [--db 保存先ファイル] [--egress ホスト:口,…] [--secrets ファイル] [--strict] [--json]   # 動いている機械（ホスト・コンテナの中）で、動かす前提を一度に診断する。env-probe の事実の収集と env-verify の照合に、保存先の書き込みと空き・同じ DB を開いているプロセスの数・時刻同期・シークレットの有無（値は見ない。環境変数 GEAS_SECRET_外部_名前）を足し、`container`・`infra` を宣言していれば、保存先を宣言から決め、宣言した口の待ち受けと /api/health の応答、ホストの側ではデータフォルダの権限（番号 10001 が書けるか）・環境ファイルの権限（所有者だけが読める）と中のシークレットの有無・docker compose の有無も確かめる（コンテナの中とホストの上で、見える物が違う）。最後に「人の保証」（OS・Docker・前段・バックアップ・シークレットの値）の一覧を出す。不合格があれば終了コード 1
geas env-live アプリ.geas --url http://ホスト:口 [--facts 事実.json] [--strict] [--json]   # 動いているサーバーを外から確かめる（serverspec 的）。仕様書のルートから GET だけを作って送り、HTTP の状態が仕様どおりか見る（書き込みは送らない）。届かなければ「確かめられなかった」。--facts を足すと env-verify の照合も同じ表に並べる。https は curl で送る
geas review アプリ.geas [--plan 計画書.md] -o 出力.html  # 人が読む仕様書（スマホ幅の HTML）。計画書を渡すと突合の結果も載る
geas compare アプリ.geas 計画書.md [--no-run] [-o 結果.md]  # 計画書とコードの突合。具体例は、生成した Rust で実際に動かして確かめる
geas mutate アプリ.geas [--base 前のコード.geas | --base-git HEAD~1] [--run --plan 計画書.md] [-o 結果.md]  # 変異検査：約束を 1 つ崩した版を作り、検査器が止めるかを見る。生き残りは、検査器では守られていない約束（`--run --plan` を付けると、生き残りを計画書の具体例でも確かめ、食い違えば「具体例で検出」にする。Rust をビルドするので遅い）
geas parts アプリ.geas [--all] [--record]                # 部品（library）を、引数の範囲の端の値で実際に呼んで、宣言どおりかを確かめる（組めるか・panic・範囲・guarantee・2 回呼んで同じか）。食い違いがあれば終了コード 1。library を書いたら、trust の理由を書く前に通す。--record で、結果を台帳（アプリ.parts.json。コミットする）に書き、仕様書の「人の保証」に出す
geas receipt アプリ.geas --base-git HEAD~1 [--plan 計画書.md] [-o レシート.md]  # 変更ごとのレシート：前の版と比べた、信頼に関わる変化（保証が弱くなったもの・増えた人の保証・動きが変わるかもしれない処理・URL・画面・ユースケース・生き残り）。PR の本文に貼る
geas trace アプリ.geas サーバーのログ [--plan 計画書.md] [-o 結果.md]  # 運用のログ（生成サーバーの標準エラーに出る geas.route の行）を、仕様書のルート・処理・結果・ユースケースに結ぶ。結べない行があれば終了コード 1
geas bench ケース集/ 結果の置き場/ [--no-run] [--baseline 前回.json] [--json 今回.json]  # AI が計画書から書いた .geas の採点（検査・計画書との食い違い・具体例・直した回数）。AI は呼ばない。PR の直前・ビルドの直前に 1 回
python proofs/run.py [--timeout 秒]                          # 生成物の前置きの証明（Kani が要る。通常のテストには入らない）
python proofs/std_proofs.py [--timeout 秒] [--jobs 数] [--record]   # 標準部品の証明（Kani が要る）。全部の入力で、panic しないことと、式どおりであることを示す。--record で同梱の台帳に書く
geas rust アプリ.geas -o 出力先 [--plan 計画書.md] [--host] [--runtime] [--standalone] [--seed 最初のデータ.json] [--server URL]
geas interview init|status|render 台帳.json     # 初回の対話の台帳（計画書づくりの補助）
```

`rust` の付けものは次のとおりです。

- 何も付けない：Rust の crate（検査済みの処理の本体）だけ。
- `--host`：画面（PWA）・開発用の模擬パネル（サーバーを `--test` で起こしたときだけ配る）・窓口 `host.rs`・定義 `DEFS_JSON`、そして `page` を持つ screen があればブラウザ拡張（`ext/`）も作る。AI から使う口として、MCP のツール定義（`mcp/tools.json`、URL の定義から機械的に作る）と、サーバーへ中継する汎用の橋渡し `mcp/bridge.py`・登録例 `mcp/mcp.json` も作る。
- `--host --runtime`：サーバーの実行部（`server/`、SQLite と HTTP）も置く。`cd 出力先/server && cargo build --release` でビルドできる。動かし方・引数・公開するときの前置き（nginx など）は [docs/server.md](server.md)。`--standalone` は、窓つき exe（`window/`）も置く（画面と処理を 1 つの実行ファイルにし、通信路を使わない。[docs/standalone.md](standalone.md)）。`--seed` は、DB が空のときだけ入れる最初のデータ（`{"実体名": {"キー": {行}}}` の JSON）で、生成物に埋め込む。行は、入れる前に型の範囲で確かめる（範囲の外の行があると、入れずに止まる）。
- `--plan`：計画書の具体例を実行する bin も作る。`--host --runtime` と一緒なら、計画書の「URL の具体例」（下）から HTTP の通しテスト `tests/test_url_examples.py` 、計画書の「画面の具体例」（下）から画面の通しテスト `tests/test_screen_examples.py` も作る（サーバーをビルドしてから `python -m pytest 出力先/tests`）。

検査にエラーか「問い」が残っていると、`rust` と `compare` は先へ進みません。

## 作業の流れ（AI の動き方）

2〜4 の聞き方（問いを1問ずつ、仕様書をレビュアー目線で先に読む、変異検査の生き残りを見せる）は [skills/geas-develop](../skills/geas-develop/SKILL.md) にまとめてある。

1. **計画書を作る**（`skills/plan-interview`）。ユースケースは一問一答で聞き、コードを書く前に動作要件を聞く。計画書は Markdown。
2. **`.geas` を書く**。[language-reference.md](language-reference.md) と、近い例（お店）を見ながら。名前は半角英数、日本語の表示名は直前の `///` に書く。
3. **検査する**（`check`）。エラーは直す（指摘の行番号と、`近い名前`・直し方の案内を見る）。**問い（QUESTION）は、人に聞くべきことなので、勝手に決めずに質問する**（検査器が「決めていない」と見つけたもの）。警告も読む（表示名の抜けなど）。
4. **仕様書を作り、計画書と突き合わせる**（`spec`／`review`／`compare`）。ずれがあれば、計画書とコードのどちらが正しいかを人に確かめる。
5. **アプリにする**（`rust --host --runtime`）。手で書くコードを増やさないのが原則：計画書・`.geas` に現れない手書きコードは、無くす方向で考える。

## 計画書の「URL の具体例」

テストは手で書かず、計画書の表に書きます。見出しの列が `メソッド`・`経路`・`期待する結果`・`期待する状態` を含む表が、URL の具体例です（`入力`・`期待する本文`・`控える` は任意）。

| ID | ◆メソッド | ◆経路 | ◆入力 | ◆期待する結果 | ◆期待する状態 | 控える |
|---|---|---|---|---|---|---|
| UX6 | POST | /orders | `{"customer_id":"c-001",…,"slots":{"currency":"JPY"}}` | 受付 | 201 | order_id = body.order_id |
| UX16 | POST | /order_completions | `{"order_id":"{order_id}"}` | 在庫が足りなかった | 422 | |

- 行は上から順に、最初のデータ（`--seed`）入りの新しいサーバーへ実行します。入力は実際に送る JSON、`期待する本文` は応答に含まれるべき JSON（一部でよい）です。
- 期待する結果は、表示名でもコードの名前でもよく、結果の種類を持たない応答（一覧の 200、境界の検査の 400）は `－` にします。
- `控える` は `名前 = 応答の道すじ`（`body.order_id`、`rows.0.order_id`）で、後の行の経路と入力に `{名前}` で差し込めます。
- `compare`／`review` は、メソッドと経路がルートにあるか、結果と状態が `resource` の宣言どおりかを突き合わせ、「内部 URL ごとの確認の網羅」にも数えます。

## 計画書の「画面の具体例」

画面（PWA）と Chrome 拡張の通しテストも手で書きません。計画書の「ユースケースの手順」（画面・操作・結果）の各行に、`画面の具体例` の表で値を足します。見出しの列が `ユースケース`・`手順`・`入れる・選ぶ`・`見えること` の表です（行は `ユースケース` と `手順` で手順の表に結びつきます）。

| ユースケース | 手順 | 入れる・選ぶ | 見えること |
|---|---|---|---|
| 商品を見つけて注文する | 3 | 明細.knife-jp = 1; 顧客ID = c-001; 配送先.国 = JP | 通知: 受付 |
| 商品を見つけて注文する | 4 |  | 行 * の 状態: 完了; 含む: 10,928 |

- 実行：ユースケースごとに上から順に、最初のデータ入りの新しいサーバーへ、実際のブラウザで操作します。前のユースケースの結果は残り、次のユースケースは、前のユースケースが出した外部作用の返事（模擬）が届き終わってから始めます。「見る」の手順1はその画面の URL を開き、行を選ぶ・操作は画面の部品（`data-screen`・`data-row`・`data-field`・`data-action` など、`geas/ui` の約束）を押して入れます。操作の結果の種類は、画面の文言でなく、呼んだ URL の応答の `result` で確かめます。行き先の画面は遷移の宣言どおりかを確かめます。
- 入れる・選ぶ：`行 = ID`（行を選ぶ）／`入力欄 = 値`／`配送先.国 = JP`（合成の部品）／`明細.ID = 個数`／`通貨 = JPY`（slot）／`返事を落とす = 応答の名前`（外部作用の返事を届かないままにする）／`時計を進める = 30分`（模擬の時計を進める。30分は待たない）／`差し替え 入力欄 = 値`（画面からは作れない値を、送る直前に差し替えて、結果の扱いを確かめる。update／destroy の ID も差し替えられる）／拡張の画面は `ページ = 経路` と `ページの表 = 見出し: 値`（本物の Amazon には行かず、このページを差し替えて返す）。
- 見えること：`含む`／`含まない`／`通知`（画面の通知欄）／`行数`／`行`／`行なし`／`行 ID の 列: 値`（ID が `*` ならどれかの行）／`選択 slot: 値`／`表示なし`（拡張が何も出さない）。名前は表示名でもコードの名前でもよい。
- **待つ長さは書きません**。テストの最初に、サーバーの応答時間・返事までの秒数（`/api/test/state` の `delay_secs`）・画面の問い合わせの引き直しの間隔を実測し、待ちの上限を決めます。返事が届く・処理する・画面が更新される、の各部分は、HTTP の具体例や Rust 側のテストが別に確かめます。
- 計画書の「画面デザイン」の表は、突合はしませんが、「見る」の手順で、配置・色・明るさが画面に効いているかを確かめます。オフラインで画面が開くこと、開発用の操作パネルが定義どおり動くこと、変わっていない一覧を引き直しで描き直さないこと（サーバーが `304` を返す）は、計画書によらない汎用の確認として毎回行います。
- ダイアログ（alert／confirm）が出たら失敗にします。画面のエラー（pageerror）も失敗です。

## 守ること

- 人は `.geas` を読みません。構文の好みを人に聞かず、約束（定義・保証・意味）が変わる問いだけを聞く。
- 名前は半角英数（先頭は英字、あとは英数字と `_`）。型・entity・状態・結果・トリガ・フロー・出来事は UpperCamel、項目・変数・処理（deed）・問い合わせ・入力・部品は lower_snake。
- 仕様書に出る名前には、直前に `///` で日本語の表示名を付ける（無いと警告）。
- REST の URL（`resource`）は、考え方を [rest.md](rest.md) にまとめてある：`index`／`show`／`create`／`update`／`destroy` だけ、PATCH なし、POST は作成だけ、クエリ文字列は絞り込みの条件の `q[名前]=値` だけ、結果の種類ごとに HTTP の状態を必ず決める。流れの途中の処理（nexus・trigger の行き先）の URL は「内部」で、サーバーは `--test` のときだけ答える（画面の操作にもできない）。呼べる人を分けるなら、`actor 役割` を宣言し、外から呼べる動作すべてに `for 役割, …`（誰でもよい口は `for anyone`）を書く。本人の確かめは前に置くプロキシの仕事で、サーバーはヘッダ `X-Geas-Actor` と比べる（無ければ 401、合わなければ 403）。本人の分だけを返す問い合わせは、`scope 範囲 on データ { filter 項目 equals caller }` を宣言して `from データ.範囲` と書く（`caller` は入力にしない。URL にも本人の ID を入れない）。範囲は行の集合で、`where` も、引数（`scope 名前(引数: 型)`、使う側は `データ.範囲(入力)`）も書け、`from データ.範囲1.範囲2` と並べると、すべてに入る行だけになる。
- 外部作用（`request`）には冪等キーが必須。
- 変更は小さな PR に分ける。PR は重ねず、マージされてから次へ。
- 生成物（`tests/fixtures/*/app/generated/` など）はコミットしない（`.gitignore` 済み）。仕様書（`*_spec.md`、`*_review.html`）はテストの見本として固定してあるので、`.geas` を変えたら再生成する：

```sh
F=tests/fixtures/overseas_shop
geas spec $F/overseas_shop.geas --plan $F/overseas_shop_plan.md > $F/overseas_shop_spec.md
geas review $F/overseas_shop.geas --plan $F/overseas_shop_plan.md -o $F/overseas_shop_review.html
```

変異検査の結果（`overseas_shop_mutation.md`）も見本として固定してあり、`.geas` を変えて崩し方や生き残りが変わったら、テストが落ちる。直したら再生成する：

```sh
geas mutate $F/overseas_shop.geas -o $F/overseas_shop_mutation.md
```

仕様書は[ユースケース][動作][仕様]の3タブ。ユースケース図の「人」は、計画書のユースケースの表に任意の「アクター」列を足すと、その名前で描く（無いときは、ユースケースを始める画面で代用）。

仕様書には、答えてほしいことと件数、前の版からの変更は載せない（別の HTML）。更新のレビューでは `--diff-o 変更点.html` を足し、前の版を `--base 前のコード.geas`、または git の版なら `--base-git HEAD~1` で渡す。変更点の HTML には、変更の前に「信頼のレシート」の節が出る（`geas receipt` と同じ内容）。PR を出すときは、`geas receipt` の出力を本文に貼る。

## ソースの地図（`crates/`）

本体は Cargo の workspace で、`crates/geas-core`（ライブラリ）と `crates/geas-cli`（コマンド `geas`）の 2 つです。外部の crate は、正規表現（`regex`）・JSON（`serde_json`）・多倍長の整数（`num-*`）・資産の埋め込み（`include_dir`）・PNG の圧縮（`flate2`）に使っています。

| ファイル（`crates/geas-core/src/`） | 役目 |
|---|---|
| `lexer.rs`・`parser.rs`・`ast.rs` | 字句・構文・構文木。宣言の入口は `parser.rs` の `parse` |
| `check/` | 検査器（型・範囲・書き込みの重なり・遷移・REST・画面・拡張・rule など）。入口は `check/defs.rs` の `check` |
| `spec.rs`・`review.rs`・`svg.rs`・`plan.rs` | 仕様書（Markdown／HTML）の生成（図は `svg.rs` の自前 SVG。見た目・構造は specgap のレビュー画面に合わせる）、計画書の読み取りと突合、確認の網羅 |
| `rustgen.rs`・`hostgen.rs` | 処理の Rust 生成、窓口と定義（`DEFS_JSON`） |
| `rest.rs`・`screengen.rs` | REST の URL と状態、画面の定義 |
| `mcp.rs`・`assets/mcp_bridge.py` | MCP のツール定義（URL の定義から）と、標準入出力の橋渡し（定義を読むだけの汎用。生成物に置かれる Python の小さなスクリプト） |
| `crategen.rs` | 生成物の木（crate・サーバー実行部・窓つき exe・拡張・開発用の模擬パネル・MCP）を書き出す。部品は `assets/`（`ext`・`dev`・`server`・`window`・`ui`・`e2e`・`std`）に置き、`include_dir` で埋め込む |
| `testgen.rs`・`examples.rs` | 計画書の「URL の具体例」から HTTP の通しテスト（pytest）を、「画面の具体例」から画面・拡張の通しテスト（pytest＋Playwright）を生成。「具体例」の表から実行用の bin を作って動かし、計画と比べる |
| `assets/e2e/screen_runner.js` | 画面の通しテストの汎用の実行部（生成物の `tests/` に置かれる）。アプリを知らない |
| `receipt.rs` | 変更ごとのレシート（`geas receipt`、`review --diff-o` の「信頼のレシート」の節）。前の版との、保証の強さの変化・影響範囲・変異検査の生き残り |
| `hints.rs` | `check --json` の hint（指摘の種類ごとの、見る節と直し方）。検査器の指摘の種類を足したら、ここにも足す |
| `trace.rs` | `geas trace`。サーバーの構造化ログを仕様書の行に結ぶ |
| `bench.rs` | `geas bench`。AI が書いた `.geas` の採点 |
| `partcheck.rs`・`partrun.rs` | 部品の台帳と、境界値の検査（`geas parts`）。標準部品の手本（Rust の整数のメソッドを桁の限りの無い整数で書き直したもの）も `partrun.rs` |
| `mutate.rs` | 変異検査（`geas mutate`）。普段は走らせず、レビューの前に。`--base`／`--base-git` で、変わった行だけを崩す |
| `envreq.rs`・`envcheck.rs`・`adapter.rs` | 動かす前提の一覧、環境の事実との照合、terraform の plan（aws-ecs）から事実を作るアダプタ |
| `interview.rs` | 初回の対話の台帳 |
| `modules.rs`・`names.rs` | `use`／`module`、名前の規則と表示名の検査 |
| `crates/geas-cli/src/main.rs` | コマンド。`crates/geas-cli/tests/` に、コーパスの突き合わせ（`golden.rs`）と通しの確認（`e2e.rs`） |

出力の一致を見張るコーパス（`tests/corpus.tar.xz`）の期待値は `tests/golden/hashes.json`（Rust 化の前の Python 版の出力から作った指紋）です。意図して出力を変えたら、期待値も作り直します（`cargo test` が、食い違った入力と項目を出します）。
