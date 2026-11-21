# Not started / on hold

> This page is a translation of the Japanese original (the Japanese text is the source of truth).

Only things not yet done are placed here. Finished things, and the history and proposals of decisions, are in [history/](../history/README.md) (Japanese). Once something is started and finished, remove it from here.

## Core implementation

- What remains of **converting boundary values from raw with convert**: nested shapes, omitting escaping on screens for `dangerous` (after it is decided how to show `raw` on screens), and the situations where `unsafe` is used (none in the current outputs).
- What remains of the **language-side standard parts** (pure calculations that cannot be written in geas expressions, such as hashes): the proof of `pow` (adding an upper bound on repetition), bit-operation expressions, telling a timeout apart from a disagreement with the expression, and handling of strings (str, String), char, other crates and functions returning Option.

## The windowed exe (standalone apps)

- Checking clicks, menus and hiding on a real tray (Windows, macOS), a setting to hide the macOS Dock, auto-start at login, OS notifications (a kind of external effect), a declaration that takes the icon's display (a count, etc.) from a query, and showing the existing window on a second launch.
- Checking the Windows exe on a real machine (build and selftest on GitHub Actions' windows-latest). Checking against X's real API (OAuth refresh, rate limits).
- Extensions of `over http` (GET, inputs other than the body, the resend policy, the idempotency-key header, the OS key store for secrets).
- Extensions of file operations (listing folders, appending, moving, operations under a declared absolute path (a path value), interpreting file contents such as JSON).
- The entrance that chooses the storage location at startup is decided outside geas (the app's design; record it next to the exe, etc. Make an unwritable place an error).
- Making an app with roles (actor) a windowed exe, and testing migration with the windowed exe.

## Run-time guarantees

- Cut the storage layer out as a contract (trait) so another storage can be added.
- When several servers write the same DB, each one re-reads everything every time another writes. Make it only the changed rows.
- An upper limit on counts (after demand appears).
- `saturating +=` in the form `update { … += … }`.
- Stop `Unproven` being a reuse of evidence. Kani verification by generating fixed capacity (the standard BTreeMap and String are too slow).

## Verification and generation

- Mutation check: more ways of breaking (the levels of `trust`, rounding specifications, etc.), and reflecting them in the specification's "strength of guarantees".
- Enforcing, in the ledger (`interview`), that one cannot proceed if a reviewer-perspective question is skipped (once skipping actually becomes a problem).
- Automatically pasting the per-change receipt into the PR description (CI), and reflecting the results of example tests in the receipt.
- What remains of the prelude's Kani proofs: the check of the quotient and remainder of `floor` (heavy at 128 bits), the case folding of `fold_text`, generated code on the deed side that is not in the prelude, and other tools such as Verus.
- Placing a `bench` case collection (the shop app and several others) and comparing with the previous run, making `hints`' fixes finer, from per kind to per finding, adding a time window and the distribution of states to the log, and the screen side of `trace` (the screen's operation log).
- Adding a URL column to the use case steps. End-to-end tests of HTTP for `update` / `destroy`. Eliminating the remaining hand-written parts (the Chrome extension, the dev mock).
- Removing from the "not yet" of the specification's "coverage of checks per internal URL" the pairs of internal URL and result that no example reaches.
- Verifying whether it is meaningful to build ① (the specification built through conversation) separately.

## Things with no behavior check yet

The same as "things not yet there (✕)" in [language-test-map.md](language-test-map.md).

- `external → deed` of an `event` (the form that runs on input passed from outside)
- Deeds that use `now` and `tz_offset`
- A check that relays MCP to a real server and runs it

## Runtime environment (runtime requirements)

- Other platforms (ECS, k8s, Cloud Run, etc.), adapters that turn the output of cloud read-only CLIs (`aws ecs describe-services`, etc.) into facts, and reading resources inside child modules.
- Reachability per destination and actual existence of secrets in the real environment (needs an entrance to check from inside the environment), an agreement tying the secret names to the environment's variable names (currently a secret is always "could not be verified"), and a check with the role header attached for an app with roles.
- Placing the real environment's results beside the specification's "human guarantees", an "the environment is responsible" row in the "strength of guarantees", and increases and decreases of environment requirements in the receipt.
- A converter into check items for the real environment (goss as the first candidate, made from the list with "conditions"), a self-check at server startup (refuse to start if secrets are missing), and detecting a setup that puts sqlite on a shared file system.

### Remaining items to confirm on real machines (to be done together)

A real machine or environment is needed. When confirmed, write the result here and remove the item.

- Actually build and run the generated Dockerfile (`geas container` in the output folder of `geas rust --host --runtime` → `docker compose up -d --build`). Whether non-root, the read-only root, the volume permissions (uid 10001), the health check and `--allow-host` actually work
- A VM + Docker end-to-end: whether it is reachable over HTTPS through the front proxy, whether replacement (`docker compose up -d --build`) does not make the old and new overlap, and whether the volume's data remains
- Actually run `geas doctor` inside the container and on the host: ports, the kind of region (telling overlay from a volume), clock synchronization (timedatectl), the number of processes with the same DB open, and the env-file permissions
- Run `geas env-live` against a real HTTPS server (https is sent with curl). 401 and 403 of an app with roles (actor)
- The destination reachability of `geas env-probe` (`--egress`) in an environment actually restricted by a firewall
- The aws-ecs adapter with a real `terraform show -json` (the sample plan is still hand-written)
- Whether the specification's (.md) configuration diagram (mermaid) is actually rendered in GitHub and specification viewers

## Distinguishing who may call

- Extending how scopes are fetched in deeds: forms other than `let … else` (`data.scope[ID].field`, re-fetching inside a for).
- Dividing scopes per role. Showing, per screen, which roles can use it (for now, pressing an operation you cannot call gives 403).
- MCP tools (the bridge does not attach headers, so an entry with roles gives 401). How to attach roles when trying the screen locally (for now a proxy, or a call with the header attached).
- Recording who called in the log (the audit item of the [review in history](../history/reviews/review-2026-11.md), Japanese). Roles in the windowed exe.

## Migration of the stored shape

- Point out, with `check --base`, a field whose type changed under the same name.
- How to fix values when a type or range changes (for now it stops if any row does not fit).

## Undecided matters (datetime, modules, license)

- The kinds of virtual datetime (only dates, or generalizing groups of components such as month-day and year-month), and handling of month ends (one month after January 31).
- Where to place the standard modules (conversion from a virtual datetime to a real datetime, etc.). How to handle the same module imported from two places (make it the same entity). Collisions of display names (`///`) are looked at per module.
- The wording of the notice at the head of generated code (is a short one-line SPDX enough?), and a lawyer's check of the wording.

## Out of scope

Overlapping writes by concurrently started servers (S7), writes of a par's branches, calendar calculation (adding months, weekdays, daylight saving time, holidays), floating point, physical units, etc. The reasons are in [history/v0.1.0-scope.md](../history/v0.1.0-scope.md) (Japanese).
