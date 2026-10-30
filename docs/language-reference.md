# Geas 言語リファレンス（AI 向け）

2026-10-28 時点の Python 版（試作）の書き方の一覧です。書き方の例は、すべて [tests/fixtures/overseas_shop/overseas_shop.geas](../tests/fixtures/overseas_shop/overseas_shop.geas) にあります。経緯と設計の理由は [v0.1.0-scope.md](v0.1.0-scope.md)。食い違ったら**検査器（`python -m geas check`）が正本**です。

## 1. 全体の決まり

- ファイル拡張子は `.geas`。宣言の順は自由ですが、`ltype` は使う場所より前に書きます。
- 名前は半角英数と `_`（先頭は英字）。型・entity・状態・結果とその種類・外部作用の名前空間・トリガ・nexus・出来事・応答・resource は UpperCamel。項目・変数・処理（deed）・問い合わせ・入力・slot・部品・変換・外部作用の操作と引数は lower_snake。組み込みと同じ名前は、小文字では使えません。
- コメントは `//` から行末。`///` は**説明コメント**で、その直後に書いた名前の日本語表示名になります（仕様書だけに使われ、意味は変わらない）。宣言のキーワード（`entity` `deed` など）の前に書けば、その次の名前に付きます。項目・入力・状態・結果の種類の名前の前にも書けます。複数言語は `@ja …` `@en …`。
- 矢印は `→` ですが、ASCII の `->` で書いても同じです。
- 文の区切りは改行。`( )` の中の改行は式の続きです。
- `app "名前"`：アプリの名前（画面の見出し）。`timezone +09:00`：アプリの標準のタイムゾーン（書かなければ UTC）。`use "別ファイル.geas"` と `module 名前`：[modules.md](modules.md)。

## 2. 型

| 書き方 | 意味 |
|---|---|
| `id` | 識別子 |
| `int` / `int 1..99` / `int 0..` | 整数。範囲は `a..b`、片側は `a..` |
| `ratio` / `ratio 0..1` | 割合（分数） |
| `money` / `money(USD)` / `money(currency)` | 金額（最小単位の整数）。単位ごとに別の型で、違う単位は混ぜられない。`money(名前)` は、同じ並びにある通貨（`currency` 型）の入力・slot・項目の名前 |
| `currency` | 通貨コード（JPY・USD・EUR）。入力・slot・項目に持てる |
| `text` / `text 1..80` / `text 2..2 of "A-Z"` | 文字列。長さと使える文字（`of` は `-` でつないだ範囲） |
| `text 13..13 of "0-9" check gtin` | JAN・EAN・UPC。最後の1桁の検査数字まで境界で確かめる（長さは 8・12・13・14 のどれか1つ、文字種は `"0-9"`） |
| `time` | 日時。`30 min`・`24 hour`・`10 sec` の期間を足し引きでき、時刻どうしの引き算は秒数。`day` `week` `month` `year` は書けない |
| `bytes` | バイト列 |
| `set of id` | 集合 |
| `ref Customer` | データへの参照 |
| `Product → Quantity` / `Product → Quantity max 100` | 写像（キーは entity、値は型）。キーが `id` の写像は保存しない入れ物。`max N` は要素数の上限（`sum` の合計が範囲に収まると示すのに使い、外から来る写像は境界で要素数も確かめる） |
| 状態の名前 | 下の `state` |

範囲・組み合わせは、`ltype` で**名前を付けてから**使います（直に項目へ書けません）。

```
ltype Quantity = int 1..99
ltype ProductQuantity = Product → Quantity max 100
ltype Address {                      // 合成型：成分を持つ。作るときは Address { country: …, … }、読むときは a.country
  country: CountryCode
  line: AddressLine optional         // optional の成分は未指定にでき、未指定にアクセスしうるとエラー
}
```

**整数と金額の範囲は、物理型（`int` は `i64`、`int 0..` は `u64` など、範囲から選ぶ）に収まる範囲として扱います。** 上限や下限を書かなくても、保存できる値は物理型の範囲までです。項目へ値を書くときは、書く値が項目の範囲（と物理型）に収まると検査器が示せなければエラーです（panic する書き方は書けません）。示す手がかりは次のとおりです。

- 単価・合計・点数のように増える値の上限は、`ltype` に書く（`money(JPY) 0..1000000000000`）。
- `sum` の合計は、集まりの型の `max N` と、要素の範囲の掛け算で示す。
- 足し算（`x.n += 5`）は、`if x.n > 995 { return Full }` のような上限の確認が前にあれば示せる（数との比較だけを見る）。
- 時刻どうしの差は、外から来る時刻の範囲（10^15 秒）から示す。

組み込みの合成型に `vtime`（year month day hour minute second。仮想日時）があります。整数と金額は混ぜられません（金額×整数、金額×割合は可。端数は `round(値, floor|ceil|half_up, JPY)`）。

