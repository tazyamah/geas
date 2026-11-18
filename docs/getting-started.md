# はじめかた（チュートリアル）

geas を入れて、小さなアプリを 1 つ作り、人が読む仕様書を出すまでを 15 分ほどで通します。**`.geas` を書くのは AI で、人は読まない**のが geas の前提ですが、仕組みをつかむために、ここでは 1 度だけ中身を見ます。

## 1. 入れる

要るのは Rust（cargo）だけです。

```sh
git clone https://github.com/tazyamah/geas.git
cd geas
cargo install --path crates/geas-cli                          # geas を入れる
geas check tests/fixtures/overseas_shop/overseas_shop.geas   # 見本が検査を通る
```

生成した Rust を動かすときだけ、Rust（cargo）が要ります。

## 2. 見本の仕様書を見る

人が読むのは、`.geas` ではなく、そこから生成した仕様書です。見本は[仕様書の全体](https://tazyamah.github.io/geas/overseas_shop.html)と[前の版からの変更](https://tazyamah.github.io/geas/overseas_shop_changes.html)で、自分の手元でも作れます。

```sh
F=tests/fixtures/overseas_shop
geas review $F/overseas_shop.geas --plan $F/overseas_shop_plan.md -o 仕様書.html
```

## 3. 小さなアプリを作る

メモを書いて、公開するだけのアプリです。`tests/fixtures/tutorial/memo.geas` に置いてあります。

```
app "メモ帳"

needs exposure internal "手元で試すだけで、インターネットには出さない"

// はじめかたの例：メモを書いて、公開する。名前は半角英数、仕様書に出す日本語の名前は /// で付ける。

/// 本文
ltype NoteBody = text 1..200

/// メモ
entity Note {
  /// メモID
  note_id: id unique
  /// 本文
  body: NoteBody required
  /// 状態
  status: NoteStatus
}

/// メモの状態
state NoteStatus {
  /// 下書き
  initial Draft
  /// 公開中
  final Published
}

/// メモを書いた結果
result WriteNoteResult {
  /// 書いた
  Written(note_id: id)
}

/// 公開した結果
result PublishNoteResult {
  /// 公開した
  Done
  /// メモが無い
  NoSuchNote
  /// 下書きではない
  NotDraft
}

/// メモを書く
deed write_note {
  input body: NoteBody required
  writes Note
  transition Note: (new) → Draft
  result WriteNoteResult
  effects none

  let note = create Note { body: body, status: Draft }
  return Written(note.note_id)
}

/// メモを公開する
deed publish_note {
  input note_id: id required
  reads Note.status
  writes Note.status
  transition Note: Draft → Published
  result PublishNoteResult
  effects none

  let note = Note[note_id] else { return NoSuchNote }
  if note.status != Draft { return NotDraft }
  note.status = Published
  return Done
}

/// メモ
resource Notes {
  create write_note {
    Written → 201
  }
}

/// メモの公開
resource NotePublications {
  create publish_note {
    Done → 201
    NoSuchNote → 404
    NotDraft → 409
  }
}

/// メモの流れ
nexus MemoFlow {
  write_note: Written → reply
  publish_note: Done → reply, NoSuchNote → reply, NotDraft → reply
}
```

見るところは 4 つです。

- **`ltype`** は名前つきの型です。`text 1..200` は「1〜200 文字」で、範囲のある型は必ず名前を付けてから使います。
- **`entity`** は保存するデータ、**`state`** は状態です。メモは「下書き」から始まり「公開中」で終わります。
- **`deed`** は処理です。読む・書く・遷移・結果・外部作用を宣言し、本体はその宣言と食い違えません。
- **`resource`** は外から触れる口（URL）です。結果の種類ごとに HTTP の状態を必ず決めます（漏れるとエラー）。

## 4. 検査する

```sh
geas check tests/fixtures/tutorial/memo.geas
```

```
検査: 通りました
```

エラーが出たら、コードに書いた名前と行番号が出ます。試しに `publish_note` の `if note.status != Draft { return NotDraft }` を消して、もう一度検査してください。宣言した「読む `Note.status`」を本体で使っていない、結果 `NotDraft` を返す箇所がない、と指摘されて止まります。宣言と本体が食い違うコードは通らず、**通らないコードは先へ進めません。**

## 5. 仕様書を出す

```sh
geas review tests/fixtures/tutorial/memo.geas -o memo.html    # 人が読む仕様書（スマホ幅の HTML）
geas spec tests/fixtures/tutorial/memo.geas                   # Markdown で標準出力へ
```

仕様書には、データ、状態と遷移、処理の一覧、処理の中身（上から順の手順）、URL の表、フロー図が、コードから機械で作られて出ます。人はこれを頭から読んで「これで合っている」と言えるかを確かめます。コードは読みません。

## 6. 約束を崩して、守られているかを見る

```sh
geas mutate tests/fixtures/tutorial/memo.geas
```

約束を 1 つずつ崩した版を作って、検査器が止めるかを見ます。このアプリでは、`body: NoteBody required` から `required` を外しても検査器が何も言いません（生き残り）。「本文は必ずある」という約束が、検査器では守られていないという印です。守りたい約束なら、具体例（計画書）か不変条件に足します。

## 7. Rust にする

```sh
geas rust tests/fixtures/tutorial/memo.geas -o out/memo
```

`out/memo` に、そのまま組める Rust の crate が出ます。サーバーや画面まで出すオプション（`--host`・`--runtime`・`--standalone`）は [ai-guide.md](ai-guide.md) にあります。

## 8. 本来の流れ：会話から始める

実際の作業は、コードからでなく会話から始まります。

1. AI と会話して計画書（Markdown）を作る（[plan-interview](../skills/plan-interview/SKILL.md)）。
2. AI が計画書から `.geas` を書き、検査器に通す（[geas-develop](../skills/geas-develop/SKILL.md)）。
3. 仕様書を生成して、計画書と突き合わせる（`geas compare アプリ.geas 計画書.md`）。具体例は生成した Rust で実際に動かします。
4. 人が仕様書をレビューする。変更のたびに、[前の版からの変更](https://tazyamah.github.io/geas/overseas_shop_changes.html)の版（`review --diff-o 変更.html --base-git HEAD~1`）だけを見れば済みます。

## 次に読むもの

- [仕組み](how-it-works.md)：なぜ仕様書とコードがずれないのか。
- [言語リファレンス](language-reference.md)：書き方の一覧。
- [AI 向けの入口](ai-guide.md)：コマンドの全部と、作業の約束。
