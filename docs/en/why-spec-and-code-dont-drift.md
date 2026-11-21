# How the specification and the code stay in sync: geas's thinking and how far it can be trusted

> This page is a translation of the Japanese original (the Japanese text is the source of truth). Test names quoted below are English glosses of the Python version's Japanese test names (kept at the git tag `python-final`).

This file is an explanation for a person to judge whether geas can be trusted. It describes not only "what is guaranteed and how" but also **what is not yet guaranteed**. The claims are based on the repository's implementation and tests (the location of the evidence is attached to each section). The entry point for AI agents is [ai-guide.md](ai-guide.md).

The test names in the text (`test_…`, `tests/test_*.py`) are those of the Python version before the Rust port (kept at the git tag `python-final`; if you take them out, they run as they are). In the current Rust, the same guarantees are watched, in `crates/geas-cli/tests/`, by `golden.rs` (the corpus outputs match the Python version), `spec_coverage.rs` (a change in the code always appears in the specification), `e2e.rs` (run the examples with generated code, start the server), `lang.rs` (Japanese / English mode) and `container.rs` (container and infra).

## 1. The basic idea

The aim of geas is that **an AI writes the code, and a human can review without reading any code.** To that end, the specification and the code are not treated as separate things "compared afterwards"; they are **produced mechanically from the same source of truth**. The party that is made to keep the promises is not the app's user but the person (and AI) building with geas. The heavier the constraints, the stronger the guarantees (the "oath" in the name comes from this).

What a person reads is not the `.geas` but the specification generated from it (the Markdown of `geas spec`, the phone-width HTML of `geas review`). What the person confirms is whether, after reading the specification from the top to the end, they can say "this is right."

## 2. The skeleton of the mechanism that prevents drift

```
Conversation → plan (optional, for matching)
                  │
              .geas (source of truth) ── checker check ──┬─ specification (spec / review)
                                │                        └─ Rust (rust) → screens, extension, server, MCP
                      if it does not pass, it stops here
```

### 2.1 The specification and the code are made from the same check result

Both the specification and the code come out of the result (`Checker`) of parsing and checking the `.geas` only once.

- Specification: `spec.build(ck)` extracts the structure from the check result (`crates/geas-core/src/spec.rs`). The display for people is `review.rs` and `svg.rs`.
- Code: `rustgen.generate(ck)` (`crates/geas-core/src/rustgen.rs`).

Both receive the same `ck` in `crates/geas-cli/src/main.rs`. The specification's columns "reads, writes, transitions, external effects, physical types" are the very information used when generating the code. For example, `tests/test_geas.py`'s "the specification extracts reads, writes and transitions from the code" and "physical numbers are chosen from the range" confirm that what appears in the specification (the fields written, the realized types such as `u64`) matches the check result.

In other words, a drift of the form "the specification is right but the code does something else" has become hard to occur structurally.

### 2.2 No hand-written code that does not appear in the specification

The generated outputs, down to the screens (PWA), the browser extension, the development panel, the server runtime (SQLite and HTTP), the MCP tool definitions and the end-to-end tests, are all made from the `.geas` and the plan. The head of a generated file says "automatically generated; do not edit by hand", and the descriptions of `crategen.rs`, `hostgen.rs`, `screengen.rs` and `mcp.rs` also say "there is no hand-written code". The server runtime does not know the app and runs only on the generated definitions (`DEFS_JSON`).

The exception is **parts that call a root-language library (`library`)**. The code being called lies outside geas, and the checker cannot see its content. So the call is confined to one module of the output (`mod library`), and the declared range and guarantee (`guarantee`) are checked on every call (a violation or a panic stops the deed before anything is written). A `trust` with a reason is required, and what is called and the level of checking appear under "human guarantees" in the specification. The earlier `part`, where the app implemented `trait Parts`, has been abolished.

### 2.3 What does not pass is not allowed to go forward

`geas check` outputs errors and "questions" (QUESTION). A question is "something to ask a person, which the checker found 'not decided'" (the direction of rounding, how to handle the case where something does not exist, how to handle deletion, etc.). If errors or questions remain, `rust` and `compare` do not proceed (the `ck.ready()` judgment in `crates/geas-cli/src/main.rs`). An AI silently deciding and proceeding is stopped by the mechanism. That Rust is not generated from code with errors is also confirmed by a test (`test_do_not_generate_from_code_with_errors`).

### 2.4 Increase the "ways of writing that cannot be written" and make them guarantees

In geas, a way of writing that cannot be shown to be safe cannot be written in the first place. The main ones are as follows (the checker is `crates/geas-core/src/check/`).

