// SPDX-License-Identifier: GPL-3.0-or-later
//! 値の範囲（区間）と 1 次の式。Python 版 check.py の iv_*・lin_* の移し。
//! Python の int と Fraction は、どちらも有理数（BigRational）で持つ。None は「限りなし」。
use crate::ast::*;
use indexmap::IndexMap;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};

pub type Q = BigRational;
pub type Iv = (Option<Q>, Option<Q>);
/// 1 次の式（名前 → 係数, 定数）
pub type Lin = (IndexMap<String, i128>, i128);

pub fn q(v: i128) -> Q {
    BigRational::from_integer(BigInt::from(v))
}
pub fn qi(v: BigInt) -> Q {
    BigRational::from_integer(v)
}
pub fn q_to_i128(v: &Q) -> Option<i128> {
    if v.is_integer() {
        v.to_integer().to_i128()
    } else {
        None
    }
}
/// Python の str(Fraction)／str(int) に当たる表示（整数なら分母は出さない）
pub fn q_show(v: &Q) -> String {
    if v.is_integer() {
        v.to_integer().to_string()
    } else {
        format!("{}/{}", v.numer(), v.denom())
    }
}
pub fn q_floor(v: &Q) -> BigInt {
    v.floor().to_integer()
}
pub fn q_ceil(v: &Q) -> BigInt {
    v.ceil().to_integer()
}
pub fn q_pow2(n: u32) -> Q {
    BigRational::from_integer(BigInt::one() << n)
}
pub fn q_pow10(n: u32) -> Q {
    BigRational::from_integer(num_traits::pow(BigInt::from(10), n as usize))
}

pub const RATIO_DEN_MAX: i128 = 1_000_000_000;
pub const RATIO_VAL_MAX: i128 = 1_000_000_000;
pub const TIME_LIMIT: i128 = 1_000_000_000_000_000;
pub const UP: &str = "#上";
pub const VERIFIED_STORES: [&str; 1] = ["sqlite"];
pub const TEXT_LIMIT: i128 = 1_048_576;
pub const LEN: &str = "#件数:";
pub const NAMED: &str = "#名";

pub fn ratio_num_limit() -> Q {
    q_pow2(125)
}

/// a + k * b
pub fn lin_add(a: &Lin, b: &Lin, k: i128) -> Lin {
    let mut out = a.0.clone();
    for (x, c) in &b.0 {
        *out.entry(x.clone()).or_insert(0) += k * c;
    }
    (out.into_iter().filter(|(_, c)| *c != 0).collect(), a.1 + k * b.1)
}

/// 1 次の式の値の範囲。ivs は 名前 → (下限, 上限)
pub fn lin_bounds(form: &Lin, ivs: &IndexMap<String, Iv>) -> Iv {
    let (mut lo, mut hi) = (Some(q(form.1)), Some(q(form.1)));
    for (x, c) in &form.0 {
        let (mut a, mut b) = ivs.get(x).cloned().unwrap_or((None, None));
        if *c < 0 {
            std::mem::swap(&mut a, &mut b);
        }
        let cq = q(*c);
        lo = match (lo, &a) {
            (Some(l), Some(a)) => Some(l + &cq * a),
            _ => None,
        };
        hi = match (hi, &b) {
            (Some(h), Some(b)) => Some(h + &cq * b),
            _ => None,
        };
    }
    (lo, hi)
}

pub fn iv_add(a: &Iv, b: &Iv) -> Iv {
    (
        match (&a.0, &b.0) {
            (Some(x), Some(y)) => Some(x + y),
            _ => None,
        },
        match (&a.1, &b.1) {
            (Some(x), Some(y)) => Some(x + y),
            _ => None,
        },
    )
}

pub fn iv_neg(a: &Iv) -> Iv {
    (a.1.as_ref().map(|x| -x), a.0.as_ref().map(|x| -x))
}

pub fn iv_mul(a: &Iv, b: &Iv) -> Iv {
    if let (Some(a0), Some(b0)) = (&a.0, &b.0) {
        if !a0.is_negative() && !b0.is_negative() {
            return (
                Some(a0 * b0),
                match (&a.1, &b.1) {
                    (Some(x), Some(y)) => Some(x * y),
                    _ => None,
                },
            );
        }
    }
    if a.0.is_none() || a.1.is_none() || b.0.is_none() || b.1.is_none() {
        return (None, None);
    }
    let (a0, a1, b0, b1) = (a.0.as_ref().unwrap(), a.1.as_ref().unwrap(), b.0.as_ref().unwrap(), b.1.as_ref().unwrap());
    let ps = [a0 * b0, a0 * b1, a1 * b0, a1 * b1];
    (ps.iter().min().cloned(), ps.iter().max().cloned())
}

