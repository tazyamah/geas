# Geas 言語リファレンス（AI 向け）

Python 版（試作）の書き方の一覧です。書き方の例は、すべて [tests/fixtures/overseas_shop/overseas_shop.geas](../tests/fixtures/overseas_shop/overseas_shop.geas) にあります。経緯と設計の理由は [v0.1.0-scope.md](v0.1.0-scope.md)。食い違ったら**検査器（`python -m geas check`）が正本**です。

## 1. 全体の決まり

- ファイル拡張子は `.geas`。宣言の順は自由ですが、`ltype` は使う場所より前に書きます。
- 名前は半角英数と `_`（先頭は英字）。型・entity・状態・結果とその種類・外部作用の名前空間・トリガ・nexus・出来事・応答・resource は UpperCamel。項目・変数・処理（deed）・問い合わせ・入力・slot・部品・変換・外部作用の操作と引数は lower_snake。組み込みと同じ名前は、小文字では使えません。
- コメントは `//` から行末。`///` は**説明コメント**で、その直後に書いた名前の日本語表示名になります（仕様書だけに使われ、意味は変わらない）。宣言のキーワード（`entity` `deed` など）の前に書けば、その次の名前に付きます。項目・入力・状態・結果の種類の名前の前にも書けます。複数言語は `@ja …` `@en …`。
- 矢印は `→` ですが、ASCII の `->` で書いても同じです。
- 文の区切りは改行。`( )` の中の改行は式の続きです。
- 数は半角の `0`〜`9` で書きます（全角の `９９` は使えません。小数も無い）。
- 文字列は `"…"`。中に `"` を書くときは `\"`、`\` そのものは `\\`、改行は `\n`、タブは `\t` と書きます（`\` のあとに書けるのはこれだけ。正規表現の `\d` は `\\d`）。文字列の中でそのまま行は変えられません。長い文字列を次の行へ続けるときは、行の終わりに `\` を書きます（次の行の頭の空白は読み飛ばします）。改行を含む理由は、仕様書の表の中で改行して出ます。
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
| `bool` | 真偽。値は `true` `false`。範囲は無い。項目・入力・slot・`library` の引数と戻り値に書ける。JSON も `true` / `false`（`"yes"` や `1` は入力不正）。画面ではチェックボックス |
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

割合は、範囲を書かなくても物理上限があります（分母は 1〜10⁹、値は ±10⁹。外から入る値は境界で確かめます）。割合どうしも掛けられ（`元 * 税率 * 割引`）、結果は端数の出る値なので、`round` で丸めます。途中の分子・分母が `i128` に収まると、検査器が区間で示します。示せない組み合わせは検査エラーなので、金額や割合の範囲を狭めてください。

入力の範囲の確認は、毎回の境界で行うのが既定です。人が危険を承知で、速度のために省くときは、入力に `trust` を付けます（`input quantity: Quantity required trust assume "仕入れ先が 1〜99 を契約で保証"`）。
- `trust verify "理由"`：デバッグ／検証ビルドでは確かめ、リリースビルドでは省く。
- `trust assume "理由"`：どこでも確かめない。範囲外の値が来ると、`Fault` にならず、そのまま入る。
- `trust dangerous "理由"`：`assume` と同じく確かめず、さらに、未検査の `raw` を入力の型として受けられます（`input body: raw required trust dangerous "理由"`）。`raw` を受けた処理で使えるのは、`len(r)`（文字数）、`to_raw(文字列)`（検査済みの文字列を `raw` にする）、`raw_cat(a, b)`（`raw` をつなぐ）と、`raw` の外部作用の引数に渡すことだけです。受けた値を加工せず、または情報を付けて、外部のアプリへ中継する使い方を想定しています。`raw` を保存する項目や、結果の中身、画面には出せません。
- 理由の文章は必須です。仕様書の「人の保証」に、段階と理由が載ります。
- 検査器は、宣言した範囲を事実として証明に使います（外れたとき、証明が静かに崩れるのを承知のうえで使う）。計算のあふれ（`checked_*`）と panic の禁止は変わらず、外れても未定義動作にはなりません。
- 割合の分母の物理上限（1〜10⁹）は、`trust` を付けても残ります。

外から来る値を、検査前の生データ（`raw`）で受けて、geas の式で変換する書き方もあります（入力の選別を通す境界）。変換は論理型に結びつけます。
```
ltype PartNo = text 8..8 of "0-9" read part_no_reader
convert part_no_reader(r: raw) -> PartNo {
  require len(r) == 8 "8桁にしてください"
  require only(r, "0-9") "数字だけにしてください"
  return text(r)
}
deed put_item {
  input part_no: PartNo required
  ...
}
```
- `raw` は、`raw` から変換する `convert` の引数にだけ書けます。この `convert` の引数は `raw` 1つだけで、いまは文字列（`text`）を返せます。
- `require 条件 "理由"` が成り立たないと、入力不正です（処理は動かず、保存は変わりません）。理由は必須です。
- `raw` に使える操作は、`len(r)`（文字数）、`only(r, "a-z0-9")`（文字種だけか）、`text(r)`（文字列として取り出す）です。どれも失敗せず、メモリの安全を崩しません。
- 型の宣言の最後に `read 変換名` を書くと、その型の値は境界で必ずその変換を通ります（入力、部品の戻り値、保存する項目のどれでも）。型の宣言（範囲・長さ・文字種）の確認は、変換のあとに働きます。`trust` の段階は、変換も含めて省きます。
- 検査器は、`read` つきの型の項目へ書く値が、同じ `read` の型（入力・項目・部品の戻り値）に限ると確かめます。変換を通っていない文字列は書けません。
- この変換は、式の中では呼べません。
- 文字ごとの計算ができます。`positions(r)` は 0 から文字数-1 までの位置、`digit(r, i)` は i 文字目の数字の値（数字でない文字と文字数の外は 0。先に `only` で確かめます）、`if 条件 then 式 else 式` は条件の式です。JAN の検査数字は、次のように geas の式で書けます（組み込みの `check gtin` と同じ判定になることを、テストで確かめています）。
```
require sum { digit(r, i) * (if (len(r) - 1 - i) % 2 == 1 then 3 else 1) | i in positions(r) } % 10 == 0 "検査数字が合いません"
```

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
storage ExternalDb isolation store(sqlite)  // 保存先の定義。直列を誰が保証するかを持つ
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

### 保存の形の移行（migration）

実体の項目を変えると、前の版で作った DB は、そのままでは今の版で正しく読めません。前の版の DB を今の形にする手順を、`migration` に書きます。

```
migration 2 {                    // 保存の形の版。2 から 1 ずつ増やす。前の migration は消さずに残す
  add Parcel.weight = 5          // 足した項目に、前からある行へ入れる値
  add Parcel.memo = null         // 無いかもしれない項目は null でよい
}

