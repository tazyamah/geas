# Getting started (tutorial)

In about 15 minutes you will install geas, build one small app, and produce the specification that a human reads. In geas, **an AI writes the `.geas` code and humans do not read it**; here you will look inside once, just to see how it works.

> The documents in `docs/en/` are English translations of the Japanese originals, which remain the source of truth. The checker's messages and the generated specifications are in English by default (`--lang ja` switches to Japanese); the sample code below uses Japanese display names, which the Japanese mode shows as is.

## 1. Install

You only need Rust (cargo).

```sh
git clone https://github.com/tazyamah/geas.git
cd geas
cargo install --path crates/geas-cli                          # installs the `geas` binary
geas check tests/fixtures/overseas_shop/overseas_shop.geas   # the sample passes the checker
```

## 2. Look at a sample specification

Humans read the generated specification, not `.geas`. The samples are [the full specification](https://tazyamah.github.io/geas/overseas_shop.html) and [the changes since the previous version](https://tazyamah.github.io/geas/overseas_shop_changes.html) (both in Japanese). You can also generate them yourself:

```sh
F=tests/fixtures/overseas_shop
geas review $F/overseas_shop.geas --plan $F/overseas_shop_plan.md -o spec.html
```

## 3. Build a small app

A notepad app: write a note, then publish it. It lives in `tests/fixtures/tutorial/memo.geas`.

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

Four things to notice:

- **`ltype`** is a named type. `text 1..200` means "1 to 200 characters". A type with a range must be given a name before it is used.
- **`entity`** is stored data and **`state`** is its lifecycle. A note starts as a draft (`Draft`) and ends as published (`Published`).
- **`deed`** is a processing step. It declares what it reads, writes, transitions, returns and affects outside, and its body cannot contradict that declaration.
- **`resource`** is the outside entry point (a URL). You must decide the HTTP status for every kind of result; leaving one out is an error.

## 4. Check it

```sh
geas check tests/fixtures/tutorial/memo.geas
```

```
Check: passed
```

Output is in English by default. Add `--lang ja` (or `export GEAS_LANG=ja`) for Japanese, which prints `検査: 通りました`.

When something is wrong, the checker reports the name and line number in your code. As an experiment, delete the line `if note.status != Draft { return NotDraft }` from `publish_note` and check again. The checker stops you: the declared read of `Note.status` is not used in the body, and nothing returns the result `NotDraft`. Code whose declaration and body disagree does not pass, and **code that does not pass cannot go any further.**

## 5. Produce the specification

```sh
geas review tests/fixtures/tutorial/memo.geas -o memo.html    # the human-readable specification (phone-width HTML)
geas spec tests/fixtures/tutorial/memo.geas                   # the same as Markdown, to standard output
```

The specification contains the data, the states and transitions, the list of processing steps, what each step does (a top-to-bottom procedure), the URL table and the flow diagram, all generated mechanically from the code. A human reads it from top to bottom and decides "yes, this is right". The code is never read.

## 6. Break a promise and see whether it is caught

```sh
geas mutate tests/fixtures/tutorial/memo.geas
```

This creates versions of the code with one promise broken at a time and checks whether the checker stops each one. In this app, removing `required` from `body: NoteBody required` makes the checker say nothing (a "survivor"). That means the promise "the body is always present" is not protected by the checker. If it matters, add it to the examples in the plan or to an invariant.

## 7. Turn it into Rust

```sh
geas rust tests/fixtures/tutorial/memo.geas -o out/memo
```

`out/memo` now contains a Rust crate you can build as is. The options that also produce the server and screens (`--host`, `--runtime`, `--standalone`) are described in [ai-guide.md](ai-guide.md).

## 8. The real flow starts from a conversation

In practice the work starts from a conversation, not from code.

1. Talk with an AI and build a plan (Markdown) ([plan-interview](../../skills/plan-interview/SKILL.md)).
2. The AI writes `.geas` from the plan and runs it through the checker ([geas-develop](../../skills/geas-develop/SKILL.md)).
3. Generate the specification and compare it with the plan (`geas compare app.geas plan.md`). The examples in the plan are run against the generated Rust.
4. A human reviews the specification. After each change, only [the changes since the previous version](https://tazyamah.github.io/geas/overseas_shop_changes.html) (`review --diff-o changes.html --base-git HEAD~1`) needs to be read.

## Where to go next

- [How it works](how-it-works.md): why the specification and the code cannot drift apart.
- [Language reference](language-reference.md): the full syntax.
- [Guide for AI agents](ai-guide.md): every command and the working rules.
