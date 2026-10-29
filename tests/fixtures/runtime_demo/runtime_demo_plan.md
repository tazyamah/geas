# 計画書：小包（サーバーの実行部と画面の動作テスト用の小さな例）

## 1. 仕様（◆は突合する）

### ユースケースの手順（画面で実現できるかを突合する）

| ユースケース | 手順 | 画面 | 操作 | 結果 | URL |
|---|---|---|---|---|---|
| 小包を受け付ける | 1 | ParcelList | 見る |  | /parcels/new |
| 小包を受け付ける | 2 | ParcelList | accept | Accepted | /parcels/new |
| 重い小包は送料が高い | 1 | ParcelList | 見る |  | /parcels/new |
| 重い小包は送料が高い | 2 | ParcelList | accept | Accepted | /parcels/new |
| 数字の種別も受け付ける | 1 | ParcelList | 見る |  | /parcels/new |
| 数字の種別も受け付ける | 2 | ParcelList | accept | Accepted | /parcels/new |
| 小包を取り下げる | 1 | ParcelList | 見る |  | /parcels/new |
| 小包を取り下げる | 2 | ParcelList | 行を選ぶ |  | /parcels/new |
| 小包を取り下げる | 3 | ParcelDetail | remove_parcel | Removed | /parcels/:parcel_id/details |
| ページから種別を読んで送料を見る | 1 | FeeCheck | 見る |  | /fee_by_kinds/:kind |
| ページの表に種別があれば、それで送料を見る | 1 | FeeCheck | 見る |  | /fee_by_kinds/:kind |
| 種別が分からなければ、分からないと知る | 1 | FeeCheck | 見る |  | /fee_by_kinds/:kind |
| 商品ページでなければ、何も出さない | 1 | FeeCheck | 見る |  | /fee_by_kinds/:kind |

### 画面の具体例（入れる値と見えること。上から順に、1つの新しいサーバーに対して、実際の画面で実行する（前のユースケースの結果が残る））

| ユースケース | 手順 | 入れる・選ぶ | 見えること |
|---|---|---|---|
| 小包を受け付ける | 1 |  | 行数: 0 |
| 小包を受け付ける | 2 | kind = light | 通知: Accepted; 行数: 1; 行 parcel-0001 の fee: ￥300 |
| 重い小包は送料が高い | 1 |  | 行数: 1 |
| 重い小包は送料が高い | 2 | kind = heavy | 通知: Accepted; 行数: 2; 行 parcel-0002 の fee: ￥900 |
| 数字の種別も受け付ける | 1 |  | 行数: 2 |
| 数字の種別も受け付ける | 2 | kind = 12345678 | 通知: Accepted; 行数: 3 |
| 小包を取り下げる | 1 |  | 行: parcel-0001; 行数: 3 |
| 小包を取り下げる | 2 | 行 = parcel-0001 |  |
| 小包を取り下げる | 3 |  | 行なし: parcel-0001; 行数: 2 |
| ページから種別を読んで送料を見る | 1 | ページ = /item/tagheavy | 含む: 900 |
| ページの表に種別があれば、それで送料を見る | 1 | ページ = /item/zzz; ページの表 = Kind: 12345678 | 含む: ￥300 |
| 種別が分からなければ、分からないと知る | 1 | ページ = /item/zzz | 含む: kindが分かりません |
| 商品ページでなければ、何も出さない | 1 | ページ = / | 表示なし |

### URL の具体例（上から順に、新しいサーバーに対して実行する。入力は実際に送る JSON、{名前} は「控える」で取った値）

| ID | ◆メソッド | ◆経路 | ◆入力 | ◆期待する結果 | ◆期待する状態 | 控える |
|---|---|---|---|---|---|---|
| UX1 | GET | /parcels |  | － | 200 |  |
| UX2 | POST | /parcels | `{"kind":"heavy"}` | Accepted | 201 | parcel_id = body.parcel_id |
| UX3 | POST | /parcels | `{"kind":""}` | － | 400 |  |
| UX4 | POST | /settlements | `{"parcel_id":"nothing"}` | NoParcel | 404 |  |
| UX5 | POST | /cancellations | `{"parcel_id":"{parcel_id}"}` | Applied | 201 |  |
| UX6 | POST | /cancellations | `{"parcel_id":"{parcel_id}"}` | WrongStatus | 409 |  |
| UX7 | POST | /reconnections | `{}` | － | 201 |  |
| UX8 | PUT | /parcels/{parcel_id} | `{"kind":"light"}` | Stamped | 200 |  |
| UX9 | PUT | /parcels/nothing | `{"kind":"light"}` | NoParcel | 404 |  |
| UX10 | GET | /parcels/{parcel_id}/details |  | － | 200 |  |
| UX11 | DELETE | /parcels/{parcel_id} |  | NotRemovable | 409 |  |
| UX12 | DELETE | /parcels/nothing |  | NoParcel | 404 |  |
| UX13 | POST | /openings | `{}` | － | 201 |  |
