# Geas language reference (for AI agents)

A list of the syntax. All the examples are in [tests/fixtures/overseas_shop/overseas_shop.geas](../../tests/fixtures/overseas_shop/overseas_shop.geas). If this page and the checker disagree, **the checker (`geas check`) is the source of truth.**

> This page is a translation of the Japanese original (the Japanese text is the source of truth). Quoted strings in examples are illustrative; in the real files they may be Japanese. Keywords inside the plan's tables stay Japanese where noted.

## 1. General rules

- The file extension is `.geas`. The order of declarations is free, but an `ltype` must be written before the place that uses it.
- Names are ASCII letters, digits and `_` (starting with a letter). Types, entities, states, results and their kinds, namespaces of external effects, triggers, nexuses, events, responses and resources are UpperCamel. Fields, variables, deeds, queries, inputs, slots, parts, conversions, and the operations and arguments of external effects are lower_snake. A name equal to a built-in cannot be used in lowercase.
- A comment runs from `//` to the end of the line. `///` is a **description comment**: it becomes the display name (Japanese) of the name written right after it (used only in the specification; the meaning does not change). Written before a declaration keyword (`entity`, `deed`, etc.) it attaches to the next name. It can also be written before the name of a field, input, state or result kind. For several languages use `@ja …` and `@en …`.
- The arrow is `→`, but writing the ASCII `->` is the same.
- Statements are separated by newlines. A newline inside `( )` is a continuation of the expression.
- Numbers are written with ASCII `0`–`9` (full-width digits such as `９９` cannot be used; there are no decimals either).
- Strings are `"…"`. Inside, write `\"` for `"`, `\\` for `\` itself, `\n` for a newline and `\t` for a tab (these are the only things that can follow `\`; the regular expression `\d` is written `\\d`). A line cannot be broken as it is inside a string. To continue a long string onto the next line, write `\` at the end of the line (leading blanks on the next line are skipped). A reason containing a newline is shown broken across lines inside the specification's tables.
- `app "name"`: the app's name (the screen heading). `timezone +09:00`: the app's default time zone (UTC if omitted). `use "other.geas"` and `module name`: see [modules.md](../modules.md) (Japanese).

## 2. Types

| Syntax | Meaning |
|---|---|
| `id` | An identifier |
| `int` / `int 1..99` / `int 0..` | Integer. A range is `a..b`; one-sided is `a..` |
| `ratio` / `ratio 0..1` | A ratio (a fraction) |
| `money` / `money(USD)` / `money(currency)` | An amount of money (an integer in the smallest unit). Each unit is a different type; different units cannot be mixed. `money(name)` is the name of a currency (`currency` type) input, slot or field in the same list |
| `currency` | A currency code (JPY, USD, EUR). Can be an input, slot or field |
| `text` / `text 1..80` / `text 2..2 of "A-Z"` | A string. Length and the characters allowed (`of` is a range joined with `-`) |
| `Gtin.Gtin13` (standard module) | JAN, EAN, UPC. Checks even the last check digit at the boundary. Use it by writing `use "gtin.geas"` (the bundled `crates/geas-core/assets/std/gtin.geas`; if there is no file of the same name next to yours, the bundled one is read) (for 8, 12 and 14 digits: `Gtin.Gtin8`, `Gtin.Gtin12`, `Gtin.Gtin14`). The way of checking is written in geas expressions (`convert`) and appears in the specification's parts table. The built-in `check gtin` has been removed |
| `time` | A datetime. Durations `30 min`, `24 hour`, `10 sec` can be added and subtracted; the subtraction of two times is a number of seconds. `day`, `week`, `month` and `year` cannot be written |
| `bool` | A truth value. The values are `true` and `false`. No range. Can be written for fields, inputs, slots and the arguments and return values of a `library`. JSON is also `true` / `false` (`"yes"` or `1` is invalid input). A checkbox on screens |
| `bytes` | A byte string |
| `set of id`, `set of id max 100` | A set. `max N` is the upper limit of the count. A set that elements are added to (`add x to s`) needs `max N` |
| `ref Customer` | A reference to data |
| `Product → Quantity` / `Product → Quantity max 100` | A mapping (keys are an entity, values are a type). A mapping whose key is `id` is a container that is not stored. `max N` is the upper limit of the number of elements (used to show that the total of `sum` fits the range; a mapping coming from outside also has its element count checked at the boundary) |
| A state's name | `state`, below |

Ranges and combinations are used only after **giving them a name** with `ltype` (they cannot be written directly on a field).

```
ltype Quantity = int 1..99
ltype ProductQuantity = Product → Quantity max 100
ltype Address {                      // a composite type: has components. Build with Address { country: …, … }, read with a.country
  country: CountryCode
  line: AddressLine optional         // an optional component can be left unspecified; accessing an unspecified one is an error if possible
}
```

**Ranges of integers and money are treated as ranges that fit the physical type (`int` is `i64`, `int 0..` is `u64`, etc., chosen from the range).** Even if you write no upper or lower limit, the stored values are limited to the physical type's range. When writing a value into a field, it is an error unless the checker can show that the value fits the field's range (and the physical type) (a way of writing that could panic cannot be written). The clues for showing it are as follows.

- Put the upper limit of a growing value, such as a unit price, total or score, in the `ltype` (`money(JPY) 0..1000000000000`).
- The total of a `sum` is shown from the collection type's `max N` and the multiplication of the element's range.
- An addition (`x.n += 5`) can be shown if an upper-limit check precedes it, such as `if x.n > 995 { return Full }` (only comparisons with numbers are looked at).
- The difference between two times is shown from the range of times coming from outside (10^15 seconds).

A built-in composite type is `vtime` (year month day hour minute second; a virtual datetime). Integers and money cannot be mixed (money × integer and money × ratio are allowed; fractions use `round(value, floor|ceil|half_up, JPY)`).

A ratio has a physical upper limit even without a written range (the denominator is 1 to 10⁹, the value is ±10⁹; values from outside are checked at the boundary). Ratios can also be multiplied together (`base * tax_rate * discount`), and since the result may have a fraction, round it with `round`. If the intermediate numerator and denominator fit in `i128`, the checker shows it by intervals. A combination it cannot show is a check error, so narrow the range of the money or ratio.

By default, the range of an input is checked at every boundary. When a person knowingly skips that for speed, add `trust` to the input (`input quantity: Quantity required trust assume "the supplier guarantees 1–99 by contract"`).
- `trust verify "reason"`: checked in debug / verification builds, skipped in release builds.
- `trust assume "reason"`: checked nowhere. If an out-of-range value arrives, it does not become a `Fault`; it goes in as it is.
- `trust dangerous "reason"`: does not check, like `assume`, and also lets the input take an unchecked `raw` as its type (`input body: raw required trust dangerous "reason"`). In a deed that has received a `raw`, the only things you can use are `len(r)` (the number of characters), `to_raw(string)` (make a checked string into a `raw`), `raw_cat(a, b)` (join `raw`s), and passing it as an argument of an external effect that takes `raw`. The intended use is relaying the received value to an external app without processing it, or with information added. A `raw` cannot be put in a stored field, in the content of a result, or on a screen.
- The reason text is required. The level and the reason appear under "human guarantees" in the specification.
- The checker uses the declared range as a fact in its proofs (knowing that if it is violated the proof quietly collapses). The overflow checks of calculations (`checked_*`) and the ban on panics do not change; even if violated, nothing becomes undefined behavior.
- The physical upper limit of a ratio's denominator (1 to 10⁹) remains even with `trust`.

There is also a way to receive a value from outside as unchecked raw data (`raw`) and convert it with a geas expression (a boundary that passes the selection of inputs). The conversion is tied to a logical type.
```
ltype PartNo = text 8..8 of "0-9" read part_no_reader
convert part_no_reader(r: raw) -> PartNo {
  require len(r) == 8 "must be 8 digits"
  require only(r, "0-9") "digits only"
  return text(r)
}
deed put_item {
  input part_no: PartNo required
  ...
}
```
- `raw` can be written only as the argument of a `convert` that converts from `raw`. This `convert` takes exactly one `raw` argument, and for now can return a string (`text`).
- If a `require condition "reason"` does not hold, it is invalid input (the deed does not run and storage does not change). The reason is required.
- The operations allowed on `raw` are `len(r)` (the number of characters), `only(r, "a-z0-9")` (whether it consists only of those characters) and `text(r)` (take it out as a string). None of them fails or breaks memory safety.
- Writing `read conversion_name` at the end of a type declaration makes values of that type always pass through that conversion at the boundary (for inputs, return values of parts and stored fields alike). The checks of the type's declaration (range, length, characters) work after the conversion. The levels of `trust` skip the conversion as well.
- The checker confirms that the value written into a field of a `read` type is limited to the same `read` type (input, field, return value of a part). A string that has not passed through the conversion cannot be written.
- This conversion cannot be called inside an expression.
- Character-by-character calculation is possible. `positions(r)` is the positions from 0 to length-1, `digit(r, i)` is the digit value of the i-th character (0 for a non-digit character or outside the length; check with `only` first), and `if condition then expr else expr` is a conditional expression. The check digit of a JAN can be written with geas expressions as follows (the tests confirm that the result matches the former built-in `check gtin`).
```
require sum { digit(r, i) * (if (len(r) - 1 - i) % 2 == 1 then 3 else 1) | i in positions(r) } % 10 == 0 "the check digit does not match"
```

## 3. Data and states

```
entity Order {
  order_id: id unique
  customer: ref Customer required      // without required, it may be absent (null)
  items: ProductQuantity required
  currency: currency required
  total: money(currency) required
  status: OrderStatus                  // a state field is required
  on delete Customer.orders → detach   // what happens to referrers on delete: detach (empty the reference) | cascade (delete referrers too) | leave (as is)
  removable in Draft, Archived         // states in which deletion is allowed (writing keep forbids deletion)
}
storage ExternalDb isolation store(sqlite)  // definition of a storage; holds who guarantees serialization
store Order in ExternalDb              // the storage is written outside the entity. Placed outside, the language adds the unsynchronized state "recorded"

