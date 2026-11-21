# The ideas behind the language (a reading guide)

> This page is a translation of the Japanese original (the Japanese text is the source of truth).

The list of syntax is [language-reference.md](language-reference.md). Here the ideas and examples are given in the form of a reading guide (if they disagree, the checker is the source of truth). Names are ASCII, and Japanese display names are attached with `///`.

The naming rules are in chapter 1 of [language-reference.md](language-reference.md). The examples below use illustrative names (the Japanese original uses Japanese names for explanation, which would not pass the real check). The shop example is `tests/fixtures/shop/shop.geas`.

```
ltype Quantity = int 1..99                                    // a logical type: a range or a combination must always be given a name before use (write it before the place that uses it)
ltype ProductQuantity = Product → Quantity
ltype Point { x: Quantity, y: Quantity, z: Quantity optional }                                // a composite type: components are value types (a named logical type or an unconstrained built-in). Build with Point { x: 1, y: 2 }, read with position.x (flat even when nested). An optional component can be left unspecified, and a path that may access an unspecified one is an error. The built-in vtime (year month day hour minute second, all optional; the virtual datetime) is also this kind of type
entity Order { order_id: id unique, status: OrderStatus ... }      // built-in types: id text time bytes int ratio money(JPY) ref X (ranges and combinations are ltype)
store Order in ExternalDb                                      // the storage is written outside the entity (not written if there is no requirement to store). For what is placed outside, the language adds the unsynchronized state "recorded"
deed request_payment {
  input order_id: id required                             // declarations: input slot reads writes transition result effects
  reads Order.status
  ...
  let order = Order[order_id] else { return NoSuchOrder }        // the body is Rust-like (no body keyword, no then)
  if order.status != Received { return WrongState }
  for k in order.lines { request Payment.charge(k, idempotency_key = (order_id, k)) }
  return Requested
}
nexus OrderIntake { place_order: Received → request_payment, NoSuchCustomer → reply, ... }   // flow is an alias of nexus. A destination is reply or wait trigger_name
trigger PaymentPending(order_id: id) { reply PaymentSucceeded → ... , deadline now + 30 min → ... }
```

Fields of data are, in principle, all treated as "possibly absent" (they may be null). A promise that something is always present is written with `required` and appears as "required" in the specification (keys and state fields are required). A field that may be null is checked before use with `let title = memo.title else { return NoTitle }` or `if memo.title is null { return NoTitle }` (using it without checking is stopped by the check, so generated code does not break on null). In a new creation, a field not written becomes null, and `memo.title = null` returns it to null. A row or field itself being absent (undefined; checked with `D[k] is absent`) is different from a value being null. In queries, null satisfies no condition or filter, and comes last in ordering.

Inputs of deeds and queries are the same: if not written they are "possibly absent" inputs. Write `required` on an input that is always needed (`input order_id: id required`); the specification's input field shows "(required)" and it is matched against the plan. An input that may be absent is checked before use with `let x = input else { return … }` or `if input is null { return … }` (if it is merely put into a field that may be null, no check is needed). When receiving from outside, a missing key and null are treated the same as "absent", and an empty string is "present". To tell an empty string from no input, reject it as invalid at the input boundary before the deed. A required input that is absent or null is invalid input. A `trigger`'s arguments can only be passed required inputs.

Integers and money are different types and cannot be mixed as they are. What is allowed is a calculation between the same types, money × integer (by quantity), and money × ratio (a fraction is made an integer with `round(value, floor|ceil|half_up, JPY)`). To turn one into the other type, change it with `money(count)` or `int(amount)` before calculating. A number written as it is (such as `0`) can be used as either type.

A name is given to the content of a result (`result PlaceOrderResult { Received(order_id: id), OutOfStock(short: IdSet), NoSuchCustomer }`). When moving to the next deed in a nexus, the content of the result is passed to the next deed's inputs by name and type. An excess, a shortage or a difference in type is an error (the same rule as a `trigger`'s arguments).

Root-language (Rust) libraries are called with `library` (`library tax_included(x: Yen) -> Yen from rust "std" "" call "x + x / 10" guarantee result >= x trust check "integer arithmetic only"`). On the lines after the declaration, write at least one example (`example tax_included(100) == 110`; `geas parts` actually calls and compares). Repetition (loop, while, for, closures) cannot be written in the `call` expression. A declaration with neither a range for the return value nor a guarantee, a guarantee that does not use the return value (`result`), and a `trust` without a reason are check errors. A Rust `if` that checks the return value's range and guarantee is generated for every call, and when violated (including a library panic) the deed is stopped (writes are not applied and storage does not change). What is called appears under "human guarantees" in the specification. The types of arguments and return values (money or integer) are checked by the check. Built-in parts such as `round` are not declared.

