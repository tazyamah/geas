# How to run the generated server

> This page is a translation of the Japanese original (the Japanese text is the source of truth).

The `server/` produced by `geas rust --host --runtime` is a small server for running locally. It is not meant to be exposed directly to the internet. When publishing, place a web server such as nginx (a reverse proxy) in front. There is no plan to meet the requirements of publication with geas's features alone.

## Startup arguments

`geas_server [--port 8787] [--host 127.0.0.1] [--workers 4] [--db app.sqlite] [--pwa ../ui] [--test [--dev ../dev]] [--allow-host name] [--seed other-initial-data.json] [--secrets secrets.json]`

- `--host`: the address to listen on. The default is `127.0.0.1` (only from the same machine). If you put a proxy in front, leave it as it is so that only the proxy can reach it.
- `--workers`: the number of threads that process requests (1 to 256, default 4).
- `--test`: opens the entrances for testing and development (see "Entrances that can be called from outside" below). The generated end-to-end tests start the server with this. Do not add it to a server that is published.
- `--dev`: the folder of the development mock operation panel. Served only with `--test`.
- `--secrets`: a file of the values used for authenticating external effects (`over http`) (`{"external.name": "value"}`). The environment variable `GEAS_SECRET_<external>_<name>` (uppercase) takes precedence. Values appear neither in the specification nor in the log.
- `--effect-url external.operation=URL`: (only with `--test`) replaces a declared destination with a URL for testing. If added without `--test`, startup is refused.
- `--allow-host`: a name accepted as Host, in addition to `localhost` and IP notations (`127.0.0.1`, `[::1]`, `192.168.0.5`, etc.). It can be written any number of times, or comma-separated. A `--host` written as a name is also accepted. Requests with any other name are refused with 403 (see "What is guaranteed" below).
- `--seed`: JSON used in place of the initial data embedded in the output (`{"entity": {"key": {row}}}`). The initial data, whether embedded or by `--seed`, is read and put in only when the DB is empty. It is not read when starting with a DB that has rows, so startup time and memory do not depend on the size of the initial data.

## Entrances that can be called from outside

Without `--test`, the server answers only the following.

- The entrances whose "from outside" is "callable" in the specification's URL table (RESTful URLs made from the `resource` declarations). A screen's URL returns the screen's shell (HTML) when opened in a browser.
- The screen's files (the `--pwa` folder), `/manifest.webmanifest`, the definitions the screen uses `/api/defs` (only the screens and the URLs callable from outside; the whole definitions only with `--test`), and the liveness check `/api/health` (503 if the DB cannot be reached).
- An input error returns 400 with the reason. A failure inside the server (DB, etc.) is 500, and the reason goes only to the standard-error log (`geas.error`).
- If you start with a DB made by the previous version after changing the fields of an entity in the `.geas`, startup stops after stating the reason. When you add, rename or delete a field, if you write it in the `.geas`'s `migration`, the previous version's DB is rewritten to the current shape at startup ("Migration of the stored shape" in [language-reference.md](language-reference.md)).

With `--test`, the following are also opened. In every case, they let you call any deed or query by name without going through the specification's URL table, or change the clock or the mock replies so that they affect everyone's orders.

- The entrances that are "internal (mid-flow)" in the specification's URL table: those that call deeds advanced from a reply, a deadline or the result of a previous deed (confirming an order, judging a payment, etc.). If they could be called from outside, one could skip checks such as a payment reply and move ahead. Without `--test` they are 404. Calling them inside a flow works either way.
- `/api/test/…`: advancing the clock, setting mock replies, and the records of waits for replies and external effects.
- `/api/deeds/{deed}`, `/api/queries/{query}`, `/api/events/{event}`: calls by name that do not go through the declarations.
- The development mock operation panel (the `--dev` folder, `/test.html`).

## Log

Each time a REST route is processed, one line of JSON is written to standard error (`"log":"geas.route"`: the action, the route, the deed called, the result kind, the HTTP status, and the elapsed ms). The body and input values are not included. `geas trace app.geas log` ties these lines to the rows of the specification's route table, and shows routes, results and statuses that are not in the specification as "could not be tied".

## What is guaranteed

- The body is up to 16 MiB (413 if exceeded). JSON nesting is up to 64 levels. A string with no length written is up to 1,048,576 characters.
- Input from the Web is first cut down by the selection (the input selection that lets only the declared fields through). Whatever limit is loosened is the author's responsibility.
- One request is one transaction, and the DB is responsible for serialization (`storage … isolation …`).
- POST accepts only `Content-Type: application/json` (415 otherwise). A page on another site can have a form or a `text/plain` POST sent without a preflight, but a request claiming to be JSON is stopped by the browser's preflight (CORS). The server does not open CORS, so it does not reach from other sites (CSRF protection).
- The body is read as UTF-8 (400 otherwise). A `+` in a path is not made a space, and a `%` not followed by two hexadecimal digits is left as it is.
- Every response carries a CSP (inline scripts do not run), `X-Content-Type-Options: nosniff` and `Referrer-Policy: same-origin`.
- A request whose Host is none of `localhost`, `*.localhost`, an IP notation and the `--allow-host` names is refused with 403 for both reading and writing. Even if a malicious page opened in a browser reaches the `127.0.0.1` server as the same origin through DNS rebinding, the browser writes that page's name in Host.
- Processing of deadlines and replies advances one wait at a time. Even if one fails, one line is left on standard error (`"log":"geas.tick"`: the wait that failed, the kind and the reason), and the other waits are not stopped.

