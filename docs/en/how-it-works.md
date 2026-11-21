# How it works: why the specification and the code do not drift

Geas aims for this: **an AI writes the code, and a human reviews only the specification, without reading any code.** To get there, the specification and the code are not two separate things to be compared later. **Both are generated mechanically from one source of truth, the `.geas` file.** This page is the overview; the evidence (tests) and the full list of what is not yet guaranteed are in [why-spec-and-code-dont-drift.md](../why-spec-and-code-dont-drift.md) (Japanese).

## The overall shape

```
conversation → plan (optional, for comparison)
                    │
              .geas (source of truth) ── checker `check` ──┬─ specification (review / spec)
                              │                            └─ Rust (rust) → screens, extension, server, MCP
                    if it does not pass, we stop here
```

The specification and the code both come from the same result of parsing and checking `.geas` once. The "reads / writes / transitions / external effects" columns of the specification are exactly the information used to generate the code. So "the specification is right but the code does something else" cannot happen by construction.

## Four mechanisms

### 1. Make unprovable code impossible to write

Anything that cannot be shown to keep its promise cannot be written at all. The checker stops it when the code is written.

| Promise | What the checker does |
|---|---|
| Invariants (e.g. stock never goes below 0) | Every write to a field must be shown, from value ranges, to keep the invariant. Otherwise it is an error |
| State transitions | Only the declared order is allowed. Changing a state without checking the current one is an error |
| Missing values / null | A value that may be missing must be checked before it is used |
| Money and integers | They are different types and cannot be mixed. A calculation that produces fractions needs a rounding rule |
| External effects | An idempotency key is mandatory |
| Value ranges | Even intermediate results must be shown to fit in the type's range, or it is an error |

### 2. Check every value that comes from outside, at the door

Internal values are protected by types and the checker. Only values coming from outside (people, other systems) are checked at runtime, at the entrance: length, character set, range, check digits, time range and so on.

### 3. No hand-written code that the specification does not show

Screens, the browser extension, the server runtime, MCP tool definitions and the end-to-end tests are all generated from `.geas` and the plan. The only exception is a part that calls a library of the root language (`library`); what it calls and how strongly it is checked both appear in the specification as a "human guarantee".

### 4. Everything the code does appears in the specification

"No drift" alone is not enough for a review that never reads code: if some behavior is missing from the specification, changing it would not change the specification and no one would notice. So even what each step does (under which condition which result is returned) appears in the specification, and tests measure whether anything has been left out.

## Verified by running it

A passing check is not the end: the generated Rust is actually run. Every language feature has behavior tests (see the [mapping table](../language-test-map.md), Japanese), and HTTP end-to-end tests and real-browser screen tests are generated from the examples in the plan, so the expected values are independent of the code. When something changes, only the difference from the previous version is shown to the human.

## How far can you trust it

| Layer | Current guarantee |
|---|---|
| Rust's own memory and type safety | Guaranteed by rustc. Generated code avoids constructs that lead to panics or undefined behavior |
| What the checker proves statically | Invariants, value ranges, non-null, transition order, parallel overlaps |
| Values from outside, results of parts | Checked at runtime every time |
| Generated code behaves as `.geas` says | Behavior tests and end-to-end tests (checks by example) |

**Geas does not yet carry a mathematical proof for the whole code.** The soundness of the checker itself, the insides of `library` parts and the details of screen design are outside the guarantee or weak, and what is weak is shown in the "strength of guarantees" table of the specification. The full list of gaps is in section 3 of [why-spec-and-code-dont-drift.md](../why-spec-and-code-dont-drift.md) (Japanese).

## Where to go next

- [Getting started](getting-started.md): try it by hand.
- [Language reference](language-reference.md): the full syntax.
- [How URLs are decided (REST)](rest.md): why the language decides the URLs.
