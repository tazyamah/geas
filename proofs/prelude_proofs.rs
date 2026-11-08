// SPDX-License-Identifier: MIT OR Apache-2.0
// 生成物の固定の前置き（geas/rustgen.py の PRELUDE）の証明。Kani（モデル検査）で、どんな入力でも成り立つことを示す。
// 前置きの末尾に連結して使う（python proofs/run.py）。アプリごとには変わらないので、一度示せば全アプリに効く。
//
// 書き方の約束：
// - Result は unwrap せず match か matches! で見る（unwrap は異常の Debug 表示を展開して終わらなくなる）。
// - format! を含む関数は証明しない。判定（range_ok・ratio_ok など、真偽だけを返す純粋な関数）に分けて、そちらを示す。
// - 128 ビットの掛け算・剰余で値の正しさを示すのは重い（floor の商と余りの検算は 200 秒で終わらなかった）ので、今は入れない。

#[cfg(kani)]
mod proofs {
    use super::*;

    fn any_ratio() -> Ratio {
        Ratio { num: kani::any(), den: kani::any() }
    }

    /// 範囲の判定は「lo <= x <= hi」と同値
    #[kani::proof]
    fn range_ok_exact() {
        let (x, lo, hi): (i128, i128, i128) = (kani::any(), kani::any(), kani::any());
        assert!(range_ok(x, Some(lo), Some(hi)) == (x >= lo && x <= hi));
    }

    /// 割合の判定が通れば、分母は 1 以上 RATIO_DEN_MAX 以下（以後の掛け算・割り算が分母 0 を踏まない根拠）
    #[kani::proof]
    fn ratio_ok_den_bounds() {
        let x = any_ratio();
        let (lo, hi): (i128, i128) = (kani::any(), kani::any());
        if ratio_ok(&x, Some(lo), Some(hi)) {
            assert!(x.den > 0 && x.den <= RATIO_DEN_MAX);
        }
    }

    /// 分母が 0 以下の割合は、floor・ceil・half_up のどれも、panic せず異常を返す
    #[kani::proof]
    fn rounding_rejects_nonpositive_den() {
        let x = any_ratio();
        kani::assume(x.den <= 0);
        assert!(matches!(floor(x), Err(_)));
        assert!(matches!(ceil(x), Err(_)));
        assert!(matches!(half_up(x), Err(_)));
    }

    /// 割合の計算は、どんな入力でも panic しない（あふれは Fault で返る）
    #[kani::proof]
    fn ratio_arithmetic_never_panics() {
        let _ = mul_ratio(any_ratio(), any_ratio());
        let _ = scale(kani::any(), any_ratio());
        let _ = floor(any_ratio());
        let _ = ceil(any_ratio());
        let _ = half_up(any_ratio());
    }
}