## 3. データと状態

```
entity Order {
  order_id: id unique
  customer: ref Customer required      // required がなければ「無いかもしれない」（null）
  items: ProductQuantity required
  currency: currency required
  total: money(currency) required
  status: OrderStatus                  // 状態の項目は必須
  on delete Customer.orders → detach   // 消したときの参照元の扱い：detach（参照を空に）｜cascade（参照元も消す）｜leave（そのまま）
  removable in Draft, Archived         // 消してよい状態（keep と書くと削除させない）
}
store Order in ExternalDb              // 保存先は entity の外に書く。外に置くと、未同期の状態「記録」を言語が足す

state OrderStatus {
  initial Received
  middle PaymentRequested
  final Completed, Canceled            // final の後ろには、endless（終わりなく続く）や reject も書ける
  reject Completed → Canceled reason "完了後は返品で扱う"
}
```

- 項目はすべて「無いかもしれない」。必ずあるのは `required`（キーと状態は必須）。使う前に `let x = o.f else { return … }` か `if o.f is null { return … }` で確かめないと、検査で止まります。
- 1件や項目そのものが無いこと（`D[k] is absent`）と、値が null であることは別です。
- `invariant Product: stock >= 0` と `invariant Order: status == PaymentRequested implies total > 0`。書き込むたびに、値の範囲から守れることを検査が示し、示せなければエラーです。

## 4. 処理（deed）

```
deed place_order {
  input customer_id: id required, items: ProductQuantity required, ship_to: Address required
  slot currency: currency              // 外から埋まる値。通貨は画面が選ばせる／サーバーが rule で決める
  slot shipping: money(currency)
  reads Product.stock, Product.price, Customer       // 読む項目を宣言
  writes Order                                       // 書く項目を宣言
  transition Order: (new) → Received                 // 状態の動き
  result PlaceOrderResult                            // 結果の形
  effects none                                       // 外部作用（なければ none）

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

- 宣言（`input slot reads writes transition result effects`）が先、本体の文があと。宣言と本体のずれは検査器が見つけます。
- 文：`let 名前 = 式 [else { return … }]`、`let 名前 = create エンティティ { 項目: 式, … }`、`x.f = 式`、`x.f += 式`、`update { 対象 -= 式 | (k, v) in 集まり }`、`if 条件 { … }`、`for 要素 in 集まり { … }`、`request 名前空間.操作(引数, idempotency_key = 式)`、`delete 変数`、`return 結果の種類(中身)`。
- 式：`==` `!=` `<` `<=` `>` `>=`、`and`、`+ - *`、`D[k]`（キーで引く）、`x is null|absent|present|empty|nonempty`、`{ k | (k, v) in 集まり, 条件 }`（集合の内包表記）、`sum { 式 | (k, v) in 集まり }`、`round(値, floor, 通貨)`、`now`、`tz_offset`。
- 結果は `result 名前 { 種類(中身: 型), 種類, … }`。**すべての `return` と、フローの行き先が、結果の種類を漏れなく覆う**必要があります。
- 外部作用 `request` には冪等キー（`idempotency_key`）が必須。`for` の中では、回す値と、外で決まる値の両方を組（`(order_id, k)`）にします。
- 純粋な値の部品：`convert 名前(x: 型) -> 型 = 式`（式で書く一方通行の変換）、`part 名前(x: 型) -> 型 guarantee result >= x`（保証は式で書き、実装は外から足す）。

## 5. slot の値の決め方（rule）

サーバーが決める slot（税率・送料など）は、`rule` に書きます。上の行から順に、最初に当てはまった行の値になります。条件の右は名前・文字列・数、複数の条件は `and` でつなぎます。最後は必ず `default:` です。値は、割合なら `10/100`、金額・整数なら数です。

```
rule shipping {
  currency == JPY and ship_to.country == "JP": 500
  currency == JPY: 2500
  default: 3000
}
```

左は、処理の入力（`ship_to.country` のように成分も）か、先に決まった slot です。`rule` が無い slot（通貨など）は、呼び出し元（画面）から渡されます。新しい ID は、生成物が自動で採番します。

## 6. 外とのやりとり

```
effect Payment. charge(order_id: id, currency: currency, amount: money(currency)) replies PaymentSettled
effect Payment. refund(order_id: id, auth_id: id) no_wait "返金の失敗は人が見る"
effect Warehouse. hold(order_id: id, to: Address) joins OrderPending replies StockSettled

reply PaymentSettled(auth_id: id) {       // 応答の中身と、結果の種類（先頭が平常。開発用の模擬が使う）
  Approved gives auth_id
  Declined
}

