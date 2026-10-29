# AI 向けの入口：geas を入れて使うまで

このファイルは、AI（Claude Code など）が geas を使う作業を始めるときに、最初に読む入口です。人向けの説明は [README.md](../README.md) にあります。

geas は、会話で作る計画書（plan）と、AI が書く言語 `.geas` のコードを突き合わせて、言ったとおりに作れているかを機械で保証する道具です。**`.geas` を書くのは AI で、人は読みません。** 人が読むのは、コードから生成した仕様書（Markdown／HTML）です。

## 読む順番

| 知りたいこと | 読むもの |
|---|---|
| 言語の書き方（宣言・文・式の一覧） | [language-reference.md](language-reference.md) |
| 動く全体の例（海外にも売るお店：画面・REST・拡張・サーバーまで） | [tests/fixtures/overseas_shop/overseas_shop.geas](../tests/fixtures/overseas_shop/overseas_shop.geas) と同じフォルダの `overseas_shop_plan.md`（計画書）・`overseas_shop_spec.md`（生成した仕様書） |
| 小さな例 | [tests/fixtures/shop/shop.geas](../tests/fixtures/shop/shop.geas)、削除の例 `tests/fixtures/delete_demo/` |
| 計画書の作り方（会話で聞く手順） | [skills/plan-interview/SKILL.md](../skills/plan-interview/SKILL.md) |
| 言語機能ごとの動作テスト（どの機能が実際に動かして確かめられているか） | [language-test-map.md](language-test-map.md) |
| 名前の付け方 | [naming.md](naming.md)、[v0.1.0-scope.md](v0.1.0-scope.md) の「名前の書き方」 |
| 決まった設計の経緯（par・待ち合わせ・金額の単位・検査数字など） | [v0.1.0-scope.md](v0.1.0-scope.md) |
| モジュール（`use`／`module`） | [modules.md](modules.md) |
| 日時の考え方 | [datetime.md](datetime.md) |
| 未決の事柄 | [open-questions.md](open-questions.md) |

迷ったら、**検査器（`python -m geas check`）が正本**です。書き方の説明と検査器が食い違ったら、検査器の動きを信じて、説明のほうを直してください。

## 入れる

geas は、いまは Python の試作版です（最終的には Rust にする予定）。

| 道具 | 要るか | 用途 |
|---|---|---|
| Python 3.10 以上 | 必須 | `python -m geas …`。外部のライブラリは要りません |
| pytest | テストを回すとき | `python -m pytest tests` |
| Rust（cargo） | `rust` で作ったコードを動かす・具体例を実行するとき | 生成した crate のビルド |
| node と Playwright | 画面・拡張の通しの確認をするとき（無ければその確認は飛ばされる） | 生成した `tests/test_screen_examples.py`（実行部は `geas/e2e/screen_runner.js`） |

```sh
git clone https://github.com/tazyamah/geas.git
cd geas
python -m geas check tests/fixtures/overseas_shop/overseas_shop.geas   # 動作確認
python -m pytest tests                                                # テスト（数分かかる。cargo がある環境では、お店アプリのビルドと通しも含む）
```

インストール作業は要りません（リポジトリの直下で `python -m geas` と打つだけです）。

## コマンド

```sh
python -m geas check アプリ.geas                       # 検査。エラー・警告・問いを出す。終了コードは、エラーがあれば 1
python -m geas spec アプリ.geas [--plan 計画書.md] [-o 出力.md]   # 仕様書（Markdown）を生成。--plan で、内部 URL ごとの確認の網羅も載る
python -m geas review アプリ.geas [--plan 計画書.md] -o 出力.html  # 人が読む仕様書（スマホ幅の HTML）。計画書を渡すと突合の結果も載る
python -m geas compare アプリ.geas 計画書.md [--no-run] [-o 結果.md]  # 計画書とコードの突合。具体例は、生成した Rust で実際に動かして確かめる
python -m geas rust アプリ.geas -o 出力先 [--plan 計画書.md] [--host] [--runtime] [--seed 最初のデータ.json] [--server URL]
python -m geas interview init|status|render 台帳.json     # 初回の対話の台帳（計画書づくりの補助）
```

`rust` の付けものは次のとおりです。

- 何も付けない：Rust の crate（検査済みの処理の本体）だけ。
- `--host`：画面（PWA）・開発用の模擬パネル・窓口 `host.rs`・定義 `DEFS_JSON`、そして `page` を持つ screen があればブラウザ拡張（`ext/`）も作る。
- `--host --runtime`：サーバーの実行部（`server/`、SQLite と HTTP）も置く。`cd 出力先/server && cargo build --release` でビルドできる。`--seed` は、DB が空のときだけ入れる最初のデータ（`{"実体名": {"キー": {行}}}` の JSON）で、生成物に埋め込む。
- `--plan`：計画書の具体例を実行する bin も作る。`--host --runtime` と一緒なら、計画書の「URL の具体例」（下）から HTTP の通しテスト `tests/test_url_examples.py` 、計画書の「画面の具体例」（下）から画面の通しテスト `tests/test_screen_examples.py` も作る（サーバーをビルドしてから `python -m pytest 出力先/tests`）。

検査にエラーか「問い」が残っていると、`rust` と `compare` は先へ進みません。

## 作業の流れ（AI の動き方）

