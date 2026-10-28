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
| node と Playwright | 画面・拡張の通しの確認をするとき（無ければその確認は飛ばされる） | `tests/fixtures/overseas_shop/app/e2e/` |

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
- `--plan`：計画書の具体例を実行する bin も作る。

検査にエラーか「問い」が残っていると、`rust` と `compare` は先へ進みません。

## 作業の流れ（AI の動き方）

1. **計画書を作る**（`skills/plan-interview`）。ユースケースは一問一答で聞き、コードを書く前に動作要件を聞く。計画書は Markdown。
2. **`.geas` を書く**。[language-reference.md](language-reference.md) と、近い例（お店）を見ながら。名前は半角英数、日本語の表示名は直前の `///` に書く。
3. **検査する**（`check`）。エラーは直す。**問い（QUESTION）は、人に聞くべきことなので、勝手に決めずに質問する**（検査器が「決めていない」と見つけたもの）。警告も読む（表示名の抜けなど）。
4. **仕様書を作り、計画書と突き合わせる**（`spec`／`review`／`compare`）。ずれがあれば、計画書とコードのどちらが正しいかを人に確かめる。
5. **アプリにする**（`rust --host --runtime`）。手で書くコードを増やさないのが原則：計画書・`.geas` に現れない手書きコードは、無くす方向で考える。

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
| `modules.py` | `use`／`module` |
| `names.py` | 名前の規則と表示名の検査 |
| `cli.py` | コマンド |