trigger OrderPending(order_id: id) {
  all reply PaymentSettled, StockSettled → settle_order     // 全部そろったら（待ち合わせ）
  reply PaymentDeclined → handle_declined                   // どれか1つ
  deadline now + 30 min → expire_order                      // 期限（必須）
}
```

- `effect` の末尾の句（`failures`・`no_wait`・`joins`・`replies`）は順不同。外部作用は、返事を待つか（トリガ）、`no_wait` で理由を書くかのどちらかです。
- `joins トリガ名`：返事をそのトリガの応答として入れる。`replies 応答名`：返ってくる応答。
- 起きた応答の中身は、トリガの引数に足されて、行き先の処理の入力になります（名前と型が合わなければエラー）。

## 7. フローと出来事

```
nexus Intake {                                   // flow は別名
  place_order: Received → par { request_payment, request_stock_hold }, OutOfStock → reply, NoSuchCustomer → reply
  request_payment: Requested → wait OrderPending, NoSuchOrder → reply
}
event Reconnected on online {                    // on は online（つながりが戻った）か opened（アプリを開いた）
  each awaiting_payment → recheck_payment        // 問い合わせの結果1件ずつに処理を起こす（external → 処理 もある）
}
```

- 結果の種類ごとに行き先を書きます：次の処理、`reply`（返して終わり）、`wait トリガ名`、`par { A, B }`（全部が起こる）。
- `par` の枝は、書く項目が重ならず、読み書きが交差せず、同じ状態を動かさない（`+=` `-=` だけの重なりは可）。破るとエラー。
- 次の処理へは、結果の中身が名前と型で入力に渡されます。

## 8. 問い合わせ

```
query orders_of {
  input customer: id required
  from Order
  where status != Canceled or status == Received      // 行が or、行どうしが and
  filter customer equals customer                     // equals／at_least／at_most／contains（ignore_case 等）／within 1 hour（slot current_time: time が要る）
  order created_at newest                             // newest｜oldest
  page 20                                             // order が必須
  select order_id, customer.customer_id as customer, sum(items) as item_count
  reads Order.customer, Order.order_id, Order.items
}
```

絞り込みの入力が無いときは絞り込まず返します。`select` が無ければ ID の列だけ返します。

## 9. 画面（screen）

```
screen OrderForm(product_id: id) {
  field customer_id from customer_list, items from product_list, ship_to    // 入力欄（from は選ぶ元の問い合わせ）
  slot currency                                                              // 利用者に選ばせる slot
  show orders_of                                                             // 見せる問い合わせ
  open MyOrders                                                              // 行を選んだときの行き先
  action place_order {                                                       // 操作と、結果ごとの行き先（stay／back／画面名）
    Received → MyOrders
    OutOfStock → stay
  }
  design { layout form  accent "#0b5cad"  mode light }
}
```

- 先頭に宣言した画面か `entry` の画面が入口です。画面の URL は resource の割り当てから決まり、検査が漏れと重複を見ます。
- 画面の通しテストは、計画書の「ユースケースの手順」と「画面の具体例」から生成されます（[ai-guide.md](ai-guide.md)）。画面の部品（欄・行・処理のボタン）の名前は、この宣言の名前から決まります。
- **ブラウザ拡張の画面**は `page "URL のパターン", …` と `read { … }` を持ち、入力が1つの問い合わせを1つ見せるだけです。

```
screen PriceCheck entry {
  page "*://www.amazon.co.jp/*"
  read {
    subject ASIN url "/(?:dp|gp/product)/([A-Z0-9]{10})(?:[/?]|$)"   // 1つ目の ( ) が値
    subject ASIN element "input#ASIN" matches "[A-Z0-9]{10}"          // 要素の value
    value row "JAN", "EAN", "GTIN"                                    // 見出しの語を含む表の行・一覧の項目から数字を読む
    value lookup "B0SHOPTEA1": "4901234567894"                        // 読めなければ引く対応表
  }
  field jan
  show price_by_jan
}
```

## 10. REST（resource）

```
resource Products {                      // 名前から /products が決まる
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
resource Reconnections { create Reconnected }                         // 出来事を呼ぶ create は、結果を持たない
```

- 使えるのは `index` `show` `create` `update` `destroy` だけ（`new` と `edit` は画面の URL に出る）。PATCH なし、クエリ文字列なし。状態を進める命令も、名詞のリソースにして `create` で表します（`OrderSettlements` など）。
- `create`／`update`／`destroy` には、**結果の種類ごとの HTTP の状態**を必ず書きます（`Received → 201`）。漏れはエラーです。
- すべての処理と問い合わせは、どこかの resource に割り当てます（無いと警告）。

## 11. 検査の結果の種類

`python -m geas check` は、エラー・警告・問い（QUESTION）を出します。**エラー**は直します。**問い**は、決まっていないことを検査器が見つけたものなので、人に聞いて決めます。**警告**は、表示名の抜けなどです。エラーと問いが無いと、`rust` と `compare` が進みます。