A one-way conversion between logical types is written as an expression: `convert with_tax(x: NonNegativeMoney) -> NonNegativeMoney = x + x`. Since the definition is a geas expression, no outside implementation or guarantee is needed. That the result fits the return value's range is shown statically by the checker from the argument's range, and if it cannot be shown, it is a check error (it is not checked at run time). It only computes the expression inside the generated code (conversion from `raw`, for values received from outside, comes next).

An external effect (`request`) requires an idempotency key (`idempotency_key`), written as an ID or a string, or a pair `(order_id, k)`. Inside a `for`, include in the pair both the value that differs on every iteration and a value decided outside the `for` (with only the iterated value, the key would collide when the same product is iterated in another order, and the outside service would discard it as a duplicate). The pair is passed outside as one string in a form whose delimiters are not ambiguous (`["O1","P1"]`).

Inside an `if` you can write `return`, a field update, `update`, `request` and a nested `if` (`let` is written outside). The check follows, for each statement that changes a state, "whether the original state is checked in that branch".

A string `text` can have a length (number of characters) and the characters allowed (`text 1..20 of "a-z0-9_"`; both can be omitted, and what is omitted is unrestricted). After `of` come ranges joined by `-` and characters escaped with `\\`. Inputs and slots are checked at the boundary, and out of range is invalid input. When writing to a field, it is an error unless the check can show that the length and characters of the value fit the field (it can show this when an input, field, slot or part return value with a declaration is written as it is). Regular expressions for formats (such as email addresses) are not included. For JAN, EAN and UPC, using the type `Gtin.Gtin13`, etc. of the standard module `Gtin` (bundled with geas; write `use "gtin.geas"`) checks even the last check digit at the boundary, and a string that has not been checked can no longer be written (the way of checking is written in geas expressions and appears in the specification). Formats with structure are handled in the design of input and output types.

A time `time` can have durations (`30 min`, `24 hour`, `10 sec`) added and subtracted (`now + 24 hour`), and the subtraction of two times is a number of seconds (an integer). A duration is a value only for adding to or subtracting from a time, and cannot be put in a field. When people say "one day later", it is not necessarily 86,400 seconds later (with daylight saving time and time zones, the length of a calendar day changes), so `day`, `week`, `month` and `year` cannot be written (an error). An exact length of time is written like `24 hour`. Calendar calculation (adding months, weekdays, time zones, daylight saving time, holidays) will be introduced after being designed as logical datetimes. How a time is represented (in Rust now an integer number of seconds) and its range are, like physical types, the generator's choice and are not written in the language. In the current Rust generation, the absolute value of a time coming from outside is checked at the boundary up to 10^15 seconds (so that additions and subtractions do not overflow).

Items in the input from outside that are not in the declaration are treated as not having come (neither an error nor a warning). The party that is made to keep the promise is the person building with geas, and an item not in the declaration is one that person has not decided to accept. A misspelling of an input appears as the declared item being "absent" (as "absent" if it may be absent, and as invalid input if it is required).

When the inputs used for a query's `filter` and `page` are absent, it returns without narrowing (for `page`, all rows). This behavior appears in the specification's condition column, like "(if there is no minimum, it does not narrow)".

`page 20` or `page count` (an integer input) means "one page in this order, at most N rows", and is not the name of a mechanism like SQL's limit. Without a determined order, which page is meant is not decided, so `order` is required (an error if absent). What is used in generated code is the generator's job and is not exposed in the language. How to take the following pages will be decided together with the design of input and output types.

A `query`'s `where` is a set joined by `or` within a line, with lines joined by `and` (`where status == Published or status == Draft`, `where weight >= 10`).

Writing `select order_id, customer.customer_id as customer, sum(lines) as item_count` in a `query` returns rows with columns instead of a column of IDs (a field beyond a reference is a join; `sum` and `count` are aggregations over a keyed collection).

For string search, `filter body contains word` is an exact substring match, and the axes treated as the same are only those written (`ignore_case` = upper/lower case, `ignore_kana` = hiragana and katakana, `ignore_width` = full-width and half-width alphanumerics and symbols). Normalization (NFC) is assumed to be finished at the input boundary and is not brought into search.

What can be written in an `invariant` is the forms "field >= number" and "state field == state implies field > number". On every write to a field, the check shows on the spot from the value ranges that the promise is kept, and makes it an error if it cannot show that (the range of a field's logical type, such as `ltype StockCount = int 0..`, is shown in the same way). The value range is known from the ranges of inputs and fields, checks such as `if x.field <= 0 { return … }`, and a shortage check before decreasing.

A query's `filter field equals input` is an exact match for strings, IDs and integers (when fetching one row by a field other than the key; for example, looking up a price from a JAN code).

Besides these there are `state` (initial middle final endless reject), `result`, `effect` (failures no_wait), `library`, `invariant` (implies), `query` and `event`. A whole example is [tests/fixtures/shop/shop.geas](../../tests/fixtures/shop/shop.geas).