state OrderStatus {
  initial Received
  middle PaymentRequested
  final Completed, Canceled            // after final, endless (continues without end) and reject can also be written
  reject Completed → Canceled reason "after completion, handle it as a return"
}
```

- Every field "may be absent". Only `required` ones are always present (keys and states are required). Before use you must check with `let x = o.f else { return … }` or `if o.f is null { return … }`, or the check stops you.
- A row or field itself being absent (`D[k] is absent`) is different from a value being null.
- Defined fields: writing `held: Quantity = stock + reserved` makes a field that is not stored (the expression is computed every time it is read). What can be written in the expression is only additions and subtractions of required numeric fields of the same row and numbers. Nesting of defined fields in an expression cannot be written. They cannot be written to. They can be used in deeds, invariants and the `select` columns of a query (screen display). They cannot be written in a query's conditions, narrowing or ordering. In the "read" declaration for showing it as a column, write the expression's source fields. The check shows the type's range each time the source fields are written.
- Comparisons allowed in an invariant are a field and a constant (`stock >= 0`), or two fields of the same row (`held <= cap`). An expression such as `stock + reserved <= cap` cannot be written directly. Make the expression a defined field and then compare.
- `invariant Product: stock >= 0` and `invariant Order: status == PaymentRequested implies total > 0`. On every write the check shows from the value ranges that it is upheld, and it is an error if it cannot show that.

### Migration of the stored shape (migration)

If you change an entity's fields, a DB created by the previous version cannot be read correctly by the current version as it is. The steps to turn the previous version's DB into the current shape are written in `migration`.

```
migration 2 {                    // the version of the stored shape. Increases by 1 from 2. Do not delete earlier migrations
  add Parcel.weight = 5          // the value put into rows that already exist, for the added field
  add Parcel.memo = null         // a field that may be absent can be null
}

