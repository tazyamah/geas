# How geas thinks about REST

geas gives a URL to everything that can be touched from the outside. The shape of the URL is decided by the language; the author cannot choose it. This page collects the rules and the reasons. For syntax details see chapter 10 of [language-reference.md](../language-reference.md) (Japanese); for how the server behaves see [server.md](../server.md) (Japanese).

> These English pages are translations of the Japanese originals (the Japanese text is the source of truth).

## 1. Why fix the shape

geas aims at letting a person review using only the specification, without reading code. If there were freedom in how URLs are named, you would have to decode "what does this URL do?" one at a time. If the shape is fixed to one form, the URL table alone tells you what is created, what is read and what changes.

## 2. The rules

### Resources are nouns; there are five actions

| Action | Method and URL | Meaning |
|---|---|---|
| `index` | `GET /orders` | Read the collection |
| `show` | `GET /orders/:id` | Read one |
| `create` | `POST /orders` | Create a new one |
| `update` | `PUT /orders/:id` | Replace one |
| `destroy` | `DELETE /orders/:id` | Delete one |

- No other actions can be made. There are no exceptions (`new` and `edit` appear only in screen URLs).
- **There is no PATCH.** To change only a part, split that part out into another resource.
- A command that advances a state is also turned into a noun resource and `create`d (settle payment → `POST /order_settlements`). Verbs are not hung under an order.
- If you need to branch to different destinations, make that a separate resource. One action calls one thing.
- Nesting is only for children of a parent that has an ID (`/customers/:customer_id/orders`).

### POST only when a URL is added (one exception)

POST is the creation of a resource. That is, it is used only when a URL (or something managed like a URL) is added. If it only reads, it is GET.

There is exactly one exception. **A query that is by nature GET can be accepted as POST for a stated reason.** This is for when the conditions cannot go in the URL (too long, or like a name or phone number that should not stay in URLs and logs).

```
resource CustomerSearches {
  index customer_search for Staff post "Searched by name and phone number, so they must not stay in the URL or the log"
}
// POST /customer_searches   body {"q": {"name": "…", "phone": "…"}}   the answer is 200 and the result
```

- The reason is mandatory. The specification shows a table of "queries accepted by POST", so that it is an exception and why are visible to people.
- Conditions go in the same single object `q` as for GET (kept as JSON in the body; numbers are written as numbers). No key other than `q` can be written in the body.
- Conditions are not stored. No URL is added, and the answer is returned on the spot.
- The 304 by version (ETag) and the next page's URL (`next`) are not provided. To read the continuation, put the answer's `after` into the body's `q.after`.
- Only `index` can be written, and that resource cannot have a `create` (it would be the same `POST /customer_searches`). Make a separate resource for searching.

### Cut resources finely, and give a name to a set of conditions

Fixed narrowing such as "unpaid orders" or "my orders" is not made by having the caller pass conditions; it is a named resource (`GET /unpaid_orders`). A set of rows is named by a range (`scope`), and the candidates are written directly in the `.geas` (`where status in [PaymentRequested, Paid]`).

Because resources are cut finely, unions and differences of sets, or narrowing where the caller chooses the set of candidates, are expected to be rare. They are not in the language either.

### The path is "which resource"; the query string is "the narrowing conditions"

- What goes in the **path** is only the value that decides which resource: an ID, a parent's ID, one key (`/product_prices/:jan`).
- **Narrowing conditions** are passed in the query string. The only form is `q[name]=value`.

```
GET /order_searches?q[from_time]=1763251200&q[to_time]=1763337600
```

Lining conditions up in the path (`/orders_in_periods/:from_time/:to_time`) is not adopted, because without knowing the URL's specification you cannot tell which value is which. With `q[name]`, the name appears in the URL, the order has no meaning, and a condition you do not give is simply not written.

Writing query strings side by side (`from_time=…&to_time=…`) is not adopted either. The conditions are always gathered into one object `q`. That makes it visible from the look of the URL that they are conditions, and nothing that is not a condition can slip in.

The server rejects all of the following with 400.

- A name not in the declaration (`q[other]=1`)
- A form other than `q[name]` (`from_time=…`, `q=…`, a fragment without a value)
- A duplicate name
- A missing required condition
- A numeric condition that is not decimal digits
- A query string on a URL that takes no conditions (including URLs that call a deed)

There is no new syntax in `.geas`. The inputs of a query that do not go in the path become the conditions as they are. A `required` input is always needed; other inputs can be omitted.