migration 3 {
  rename Parcel.memo to note     // 項目の名前を変える（値はそのまま移る）
  drop Parcel.legacy             // 項目を消す（値は捨てる）
}
```

- 書けるのは `add 実体.項目 = 値`・`rename 実体.項目 to 新しい名前`・`drop 実体.項目` です。手順は、版の順に、上から順に当たります。実体そのものの改名・削除は、まだ書けません。
- `add` の値は、数・`"文字列"`・`true`/`false`・`null`・状態の名前・`分子/分母` です。値は、項目の型と範囲に入っていること（検査が見ます。あとの `rename` で名前が変わる項目は、変わった先の型で見ます）。`add` は、その項目が無い行にだけ値を入れます。
- 手順を全部当てた結果が、今の実体の項目と合っていること（検査が見ます）。`rename` の元と `drop` の項目は今の実体に無いこと、`add` と `rename` の先は今の実体にあること。足してからあとで消す項目は、今の実体に無くてかまいません。
- DB は自分の版を覚えています（覚えていなければ 1）。サーバーは起動時に、足りない版の手順だけを順に当て、書き換えた行を今の型の範囲で確かめてから、版を進めます。全部を 1 回でまとめて行い、合わない行が 1 つでもあれば、何も変えずに止まります（`rename` の元と先の両方がある行も、どちらを残すか決まらないので止まります）。行の無い DB は、今の版として始まります。このアプリより新しい版の DB は開きません。
- `python -m geas check アプリ.geas --base 前のコード.geas`（または `--base-git HEAD~1`）は、前の版にもある実体で、増えた項目・減った項目が `migration` に書かれていなければ、エラーにします。`geas review --base` でも同じです。項目を `migration` に書かずに消すと、起動は通りますが、前の版の行にその値が残り、同じ名前の項目をあとで足したときに生き返ります。
- 仕様書には「保存の形の変更」の表が出ます。

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
- 文：`let 名前 = 式 [else { return … }]`、`let 名前 = create エンティティ { 項目: 式, … }`、`x.f = 式`、`x.f += 式`、`update { 対象 -= 式 | (k, v) in 集まり }`、`if 条件 { … } [else { … }]`、`for 要素 in 集まり { … }`、`request 名前空間.操作(引数, idempotency_key = 式)`、`delete 変数`、`return 結果の種類(中身)`。
- 項目に書いた後に、同じ変数でその項目を読むと、書いた値が見えます（`x.f = 0` の後の `x.f` は 0。`x == y` のように1件をまるごと比べるときも、x は書いた値で比べる）。保存への書き込みは、処理が結果を返した後にまとめて適用します。そのため、同じデータを別の変数で引いたもの（同じ1件かもしれない）と、`update` で書いた項目には、書いた値は映りません（検査器も、その値については何も仮定しません）。
- `if` と `else`：`else` は閉じる `}` と同じ行に続けて書きます（`} else {`、`} else if 条件 {`）。`if` と `else` の中に書けるのは、`return`・項目の更新・`update`・`request`・`if` だけです（`let` は `if` の前に書く）。`else` の中では、条件が成り立たないことが分かっているものとして検査します（`if x.f > 98 { return Full } else { x.f += 1 }` は通る）。同じ道で同じ項目へ 2 回は書けませんが、`if` と `else` のように同時には通らない道で書き分けるのはかまいません。`if` と `else` の両方が `return` で終わると、その後ろの文には届かないのでエラーです（処理の最後をそうした `if` で終えるのはかまいません）。
- 式：`==` `!=` `<` `<=` `>` `>=`、`and` `or` `not`（両側と後ろは真偽。`not` が最も強く、次に `and`、`or`）、`true` `false`、`+ - *`、先頭の `-`（負の数 `-5` と、符号の反転 `-x`。`0 - x` と同じ）、`/` `%`（整数の割り算と余り。下の説明）、`min(a, b, …)`・`max(a, b, …)`・`abs(x)`（整数どうしか、同じ単位の金額どうし。範囲に挟むときは `max(min(x, 99), 0)`）、`D[k]`（キーで引く）、`x is null|absent|present|empty|nonempty`、`x in 集まり`（id が集合に入っているか・写像のキーにあるか）、`len(x)`（文字列の文字数、集合・写像の件数。文字列は型の長さ、写像は `max N` までと検査が使う。集合は上限を書けないので、数の項目へ入れるときは `min(len(s), 99)` のように挟む）、`{ k | (k, v) in 集まり, 条件 }`（集合の内包表記）、`sum { 式 | (k, v) in 集まり }`、`round(値, floor, 通貨)`、`now`、`tz_offset`。
- `let` の名前は、付け直せません。すでにある名前（入力・slot・前の `let`）で `let` を書くとエラーです（Rust の shadowing はありません。別の名前にします）。前の名前について確かめた範囲が、新しい値に残らないようにするためです。
- 確かめた範囲の引き継ぎ：`if m > 5 or m < 0 { return Bad }` の後ろでは、`m` は 0〜5 として検査します。`not` は向きを返し、`and` が成り立つ道・`or` が成り立たない道では、両側で分かることを合わせます（`or` が成り立つ道・`and` が成り立たない道からは、何も引き出しません）。条件の式 `if c then a else b` も、`a` は `c` が成り立つ、`b` は成り立たないものとして範囲を出します（`if m > 99 then 99 else m` は 99 以下）。見るのは「変数か項目 と 数」の比較だけです。
- 結果の中身（`return Done(式)`）が整数・金額のときも、型の範囲に収まると検査が示します（示せなければエラー）。
- 文字列の長さ：`text` に長さの範囲を書かないと、外から入る値は既定の上限（1,048,576 文字）で境界に止めます。大きくしたいときは `ltype` に長さを書きます（緩めるのは書いた人の責任）。サーバーは、リクエストの本文が 16 MiB を超えると 413 で断ります。JSON の入れ子は 64 段までです。
- 割り算 `x / d`・余り `x % d`（整数。切り捨ては負の無限大へ、余りは 0 以上）：`d` が定数なら正の整数。`d` が変数なら、検査器が **1 以上と示せる**ときだけ通ります（`ltype` の範囲が 1 以上、または `if n == 0 { return … }`・`if n < 1 { return … }` のように、先へ進む道で 0 を除いてある）。示せないときは、`x / d or 0` のように **0 のときの値**（`or` のあとは整数）を書きます。`or` があれば `d` は 0 でも負でもよく、結果の範囲は商（または余り）と `or` の値の合流です。生成物は `checked_div_euclid` を使い、`i64::MIN / -1` のようなあふれは `Fault::Overflow` になります。
- 足し引き `x.f += 式`・`x.f -= 式` の範囲：書く値が項目の範囲に収まると示せなければ検査が止まります（`if x.f > 999900 { return Full }` のように確かめる）。示せなくても人が引き受けるときは、**理由つき**で文の末尾に書きます。`x.f += 式 saturating "理由"` は範囲の端（上限・下限）で止まります。`x.f += 式 trust assume "理由"` は届かないと承知する書き方で、検査器は範囲に収まるものとして先へ進み、届いたら実行時に範囲の確認で異常（Fault）になって止まります。どちらも整数・金額で null にならない項目だけで、仕様書の「人の保証」に出ます。
- 保存先の定義 `storage 保存先 isolation process`／`isolation store(種類) ["理由"]`：保存先を 1 回定義し、リクエストの直列を誰が保証するかを持たせる。entity は `store 名 in 保存先` で参照する（定義の漏れも、使われない定義も検査が止める）。`isolation process` は、このアプリのプロセスの中だけ geas のサーバーが直列にする（別プロセスとの関係は約束しない）。`isolation store(種類)` は保存先が直列にする。種類は引数で、サーバー実行部が契約の試験で確かめてあるもの（いまは `sqlite`）は理由が要らず、それ以外は人の保証として理由が必須。`store` の行がない entity は標準の保存先（`sqlite`）で、書かなくてよい。`process` と確かめていない種類は仕様書の「人の保証」に出ます。
- 動かす前提 `needs`：アプリが実行環境に要求することを書く。環境の実際（いま何台か、どのポートが開いているか）は書かず、plan や実環境から読んで照合する（`env-verify`。判定は 合格・不合格・確かめられなかった・人の保証 の 4 つで、読めなかった事実は合格にしない。事実はクラウドに依存しない形で、plan から作る部分はアダプタ `geas/adapters/` が担う。いまは aws-ecs だけ）。理由 `"…"` は必須で、仕様書の「動かす前提」に出る。3 種類ある：`needs secret 外部.名前 "理由"`（必要なシークレットの名前だけ。外部は `effect` で宣言した名前空間、名前は lower_snake の半角英数。値は書かない）、`needs exposure public|internal "理由"`（インターネットに公開してよいか。リソースがあるのに書かないと警告）、`needs instances at_most N "理由"`（同時に動くサーバーの上限。保存先が `isolation process` なら導出で 1 台なので、それより緩い値はエラー。書けるのは導出した上限より厳しい値だけ）。保存先の永続、外部への到達、前段の読み込みの時間切れは、宣言から検査器が導出するので書けない。一覧は `python -m geas env-checks アプリ.geas [--json]`、各項目の番号は導出元の名前（`instances`、`durable.default`、`reach.Payment`、`secret.Payment.api_key`、`exposure`、`limits`）。
- 結果は `result 名前 { 種類(中身: 型), 種類, … }`。**すべての `return` と、フローの行き先が、結果の種類を漏れなく覆う**必要があります。
- 外部作用 `request` には冪等キー（`idempotency_key`）が必須。`for` の中では、回す値と、外で決まる値の両方を組（`(order_id, k)`）にします。
- 純粋な値の部品：`convert 名前(x: 型) -> 型 = 式`（式で書く一方通行の変換）、`library 名前(x: 型) -> 型 from rust "クレート" "版" call "Rust の式" [guarantee result >= x] trust check|verify|assume "理由"`（根の言語のライブラリを呼ぶ純粋な関数。式は引数の名前を使える。標準ライブラリだけなら `from rust "std" ""`。版は固定して `Cargo.toml` に書き出す。戻り値は呼ぶたびに、型の範囲と `guarantee` で確かめる。`verify` は検証ビルドだけ、`assume` は確かめない。理由は必須で、仕様書の「人の保証」に出る。ライブラリが panic しても `Fault` で止まり、サーバーは続く（計算の溢れも、リリースビルドを含めて panic になる）。無限ループとメモリ枯渇は防げない）。旧 `part` は廃止。標準部品は、Rust の公開 API（rustdoc の JSON）から `python -m geas.stdgen` で機械で作った `library` の束 [geas/std/num.geas](../geas/std/num.geas)（モジュール `Num`、`use "…/num.geas"` で取り込み、`Num.u8_wrapping_add(a, b)` の形で呼ぶ）。いまは整数型（i8 i16 i32 i64 u8 u16 u32）の、宣言から panic しないと言えるメソッドだけ（Panics の記述・unsafe・オーバーフロー検査の付くもの・Option を返すものは除く。bool を返すものは bool 型）。範囲は Rust の型から決まり、引数の範囲に収まると示せない値は渡せない。呼んだものだけが生成物と仕様書の「人の保証」に出る。
- 部品の境界値の検査 `python -m geas parts ファイル.geas`：`library` の宣言を、引数の範囲の端の値（両端とその隣、0 の周り、ビット幅の境目）の全部の組み合わせで実際に呼んで確かめます。見るのは、いまの toolchain で組めるか・panic しないか（デバッグビルドで呼ぶので、計算のあふれも見つかる）・戻り値が宣言の範囲に収まるか・`guarantee` が成り立つか・2 回呼んで同じ値か、の 5 つ。食い違いがあれば終了コード 1。呼ばれている部品だけを見ます（`--all` で全部。`module` のファイルを渡すと、その全部）。引数が整数・金額・真偽でない部品は、まだ確かめられません。端の値を試すだけなので証明ではなく、宣言の書き間違いを早く見つける網です。`trust` に理由を書く前に、これを通します。`--record` を付けると、結果をコードの隣の台帳（`アプリ.parts.json`）に書き、仕様書の「人の保証」の行に「宣言は端の値で N 回呼んで確かめた」と出ます。確かめていない部品は「宣言は実際に呼んで確かめていない」、宣言（型と範囲・呼ぶ式・`guarantee`）を変えたあとは「確かめ直していない」と出るので、レビューする人が見分けられます（台帳はコードと一緒にコミットします）。標準部品の台帳は geas に同梱で（`geas/std/num.parts.json`）、Kani で全部の入力について示せたものは「宣言は全部の入力で証明した」と出ます（`python proofs/std_proofs.py --record`）。標準部品は、別に書いた手本（[geas/stdref.py](../geas/stdref.py)）とも比べ、生成のとき（`python -m geas.stdgen`）に、組めないものを外し、食い違いがあれば書き出しません。

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
- 実際に外へ出る通信は、`effect` の末尾に `over http { … }` を書きます（書かなければ、実際には外へ出ず、開発用の模擬が返事を作ります）。
  ```
  effect Twitter. post(tweet_id: id, content: Body) replies PostSettled over http {
    POST "https://api.x.com/2/tweets"              // メソッドと通信先。通信先は https://ホスト名/パス だけ（IP・ユーザー名・? は不可）
    bearer Twitter.token                           // 認証。needs secret Twitter.token で宣言した同じ外部のシークレットだけ
    body "text" from content                       // JSON の本文：キー from 引数（キーと引数が同じなら `body 名前`）
    status 201 → Posted take post_id from "data.id"  // 返事の状態を、応答の結果の種類に結ぶ。take は返事の JSON から中身を取る道すじ
    status 400..499 → Rejected
    status other → Rejected                        // 書いていない状態の行き先（必須）
  }
  ```
  通信先はここに書いたものだけで、リダイレクトにも従いません。接続できない・時間切れ・シークレットが無い、は返事にならず、待つ先のトリガの期限が行き先になります。同じ要求は 1 回しか送りません（再送しない）。シークレットの値は、環境変数 `GEAS_SECRET_外部_名前`（大文字）か `--secrets` のファイルで渡し、仕様書にも生成物にも書きません。仕様書には「外へ出る通信」の表（通信先・認証・本文・状態の結び方）が出ます。通信の部品は、`over http` を書いたアプリだけに入ります。
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
  order created_at desc                               // desc（大きい順。日時なら新しい順）｜asc（小さい順・古い順）
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

### ファイル操作（over file）

`effect` の末尾に `over file { … }` を書くと、外部作用が実際にファイルに触ります（`over http` と同じく、書かなければ、実際には触らず、開発用の模擬が返事を作ります）。

```
ltype ExportDir = path                                   // 場所は、実行時に入力されるパスの型（OS の書式を確かめた値）
effect Files. export(note_id: id, dir: ExportDir, content: Body) replies ExportSettled over file {
  write "{dir}/note.txt" from content                    // 操作は 1 つ：write・overwrite・read・delete
  ok → Done take size from bytes                         // 結果を、応答の結果の種類に結ぶ
  exists → Refused
  not_found → Refused
  denied → Refused
  failed → Refused                                       // 書いていない失敗は、これになる（必須）
}
```

- 触れるのは、**path 型の引数が指す場所の下の、書いた相対パスのファイル 1 つ**だけです。`{引数}/ファイル名` の形で書き、ファイル名は `/` 区切りの相対パスです（空の区切り・`.`・`..`・`\`・`:`・`*`・`?`・`"`・`<`・`>`・`|`・制御文字は使えません。200 文字まで）。
- 操作：
  - `write "…" from 内容の引数`：**新規に作る**だけです。すでにあれば書かず、`exists` になります。
  - `overwrite "…" from 内容の引数`：作るか、置き換えます。同じフォルダの一時ファイルに書いて、名前を付け替えるので、途中で止まっても、元のファイルは半端になりません。元がリンクなら、リンクそのものを置き換えます（リンク先は書き換えません）。
  - `read "…"`：読みます（`take 名前 from content` で、中身を応答に入れます）。
  - `delete "…"`：あるファイル（またはリンク）だけを消します。フォルダは消えません。
- 結果は、`ok`・`exists`（write のときだけ）・`not_found`（ファイルか、場所の下のフォルダが無い）・`denied`（権限がない）・`failed`（そのほか。大きすぎる 1 MiB を超える、フォルダだった、など）です。`ok` と `failed` は必須です。結果の種類が `gives` を持つなら、`take 名前 from content|bytes` で取り出します。`content` は read のとき（文字列）、`bytes` はバイト数（整数。delete では取り出せません）。
- 場所は、使う直前にもう一度、実行中の OS の書式で展開し直します（環境変数が変わっていても、黙って別の場所にしません）。書く・読むは 1 MiB までです。
- 返事は、`over http` と同じように、待つ先のトリガに届きます。コミットのあとに行い、取り消した処理は行いません。結果の記録には、結果の種類だけを残し、パスやファイルの中身は載せません。
- 仕様書とレビュー画面に「ファイルを触る口」の表が出ます。「動かす前提」には、その場所に触れる権限が要ることが出ます。

### パスの値（path）とデータ用フォルダ（data_folder）

窓つき exe が、DB（`app.sqlite`）・ログ（`log.txt`）・シークレット（`secrets.json`）・起動の印を置くフォルダを宣言します。書かなければ、利用者のデータ用フォルダの下のアプリ名のフォルダです。フォルダのパスは、名前を付けた「パスの値」で書き、`data_folder` で選びます。

```
path DataRoot = absolute {
  windows "%APPDATA%\\ひとりつぶやき"                          // その OS のパスの書き方の絶対パス（geas の文字列の中の \ は \\ と書く）
  macos   "~/Library/Application Support/ひとりつぶやき"
  linux   "$XDG_DATA_HOME/tweet"                              // $名前・${名前}・先頭の ~
}
path Portable = relative "data"                               // 相対パス：OS を問わない。exe のあるフォルダが起点（持ち運び用）
needs env XDG_DATA_HOME "置き場を環境で決める"                  // 必ずあるとは限らない環境変数は、needs env で宣言する
data_folder DataRoot
```

- `absolute` の中の行は、その OS の書き方の絶対パスだけです。Windows は `%名前%`・`C:\\…`、mac・Linux は `$名前`・`${名前}`・`~`・`/…`。書き方が違うと検査が止めます。同じ OS の行は重ねられません。
- `relative` は、相対パス（`/` 区切り、`..` なし）だけです。exe のあるフォルダが起点です。
- 環境変数は、その OS で必ずある名前（Windows：APPDATA・LOCALAPPDATA・USERPROFILE・TEMP など。mac・Linux：HOME・USER。mac は TMPDIR も）か、`needs env 名前 "理由"` で宣言した名前だけ使えます。
- 組む OS では、すべての `absolute` に、その OS の行が要ります。`geas rust … --standalone --target windows|macos|linux`（省けば、いま動いている OS）で確かめます。
- 起動時に、環境変数が設定されているか、フォルダを作れて書き込めるかを確かめ、だめなら、別の場所に逃げず、エラーで止まります（Windows はメッセージの窓で知らせます）。
- `--db` は、DB だけを別の場所にする上書きです（試験用）。
- 仕様書の「データ用フォルダ」の表と、「動かす前提」に出ます。
- 実行時にユーザーが入力するパスは、ここで宣言するパスの値とは別の型で、`ltype ExportDir = path` と書きます（言語が、raw から変換する `export_dir_os_path` を足します。入力にも項目にも使えます）。
  - 受けるのは、**実行中の OS の書式だけ**です。Windows は絶対パス（`C:\…`、`\\サーバー\…`）で、使えない文字（`< > " | ? *`、ドライブ以外の `:`）は入力不正です。mac・Linux は `/` で始まる絶対パスです。
  - 展開するのは、OS の「場所の別名」だけです。Windows は `%名前%`、mac・Linux は先頭の `~`（自分のホームだけ。`~名前` は使えません）。`$名前` は展開せず、文字どおりのパス名として扱います（絶対パスでないので、入力不正になります）。未定義の環境変数は、入力不正です。
  - 改行・NUL・`..`・4096 文字を超える長さは、入力不正です。入力不正のとき、処理は動かず、保存は変わりません。
  - 保存される値は入力のままです。展開した値でファイルに触るのは、外部作用のファイル操作（`over file`）で、使う直前に展開し直します。
  - `os_path(r)` は、この変換の中でだけ使えます（ほかの `convert` の中では、`only` などと同じく raw に使える操作）。
- 起動時に保存先を選ぶ入口は、geas の外（アプリの設計。exe の隣に記録するなど）で決めます。exe の隣が書けない場所のときは、別の場所に逃げず、エラーにします。

### 常駐（resident）

窓つき exe（`--standalone`）を、タスクトレイ（macOS はメニューバー）に居続けるアプリにします。書かなければ、窓を閉じたら終了します。

```
resident {
  tooltip "ひとりつぶやき"                  // アイコンに載せる説明（省略はアプリ名。64 文字まで）
  start hidden                              // 省略は shown（窓を開いて始める）。hidden はアイコンだけで始める
  click toggle                              // 左クリックの動き。toggle（省略。窓の開閉）か menu（右クリックと同じメニューを出す）
  menu "つぶやきを書く" → TweetList          // 右クリックのメニューの項目。入口の画面（entry か最初の画面。引数なし）を開く
  separator                                 // 区切り線（先頭・末尾・連続には置けない）
  menu "未投稿をまとめて X へ" → fire PostSaved   // 出来事（event）を起こす項目。窓は出さない。each だけの出来事（external の入力を持たないもの）に限る
}
```

- 窓を閉じても終了せず、窓を隠します。アイコンのクリック（Windows・macOS）で窓を出し入れします。Linux のアイコンはクリックを通知しないので、メニューの「開く」で出します。
- 右クリックのメニューは、「開く」、区切り線、書いた項目（書いた順）、区切り線、「終了」の順です。「開く」と「終了」は言語が常に足します（同じラベルは書けません）。項目のラベルは 1〜40 文字で、重ねられません。
- `fire` の項目は出来事を窓の外から 1 回起こし、起きた件数か失敗を標準エラーに 1 行の JSON（`geas.resident`）で残します。
- 同じ保存先（`--db`）では 1 つだけ起動できます。2 つ目は、すでに起動していることを伝えて終わります。
- トレイのアイコンを作れない環境では、常駐にせず、窓を出したまま閉じたら終了します（隠れて開けなくなるのを避けるため）。
- 仕様書とレビュー画面に「常駐」の表が出ます。常駐の部品（tray-icon）は、`resident` を書いたアプリだけに入ります。

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
- **流れの途中の処理**（nexus の行き先・par の枝、trigger の返事と期限の行き先）を呼ぶ URL は「内部」です。外から直接呼ぶと、決済の返事などの確かめを飛ばして先へ進められるので、生成したサーバーは答えません（`--test` で起こしたときだけ答える）。仕様書の URL の表の「外から」の列に「内部（流れの途中）」と出て、MCP のツールにもなりません。書き方は変わらず、どれが内部かは流れから決まります。出来事（`event`）が起こす処理と、流れの最初の処理は「呼べる」です。
- 画面の操作（`action`）に、流れの途中の処理は使えません（エラー）。画面からは流れの最初の処理を呼びます。

## 11. 検査の結果の種類

`python -m geas check` は、エラー・警告・問い（QUESTION）を出します。**エラー**は直します。**問い**は、決まっていないことを検査器が見つけたものなので、人に聞いて決めます。**警告**は、表示名の抜けなどです。エラーと問いが無いと、`rust` と `compare` が進みます。

- 指摘の名前は、コードに書いたとおり（`Product.stock`）で、分かるものには行番号（`（30行目）`）が付きます。仕様書（`spec`・`review`）に載るときは、表示名に置き換わります。
- 打ち間違いらしい名前には `（近い名前：stock）` が、他の言語の型名や値（`string`・`True` など）には geas での書き方が付きます。
- 最後の行は件数です（`検査: エラーがあります（エラー 1 件・問い 0 件・警告 2 件）`）。問いが残っていると、通っても「人に聞いて決める」と出ます。
- 構文エラーは、行と列、その行の抜き出しと位置の印（`^`）を出し、他の言語の書き方らしいとき（`&&`・`elif`・`;` など）は直し方を続けます。`{` と `}` の対応が崩れていれば、閉じていない `{` の行も出します。
- `--json` では、指摘ごとに `line`（構文エラーは `col` も）と、見る節と直し方の `hint` が付きます。

## 12. 他の言語から来たときに間違えやすいこと

geas は Rust や TypeScript に似せていますが、次は違います。検査器はどれもエラーにして直し方を出すので、黙って別の意味になることはありません。

| 他の言語の書き方 | geas の書き方 |
|---|---|
| `a && b`・`a \|\| b`・`!a` | `a and b`・`a or b`・`not a` |
| `elif`・`elsif`・`match`・`switch` | `if 条件 { … } else if 条件 { … } else { … }`（`else` は `}` と同じ行に続ける）。値を選ぶだけなら式の `if 条件 then 式 else 式` |
| `x == null`・`x != null` | `x is null`・`not (x is null)`（1件の有無は `D[k] is absent`・`D[k] is present`） |
| 文末の `;` | 改行（`( )` の中の改行は式の続き） |
| `=>` | `→` か `->` |
| `x++`・`x--` | `名前.項目 += 1`（足し引きできるのは項目だけ。変数は作り直さない） |
| `let mut x`・`var x`・`const x` | `let x = 式` |
| `string`・`boolean`・`uuid`・`datetime`・`i32` | `text`・`bool`・`id`・`time`・`int`（範囲は `ltype Quantity = int 1..99` と名前を付けて書く） |
| `float`・`double`・`decimal`・`0.5` | 小数の型も小数の数も無い。金額は `money(JPY)`（最小単位の整数）、割合は `ratio`（定数は rule の値に `10/100`） |
| 全角の数字 `９９` | 半角の `99` |
| 文字列の中でそのまま改行する・`"""…"""`・`` `…` `` | 行の終わりに `\` を書いて次の行へ続ける（次の行の頭の空白は読み飛ばす）。改行そのものは `\n` |
| 正規表現の `"\d+"`（`\` を 1 つ） | `"\\d+"`（`\` そのものは `\\`。`\` のあとに書けるのは `n` `t` `"` `\` と行の終わりだけ） |
| `text?`・`Option<T>` | 何も付けない（項目と入力は、書かなければ無いかもしれない扱い）。必ずあるものに `required` |
| `type X = …`・`enum`・`struct`・`class`・`fn` | `ltype`・`state`／`result`・`entity`（値の組は合成の `ltype`）・`deed`／`convert` |
| `import` | `use "ファイル.geas"` |
| `True`・`False`・`None`・`'x'` | `true`・`false`・`null`・`"x"` |
| `return Done()`・`return` | `return Done`（結果の種類を必ず返す） |
| `while`・`throw`・`try` | 無い。繰り返しは `for 要素 in 集まり`、失敗は結果の種類を `return` |
| `/* … */` | `//`（`///` は表示名） |
| `where a and b`・`where a, b` | `where` の行を分ける（行どうしが and、1行の中は `or`） |
| `ORDER BY x DESC`・`order by x desc` | `order x desc`（`by` は書かない。向きは小文字の `desc`・`asc`） |
| `filter x >= y` | `filter x at_least y`（`equals`・`at_least`・`at_most`・`contains`・`within`） |
| `limit 20` | `page 20`（`order` が要る） |
| resource の `get`・`post`・`put`・`patch`・`delete` | `index`／`show`・`create`・`update`・（PATCH は無い）・`destroy` |
| 定義の外の定数 | 範囲は `ltype`、サーバーが決める値は `slot` と `rule` |