migration 3 {
  rename Parcel.memo to note     // rename a field (values move as they are)
  drop Parcel.legacy             // delete a field (values are discarded)
}
```

- What can be written is `add entity.field = value`, `rename entity.field to new_name`, `drop entity.field`, and per entity `rename entity Entity to NewName` and `drop entity Entity`. The steps apply in version order, from the top down.
- Steps for a whole entity go through the word `entity` (so they are not confused with the steps for fields). `rename entity Order to Purchase` moves the rows as they are to the entity with the new name (the numbering continues as well). `drop entity Coupon` discards all rows. In both cases the entity's definition must also be changed (the check looks at this). If a field referring to the deleted entity remains, the type check stops it. If you only add an entity, nothing is written in `migration`. The `migration` of a previous version keeps the entity's name as of that version.
- The value of `add` is a number, a `"string"`, `true`/`false`, `null`, a state's name, or `numerator/denominator`. The value must fit the field's type and range (the check looks at this; for a field renamed by a later `rename`, it is looked at with the type of the renamed field). `add` puts the value only into rows that do not have that field.
- The result of applying all the steps must match the current entity's fields (the check looks at this). The source of a `rename` and a field of a `drop` must not be in the current entity; the target of `add` and `rename` must be in the current entity. A field that is added and later deleted need not be in the current entity.
- The DB remembers its own version (1 if it remembers nothing). At startup the server applies only the missing versions' steps in order, checks the rewritten rows against the current types' ranges, and then advances the version. Everything is done in one go, and if even one row does not fit, it stops without changing anything (a row that has both the source and the target of a `rename` also stops, because it is undecided which to keep). A DB with no rows starts as the current version. A DB newer than this app is not opened.
- `geas check app.geas --base previous.geas` (or `--base-git HEAD~1`) makes it an error if, in an entity that also exists in the previous version, an added or removed field is not written in a `migration`. The same goes for `geas review --base`. If you delete a field without writing it in a `migration`, startup succeeds, but the value remains in the previous version's rows and comes back to life when a field with the same name is added later.
- The specification shows a table of "changes to the stored shape".

## 4. Deeds

```
deed place_order {
  input customer_id: id required, items: ProductQuantity required, ship_to: Address required
  slot currency: currency              // a value filled from outside. The screen lets the user choose the currency / the server decides it by a rule
  slot shipping: money(currency)
  reads Product.stock, Product.price, Customer       // declare the fields read
  writes Order                                       // declare the fields written
  transition Order: (new) → Received                 // the movement of states
  result PlaceOrderResult                            // the shape of the result
  effects none                                       // external effects (none if there are none)

  let customer = Customer[customer_id] else { return NoSuchCustomer }
  let short = { k | (k, v) in items, Product[k].stock < v }
  if short is nonempty { return OutOfStock(short) }
  let subtotal = sum { Product[k].price * v | (k, v) in items }
  let order = create Order { customer: customer_id, items: items, total: subtotal, status: Received }
  update { Product[k].stock -= v | (k, v) in items }
  order.status = Completed
  request Payment.charge(order_id, order.currency, order.total, idempotency_key = order_id)
  delete product
  for k in order.items { … }
  return Received(order.order_id)
}
```

- The declarations (`input slot reads writes transition result effects`) come first, the body statements after. The checker finds any mismatch between the declarations and the body.
- Statements: `let name = expr [else { return … }]`, `let name = create Entity { field: expr, … }`, `x.f = expr`, `x.f += expr`, `update { target -= expr | (k, v) in collection }`, `if condition { … } [else { … }]`, `for element in collection { … }`, `request namespace.operation(arguments, idempotency_key = expr)`, `delete variable`, `return result_kind(content)`.
- After writing to a field, reading that field through the same variable shows the written value (`x.f` after `x.f = 0` is 0. When comparing a whole row, as in `x == y`, x is compared with its written value). Writes to storage are applied together after the deed returns a result. Therefore the written value is not reflected in something fetched from the same data through another variable (which may be the same row), nor in a field written with `update` (the checker assumes nothing about those values either).
- `if` and `else`: `else` follows on the same line as the closing `}` (`} else {`, `} else if condition {`). `let` can be written inside `if`, `else` and `for` too. That name disappears when leaving the branch (different branches that do not overlap may use the same name. Re-binding a name that is visible cannot be written inside a branch either). Inside `else`, the check treats the condition as known not to hold (`if x.f > 98 { return Full } else { x.f += 1 }` passes). You cannot write the same field twice along the same path, but writing separately in paths that cannot be taken together, such as `if` and `else`, is fine. If both `if` and `else` end in `return`, the statements after cannot be reached, so it is an error (ending a deed with such an `if` is fine).
- Expressions: `==` `!=` `<` `<=` `>` `>=`, `and` `or` `not` (both sides and the operand are truth values; `not` binds strongest, then `and`, then `or`), `true` `false`, `+ - *`, a leading `-` (a negative number `-5` and sign reversal `-x`; the same as `0 - x`), `/` `%` (integer division and remainder; see the explanation below), `min(a, b, …)`, `max(a, b, …)`, `abs(x)` (integers, or amounts of money of the same unit; to clamp into a range, `max(min(x, 99), 0)`), `D[k]` (fetch by key), `x is null|absent|present|empty|nonempty`, `x in collection` (whether the id is in the set / is a key of the mapping), `len(x)` (the number of characters of a string, the count of a set or mapping; the check uses the type's length for strings and up to `max N` for mappings; a set has no upper limit that can be written, so when putting it in a numeric field, clamp it like `min(len(s), 99)`), `{ k | (k, v) in collection, condition }` (a set comprehension), `sum { expr | (k, v) in collection }`, `round(value, floor, currency)`, `now`, `tz_offset`.
- A `let` name cannot be re-bound. Writing `let` with a name that already exists (an input, slot or earlier `let`) is an error (there is no Rust-style shadowing; use another name). This is so that the range checked for the earlier name does not remain on the new value.
- Carrying over what has been checked: after `if m > 5 or m < 0 { return Bad }`, `m` is checked as 0 to 5. `not` flips the direction; on the path where `and` holds and the path where `or` does not hold, what is known on both sides is combined (nothing is drawn from the path where `or` holds or the path where `and` does not hold). For the conditional expression `if c then a else b` too, `a` has its range derived assuming `c` holds and `b` assuming it does not (`if m > 99 then 99 else m` is at most 99). Only comparisons of "a variable or field and a number" are looked at.
- Intermediate calculations: for the intermediate values of additions, subtractions and multiplications, the check shows from the value ranges that they fit the width used for calculation (i128). A calculation that has been shown is not checked for overflow at run time. If it cannot be shown, it is an error. When a person takes it on, write `trust level "reason"` at the end of a statement (`let`, field update, `create`, `return`). `prove` is the same as writing nothing (an error if it cannot be shown); `check` gives a warning in the check, and stops with a fault if it overflows at run time. `verify` checks only in verification builds, not in normal builds (an overflowed value becomes a wrapped value and is used as is). `assume` does not check. `check`, `verify` and `assume` appear under "human guarantees" in the specification. Writing it on a statement that has been shown is an error.
- Strings: they can be compared with a constant by `==` and `!=` (`if code == "A1" { … }`; ordering cannot be compared). A comparison whose constant does not fit the type's length or character set is a check error. Strings can be joined with `+`. The upper limit of the length is the sum of both sides, and it is a check error if it can exceed the length of the destination (there is no truncation). A number and a string cannot be joined with `+`.
- Sets: `add x to s.f` adds one and `remove x from s.f` removes one (`+=` and `<<` cannot be written). The target to add to is a required set with `max N`, and before adding you check the upper limit, like `if len(s.f) >= N { return Full }`.
- A deed always stops. The only repetitions are `for`, `update` and `sum`, which go around a collection with an upper limit on its count; there is no recursion and no `while`. Repetition cannot be written in the wrapping expression of a `library` (`call "…"`) either. The only place where non-termination is possible is inside the root-language library that a `library` calls (geas does not look inside; using an external library is itself the door for knowingly taking a risk).
- When the content of a result (`return Done(expr)`) is an integer or money, the check also shows that it fits the type's range (an error if it cannot be shown).
- Length of strings: if you do not write a length range for `text`, a value from outside is stopped at the boundary by the default upper limit (1,048,576 characters). To make it larger, write the length in the `ltype` (loosening it is the author's responsibility). The server refuses a request body over 16 MiB with 413. JSON nesting is up to 64 levels.
- Division `x / d` and remainder `x % d` (integers; truncation toward negative infinity, remainder is 0 or more): if `d` is a constant it is a positive integer. If `d` is a variable, it passes only when the checker can **show it is 1 or more** (the `ltype` range is 1 or more, or 0 has been excluded on the path forward, as with `if n == 0 { return … }` or `if n < 1 { return … }`). If it cannot be shown, write **the value for 0** as in `x / d or 0` (the part after `or` is an integer). With `or`, `d` may be 0 or negative, and the range of the result is the union of the quotient (or remainder) and the `or` value. The generated code uses `checked_div_euclid`, and an overflow such as `i64::MIN / -1` becomes `Fault::Overflow`.
- Range of `x.f += expr` and `x.f -= expr`: the check stops unless it can show that the value written fits the field's range (check as in `if x.f > 999900 { return Full }`). When a person takes it on even though it cannot be shown, write it **with a reason** at the end of the statement. `x.f += expr saturating "reason"` stops at the edge of the range (upper or lower limit). `x.f += expr trust assume "reason"` is a way of writing that knowingly accepts it may not be reached: the checker proceeds as if it fits the range, and if it is exceeded, the range check at run time makes it a fault (Fault) and it stops. Both are only for integer and money fields that cannot be null, and appear under "human guarantees" in the specification.
- Definition of storage `storage name isolation process` / `isolation store(kind) ["reason"]`: define a storage once and have it hold who guarantees the serialization of requests. An entity refers to it with `store name in storage` (a missing definition and an unused definition are both stopped by the check). `isolation process` means geas's server serializes only inside this app's process (the relation with other processes is not promised). `isolation store(kind)` means the storage serializes. The kind is an argument; kinds that the server runtime has verified in a contract test (for now `sqlite`) need no reason, and the others need a reason as a human guarantee. An entity without a `store` line uses the default storage (`sqlite`) and need not write it. `process` and kinds that have not been verified appear under "human guarantees" in the specification.
- Runtime requirements `needs`: write what the app requires of its runtime environment. The actual environment (how many instances there are now, which ports are open) is not written; it is read from a plan or the real environment and matched (`env-verify`. The verdicts are four: pass, fail, could not be verified, and human guarantee; a fact that could not be read does not count as a pass. Facts are in a cloud-independent form, and the part made from a plan is done by an adapter (`crates/geas-core/src/adapter.rs`; for now only aws-ecs)). The reason `"…"` is required and appears under "runtime requirements" in the specification. There are three kinds: `needs secret external.name "reason"` (only the name of a needed secret; the external is a namespace declared with `effect`, the name is lower_snake ASCII; the value is not written), `needs exposure public|internal "reason"` (whether it may be exposed to the internet; a warning if there is a resource and it is not written), `needs instances at_most N "reason"` (the upper limit of servers running at once. If the storage is `isolation process`, it is derived as 1, so a looser value is an error; only values stricter than the derived upper limit can be written). Durability of the storage, reachability of externals and the read timeout of the front are derived by the checker from the declarations, so they cannot be written. The list is `geas env-checks app.geas [--json]`, and the number of each item is the name of its source of derivation (`instances`, `durable.default`, `reach.Payment`, `secret.Payment.api_key`, `exposure`, `limits`).
- The container layer and the infrastructure layer `container` and `infra`: declare the configuration that runs the app in a container (only one of each; `infra` needs a `container`). `container { port 8080 / data "/var/lib/app" / secret external.name }` is the layer independent of the infrastructure: the port the server listens on, the container-side path of the volume where data is placed (the storage file is `data/app.sqlite`), and the secrets it receives (must match exactly, with no excess or shortage, those declared with `needs secret`; values are received through the environment variable `GEAS_SECRET_<external>_<name>`, and geas holds no values). `infra vm_docker "name" { host_port 443 via proxy|direct / volume host "/srv/…" / secrets_from env_file "/etc/…" / host_name "…" }` is the per-infrastructure layer (for now only vm_docker). `via proxy` means a front proxy receives on the host's port and the container's port is exposed only on the host's 127.0.0.1. `via direct` exposes it directly on the host's port (not compatible with `needs exposure internal`). `volume host` is the host-side path and is required. If the container receives secrets, `secrets_from env_file` is required. `host_name` is the name the user opens (the server accepts it as Host). Paths are absolute paths starting with `/`, and `..` and blanks cannot be used (written as strings). `geas container app.geas -o output-folder` (the output folder of `geas rust --host --runtime`) generates Dockerfile, compose.yaml, .dockerignore and secrets.env.example. Non-root, read-only root, all capabilities dropped, and the number of instances fixed to 1. The specification shows a "container and infrastructure" table and a configuration diagram (mermaid in the specification, SVG on the review screen; it draws, from the declarations, the user, the front proxy, the container inside the host, the data folder, the env file and the external systems; a dashed frame is a human guarantee). The host's OS, Docker, the front proxy (TLS, DNS), backups and the secret values are outside geas (human guarantees). Diagnosis is `geas doctor` (checks the declared ports, responses, data folder and env file on the running machine).
- A result is `result Name { kind(content: type), kind, … }`. **Every `return`, and the destinations of the flow, must cover the result's kinds without omission.**
- An external effect `request` requires an idempotency key (`idempotency_key`). Inside a `for`, make a tuple (`(order_id, k)`) of both the value being iterated and the value decided outside.
- Pure value parts: `convert name(x: type) -> type = expr` (a one-way conversion written as an expression), `library name(x: type) -> type from rust "crate" "version" call "Rust expression" [guarantee result >= x] trust check|verify|assume "reason"` (a pure function that calls a root-language library. The expression can use the argument names. For the standard library only, `from rust "std" ""`. The version is fixed and written out to `Cargo.toml`. The return value is checked on every call against the type's range and the `guarantee`. `verify` checks only in verification builds, `assume` does not check. The reason is required and appears under "human guarantees" in the specification. Even if the library panics it stops with a `Fault` and the server continues (an overflow in a calculation also becomes a panic, release builds included). Infinite loops and memory exhaustion cannot be prevented). The former `part` is abolished. The standard parts are a bundle of `library` declarations made mechanically from Rust's public API (rustdoc JSON) with `stdgen` (the generation tool in `tools` at the tag python-final): [crates/geas-core/assets/std/num.geas](../../crates/geas-core/assets/std/num.geas) (module `Num`; import with `use "…/num.geas"` and call as `Num.u8_wrapping_add(a, b)`). For now only the methods of integer types (i8 i16 i32 i64 u8 u16 u32) that can be said not to panic from their declarations (those with a Panics description, unsafe ones, ones with overflow checks and ones returning Option are excluded; ones returning bool have the bool type). The range is decided from Rust's type, and a value that cannot be shown to fit the argument's range cannot be passed. Only the ones called appear in the output and under "human guarantees" in the specification.
- The edge-value check of parts `geas parts file.geas`: it actually calls the `library` declarations with all combinations of values at the edges of the argument ranges (both ends and their neighbors, around 0, the boundaries of the bit width). It looks at five things: whether it builds with the current toolchain, whether it panics (called in a debug build, so calculation overflows are found), whether the return value fits the declared range, whether the `guarantee` holds, and whether two calls return the same value. Exit code 1 if anything disagrees. Only the parts that are called are looked at (`--all` for all; if you pass a `module` file, all of it). Parts whose arguments are not integers, money or booleans cannot be checked yet. It only tries edge values, so it is not a proof; it is a net for finding mistakes in declarations early. Run it before writing a reason in `trust`. With `--record`, the result is written to a ledger next to the code (`app.parts.json`), and the "human guarantees" row of the specification shows "the declaration was checked by N calls at the edge values". A part that has not been checked shows "the declaration has not been verified by actually calling", and after the declaration (type and range, the call expression, `guarantee`) has changed it shows "not re-verified", so the reviewer can tell them apart (commit the ledger together with the code). The ledger of the standard parts is bundled with geas (`crates/geas-core/assets/std/num.parts.json`), and those that Kani showed for all inputs appear as "the declaration was proved for all inputs" (`python proofs/std_proofs.py --record`). The standard parts are also compared with a separately written reference ([crates/geas-core/src/partrun.rs](../../crates/geas-core/src/partrun.rs)), and at generation time (`stdgen`) the ones that do not build are dropped, and nothing is written out if there is a disagreement.

## 5. How the value of a slot is decided (rule)

A slot decided by the server (tax rate, shipping fee, etc.) is written in a `rule`. From the top line down, the value is that of the first line that applies. The right side of a condition is a name, a string or a number; several conditions are joined with `and`. The last must always be `default:`. The value is `10/100` for a ratio, and a number for money or an integer.

```
rule shipping {
  currency == JPY and ship_to.country == "JP": 500
  currency == JPY: 2500
  default: 3000
}
```

The left side is an input of the deed (including components such as `ship_to.country`) or a slot decided earlier. A slot with no `rule` (such as currency) is passed from the caller (the screen). New IDs are numbered automatically by the generated code.

## 6. Dealing with the outside

```
effect Payment. charge(order_id: id, currency: currency, amount: money(currency)) replies PaymentSettled
effect Payment. refund(order_id: id, auth_id: id) no_wait "refund failures are looked at by a person"
effect Warehouse. hold(order_id: id, to: Address) joins OrderPending replies StockSettled

reply PaymentSettled(auth_id: id) {       // the content of a reply and its result kinds (the first is the normal one; used by the development mock)
  Approved gives auth_id
  Declined
}

trigger OrderPending(order_id: id) {
  all reply PaymentSettled, StockSettled → settle_order     // when all have arrived (a join)
  reply PaymentDeclined → handle_declined                   // any one
  deadline now + 30 min → expire_order                      // a deadline (required)
}
```

- The trailing clauses of an `effect` (`failures`, `no_wait`, `joins`, `replies`) are in any order. An external effect either waits for a reply (a trigger), or writes a reason with `no_wait`.
- For communication that actually goes outside, write `over http { … }` at the end of the `effect` (if not written, nothing actually goes outside and the development mock makes the reply).
  ```
  effect Twitter. post(tweet_id: id, content: Body) replies PostSettled over http {
    POST "https://api.x.com/2/tweets"              // method and destination. The destination is only https://host/path (no IP, user name or ?)
    bearer Twitter.token                           // authentication. Only a secret of the same external declared with needs secret Twitter.token
    body "text" from content                       // JSON body: key from argument (if the key and argument are the same, `body name`)
    status 201 → Posted take post_id from "data.id"  // tie the reply's status to a result kind of the reply. take picks content out of the reply's JSON by a path
    status 400..499 → Rejected
    status other → Rejected                        // the destination for statuses not written (required)
  }
  ```
  The destinations are only those written here, and redirects are not followed. Being unable to connect, a timeout and a missing secret do not become a reply; the deadline of the trigger being waited on is where it goes. The same request is sent only once (no resending). Secret values are passed through the environment variable `GEAS_SECRET_<external>_<name>` (uppercase) or a `--secrets` file, and are written neither in the specification nor in the output. The specification shows a table of "communication going outside" (destination, authentication, body, how statuses are tied). The communication parts are included only in apps that write `over http`.
- `joins trigger_name`: put the reply into that trigger as its reply. `replies reply_name`: the reply that comes back.
- The content of a reply that occurred is added to the trigger's arguments and becomes the input of the destination deed (an error if the names and types do not match).

## 7. Flows and events

```
nexus Intake {                                   // flow is an alias
  place_order: Received → par { request_payment, request_stock_hold }, OutOfStock → reply, NoSuchCustomer → reply
  request_payment: Requested → wait OrderPending, NoSuchOrder → reply
}
event Reconnected on online {                    // on is online (the connection came back) or opened (the app was opened)
  each awaiting_payment → recheck_payment        // start a deed for each row of a query's result (there is also external → deed)
}
```

- Write a destination for each result kind: the next deed, `reply` (return and finish), `wait trigger_name`, `par { A, B }` (all of them happen).
- The branches of a `par` must not overlap in the fields they write, must not cross in reads and writes, and must not move the same state (an overlap of only `+=` and `-=` is allowed). Breaking this is an error.
- To the next deed, the content of the result is passed to its inputs by name and type.

## 8. Queries

```
query orders_of {
  input customer: id required
  from Order
  where status != Canceled or status == Received      // lines are or'd; the conditions across lines are and'd
  filter customer equals customer                     // equals / at_least / at_most / contains (ignore_case etc.) / within 1 hour (needs slot current_time: time)
  order created_at desc                               // desc (largest first; newest first for datetimes) | asc (smallest / oldest first). With two or more lines, the caller can choose with q[order]=field.direction (the first line is the default). The screen's list shows a field to choose the order
  page 20                                             // order is required. For the rest, follow the answer's next (the next page's URL; q[after]=mark)
  select order_id, customer.customer_id as customer, sum(items) as item_count
  reads Order.customer, Order.order_id, Order.items
}
```

When there is no input for narrowing, it returns without narrowing. If there is no `select`, it returns only the column of IDs.

## 9. Screens (screen)

```
screen OrderForm(product_id: id) {
  field customer_id from customer_list, items from product_list, ship_to    // input fields (from is the query to choose from)
  slot currency                                                              // a slot the user chooses
  show orders_of                                                             // the query to show
  open MyOrders                                                              // where to go when a row is chosen
  action place_order {                                                       // an operation, and the destination for each result (stay / back / screen name)
    Received → MyOrders
    OutOfStock → stay
  }
  design { layout form  accent "#0b5cad"  mode light }
}
```

- The entry is the screen declared first, or the screen with `entry`. The URL of a screen is decided from the resource assignment, and the check looks for omissions and duplicates.
- The end-to-end tests of screens are generated from the plan's "use case steps" and "screen examples" ([ai-guide.md](ai-guide.md)). The names of the screen's parts (fields, rows, deed buttons) are decided from the names in this declaration.
- **A browser-extension screen** has `page "URL pattern", …` and `read { … }`, and only shows one query that has one input.

```
screen PriceCheck entry {
  page "*://www.amazon.co.jp/*"
  read {
    subject ASIN url "/(?:dp|gp/product)/([A-Z0-9]{10})(?:[/?]|$)"   // the first ( ) is the value
    subject ASIN element "input#ASIN" matches "[A-Z0-9]{10}"          // the value of an element
    value row "JAN", "EAN", "GTIN"                                    // read digits from a table row / list item containing the heading word
    value lookup "B0SHOPTEA1": "4901234567894"                        // a lookup table consulted when it cannot be read
  }
  field jan
  show price_by_jan
}
```

### File operations (over file)

Writing `over file { … }` at the end of an `effect` makes the external effect actually touch a file (as with `over http`, if not written, it does not actually touch anything and the development mock makes the reply).

```
ltype ExportDir = path                                   // the location is a path type entered at run time (a value whose OS format has been checked)
effect Files. export(note_id: id, dir: ExportDir, content: Body) replies ExportSettled over file {
  write "{dir}/note.txt" from content                    // one operation: write, overwrite, read, delete
  ok → Done take size from bytes                         // tie the result to a result kind of the reply
  exists → Refused
  not_found → Refused
  denied → Refused
  failed → Refused                                       // failures not written become this (required)
}
```

- What it can touch is only **one file with the written relative path, under the location a path-type argument points to**. Write it in the form `{argument}/file-name`, where the file name is a relative path separated by `/` (empty segments, `.`, `..`, `\`, `:`, `*`, `?`, `"`, `<`, `>`, `|` and control characters cannot be used; up to 200 characters).
- Operations:
  - `write "…" from content-argument`: only **creates anew**. If it already exists, nothing is written and the result is `exists`.
  - `overwrite "…" from content-argument`: creates or replaces. It writes to a temporary file in the same folder and then renames it, so even if it stops midway the original file is never left half-written. If the original is a link, the link itself is replaced (the link target is not rewritten).
  - `read "…"`: reads (`take name from content` puts the content into the reply).
  - `delete "…"`: deletes only an existing file (or link). Folders are not deleted.
- The results are `ok`, `exists` (only for write), `not_found` (the file, or a folder under the location, does not exist), `denied` (no permission) and `failed` (anything else, such as too large, over 1 MiB, or it was a folder). `ok` and `failed` are required. If a result kind has `gives`, take it out with `take name from content|bytes`. `content` is for read (a string); `bytes` is the number of bytes (an integer; cannot be taken for delete).
- The location is expanded again, just before use, in the format of the running OS (even if environment variables have changed, it does not silently go to another place). Writing and reading are up to 1 MiB.
- The reply reaches the trigger being waited on, as with `over http`. It is done after the commit, and is not done for a deed that was rolled back. The record of the result keeps only the result kind, and does not carry paths or file contents.
- The specification and the review screen show a table of "doors that touch files". Under "runtime requirements", it appears that permission to touch that location is needed.

### Path values (path) and the data folder (data_folder)

Declare the folder where the windowed exe places the DB (`app.sqlite`), the log (`log.txt`), the secrets (`secrets.json`) and the startup marker. If not written, it is a folder named after the app under the user's data folder. The folder's path is written as a named "path value" and chosen with `data_folder`.

```
path DataRoot = absolute {
  windows "%APPDATA%\\TweetAlone"                             // an absolute path in that OS's notation (write \ in a geas string as \\)
  macos   "~/Library/Application Support/TweetAlone"
  linux   "$XDG_DATA_HOME/tweet"                              // $name, ${name}, and a leading ~
}
path Portable = relative "data"                               // a relative path: independent of the OS. The origin is the folder where the exe is (for carrying around)
needs env XDG_DATA_HOME "the location is decided by the environment"   // an environment variable that is not guaranteed to exist is declared with needs env
data_folder DataRoot
```

- A line inside `absolute` is only an absolute path in that OS's notation. On Windows, `%name%` and `C:\\…`; on mac and Linux, `$name`, `${name}`, `~` and `/…`. A wrong notation is stopped by the check. Lines for the same OS cannot be repeated.
- `relative` is only a relative path (separated by `/`, no `..`). The origin is the folder where the exe is.
- For environment variables, only names that always exist on that OS (Windows: APPDATA, LOCALAPPDATA, USERPROFILE, TEMP, etc.; mac and Linux: HOME, USER; mac also TMPDIR) or names declared with `needs env name "reason"` can be used.
- For the OS being built, every `absolute` needs a line for that OS. Check with `geas rust … --standalone --target windows|macos|linux` (if omitted, the OS now running).
- At startup it checks that the environment variables are set and that the folder can be created and written; if not, it does not escape to another place but stops with an error (on Windows it reports in a message window).
- `--db` is an override that puts only the DB in another place (for testing).
- It appears in the specification's "data folder" table and under "runtime requirements".
- A path that a user enters at run time is a different type from the path values declared here, written `ltype ExportDir = path` (the language adds a conversion `export_dir_os_path` from raw. It can be used for inputs and fields).
  - What it accepts is **only the format of the running OS**. On Windows, an absolute path (`C:\…`, `\\server\…`); characters that cannot be used (`< > " | ? *`, and `:` other than in the drive) are invalid input. On mac and Linux, an absolute path starting with `/`.
  - What it expands is only the OS's "aliases for locations". On Windows `%name%`; on mac and Linux a leading `~` (only your own home; `~name` cannot be used). `$name` is not expanded and is treated as a literal path name (it is not an absolute path, so it is invalid input). An undefined environment variable is invalid input.
  - A newline, NUL, `..`, and a length over 4096 characters are invalid input. On invalid input the deed does not run and storage does not change.
  - The stored value is the input as it is. What touches a file with the expanded value is the file operation of an external effect (`over file`), which expands again just before use.
  - `os_path(r)` can be used only inside this conversion (in other `convert`s, it is among the operations usable on raw, like `only`).
- The entrance that chooses the storage location at startup is decided outside geas (the app's design, such as recording next to the exe). If the place next to the exe cannot be written, it does not escape to another place but makes an error.

### Resident (resident)

Makes the windowed exe (`--standalone`) an app that stays in the task tray (the menu bar on macOS). If not written, the app ends when the window is closed.

```
resident {
  tooltip "TweetAlone"                      // the description shown on the icon (the app name if omitted; up to 64 characters)
  start hidden                              // shown if omitted (starts with the window open). hidden starts with only the icon
  click toggle                              // what a left click does. toggle (default; opens / closes the window) or menu (shows the same menu as a right click)
  menu "Write a tweet" → TweetList          // an item of the right-click menu. Opens an entry screen (entry or the first screen; without arguments)
  separator                                 // a separator line (cannot be placed first, last or consecutively)
  menu "Post all unposted to X" → fire PostSaved   // an item that fires an event. No window appears. Limited to events with only each (those with no external input)
}
```

- Closing the window does not end the app; it hides the window. Clicking the icon (Windows, macOS) shows and hides the window. The Linux icon does not notify clicks, so it is shown with "Open" in the menu.
- The right-click menu is, in order: "Open", a separator, the items written (in the order written), a separator, "Quit". "Open" and "Quit" are always added by the language (the same label cannot be written). An item's label is 1 to 40 characters and cannot be repeated.
- A `fire` item fires an event once from outside the window, and leaves the number of occurrences, or the failure, as one line of JSON (`geas.resident`) on standard error.
- Only one can be started with the same storage (`--db`). The second says it is already running and ends.
- In an environment where a tray icon cannot be made, it does not become resident; it keeps the window shown and ends when closed (to avoid becoming hidden and unopenable).
- The specification and the review screen show a "resident" table. The resident part (tray-icon) is included only in apps that write `resident`.

## 10. REST (resource)

```
resource Products {                      // the name decides /products
  index product_list                     // GET /products
  update restock_product {               // PUT /products/:id
    Restocked → 200
    NoSuchProduct → 404
  }
  destroy withdraw_product {             // DELETE /products/:id
    Withdrawn → 200
    NoSuchProduct → 404
  }
}
resource CustomerOrders under Customers { index orders_of }           // /customers/:customer_id/orders
resource ProductPrices key jan { show price_by_jan }                  // /product_prices/:jan
resource OrderSearches { index order_search }                         // /order_searches?q[from_time]=…&q[to_time?]=…
resource CustomerSearches { index customer_search post "Searched by name, so it must not stay in the URL or the log" }   // POST /customer_searches (body {"q": {…}})
resource Reconnections { create Reconnected }                         // a create that calls an event has no result
```

- What goes in the **path** is only the value that decides which resource (an ID, a parent's ID, one key of `key name`; the key is a required input of the `show` query). The inputs of a query that do not go in the path are passed as **narrowing conditions** in the query string as `q[name]=value` (no new syntax is needed; inputs that are not `required` can be omitted). A name not in the declaration, a form other than `q[name]`, a duplicate name, and a query string on a URL that takes no conditions are 400. The types that can go in a URL are ID, string, state name, integer, money and datetime, and numbers are written as decimal digits (for datetimes, the same number as in the JSON body). The reasoning is in [rest.md](rest.md).
- **POST for a stated reason**: writing `index query post "reason"` accepts a query that is by nature GET as POST (when the conditions cannot go in the URL). The conditions are the body's `{"q": {…}}`, the answer is 200 and the result, and the conditions are not stored. The reason is required and appears in the specification's "queries accepted by POST" table. Only `index` can be written, and the same resource cannot have a `create`. 304 and `next` are not provided; to read the continuation, put the answer's `after` into `q.after`.
- The only actions are `index` `show` `create` `update` `destroy` (`new` and `edit` appear in screen URLs). No PATCH. A command that advances a state is also made a noun resource and expressed with `create` (`OrderSettlements`, etc.).
- For `create` / `update` / `destroy` you must write the **HTTP status for each result kind** (`Received → 201`). An omission is an error.
- Every deed and query must be assigned to some resource (a warning if not).
- The URLs that call **mid-flow deeds** (destinations of a nexus, branches of a par, destinations of a trigger's replies and deadline) are "internal". If called directly from outside, they would let you skip checks such as a payment reply and move ahead, so the generated server does not answer (it answers only when started with `--test`). The specification's URL table shows "internal (mid-flow)" in the "from outside" column, and they do not become MCP tools. The way of writing does not change; which ones are internal is decided from the flow. The deeds started by an event (`event`) and the first deed of a flow are "callable".
- Mid-flow deeds cannot be used for a screen operation (`action`) (an error). From a screen, call the first deed of the flow.

### Distinguishing who may call (actor and for)

```
/// Customer
actor Customer                            // a role. The display name is the description comment (///)
/// Shop staff
actor Staff

resource Products {
  index product_list for anyone           // an entry anyone can call is written for anyone
  update restock_product for Staff {      // only shop staff
    Restocked → 200
    NoSuchProduct → 404
  }
}
resource Orders { create place_order for Customer, Staff { Received → 201 } }   // several are listed with commas
```

- Once even one `actor` is declared, every externally callable action needs `for` (an error if missing, so that forgetting to write it does not create an entry anyone can call). In an app with no `actor`, `for` cannot be written, and, as before, anyone can call.
- Mid-flow deeds ("internal" URLs) cannot be called from outside, so `for` cannot be written on them.
- **Authenticating the person (login) is outside geas.** A proxy placed in front verifies the person and puts the role's name (the code name, `Customer`) in the header `X-Geas-Actor` (comma-separated for a person with several roles). The generated server only compares this header's value with `for`.
- A call without the header is 401, and a call with a mismatched role is 403, and the deed does not run. These two are decided by the language and are not written among the statuses per result kind.
- The specification's URL table shows a "who can call" column, along with a table of "callable URLs" per role. Under "runtime requirements" appears the human guarantee (`actor`) that the proxy discards any `X-Geas-Actor` that came from outside and puts the role it verified. A change that widens who may call appears under "weakened" in the receipt.
- An app with roles cannot become a windowed exe (`--standalone`, `resident`) (there is no proxy in front).

### Scope: a named set of rows

```
scope own_orders on Order {
  filter customer equals caller            // caller is the ID of the person who called
}

scope unpaid on Order {
  where status in [PaymentRequested, Paid] // the same form as a query's where (lines are and'd, within a line or'd). in [candidate, …] means "equal to any of them"
}

scope since(from_time: time) on Order {    // a scope with an argument. The filter that narrows by an argument is equals, at_least or at_most
  filter ordered_at at_least from_time
}

query my_unpaid_orders {
  input from_time: time required
  from Order.own_orders.unpaid.since(from_time)   // only rows that are in all the scopes listed (an intersection). The order does not affect the result
  select order_id, status, total
  reads Order.customer, Order.order_id, Order.status, Order.total   // also write the fields the scopes read (customer, status)
}

resource MyUnpaidOrders {
  index my_unpaid_orders for Customer      // the customer's ID is not needed in the URL (/my_unpaid_orders)
}
```

- `scope name on data { … }` gives a name to a set of the data's rows. What can be written inside is `where` (comparison with states and numbers, the same as in a query) and `filter field equals caller` (at most one; the field is an ID or a `ref` reference).
- A scope with arguments (`scope name(arg: type, …) on data { filter field equals arg }`) is used by the user as `data.scope(input, …)`, passing the query's inputs. The inputs passed are `required`, and their number and types must match the arguments. Each argument is used in some filter.
- `where field in [value, value]` has the same meaning as `where field == value or field == value` (it can also be written in a query's where). The candidates are written directly here. Inputs cannot be written (the caller is not made to choose the candidates; a set of candidates is given a scope and a resource name). `field == [value, value]` and `field = [value, value]` are errors.
- To use one, write `from data.scope`; lining them up as `from data.scope1.scope2` gives only **the rows that are in all the scopes**. There is no union or difference (when needed, split the resource). `from data in scope` is not written (an error).
- A scope's name cannot overlap with the names of that data's fields.
- `caller` is not a value the caller passes (it cannot be used as an input name either). It compares a **field of the stored row** with the value the proxy put in the header `X-Geas-Actor-Id`. If the header is absent, it is 401.
- A query that includes even one scope using `caller` needs a role (`for role`) on the action that calls it. `for anyone` is not allowed. It cannot be used in an event's `each` either (there is no person who called). Scopes that do not use `caller` can be used anywhere.
- The specification shows a "scope" table, and in a query's conditions the scope's conditions are listed with "(scope name)". For a query narrowed by the person, the "who can call" in the URL table is "Customer (only their own)".
- A deed can also pass through a scope when it fetches one row by ID: `let o = Order.own_orders.unpaid[order_id] else { return NoOrder }`. **A row outside the scope is treated as "absent"** and goes to `else` (even if you pass the ID of someone else's order, the answer is the same as for an order that does not exist). Only this `let … else` form can be written. The fields the scope compares go in the deed's `reads`. A deed that passes through a scope using `caller` needs `for role` on the resource's action (`for anyone` is not allowed), and cannot be a mid-flow deed or a deed started by an event. A scope with arguments cannot be used in deeds yet.
- Not yet possible: dividing scopes per role (whichever role calls, the same ID is compared).

## 11. Kinds of check results

`geas check` outputs errors, warnings and questions (QUESTION). **Errors** are fixed. **Questions** are things the checker found not yet decided, so ask a person and decide. **Warnings** are things such as missing display names. If there are no errors and no questions, `rust` and `compare` proceed.

- Names in findings are as written in the code (`Product.stock`), and where known a line number is attached (`(line 30)`). When they appear in the specification (`spec`, `review`), they are replaced with display names.
- A name that looks like a typo gets `(similar name: stock)`, and a type name or value from another language (`string`, `True`, etc.) gets the way to write it in geas.
- The last line is the count (`Check: there are errors (1 error, 0 questions, 2 warnings)`). If questions remain, even when it passes it says "ask a person and decide".
- A syntax error shows the line and column, an excerpt of that line and a position marker (`^`), and, when it looks like another language's syntax (`&&`, `elif`, `;`, etc.), continues with how to fix it. If the pairing of `{` and `}` is broken, it also shows the line of the `{` that is not closed.
- With `--json`, each finding gets `line` (and `col` for syntax errors) and a `hint` with the section to read and how to fix.

## 12. Mistakes easy to make when coming from other languages

geas resembles Rust and TypeScript, but the following differ. The checker makes each of them an error and shows how to fix it, so nothing silently takes on another meaning.

| Other languages | geas |
|---|---|
| `a && b`, `a \|\| b`, `!a` | `a and b`, `a or b`, `not a` |
| `elif`, `elsif`, `match`, `switch` | `if condition { … } else if condition { … } else { … }` (`else` follows on the same line as `}`). To only choose a value, the expression `if condition then expr else expr` |
| `x == null`, `x != null` | `x is null`, `not (x is null)` (for the presence of a row, `D[k] is absent`, `D[k] is present`) |
| A trailing `;` | A newline (a newline inside `( )` continues the expression) |
| `=>` | `→` or `->` |
| `x++`, `x--` | `name.field += 1` (only fields can be added to or subtracted from; variables are not re-made) |
| `let mut x`, `var x`, `const x` | `let x = expr` |
| `string`, `boolean`, `uuid`, `datetime`, `i32` | `text`, `bool`, `id`, `time`, `int` (a range is written by giving it a name, `ltype Quantity = int 1..99`) |
| `float`, `double`, `decimal`, `0.5` | There is no decimal type and no decimal number. Money is `money(JPY)` (an integer in the smallest unit), a ratio is `ratio` (a constant is `10/100` as a rule's value) |
| Full-width digits `９９` | ASCII `99` |
| A raw newline inside a string, `"""…"""`, `` `…` `` | Write `\` at the end of the line to continue onto the next line (leading blanks on the next line are skipped). A newline itself is `\n` |
| Regular expression `"\d+"` (one `\`) | `"\\d+"` (`\` itself is `\\`; what can follow `\` is only `n`, `t`, `"`, `\` and the end of a line) |
| `text?`, `Option<T>` | Nothing is attached (fields and inputs, if not written, are treated as possibly absent). Add `required` to what is always present |
| `type X = …`, `enum`, `struct`, `class`, `fn` | `ltype`, `state` / `result`, `entity` (a group of values is a composite `ltype`), `deed` / `convert` |
| `import` | `use "file.geas"` |
| `True`, `False`, `None`, `'x'` | `true`, `false`, `null`, `"x"` |
| `return Done()`, `return` | `return Done` (always return a result kind) |
| `while`, `throw`, `try` | They do not exist. Repetition is `for element in collection`; failure is `return` of a result kind |
| `/* … */` | `//` (`///` is a display name) |
| `where a and b`, `where a, b` | Split the `where` lines (lines are and'd, within a line is `or`) |
| `ORDER BY x DESC`, `order by x desc` | `order x desc` (no `by`; the direction is lowercase `desc`, `asc`) |
| `filter x >= y` | `filter x at_least y` (`equals`, `at_least`, `at_most`, `contains`, `within`) |
| `limit 20` | `page 20` (needs an `order`) |
| `get`, `post`, `put`, `patch`, `delete` of a resource | `index` / `show`, `create`, `update`, (there is no PATCH), `destroy` |
| A constant outside a definition | A range is an `ltype`; a value the server decides is a `slot` and a `rule` |
