# Entry point for AI agents: from installing geas to using it

This is the first file an AI agent (Claude Code, etc.) should read when starting work with geas. The human-facing introduction is [README.en.md](../../README.en.md).

geas is a tool that matches a plan (a specification built through conversation) against code in the `.geas` language written by an AI, and guarantees mechanically that what was asked for has been built. **The AI writes `.geas`; humans do not read it.** What humans read is the specification (Markdown / HTML) generated from the code.

> These English pages are translations of the Japanese originals (the Japanese text is the source of truth). Links marked "(Japanese)" go to Japanese pages that have not been translated yet.

## Reading order

| What you want to know | Read |
|---|---|
| How to write the language (declarations, statements, expressions) | [language-reference.md](../language-reference.md) (Japanese) |
| Mistakes easy to make from other languages (`&&`, `else`, `string`, `;`, …) | [Section 12 of language-reference.md](../language-reference.md#12-他の言語から来たときに間違えやすいこと) (Japanese) |
| A complete worked example (a shop that also sells overseas: screens, REST, extension, server) | [tests/fixtures/overseas_shop/overseas_shop.geas](../../tests/fixtures/overseas_shop/overseas_shop.geas), and in the same folder `overseas_shop_plan.md` (plan) and `overseas_shop_spec.md` (generated specification) |
| Small examples | [tests/fixtures/shop/shop.geas](../../tests/fixtures/shop/shop.geas); the delete example `tests/fixtures/delete_demo/` |
| How to build a plan (the interview procedure) | [skills/plan-interview/SKILL.md](../../skills/plan-interview/SKILL.md) |
| How to write `.geas`, check it and show the specification to a person (questions to ask, reviewer-perspective questions, surviving mutants) | [skills/geas-develop/SKILL.md](../../skills/geas-develop/SKILL.md) |
| Behavior tests per language feature (which features are verified by actually running them) | [language-test-map.md](../language-test-map.md) (Japanese) |
| The basic idea of geas and how the specification and the code stay in sync (how far the guarantee goes) | [why-spec-and-code-dont-drift.md](../why-spec-and-code-dont-drift.md) (Japanese) |
| Naming rules | Chapter 1 of [language-reference.md](../language-reference.md) (Japanese) |
| History, proposals and records of how things were decided (no need to read) | [history/](../history/README.md) (Japanese) |
| Modules (`use` / `module`) | [modules.md](../modules.md) (Japanese) |
| How dates and times are handled | [datetime.md](../datetime.md) (Japanese) |
| List of documents | [README.md](README.md) |
| What is not done yet | [roadmap.md](../roadmap.md) (Japanese) |

When in doubt, **the checker (`geas check`) is the source of truth.** If the documentation and the checker disagree, trust the checker and fix the documentation.

## Installing

The geas core (checker, generator, CLI) is written in Rust. (The earlier Python version is kept under the git tag `python-final`.)

| Tool | Needed? | Purpose |
|---|---|---|
| Rust (cargo) | Required | `cargo install --path crates/geas-cli` installs `geas`. Also used to run code built with `rust`, to run examples, and for tests (`cargo test`) |
| Python 3 and pytest | Only to run the generated end-to-end tests (`tests/test_url_examples.py`, `tests/test_screen_examples.py`) | Not needed for geas itself. The generated app's tests are written in pytest |
| node and Playwright | Only for end-to-end checks of screens and the extension (skipped if absent) | The generated `tests/test_screen_examples.py` (the runner is the generated `tests/screen_runner.js`) |

```sh
git clone https://github.com/tazyamah/geas.git
cd geas
cargo install --path crates/geas-cli                          # install geas
geas check tests/fixtures/overseas_shop/overseas_shop.geas   # smoke test
cargo test --release                                         # tests (matching against the 3,894-item corpus, plus end-to-end examples)
cargo test --release -- --ignored                            # build and start the generated server (slow; needs network access to fetch crates)
```

To avoid installing, you can run from the repository root with `cargo run --release -p geas-cli -- check app.geas`.

Files geas reads and writes, and its standard output and standard error, are UTF-8 on every OS (even on Windows, redirected specifications and generated Rust do not become cp932).

## Commands

The output language is **English** unless you specify otherwise. To get Japanese, add `--lang ja` (accepted by every command) or set the environment variable `GEAS_LANG=ja`. See [i18n.md](../i18n.md) (Japanese) for details.

```sh
geas check app.geas [--json]               # Check. Prints errors, warnings and questions with the names and line numbers used in the code, and a count on the last line. Exit code 1 if there are errors, 2 for syntax errors and missing files. --json adds line and hint (the section to read and how to fix) to each finding
geas spec app.geas [--plan plan.md] [-o out.md]   # Generate the specification (Markdown). With --plan, the coverage of checks per internal URL is included
geas env-checks app.geas [--json]              # List what the app requires of its runtime environment, as tool-independent check items. Matching is done by env-verify
geas env-facts plan.json [--adapter aws-ecs] [--service address]   # Build environment facts (cloud-independent) from the output of `terraform show -json`. Facts that could not be read are null
geas env-verify app.geas (--plan plan.json | --facts facts.json) [--strict] [--json]   # Match the runtime requirements against environment facts. Exit code 1 if anything fails (--strict also fails on "could not be verified")
geas env-probe [--db store-file] [--egress host:port,…]   # Read environment facts on a running machine (a serverspec-like collector; reads Linux /proc). Open ports, the mount type under the store file (does it vanish?), TCP reachability to destinations. Unreadable items are null. The output has the same shape as env-facts, so save it as `> facts.json` and match it with env-verify --facts
geas container app.geas [-o output-folder]   # Generate Dockerfile, compose.yaml, .dockerignore and secrets.env.example from the container / infra vm_docker declarations (same output folder as geas rust --host --runtime; standard output if -o is omitted)
geas doctor app.geas [--db store-file] [--egress host:port,…] [--secrets file] [--strict] [--json]   # On a running machine (host or inside the container), diagnose the runtime requirements in one go. On top of collecting facts (env-probe) and matching (env-verify), it checks writability and free space of the store, the number of processes with the same DB open, clock synchronization, and presence of secrets (values are not read; environment variable GEAS_SECRET_<external>_<name>). If `container` / `infra` is declared, it determines the store from the declaration, checks the declared listening ports and the /api/health response, and on the host side checks data-folder permissions (can uid 10001 write?), env-file permissions (readable only by the owner), whether the secrets are present, and whether docker compose exists (what is visible differs inside the container and on the host). Finally prints the list of "human guarantees" (OS, Docker, front proxy, backups, secret values). Exit code 1 if anything fails
geas env-live app.geas --url http://host:port [--facts facts.json] [--strict] [--json]   # Check a running server from the outside (serverspec-like). Builds GET requests only from the specification's routes and checks the HTTP statuses match (no writes are sent). Unreachable means "could not be verified". With --facts, the env-verify matching is listed in the same table. https is sent with curl
geas review app.geas [--plan plan.md] -o out.html  # The specification for humans (HTML at phone width). With a plan, the matching results are included
geas compare app.geas plan.md [--no-run] [-o result.md]  # Match the plan against the code. Examples are actually run with the generated Rust
geas mutate app.geas [--base previous.geas | --base-git HEAD~1] [--run --plan plan.md] [-o result.md]  # Mutation check: build a version with one promise broken and see whether the checker stops it. Survivors are promises the checker does not protect (with `--run --plan`, survivors are also tested against the plan's examples, and become "detected by example" if they disagree. Builds Rust, so it is slow)
geas parts app.geas [--all] [--record]                # Actually call parts (library) with values at the edges of their argument ranges and check they are as declared (builds, panics, range, guarantee, same result on two calls). Exit code 1 if anything disagrees. After writing a library, run it before writing the reason for trust. --record writes the result to the ledger (app.parts.json; commit it) and shows it under "human guarantees" in the specification
geas receipt app.geas --base-git HEAD~1 [--plan plan.md] [-o receipt.md]  # A per-change receipt: trust-related changes compared with the previous version (weakened guarantees, added human guarantees, deeds whose behavior may change, URLs, screens, use cases, survivors). Paste it into the PR description
geas trace app.geas server-log [--plan plan.md] [-o result.md]  # Tie operational logs (the geas.route lines on the generated server's standard error) to the specification's routes, deeds, results and use cases. Exit code 1 if some lines cannot be tied
geas bench cases/ results-dir/ [--no-run] [--baseline previous.json] [--json current.json]  # Score `.geas` files an AI wrote from plans (check, mismatches with the plan, examples, number of fix-ups). Not called by the AI. Once right before a PR or a build
python proofs/run.py [--timeout seconds]                          # Proofs of the generated prelude (needs Kani; not part of the normal tests)
python proofs/std_proofs.py [--timeout seconds] [--jobs n] [--record]   # Proofs of the standard parts (needs Kani). Shows for all inputs that they do not panic and that they equal their expressions. --record writes the bundled ledger
geas rust app.geas -o out-dir [--plan plan.md] [--host] [--runtime] [--standalone] [--seed initial-data.json] [--server URL]
geas interview init|status|render ledger.json     # The ledger of the first interview (aid for building a plan)
```

The options of `rust` are:

- Nothing: only the Rust crate (the body of the checked deeds).
- `--host`: also produces screens (PWA), the development mock panel (served only when the server is started with `--test`), the entry point `host.rs`, the definitions `DEFS_JSON`, and, if a screen has `page`, the browser extension (`ext/`). As an entry point for AIs it also produces the MCP tool definitions (`mcp/tools.json`, derived mechanically from the URL definitions), a generic bridge that relays to the server (`mcp/bridge.py`) and a registration example (`mcp/mcp.json`).
- `--host --runtime`: also places the server runtime (`server/`, SQLite and HTTP). Build it with `cd out-dir/server && cargo build --release`. How to run it, its arguments and what to put in front when publishing (nginx, etc.) are in [docs/server.md](../server.md) (Japanese). `--standalone` also places a windowed exe (`window/`) that puts the screens and the deeds in one executable without a transport ([docs/standalone.md](../standalone.md), Japanese). `--seed` is the initial data inserted only when the DB is empty (JSON of `{"entity": {"key": {row}}}`), embedded into the output. Rows are checked against the types' ranges before insertion (if a row is out of range, it stops without inserting).
- `--plan`: also produces a bin that runs the plan's examples. Together with `--host --runtime`, it produces the HTTP end-to-end tests `tests/test_url_examples.py` from the plan's "URL examples" (below) and the screen end-to-end tests `tests/test_screen_examples.py` from the plan's "screen examples" (below) (build the server, then `python -m pytest out-dir/tests`).

If the check has errors or leftover "questions", `rust` and `compare` do not proceed.

## Working flow (how an AI should proceed)

The way of asking in steps 2–4 (questions one at a time, reading the specification first from a reviewer's perspective, showing survivors of the mutation check) is collected in [skills/geas-develop](../../skills/geas-develop/SKILL.md).

1. **Build the plan** (`skills/plan-interview`). Ask use cases one question at a time, and ask for the behavioral requirements before writing code. The plan is Markdown.
2. **Write the `.geas`**, with [language-reference.md](../language-reference.md) (Japanese) and the nearest example (the shop) at hand. Names are ASCII; write the Japanese display name in the `///` just before.
3. **Check** (`check`). Fix errors (see the line number of the finding and the guidance such as `similar name` and how to fix). **Questions (QUESTION) are things to ask a person, so do not decide them yourself; ask** (the checker found something "not decided"). Also read the warnings (missing display names, etc.).
4. **Generate the specification and match it against the plan** (`spec` / `review` / `compare`). If they disagree, ask the person which is right, the plan or the code.
5. **Make the app** (`rust --host --runtime`). The principle is not to add hand-written code: code that does not appear in the plan or the `.geas` should be removed, not added.

## The plan's "URL examples"

Tests are not written by hand; they are written as tables in the plan. A table whose header columns include `method`, `path`, `expected result` and `expected status` is the URL examples (`input`, `expected body` and `keep` are optional). The column names are written in Japanese in the plan (`メソッド`・`経路`・`期待する結果`・`期待する状態`・`入力`・`期待する本文`・`控える`).

| ID | ◆メソッド (method) | ◆経路 (path) | ◆入力 (input) | ◆期待する結果 (expected result) | ◆期待する状態 (expected status) | 控える (keep) |
|---|---|---|---|---|---|---|
| UX6 | POST | /orders | `{"customer_id":"c-001",…,"slots":{"currency":"JPY"}}` | 受付 | 201 | order_id = body.order_id |
| UX16 | POST | /order_completions | `{"order_id":"{order_id}"}` | 在庫が足りなかった | 422 | |

- Rows run from the top against a fresh server with the initial data (`--seed`). The input is the JSON actually sent; `expected body` is JSON the response must contain (a part is enough).
- The expected result may be a display name or a code name; for responses with no result kind (a 200 for a list, a 400 from a boundary check), use `－`.
- `keep` is `name = path in the response` (`body.order_id`, `rows.0.order_id`) and can be substituted as `{name}` in later rows' paths and inputs.
- `compare` / `review` check that the method and path exist in the routes and that the result and status are as declared in `resource`, and also count them in the "coverage of checks per internal URL".

## The plan's "screen examples"

End-to-end tests of screens (PWA) and the Chrome extension are not written by hand either. To each row of the plan's "use case steps" (screen, action, result), you add values with a `screen examples` table. A table whose header columns include `use case`, `step`, `enter / choose` and `visible` (`ユースケース`・`手順`・`入れる・選ぶ`・`見えること`). Rows are tied to the steps table by `use case` and `step`.

| ユースケース (use case) | 手順 (step) | 入れる・選ぶ (enter / choose) | 見えること (visible) |
|---|---|---|---|
| 商品を見つけて注文する | 3 | 明細.knife-jp = 1; 顧客ID = c-001; 配送先.国 = JP | 通知: 受付 |
| 商品を見つけて注文する | 4 |  | 行 * の 状態: 完了; 含む: 10,928 |

- Execution: per use case, from the top, against a fresh server with the initial data, operating a real browser. Results of the previous use case remain, and the next use case starts only after the replies (mocks) of the external effects emitted by the previous one have arrived. Step 1 of "view" opens that screen's URL; selecting a row and operating push the screen's parts (`data-screen`, `data-row`, `data-field`, `data-action`, etc., the `geas/ui` conventions). The result kind of an operation is checked not by the screen's wording but by the `result` of the response of the URL that was called. The destination screen is checked against the transition declaration.
- Enter / choose: `row = ID` (select a row) / `input = value` / `address.country = JP` (a composite part) / `lines.ID = quantity` / `currency = JPY` (slot) / `drop reply = response name` (leave the reply of an external effect undelivered) / `advance clock = 30 min` (advance the mock clock; 30 minutes are not actually waited) / `override input = value` (replace, just before sending, a value the screen cannot produce, to check how the result is handled; the ID of update / destroy can be overridden too) / for extension screens, `page = path` and `page table = heading: value` (it does not go to the real Amazon; this page is substituted and returned). (The keywords are written in Japanese in the plan: `行`・`返事を落とす`・`時計を進める`・`差し替え`・`ページ`・`ページの表`.)
- Visible: `contains` / `does not contain` / `notice` (the screen's notice area) / `row count` / `row` / `no row` / `row ID's column: value` (`*` as the ID means any row) / `selected slot: value` / `nothing shown` (the extension shows nothing). Names may be display names or code names. (Keywords in the plan: `含む`・`含まない`・`通知`・`行数`・`行`・`行なし`・`選択`・`表示なし`.)
- **Do not write how long to wait.** At the start of the test, the server's response time, the seconds until a reply (`delay_secs` of `/api/test/state`) and the screen's re-query interval are measured, and the wait limit is decided from them. The parts "a reply arrives", "it is processed" and "the screen is updated" are checked separately by the HTTP examples and the Rust-side tests.
- The plan's "screen design" table is not matched, but a "view" step checks that layout, color and brightness take effect on the screen. That the screen opens offline, that the development operation panel works as defined, and that a list that has not changed is not redrawn on re-query (the server returns `304`) are generic checks done every time, independent of the plan.
- If a dialog (alert / confirm) appears, it fails. A screen error (pageerror) also fails.

## Rules to keep

- Humans do not read `.geas`. Do not ask people about syntax preferences; ask only questions that change promises (definitions, guarantees, meaning).
- Names are ASCII (start with a letter, then letters, digits and `_`). Types, entities, states, results, triggers, flows and events are UpperCamel; fields, variables, deeds, queries, inputs and parts are lower_snake.
- Names that appear in the specification get a Japanese display name in a preceding `///` (a warning if missing).
- REST URLs (`resource`) have their rationale in [rest.md](../rest.md) (Japanese): only `index` / `show` / `create` / `update` / `destroy`, no PATCH, POST only for creation, the only query string is the filter condition `q[name]=value`, and an HTTP status must be decided for every result kind. URLs of mid-flow deeds (nexus, trigger destinations) are "internal"; the server answers them only under `--test` (they cannot be screen actions either). To distinguish who may call, declare `actor role` and write `for role, …` on every externally callable action (`for anyone` for open ones). Authentication of the person is the job of the proxy in front; the server compares the header `X-Geas-Actor` (401 if absent, 403 if it does not match). A query that returns only the caller's own rows declares `scope range on data { filter field equals caller }` and is written `from data.range` (`caller` is not an input; the person's ID does not go in the URL either). A range is a set of rows; it can have `where` and arguments (`scope name(arg: type)`, used as `data.range(input)`), and `from data.range1.range2` gives only the rows that are in all of them.
- External effects (`request`) require an idempotency key.
- Split changes into small PRs. Do not stack PRs; go to the next after the merge.
- Do not commit generated outputs (`tests/fixtures/*/app/generated/`, etc.; already in `.gitignore`). The specifications (`*_spec.md`, `*_review.html`) are fixed as samples for the tests, so regenerate them when you change the `.geas`:

```sh
F=tests/fixtures/overseas_shop
geas spec $F/overseas_shop.geas --plan $F/overseas_shop_plan.md > $F/overseas_shop_spec.md
geas review $F/overseas_shop.geas --plan $F/overseas_shop_plan.md -o $F/overseas_shop_review.html
```

The mutation result (`overseas_shop_mutation.md`) is also fixed as a sample; if you change the `.geas` so that the mutations or survivors change, the test fails. Regenerate it after fixing:

```sh
geas mutate $F/overseas_shop.geas -o $F/overseas_shop_mutation.md
```

The specification has three tabs: [use cases] [behavior] [specification]. The "person" in the use case diagram is drawn with the name from an optional "actor" column that you can add to the plan's use case table (if absent, the screen that starts the use case stands in).

The specification does not include the questions to answer, their counts, or changes from the previous version (those go in a separate HTML). For reviewing an update, add `--diff-o changes.html` and pass the previous version with `--base previous.geas`, or for a git version `--base-git HEAD~1`. The changes HTML has a "trust receipt" section before the changes (the same content as `geas receipt`). When opening a PR, paste the output of `geas receipt` into the description.

## Source map (`crates/`)

The core is a Cargo workspace of two crates: `crates/geas-core` (library) and `crates/geas-cli` (the `geas` command). External crates are used for regular expressions (`regex`), JSON (`serde_json`), arbitrary-precision integers (`num-*`), embedding assets (`include_dir`) and PNG compression (`flate2`).

| File (`crates/geas-core/src/`) | Role |
|---|---|
| `lexer.rs`, `parser.rs`, `ast.rs` | Lexing, parsing, syntax tree. The entry for declarations is `parse` in `parser.rs` |
| `check/` | The checker (types, ranges, write overlap, transitions, REST, screens, extension, rule, etc.). The entry is `check` in `check/defs.rs` |
| `spec.rs`, `review.rs`, `svg.rs`, `plan.rs` | Generation of the specification (Markdown / HTML; diagrams are the in-house SVG of `svg.rs`; look and structure follow specgap's review screen), reading and matching the plan, coverage of checks |
| `rustgen.rs`, `hostgen.rs` | Rust generation of deeds, the entry point and definitions (`DEFS_JSON`) |
| `rest.rs`, `screengen.rs` | REST URLs and statuses, screen definitions |
| `mcp.rs`, `assets/mcp_bridge.py` | MCP tool definitions (from the URL definitions) and the standard-I/O bridge (generic, only reads the definitions; a small Python script placed in the output) |
| `crategen.rs` | Writes out the tree of generated outputs (crate, server runtime, windowed exe, extension, development mock panel, MCP). The parts live in `assets/` (`ext`, `dev`, `server`, `window`, `ui`, `e2e`, `std`) and are embedded with `include_dir` |
| `testgen.rs`, `examples.rs` | Generate the HTTP end-to-end tests (pytest) from the plan's "URL examples" and the screen / extension end-to-end tests (pytest + Playwright) from the "screen examples". Build the bin for running from the "examples" tables, run it and compare with the plan |
| `assets/e2e/screen_runner.js` | The generic runner of the screen end-to-end tests (placed in the output's `tests/`; it does not know the app) |
| `receipt.rs` | The per-change receipt (`geas receipt`, the "trust receipt" section of `review --diff-o`): change of guarantee strength, impact, survivors of the mutation check compared with the previous version |
| `hints.rs` | The hint of `check --json` (the section to read and how to fix per kind of finding). When you add a kind of finding to the checker, add it here too |
| `trace.rs` | `geas trace`. Ties the server's structured logs to the specification's rows |
| `bench.rs` | `geas bench`. Scores `.geas` files written by an AI |
| `partcheck.rs`, `partrun.rs` | The ledger of parts and the edge-value check (`geas parts`). The reference for the standard parts (Rust's integer methods rewritten with unbounded integers) is also in `partrun.rs` |
| `mutate.rs` | The mutation check (`geas mutate`). Not run normally; run before review. With `--base` / `--base-git`, only changed lines are broken |
| `envreq.rs`, `envcheck.rs`, `adapter.rs` | The list of runtime requirements, matching against environment facts, and the adapter that builds facts from a terraform plan (aws-ecs) |
| `interview.rs` | The ledger of the first interview |
| `modules.rs`, `names.rs` | `use` / `module`, naming rules and the display-name check |
| `crates/geas-cli/src/main.rs` | The commands. `crates/geas-cli/tests/` has the corpus matching (`golden.rs`) and end-to-end checks (`e2e.rs`) |

The corpus (`tests/corpus.tar.xz`) that watches for output differences has its expected values in `tests/golden/hashes.json` (fingerprints made from the output of the Python version before the Rust port). If you change output on purpose, regenerate the expected values (`cargo test` shows the differing inputs and items).