| Promise | What the checker does | Tests of examples that are stopped (Python version names; the Rust counterparts are in [language-test-map.md](language-test-map.md)) |
|---|---|---|
| Invariants (stock does not fall below 0, etc.) | On every write to a field, it shows on the spot from the value ranges that the promise is kept. An error if it cannot show it. Whether it is "proved" appears in the specification | `test_removing_the_stock_check_means_the_invariant_cannot_be_shown`, `test_decreasing_stock_by_a_field_update_shows_neither_range_nor_invariant` |
| State transitions | They advance only in the declared order. Changing without checking the original state is an error | `test_a_transition_not_in_the_declaration_is_stopped`, `test_without_a_check_of_the_original_state_it_is_stopped` |
| Absent / null | A value that may be absent cannot be used until it has been checked | `test_a_reference_without_an_existence_check_becomes_a_question` |
| Money and integers | They are different types and cannot be mixed. A calculation that produces a fraction needs a rounding specification | `test_without_a_rounding_specification_it_becomes_a_question` |
| External effects | An idempotency key is required | `test_without_an_idempotency_key_it_is_stopped` |
| Parallelism (`par`) | If the branches write the same field or their reads and writes overlap, the order would affect the result, so it is an error | `test_par_branches_…` |
| Value ranges | A value wider than the field's type cannot be written. Physical types (`u8`, `u64`, etc.) are chosen from the range, and `overflow-checks = true` even in release builds (also when building the server; Cargo uses only the settings of the build's starting point, so it is written in the server's `Cargo.toml` too) | `test_physical_numbers_are_chosen_from_the_range`, `test_even_when_built_as_a_server_an_overflow_in_a_library_calculation_stops_as_a_panic` |

### 2.5 Values coming from outside are checked at the boundary every time

Internal values are protected by types and the checker, and **only the values coming from outside (people, other systems)** are checked at run time at the entrance (length, character set, range, conversions tied to the type (such as check digits), the range of times, etc.). An item not in the declaration is treated as "not having come". The evidence is `test_geas.py`'s "the host checks the length and characters of inputs at the boundary" and "slots are also checked against the declared range at the boundary", and the shop's end-to-end `test_values_from_outside_are_checked_at_the_boundary`.

### 2.6 Run it and confirm

We do not stop at "the check passed"; the generated Rust is actually run.

- **Behavior tests per language feature**: [language-test-map.md](language-test-map.md) lists, for each item of the reference, whether there is a test that runs the output and confirms it (◎ confirmed by running / ○ only stopped statically / ✕ not yet there). When a feature is added, a row is added to the table.
- **Determinism**: the same `.geas` produces the same Rust. A test confirms that it matches the committed outputs (`test_generation_is_deterministic_and_matches_the_committed_outputs`).
- **The shop app end to end**: waiting for payment and warehouse replies, expiry, restoration after a restart, re-charging with idempotency keys and so on are run with the server built (in Rust, `crates/geas-cli/tests/e2e.rs`; in the Python version, `tests/test_shop_app.py` and `tests/test_runtime.py`).
- **The plan's examples**: from the "examples", "URL examples" and "screen examples" tables of the plan (the specification built through conversation), the HTTP end-to-end tests and the real-browser (Playwright) screen tests are **generated** (`testgen.rs`). The expected values are on the plan's side and independent of the code. Tests are not written by hand, and when fixing them, you fix the plan's tables.

### 2.7 Matching against the plan (an optional extra net)

`geas compare` matches the plan's ◆ columns against the specification generated from the code, and outputs "in the plan but not in the code", "in the code but not in the plan", "values differ", "examples disagree" and "unspecified" (`plan.rs`). This is **an optional check, not a mandatory gate**, because the thing a person reviews is "the whole of the generated specification" in any flow. However, test generation from the plan's examples remains, as expected values independent of the code.

When updating, you can compare with the specification generated from the previous version (the previous git commit, etc.) and output the differences (`geas review --diff-o … --base-git HEAD~1`).

### 2.8 No omissions: everything the code does appears in the specification

"No drift" (what is written in the specification matches the code) alone does not make a review without reading code possible. If there is behavior that does not appear in the specification, the specification does not change even when that is changed, and people cannot notice. One more thing is needed: "no omissions" (everything the code does appears in the specification).

When we changed the sample `.geas` one place at a time and measured, we found behavior that did not appear in the specification, such as the content of deeds (under which condition which result occurs, what is written and how many). We put them in the specification, and now the following are shown.

- **The content of deeds**: for each deed, the statements in order from the top (the "content of the deed" section). In HTML it is a flowchart (rectangles are things done, hexagons are branches, thick rounded frames are results), and in Markdown a numbered procedure. Names are replaced with display names.
- The type and range of values decided from outside (slots), the upper limit of the number of elements of collections (`max N`), the way the browser extension reads values from a page and the content of the lookup table, the place where a value is taken out of an external reply, and the app's name.

What keeps this is `crates/geas-cli/tests/spec_coverage.rs`. It changes the sample one place at a time, and if there is even one case where the output changed but the specification (HTML) or the "changes from the previous version" did not, the test fails. When a feature is added to the language, adding a sample that uses that feature makes it stop here if showing it in the specification was forgotten.

Limits: there are five kinds of changes (increase a number by 1, replace a comparison or addition/subtraction operator, replace a word, change a string, delete one line), and only ways of writing present in the sample are looked at. "Appears" is counted by whether the characters changed, and whether it is shown in a way people can notice is not measured.

## 3. How far it can be trusted, and what is not yet guaranteed

