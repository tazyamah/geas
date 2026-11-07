# 変異検査（overseas_shop.geas）

崩し方の範囲：すべての行。崩した版 44 件のうち、検査器が止めたもの（検出）36、止めなかったもの（生き残り）8、構文として成り立たず試せなかったもの（対象外）0。

## 生き残り（検査器が何も言わなかった崩し方）

異常とは限りません。その約束は、検査器ではなく、具体例のテストか人の目で守られています。元から冗長な確認なら、このままで構いません。守るべき約束なら、仕様（不変条件・遷移）か具体例に足してください。

| 行 | 崩し方 | 元の行 |
|---|---|---|
| 10 | 範囲の上限を狭める | `ltype Quantity = int 1..99` |
| 14 | 範囲の上限を狭める | `ltype UnitPrice = money(currency) 0..10000000` |
| 16 | 範囲の上限を狭める | `ltype ShippingFee = money(currency) 0..100000` |
| 50 | required を外す | `name: ProductName required` |
| 52 | required を外す | `jan: JanCode required` |
| 56 | required を外す | `currency: currency required` |
| 82 | required を外す | `currency: currency required` |
| 256 | 確認を消す | `if short is nonempty { return OutOfStock(short) }` |