1. **計画書を作る**（`skills/plan-interview`）。ユースケースは一問一答で聞き、コードを書く前に動作要件を聞く。計画書は Markdown。
2. **`.geas` を書く**。[language-reference.md](language-reference.md) と、近い例（お店）を見ながら。名前は半角英数、日本語の表示名は直前の `///` に書く。
3. **検査する**（`check`）。エラーは直す。**問い（QUESTION）は、人に聞くべきことなので、勝手に決めずに質問する**（検査器が「決めていない」と見つけたもの）。警告も読む（表示名の抜けなど）。
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

- 実行：ユースケースごとに上から順に、最初のデータ入りの新しいサーバーへ、実際のブラウザで操作します。「見る」の手順1はその画面の URL を開き、行を選ぶ・操作は画面の部品（`data-screen`・`data-row`・`data-field`・`data-action` など、`geas/ui` の約束）を押して入れます。操作の結果の種類は、画面の文言でなく、呼んだ URL の応答の `result` で確かめます。行き先の画面は遷移の宣言どおりかを確かめます。
- 入れる・選ぶ：`行 = ID`（行を選ぶ）／`入力欄 = 値`／`配送先.国 = JP`（合成の部品）／`明細.ID = 個数`／`通貨 = JPY`（slot）／`返事を落とす = 応答の名前`（外部作用の返事を届かないままにする）／`時計を進める = 30分`（模擬の時計を進める。30分は待たない）／`差し替え 入力欄 = 値`（画面からは作れない値を、送る直前に差し替えて、結果の扱いを確かめる。update／destroy の ID も差し替えられる）／拡張の画面は `ページ = 経路` と `ページの表 = 見出し: 値`（本物の Amazon には行かず、このページを差し替えて返す）。
- 見えること：`含む`／`含まない`／`通知`（画面の通知欄）／`行数`／`行`／`行なし`／`行 ID の 列: 値`（ID が `*` ならどれかの行）／`選択 slot: 値`／`表示なし`（拡張が何も出さない）。名前は表示名でもコードの名前でもよい。
- **待つ長さは書きません**。テストの最初に、サーバーの応答時間・返事までの秒数（`/api/test/state` の `delay_secs`）・画面の問い合わせの引き直しの間隔を実測し、待ちの上限を決めます。返事が届く・処理する・画面が更新される、の各部分は、HTTP の具体例や Rust 側のテストが別に確かめます。
- 計画書の「画面デザイン」の表は、突合はしませんが、「見る」の手順で、配置・色・明るさが画面に効いているかを確かめます。オフラインで画面が開くことと、開発用の操作パネルが定義どおり動くことは、計画書によらない汎用の確認として毎回行います。
- ダイアログ（alert／confirm）が出たら失敗にします。画面のエラー（pageerror）も失敗です。

## 守ること

- 人は `.geas` を読みません。構文の好みを人に聞かず、約束（定義・保証・意味）が変わる問いだけを聞く。
- 名前は半角英数（先頭は英字、あとは英数字と `_`）。型・entity・状態・結果・トリガ・フロー・出来事は UpperCamel、項目・変数・処理（deed）・問い合わせ・入力・部品は lower_snake。
- 仕様書に出る名前には、直前に `///` で日本語の表示名を付ける（無いと警告）。
- REST の URL（`resource`）は Rails の流儀：`index`／`show`／`create`／`update`／`destroy` だけ、PATCH なし、クエリ文字列なし、結果の種類ごとに HTTP の状態を必ず決める。
- 外部作用（`request`）には冪等キーが必須。
- 変更は小さな PR に分ける。PR は重ねず、マージされてから次へ。
- 生成物（`tests/fixtures/*/app/generated/` など）はコミットしない（`.gitignore` 済み）。仕様書（`*_spec.md`、`*_review.html`）はテストの見本として固定してあるので、`.geas` を変えたら再生成する：

```sh
F=tests/fixtures/overseas_shop
python -m geas spec $F/overseas_shop.geas --plan $F/overseas_shop_plan.md > $F/overseas_shop_spec.md
python -m geas review $F/overseas_shop.geas --plan $F/overseas_shop_plan.md -o $F/overseas_shop_review.html
```

## ソースの地図（`geas/`）

| ファイル | 役目 |
|---|---|
| `lexer.py`・`parser.py`・`ast.py` | 字句・構文・構文木。宣言の入口は `Parser.program` |
| `check.py` | 検査器（型・範囲・書き込みの重なり・遷移・REST・画面・拡張・rule など） |
| `spec.py`・`review.py`・`plan.py` | 仕様書（Markdown／HTML）の生成、計画書の読み取りと突合、確認の網羅 |
| `rustgen.py`・`hostgen.py` | 処理の Rust 生成、窓口と定義（`DEFS_JSON`） |
| `rest.py`・`screengen.py` | REST の URL と状態、画面の定義 |
| `extgen.py`・`devgen.py`・`servergen.py` | ブラウザ拡張、開発用の模擬パネル、サーバー実行部の生成。部品は `geas/ext`・`geas/dev`・`geas/server`・`geas/ui` |
| `testgen.py` | 計画書の「URL の具体例」から HTTP の通しテスト（pytest）を、「画面の具体例」から画面・拡張の通しテスト（pytest＋Playwright）を生成 |
| `e2e/screen_runner.js` | 画面の通しテストの汎用の実行部（生成物の `tests/` に置かれる）。アプリを知らない |
| `modules.py` | `use`／`module` |
| `names.py` | 名前の規則と表示名の検査 |
| `cli.py` | コマンド |
