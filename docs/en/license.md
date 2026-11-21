# How geas thinks about licensing

> This page is a translation of the Japanese original (the Japanese text is the source of truth).

This document explains the policy. It is a draft on the premise that the final check of the legal wording is requested from a lawyer.

## Three layers

| Layer | Target | License |
|---|---|---|
| The geas core | Rust code in `crates/` such as parsing, checking, the generator and the CLI; docs; skills | GPL-3.0-or-later ([LICENSE](../../LICENSE)) |
| Parts embedded in generated code | Code fragments copied into the generated Rust: range checks, rationals, rounding, ratio checks, idempotency-key pairs, guarantee violations, JSON input and output, the WASM entry, etc. | MIT OR Apache-2.0 ([LICENSE-MIT](../../LICENSE-MIT), [LICENSE-APACHE](../../LICENSE-APACHE)) |
| Generated Rust and binaries | Rust code made from `.geas`, and what is built from it | The user's. No restrictions (only the MIT/Apache notice of the embedded parts must be kept) |

## Intent

- Use is free. If you modify (improve) geas and distribute it, publish those modifications under the GPL too.
- Apps that users build with geas can be used freely, with no obligation to publish. To avoid the "contamination" of generated outputs by the GPL, the code fragments copied into generated code have another, permissive license (a different solution to the same problem as the GCC Runtime Library Exception).
- Following Rust's convention, the embedded parts are dual MIT and Apache-2.0 (either can be chosen).

## Scope of the embedded parts

The following are the embedded parts. In the source they carry the marker `SPDX-License-Identifier: MIT OR Apache-2.0`, and the same marker is output at the head of the generated code.

- `crates/geas-core/src/assets/rust_prelude.txt` (`PRELUDE` in `rustgen.rs`)
- `crates/geas-core/src/assets/host_json_prelude.txt` and `host_call_wasm.txt` (`JSON_PRELUDE` and `CALL_WASM` in `hostgen.rs`)
- In general, code fragments the generator outputs as Rust strings depending on the content of the `.geas` (the `if` of a check, guarantee confirmation, the context's `check()`, etc.)

Everything else under `geas/` is the core (GPL).

## A future path to AGPL

If it becomes desirable to close the case where geas is modified and offered as a web service (where the GPL's obligation to publish does not apply because nothing is distributed), it should be possible to move to AGPL. For that reason, contributions are accepted under the conditions of [CONTRIBUTING.md](../../CONTRIBUTING.md) (a DCO sign-off, and the copyright holder's consent to re-licensing).