/// min の値の範囲。下限は両方分かるときだけ、上限は分かるほうの小さいほう（Lean の Iv.min と対応）
pub fn iv_min(a: &Iv, b: &Iv) -> Iv {
    let lo = match (&a.0, &b.0) {
        (Some(x), Some(y)) => Some(x.min(y).clone()),
        _ => None,
    };
    let hi = match (&a.1, &b.1) {
        (Some(x), Some(y)) => Some(x.min(y).clone()),
        (Some(x), None) => Some(x.clone()),
        (None, Some(y)) => Some(y.clone()),
        (None, None) => None,
    };
    (lo, hi)
}

/// max の値の範囲（Lean の Iv.max と対応）
pub fn iv_max(a: &Iv, b: &Iv) -> Iv {
    let lo = match (&a.0, &b.0) {
        (Some(x), Some(y)) => Some(x.max(y).clone()),
        (Some(x), None) => Some(x.clone()),
        (None, Some(y)) => Some(y.clone()),
        (None, None) => None,
    };
    let hi = match (&a.1, &b.1) {
        (Some(x), Some(y)) => Some(x.max(y).clone()),
        _ => None,
    };
    (lo, hi)
}

/// 絶対値の値の範囲（Lean の Iv.abs と対応）
pub fn iv_abs(a: &Iv) -> Iv {
    let (lo, hi) = a.clone();
    if let Some(l) = &lo {
        if !l.is_negative() {
            return (lo, hi);
        }
    }
    if let Some(h) = &hi {
        if !h.is_positive() {
            return (Some(-h.clone()), lo.map(|l| -l));
        }
    }
    let top = match (&lo, &hi) {
        (Some(l), Some(h)) => Some((-l.clone()).max(h.clone())),
        _ => None,
    };
    (Some(q(0)), top)
}

pub fn phys_iv(name: &str) -> (Q, Q) {
    let b = |bits: u32| BigInt::one() << bits;
    match name {
        "u8" => (q(0), qi(b(8) - 1)),
        "u16" => (q(0), qi(b(16) - 1)),
        "u32" => (q(0), qi(b(32) - 1)),
        "u64" => (q(0), qi(b(64) - 1)),
        "i8" => (qi(-b(7)), qi(b(7) - 1)),
        "i16" => (qi(-b(15)), qi(b(15) - 1)),
        "i32" => (qi(-b(31)), qi(b(31) - 1)),
        "i64" => (qi(-b(63)), qi(b(63) - 1)),
        _ => (qi(-b(127)), qi(b(127) - 1)),
    }
}

/// 型の値の範囲。整数と金額は、書いた範囲に、物理数に収まる範囲を重ねる
pub fn iv_of(t: &Type) -> Iv {
    if t.kind == "整数" || t.kind == "金額" {
        let (plo, phi) = phys_iv(super::phys(t));
        let r = t.range.as_ref();
        return (
            Some(match r.and_then(|r| r.lo) {
                Some(v) => q(v),
                None => plo,
            }),
            Some(match r.and_then(|r| r.hi) {
                Some(v) => q(v),
                None => phi,
            }),
        );
    }
    if t.kind == "割合" {
        let r = t.range.as_ref();
        return (Some(q(r.and_then(|r| r.lo).unwrap_or(-RATIO_VAL_MAX))), Some(q(r.and_then(|r| r.hi).unwrap_or(RATIO_VAL_MAX))));
    }
    (None, None)
}

pub fn iv_show(a: &Iv) -> String {
    format!("{}..{}", a.0.as_ref().map_or(String::new(), q_show), a.1.as_ref().map_or(String::new(), q_show))
}

/// 2つの値の範囲の合流（どちらにもなりうる値の範囲）
pub fn iv_join_one(a: &Iv, b: &Iv) -> Iv {
    (
        match (&a.0, &b.0) {
            (Some(x), Some(y)) => Some(x.min(y).clone()),
            _ => None,
        },
        match (&a.1, &b.1) {
            (Some(x), Some(y)) => Some(x.max(y).clone()),
            _ => None,
        },
    )
}

pub fn zero() -> Q {
    Q::zero()
}
