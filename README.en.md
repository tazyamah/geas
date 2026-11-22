# Geas

[日本語](README.md) | English

**An AI writes the code, and a human reviews only the specification, without reading any code.** Geas is a language that guarantees this.

The specification (read by humans) and the code (written by the AI) are tightly bound: behavior that does not appear in the specification cannot be written in the code. Whether the code does what was asked is not something you read and confirm; the checker shows it mechanically.

The name comes from the Irish *geis*, a vow or taboo that must be kept. The heavier the constraints, the stronger the guarantee.

## See a generated specification first

An online-shop example, with the specification generated from its `.geas` (this is all a human reads). The specifications themselves are currently in Japanese.

- The full specification ([English](https://tazyamah.github.io/geas/en/overseas_shop.html) / [日本語](https://tazyamah.github.io/geas/overseas_shop.html)): use cases, screen transitions, examples, states, storage, human guarantees and the runtime environment (container and infrastructure diagram). Both the diagrams and the tables are generated from the `.geas` declarations; none is drawn by hand.
- Changes since the previous version, and what the reviewer is asked ([English](https://tazyamah.github.io/geas/en/overseas_shop_changes.html) / [日本語](https://tazyamah.github.io/geas/overseas_shop_changes.html)): after each update, this is all a human looks at.

## The flow

1. Build a plan (Markdown) by conversation ([skills/plan-interview](skills/plan-interview/SKILL.md)).
2. An AI writes the code (`.geas`) from the plan.
3. Check it, generate the specification, and compare it with the plan. The examples are run against the generated Rust. A human reviews the specification ([skills/geas-develop](skills/geas-develop/SKILL.md)).

## Usage

Only Rust (cargo) is needed. The tool, the generated code and the server runtime are all Rust.

```
cargo install --path crates/geas-cli                    # install (the binary is called geas)
geas check app.geas                                     # check
geas review app.geas --plan plan.md -o spec.html        # human-readable specification
geas rust app.geas -o outdir                            # generate Rust
geas compare app.geas plan.md                           # compare with the plan (runs the examples)
cargo test                                              # tests
```

## Read more

- [Getting started](docs/en/getting-started.md) and [How it works](docs/en/how-it-works.md) (English), or the documentation site ([English](https://tazyamah.github.io/geas/en/docs/) / [日本語](https://tazyamah.github.io/geas/docs/))
- For AI agents: [AGENTS.md](AGENTS.md) → ai-guide ([English](docs/en/ai-guide.md) / [日本語](docs/ai-guide.md))
- A full worked example: [tests/fixtures/overseas_shop/](tests/fixtures/overseas_shop/)
- What you can and cannot trust (what is guaranteed and what is not): [English](docs/en/why-spec-and-code-dont-drift.md) / [日本語](docs/why-spec-and-code-dont-drift.md)

## License

The geas tool is GPL-3.0-or-later. The generated Rust and binaries are free to use without restriction. See license ([English](docs/en/license.md) / [日本語](docs/license.md)) and [CONTRIBUTING.md](CONTRIBUTING.md).