```
query order_search {
  input from_time: time required
  input to_time: time
  …
}
resource OrderSearches {
  index order_search        // GET /order_searches?q[from_time]=…&q[to_time?]=…
}
```

The types that can go in a URL are ID, string, state name, integer, money and datetime. Integers, money and datetimes are written as decimal digits, the same numbers as in the JSON body.

A search resource is a **logical resource**. The value of `q` does not correspond one-to-one to stored rows (physical resources). So, besides the narrowing conditions, "how to read", such as the position of the continuation (see "Reading the continuation" below), goes into the same `q`. The query-string object is always only the one `q`.

Searching is basically done with a resource for searching (`OrderSearches`). It is also possible to give the original resource's `index` conditions (`GET /orders?q[status]=Paid`).

### Reading the continuation: `q[after]`

A query with `page 20` returns the first 20 rows. If there is more, the answer contains the next page's URL (`next`).

```
GET /order_searches?q[status]=Paid
→ { "rows": [ …20 rows… ], "next": "/order_searches?q[status]=Paid&q[after]=o-0020" }
```

- The caller GETs `next` as it is. The value of `q[after]` is a mark made by the server; its content need not be known. The last page has no `next`.
- Nothing is written in `.geas`. A query with `page` automatically accepts `q[after]` (in that query, `after` cannot be used as an input name).
- Passing the page number as a number (`page=3`) is not adopted. If rows are added or removed while reading, the same row would be read twice or skipped. The mark means "after this row", so it does not drift.
- If the marked row has been deleted, it is 400 (read again from the first page).

### Choosing the order: `q[order]`

The order is decided by `order` in the `.geas`. To let the caller choose, list the selectable orders.

```
query order_search {
  …
  order created_at desc     // the first line is the order when none is chosen
  order total desc
  order total asc
}
// GET /order_searches?q[order]=total.desc
```

- The value is `field.direction` (`total.desc`). Anything other than the written candidates is 400.
- For a query with only one `order` line, the order cannot be chosen (writing `q[order]` gives 400).
- The query table in the specification lists all selectable orders. The next page's URL (`next`) also keeps the chosen order.

### Decide the HTTP status for each result kind

For `create` / `update` / `destroy`, you must write the status for each result kind of the deed (`Received → 201`, `NoSuchProduct → 404`). An omission is stopped by the check.

### Version check on writes: PUT and DELETE require If-Match

If someone else has rewritten the same row between reading and writing, that write is not let through.

- A row has a version (a value made from the row's content). It is received as `_etag` of each row in a list answer, and as the response's `ETag` (`"row-version.answer-version"`) when reading one (show).
- PUT and DELETE carry the version read in the header `If-Match` (the `_etag` value wrapped in `"…"`, or the show `ETag` as it is). If it is absent, the answer is 428; if it differs from the current version, 412; in both cases the deed does not run. `*` and weak validators (`W/`) are not accepted. There are no exceptions.
- The 428 answer does not carry the current version. The version belongs only to people who could read that row.
- If there is no row, the deed answers "not found" (the status for that result kind). For URLs that handle only the caller's own rows, 412 is returned even for a missing row (so as not to reveal through a difference in answers whether someone else's row exists).
- The `ETag` of a successful write response is the version of the row after the write.
- POST commands (`create`) do not need it. The check of state transitions takes that role.
- Nothing is written in `.geas`. The deeds for `update` and `destroy` must fetch one row by an ID input (`let x = entity[id] else { … }`) (this is what decides which row's version is compared). The specification shows it once, under the URL table.
- A generated screen sends the version of the row it shows. On 412 it shows "changed elsewhere" and refreshes the screen. MCP write tools accept the version in the input `if_match`.

### URLs that cannot be called from outside, and who can call

Mid-flow deeds (those advanced by a reply or a deadline) also have URLs, but they are "internal". The server does not answer calls from outside. In an app that declares roles (`actor`), every externally callable action carries who may call it (`for role` / `for anyone`).

## 3. How it looks on generated screens

A screen's list (`show query`) turns the two items above directly into operations. There is nothing to add.

- A list of a query with `page` shows a "read more" button while there is a continuation. Pressing it follows the answer's `next` (`after` for a POST query), reads the next page and appends it below. When everything has been read, the button disappears.
- A list of a query with two or more `order` lines shows a field to choose the "order". Choosing it adds `q[order]` and reads again from the first page.
- The periodic re-query re-reads up to the page currently shown (the continuation already read does not disappear).

The history of how this was decided is in [history/rest-decisions.md](../history/rest-decisions.md) (Japanese).
