# geas の生成を Lean で証明する（試作）

目的：テストも生成器も AI が書いているために起きる「AI どうしの共倒れ」を、機械が確かめる証明で断つ。いまは小さな核 1 だけの試作です。

## いま証明してあるもの（`Geas/Arith.lean`）

式（数・名前・符号反転・絶対値・足す・引く・掛ける・min・max）について、次の 3 つを Lean 4 で証明しています（`sorry` なし。使う公理は Lean 標準の propext・Classical.choice・Quot.sound のみ）。

1. `sound`：値が宣言の範囲に入っていれば、式の値は検査器が計算した区間に入る。
2. `checked`：検査が通れば（途中の計算がどれも i128 に収まると示せれば）、生成した Rust の `checked_*` はあふれず、`.geas` の意味（桁の限りがない整数）と同じ値を返す。
3. `wrapping`：同じ検査で、`wrapping_*`（`trust prove`／`assume` の式）も同じ値を返す。

## 実装との対応

| Lean | Rust |
|---|---|
| `Iv.add`／`Iv.neg`／`Iv.mul` | `crates/geas-core/src/check/num.rs` の `iv_add`／`iv_neg`／`iv_mul` |
| `Iv.min`／`Iv.max`／`Iv.abs` | `check/procs2.rs` の `interval`（min・max・abs の枝） |
| `Expr.ok`（演算の区間が i128 に収まる） | `check/procs3.rs` の `check_calc`（収まれば `prove`） |
| `Expr.evalC` | `rustgen.rs` の `checked_add`／`checked_sub`／`checked_mul`／`checked_abs` |
| `Expr.evalW` | `rustgen.rs` の `wrapping_*`（`prove`・`assume`） |

## 範囲の外（これから）

区間は Rust では有理数（BigRational）、ここでは整数。割り算・剰余・if 式・割合（Ratio）・文字列・集合の合計・`verify`（検証ビルドだけ確かめる）・時刻の差は未対応。

## Rust との突き合わせ

`crates/geas-core/tests/lean_diff.rs` が、区間の計算（`iv_add`・`iv_neg`・`iv_mul`・`iv_min`・`iv_max`・`iv_abs`）に乱数の入力 3,000 件（固定の種。限りなし・0・大きな値を含む）を渡し、Rust の答えと、Lean のモデル（`Geas/Diff.lean` 経由）の答えが同じであることを確かめます。`lean` が PATH に無ければ飛ばします（`GEAS_LEAN` で場所を指定）。`iv_min`・`iv_max`・`iv_abs` は、突き合わせるために `interval` の中から `num.rs` に出した関数です（動きは変えていません）。まだ機械で確かめていないのは、`check_calc` の「収まる」の判定と、`rustgen.rs` の生成文字列です。

## 動かし方

```sh
lean Geas/Arith.lean    # Lean 4（4.33 で確認）。エラーなしで終わり、公理の一覧が出れば証明が通っている
```
