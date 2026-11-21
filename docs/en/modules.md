# Modules (splitting files and reuse)

> This page is a translation of the Japanese original (the Japanese text is the source of truth).

What is distributed is the source itself (no dedicated distribution format, signatures or distribution-site mechanism is built). The importing side can notice a swap through the version it depends on and an (optional) pinned hash of the source. The history of how this policy was decided is in [history/modules-history.md](../history/modules-history.md) (Japanese).

## Conclusion

**A module is limited to things that hold no state.** It is a collection of types and expression parts, and has no entity. It can have deeds as long as they do not touch state (they only return a result from their inputs). A module that holds state is not introduced until it is needed.

This removes the following troubles.

- No check of "only the owning module may write an entity" is needed (modules have no entities).
- No invariant that spans modules arises (invariants are written inside the app, about the app's entities).
- When importing separately checked results, the premises of proofs do not collapse.

## Purpose and unit

- Make frequently used definitions into reusable units. The unit is a **common concept not specific to a business** (logical dates, standard logical types, conversions and parts, etc.).
- Business units (such as orders) are not made modules. Cancelling an order involves requirements the author cannot know, such as payment.
- Splitting for readability is not needed (people do not read Geas code). If a file becomes huge and causes harm, we will think about it separately then.
- In the future, modules will be checked and compiled beforehand so that only the result needs to be imported (making checking lighter).

## What can be written in a module

- Logical types (`ltype`, including composite types), conversions (`convert`), and parts that call a root-language library (`library`).
- Deeds that do not touch state. They cannot read or write data, make state transitions, assign numbers or perform external effects (the checker stops it). They are connected into the app's flow in the form `Mod.grade: Pass → reply` (there is not yet a way in the language to call a deed from inside an expression).
- The shape of results (`result`) and declarations of external effects (`effect`). Both are types and declarations that hold no state, so they can be shared.
- What cannot be written: `entity`, `state`, `query`, `nexus`, `trigger`, `event`, `invariant`, `store`.

## Containers of values

- Containers that are not stored, can be looked up by key, and are read-only are written as a mapping whose key is `id`, as in `ltype Table = id → Pos` (the value may be a composite type). Using an entity as a temporary container brings along write commands and a container of state (`store`), so it is not used. In Rust it is just passing a `BTreeMap<String, Pos>` by reference.

## Rules about types (kept at module boundaries too)

- Only built-in types can be compared, used in arithmetic and assigned. A logical type is "a built-in type + constraints", and operations are done on the underlying built-in type (an implicit conversion. The physical type is the same and the constraints are only looked at by the checker, so there is no run-time cost). A composite type is read as built-in types, split into components.
- Therefore, what crosses a module boundary is only "the underlying built-in type + constraints (range, etc.)". The physical type does not cross.
- An invariant is written about the app's entities, in a form that compares values as built-in types.

## Promises (what the language enforces)

1. A module holds no state (only the "what can be written" above can be written).
2. Modules cannot be imported circularly.
3. Names are placed in a namespace per module. A name in another module is referred to as `Module.Name`.

## What is published (the part corresponding to a header)

- The underlying built-in types and constraints of types, the types and ranges of the inputs and return values of conversions and parts, the shape of results, and declarations of external effects.
- What is hidden: physical types, and the bodies of conversions and parts. The module is responsible for deciding the physical type, and the outside does not interfere. What the outside knows is only "what the underlying built-in type is and what it can be converted to".
- Publication is private by default, and only what is written is published.

## How to write

- A module's file makes its first statement `module Name`. The other statements are only those in "what can be written" above.
- The importing side writes `use "geo.geas"` (a path relative to the importing file) at the head of the file and refers to things as `Geo.Pos` and `Geo.same(x)`.
- The implementation is the expansion of token sequences. The module's names are renamed on import, as in `Geo_Pos`, and `Geo . Pos` is replaced by the renamed name. The same file is read only once. A cycle, a collision of module names, a declaration that cannot be written and a nonexistent name stop with a ModuleError.
- The check of name formats is done on the original name (`Pos`). The display name uses the `///` on the module side, and `Geo.Pos` is shown if there is none.
- Conversions and parts are functions that return values, so they are lower_snake (`Geo.same`).

## Stages

1. **Import the source as it is.** Import the file, make the whole into one program, and check and generate it. A namespace is introduced. A section for the defined modules is added to the specification.
2. Pinning the version and hash. `use "geo.geas" version "1.2.0" sha256 "…"` (both optional). It is matched against the source's `module Geo version "1.2.0"`, and a mismatch is an error. No optimization across modules is done.
3. (When needed) modules that hold state, and parameterized modules. At that time, the check "only the owner may write an entity" and the handling of invariants across modules will be decided anew.