## What is not guaranteed (left to what is placed in front)

- **Slow sending**: a peer that sends the body slowly occupies one worker in the meantime. The HTTP server part (tiny_http) has no way to set a read timeout, so the server itself cannot cut it. nginx relays after it has received the whole body, so set `client_body_timeout`, `client_header_timeout` and `client_max_body_size`.
- **Authenticating the person (login)**: the server does not do this. For an app that writes roles (`actor`) and the roles that may call each URL (`for`) in the `.geas`, have the proxy verify the person and put the role's name in the header `X-Geas-Actor` (comma-separated for several). **Always discard an `X-Geas-Actor` that came from outside at the proxy** (otherwise anyone can claim any role). Also do not create a path that reaches the server without going through the proxy. The server returns 401 if the header is absent and 403 if the role does not match. With nginx, always overwrite the value, as in `proxy_set_header X-Geas-Actor $verified_role;`. For an app with a scope narrowed by the person (`caller` in `scope`), put the verified person's ID in `X-Geas-Actor-Id` (again, always discard a value that came from outside). This ID is the same value as the field (the customer's ID, etc.) that the specification's "scope" table compares with the caller's ID.
- **Distinguishing who may call in an app that writes no roles**: there is none. Anyone who reaches the server can call every entrance in the specification's URL table (both customer operations and shop-side operations). When publishing, restrict who can reach it at the proxy's authentication entrance.
- **Connection counts, rate limits, TLS and the authentication entrance**: set `limit_conn`, `limit_req`, certificates and so on on the proxy side.
- **Several servers**: as long as they use the same DB file, serialization is kept even if you run several servers (tested). The firing of deadlines also happens only once per wait (only the one server that could delete the wait proceeds; tested).

## An nginx example

```
location / {
    proxy_pass http://127.0.0.1:8787;
    client_max_body_size 16m;
    client_body_timeout 10s;
    client_header_timeout 10s;
}
```

In this example, the Host nginx sends to the server is `127.0.0.1:8787`, so `--allow-host` is not needed. If you pass the public name with `proxy_set_header Host $host;`, write that name in `--allow-host`.

## Storage and memory

- The server holds the storage (the rows of all entities) in memory, and deeds and queries run on it. Only the rows a deed changed are written to the DB. The whole storage is not re-read at every request (with the shop app at 10,000 rows each, 30,000 in all, one order takes about 0.4 ms).
- Memory is proportional to the number of rows stored (about 30 MB for the 30,000 rows above). It is not read at startup; everything is read at the first request.
- If another server or tool (`sqlite3`, etc.) has rewritten the same DB file, this is noticed at the start of the next request through SQLite's `data_version`, and then everything is re-read. When several servers write the same DB, it re-reads each time, so it is slower than with one server.
- If a deed fails midway and the transaction is rolled back, the in-memory storage is also discarded and re-read at the next request.

## The version of a query (ETag)

- The responses of query URLs (`index`, `show`) carry a version of the answer (`ETag`). The version is made from the generation of the in-memory storage (it changes each time a deed writes rows or it is re-read from the DB), a per-startup value, and the URL. A query is decided by the storage and the inputs only (no clock is used), so if the version is the same for the same URL, the answer is the same.
- If the request's `If-None-Match` equals the current version, `304` (no body) is returned without running the query. Writes by another connection are checked beforehand with `data_version`. After a restart, `304` does not occur against a previous version (so an old list is not used even if the DB was rewritten while it was stopped).
- When a generated screen re-queries the list it shows every 2 seconds, it sends the previous version. On `304` it uses the rows it remembers and does not redraw (with the shop app at 10,000 products, over 10 seconds of opening the list and doing nothing, the time the browser was busy went from 8.6 s to 0.01 s, and the amount sent from 5.4 MB to 0).
- Even when the answer changes, rows whose HTML is the same as the previously drawn row are kept, and only the changed parts are rebuilt. The options of a choice field are not rebuilt if they are the same as before (with the shop app at 10,000 products, the time from pressing restock on the product management screen to finishing drawing went from 481 ms to 206 ms).
- A log line is also written when `304` is returned (the status is `304`, no result kind).
- The answer of a query with rows (`{"rows":[…]}`) uses, as the body as it is without re-parsing on the server, the JSON the generated deed wrote by appending the rows into one string. The fields of a row are in the order written in `select` (with the shop app at 10,000 products, the list response went from 46.6 ms to 5.8 ms, and at 100,000 from 478 ms to 65 ms).