This is the honest part. **geas has not yet given mathematical proof to the whole of the code.** Guarantees differ in strength.

| Layer | The current guarantee | Strength |
|---|---|---|
| Rust's own memory safety and type safety | Guaranteed by rustc (most of the output is safe Rust) | By the compiler |
| What the checker shows statically | Invariants, value ranges, non-null, existence checks, order of transitions, overlap of par | As long as the checker's implementation is correct |
| Values from outside and return values of parts | Checked at run time every time | Run-time checks |
| The output works as the `.geas` says | Behavior tests per language feature, end-to-end tests | Tests (confirmation by examples) |
| The specification correctly represents the `.geas` | Made from the same check result (2.1) | By structure. Rendering of the display is separate |

This difference in strength also appears in the "strength of guarantees" table of the generated specification. For invariants, whether "the checker showed it" or "it has not been shown", and for human guarantees, which of "checked at run time every time", "checked only in verification builds", "stops at the edge of the range" and "not checked", are lined up in order of strength. It is built so that a person only has to read the lower rows with emphasis (following how GitHub Next's PDD Kit labels results "proof" or "test").

The main gaps are as follows.

1. **The soundness of the checker has not been proved.** Part of what the checker judges "shown" becomes a premise for skipping run-time checks. However, **the generated Rust is bound not to use syntax that can cause a panic or undefined behavior** (`unsafe`, `unwrap`, `expect`, indexing and bare arithmetic are not used, `#![deny(unsafe_code, clippy::unwrap_used, …)]` is placed at the head of the output, and it is enforced by `cargo clippy`). Fetching the write target and non-null values, conversion to physical types and calculation overflow return `Fault` when they fail. For this reason, even if the checker has a hole, what happens is not a panic or memory corruption but an abnormality found as a `Fault`. A deed is a pure function, so on a `Fault` writes are not applied and storage does not change. The test is `tests/test_geas.py`'s "generated deeds pass clippy's panic ban, and fail if the ban is broken". The remaining gap is that `Fault` not actually occurring (= what the checker showed) is still confirmed only by test examples.
2. **There are still ✕ in the behavior tests per language feature** (the end of [language-test-map.md](language-test-map.md)): `event … external → deed`, deeds that use `now` and `tz_offset`, and a check that relays MCP to a real server.
3. **The details of screen design and the rendering of the specification (diagrams, colors)** are not the target of the "no drift" guarantee; they are parts confirmed by a person's eyes.
4. **The content of libraries (`library`)** lies outside geas, and the guarantee is the run-time check (and a human guarantee). Infinite loops and memory exhaustion cannot be prevented.
5. Tests are confirmation by examples, not proofs for all inputs.

## 4. What is proved (Kani)

The binding of the generation rules (removing from the output the syntax that can panic, and enforcing it with clippy settings) is in place.

**The prelude is proved**: the fixed prelude that goes at the head of every output (the calculation of `Ratio`, `floor`, `ceil`, `half_up`, the judgments of range, ratio, characters and GTIN, and case/width folding) does not change from app to app, so once shown with Kani it holds for every app. The proofs are in `proofs/prelude_proofs.rs`, and run with `cargo run -p geas-tools -- proofs` (only in an environment where Kani is installed. They are not part of the normal tests). The eight shown are as follows.
- The range judgment `range_ok` is equivalent to "at least the lower limit and at most the upper limit".
- If the ratio judgment `ratio_ok` passes, the denominator is at least 1 and at most 10⁹.
- For a ratio whose denominator is 0 or less, `floor`, `ceil` and `half_up` all return an abnormality.
- The calculation of ratios (`mul_ratio`, `scale`, `floor`, `ceil`, `half_up`) does not panic for any input.
- The character judgment `first_bad_char` (its body `first_bad_in`) is equivalent to "all characters are in the range" for strings of up to 2 characters.
- The GTIN judgment is done by the standard module `Gtin` (written in geas expressions), so it is not included in the proofs here. That it makes the same judgment is confirmed by running tests.
- Width and kana folding `fold_width_kana` does not panic for any single character and specification, does not change what is out of range, and shifts what is in range by a fixed amount.

A way of writing suited to proofs is to separate the judgment (a pure function returning only a truth value) from the assembly of error messages (`format!`). A function containing `format!`, or a proof that assembles a `String`, is unrolled by Kani without bound and does not finish. If the body of the judgment takes a sequence of characters (an iterator), it passes instantly with an input made from an array. Also, showing the "correctness of values" with 128-bit multiplication and remainder (the quotient and remainder of `floor`, etc.) is heavy and has not been included yet. The standard parts (`library`) only call Rust's standard library and there is nothing on geas's side to prove, so the basis is `trust check`, which checks the return value on every call.

## 5. Summary

- The specification and the code are made mechanically from the same check result. No hand-written code is placed.
- A way of writing that cannot be shown to be safe cannot be written. While errors and questions remain, it does not go forward.
- Values from outside are checked at the boundary every time, and the output is actually run and confirmed.
- However, the correctness of the checker itself, and the basis for places that use `unsafe`, are for now supported by tests and are not mathematical proofs.
