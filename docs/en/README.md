# Documentation (English)

Geas is a language for building applications **where an AI writes the code and a human reviews only the generated specification, without reading any code.** The specification and the code are generated from the same source of truth, so they cannot drift apart, and the checker proves what it can.

These English pages are translations of the Japanese originals (the Japanese text is the source of truth). Links marked "(Japanese)" go to the Japanese originals (the sample specifications and the history are Japanese only).

## Start here

| Document | Contents |
|---|---|
| [Getting started](getting-started.md) | Install, build a small app, produce the specification |
| [How it works](how-it-works.md) | Why the specification and the code do not drift |
| [Sample specification](https://tazyamah.github.io/geas/overseas_shop.html) (Japanese) | A generated specification of a worked example (an online shop) |
| [Changes since the previous version](https://tazyamah.github.io/geas/overseas_shop_changes.html) (Japanese) | What a reviewer reads after each change |

## Use

| Document | Contents |
|---|---|
| [Guide for AI agents](ai-guide.md) | Install, commands, working flow, rules, source map |
| [Language reference](language-reference.md) | The full syntax (the checker is the source of truth) |
| [Language concepts](language-concepts.md) | The ideas behind the language, with examples |
| [Modules](modules.md) | Splitting files and reuse |
| [REST](rest.md) | How URLs are decided |
| [Dates and times](datetime.md) | Real and virtual datetimes, time zones |
| [Japanese / English mode](i18n.md) | `--lang`, `GEAS_LANG`, how to add translations |

## Run

| Document | Contents |
|---|---|
| [Server](server.md) | How to run the generated server |
| [Windowed exe](standalone.md) | The standalone app |

## Why you can trust it

| Document | Contents |
|---|---|
| [Why the specification and the code do not drift](why-spec-and-code-dont-drift.md) | The mechanism, the evidence and the gaps |
| [Language test map](language-test-map.md) | Behavior tests per language feature |

## Working procedures for AI

| Document | Contents |
|---|---|
| [Building a plan (plan-interview)](skill-plan-interview.md) | Interview procedure that builds the plan |
| [Writing `.geas` and showing the specification (geas-develop)](skill-geas-develop.md) | Check, review questions, mutation survivors |

## What is next, license

| Document | Contents |
|---|---|
| [Roadmap](roadmap.md) | What is not done yet |
| [Handover](handoff.md) | The note to read first for the next person |
| [License](license.md) | How licensing is thought about |

## History (Japanese)

The history of decisions, withdrawn proposals and past reviews are in [history/](../history/README.md). They are not the current specification.
