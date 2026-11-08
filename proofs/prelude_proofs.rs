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

    /// 文字の判定：2 文字までの文字列で、「使えない文字が無い」ことと「すべての文字が範囲に入る」ことは同値
    #[kani::proof]
    #[kani::unwind(4)]
    fn first_bad_char_exact() {
        let (c1, c2, a, b): (char, char, char, char) = (kani::any(), kani::any(), kani::any(), kani::any());
        let allowed = [(a, b)];
        let inside = |c: char| a <= c && c <= b;
        assert!(first_bad_in([c1, c2].into_iter(), &allowed).is_none() == (inside(c1) && inside(c2)));
        if let Some(bad) = first_bad_in([c1, c2].into_iter(), &allowed) {
            assert!(!inside(bad) && (bad == c1 || bad == c2));
        }
    }

    /// GTIN の判定：3 文字までの文字列で、「全部が数字で、右から 1 倍・3 倍・1 倍…の和が 10 の倍数」と同値
    #[kani::proof]
    #[kani::unwind(5)]
    fn gtin_ok_exact() {
        let (c1, c2, c3): (char, char, char) = (kani::any(), kani::any(), kani::any());
        let expected = match (c1.to_digit(10), c2.to_digit(10), c3.to_digit(10)) {
            (Some(d1), Some(d2), Some(d3)) => (d3 + d2 * 3 + d1) % 10 == 0,
            _ => false,
        };
        assert!(gtin_chars_ok([c1, c2, c3].into_iter()) == expected);
    }

    /// 幅とかなのそろえは、どんな 1 文字・どんな指定でも panic せず、範囲外の文字は変えず、範囲内は決まった幅だけずらす
    #[kani::proof]
    fn fold_width_kana_exact() {
        let c: char = kani::any();
        let (kana, width): (bool, bool) = (kani::any(), kani::any());
        let r = fold_width_kana(c, kana, width);
        let n = c as u32;
        let wide = width && (0xFF01..=0xFF5E).contains(&n);
        let katakana = kana && (0x30A1..=0x30F6).contains(&n);
        if wide { assert!(r as u32 == n - 0xFEE0); }
        else if katakana { assert!(r as u32 == n - 0x60); }
        else { assert!(r == c); }
    }

    /// 13 桁（JAN）の判定：どんな 13 文字でも、「全部が数字で、右から 1 倍・3 倍…の和が 10 の倍数」と同値
    #[kani::proof]
    #[kani::unwind(15)]
    fn gtin_ok_exact_13() {
        let cs: [char; 13] = kani::any();
        let mut sum: u32 = 0;
        let mut all_digits = true;
        let mut i = 0;
        while i < 13 {
            match cs[12 - i].to_digit(10) {
                Some(d) => sum += if i % 2 == 1 { d * 3 } else { d },
                None => all_digits = false,
            }
            i += 1;
        }
        assert!(gtin_chars_ok(cs.into_iter()) == (all_digits && sum % 10 == 0));
    }
}
