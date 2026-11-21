# Handover

> This page is a translation of the Japanese original (the Japanese text is the source of truth).

A note for the next person (including an AI) who works on geas to read first.

## Read first

1. [ai-guide.md](ai-guide.md): how to install, the commands, the working flow, the rules to keep, the source map.
2. [why-spec-and-code-dont-drift.md](why-spec-and-code-dont-drift.md): the mechanism that keeps the specification and the code in sync, and what can and cannot be trusted.
3. [roadmap.md](roadmap.md): the list of what is not started or on hold. **What is left undone is here.**
4. [language-reference.md](language-reference.md): how to write the language. If it disagrees with the checker, the checker is the source of truth.
5. [language-test-map.md](language-test-map.md): the table of behavior tests per language feature.

The list of documents is [README.md](README.md). The history of decisions, proposals and records of finished things are in [history/](../history/README.md) (Japanese) and need not be read.

## Current state

- The core (checker, generator, CLI), the generated outputs and the server runtime are all Rust (works with cargo alone). The generated outputs have no `unsafe`, `unwrap`, `expect`, indexing or bare arithmetic, and abnormal conditions are returned as `Fault` (enforced by clippy deny).
- The approach: "guarantee most of it through the way of writing in geas, and bring the run-time loss to zero." We discuss, one by one, the things that do not satisfy this. The rest is in [roadmap.md](roadmap.md).

## Working agreements

- Replies, comments and explanations are in Japanese (`docs/en/` and `README.en.md` are English translations).
- Make small PRs, and move on to the next after the merge. Do not stack PRs.
- Questions and impressions are not requests for fixes. Answer and confirm before fixing.
- Write only the current specification and usage in the documents. Move proposals, the history of decisions and finished discussions to [history/](../history/README.md).
- Tests: `cargo test`.
