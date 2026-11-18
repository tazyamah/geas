// SPDX-License-Identifier: GPL-3.0-or-later
//! Python の int と float の使い分け（表示が「38」か「38.0」かが変わる）を、そのまま持つ数。図（SVG）の座標を、Python 版と同じ文字にするために使う。
use std::ops::{Add, Div, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug)]
pub enum N {
    I(i64),
    F(f64),
}

impl N {
    pub fn f(self) -> f64 {
        match self {
            N::I(i) => i as f64,
            N::F(f) => f,
        }
    }
    /// 小数 1 桁（Python の `:.1f`）
    pub fn d1(self) -> String {
        format!("{:.1}", self.f())
    }
    /// 小数 0 桁（Python の `:.0f`）
    pub fn d0(self) -> String {
        format!("{:.0}", self.f())
    }
}

impl std::fmt::Display for N {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            N::I(i) => write!(f, "{}", i),
            N::F(x) => {
                if x.is_finite() && x.fract() == 0.0 && x.abs() < 1e16 {
                    write!(f, "{:.1}", x)
                } else {
                    write!(f, "{}", x)
                }
            }
        }
    }
}

impl From<i64> for N {
    fn from(v: i64) -> N {
        N::I(v)
    }
}
impl From<usize> for N {
    fn from(v: usize) -> N {
        N::I(v as i64)
    }
}
impl From<f64> for N {
    fn from(v: f64) -> N {
        N::F(v)
    }
}

macro_rules! arith {
    ($tr:ident, $m:ident, $op:tt) => {
        impl $tr<N> for N {
            type Output = N;
            fn $m(self, o: N) -> N {
                match (self, o) {
                    (N::I(a), N::I(b)) => N::I(a $op b),
                    (a, b) => N::F(a.f() $op b.f()),
                }
            }
        }
        impl $tr<i64> for N {
            type Output = N;
            fn $m(self, o: i64) -> N {
                self $op N::I(o)
            }
        }
        impl $tr<f64> for N {
            type Output = N;
            fn $m(self, o: f64) -> N {
                self $op N::F(o)
            }
        }
        impl $tr<N> for i64 {
            type Output = N;
            fn $m(self, o: N) -> N {
                N::I(self) $op o
            }
        }
    };
}
arith!(Add, add, +);
arith!(Sub, sub, -);
arith!(Mul, mul, *);

/// Python の `/`（いつも float）
impl Div<N> for N {
    type Output = N;
    fn div(self, o: N) -> N {
        N::F(self.f() / o.f())
    }
}
impl Div<i64> for N {
    type Output = N;
    fn div(self, o: i64) -> N {
        N::F(self.f() / o as f64)
    }
}
impl Neg for N {
    type Output = N;
    fn neg(self) -> N {
        match self {
            N::I(i) => N::I(-i),
            N::F(f) => N::F(-f),
        }
    }
}
impl PartialEq for N {
    fn eq(&self, o: &N) -> bool {
        self.f() == o.f()
    }
}
impl PartialOrd for N {
    fn partial_cmp(&self, o: &N) -> Option<std::cmp::Ordering> {
        self.f().partial_cmp(&o.f())
    }
}

/// Python の max(a, b)：b が真に大きいときだけ b、そうでなければ a（元の型のまま）
pub fn pymax(a: N, b: N) -> N {
    if b > a {
        b
    } else {
        a
    }
}
pub fn pymin(a: N, b: N) -> N {
    if b < a {
        b
    } else {
        a
    }
}
/// Python の max(列)：最初の最大の要素
pub fn pymax_of(xs: impl IntoIterator<Item = N>) -> N {
    let mut it = xs.into_iter();
    let mut m = it.next().expect("max of empty");
    for x in it {
        m = pymax(m, x);
    }
    m
}
pub fn pymin_of(xs: impl IntoIterator<Item = N>) -> N {
    let mut it = xs.into_iter();
    let mut m = it.next().expect("min of empty");
    for x in it {
        m = pymin(m, x);
    }
    m
}

/// Python の html.escape（quote=True）
pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#x27;"),
            c => o.push(c),
        }
    }
    o
}

/// Python 3.12 以降の sum()：整数だけなら整数、小数が混じれば補正つきの和（Neumaier）
pub fn pysum(xs: impl IntoIterator<Item = N>) -> N {
    let (mut isum, mut f, mut c, mut float_mode) = (0i64, 0f64, 0f64, false);
    for x in xs {
        match (float_mode, x) {
            (false, N::I(i)) => isum += i,
            _ => {
                if !float_mode {
                    float_mode = true;
                    f = isum as f64;
                }
                let v = match x {
                    N::I(i) => {
                        f += i as f64;
                        continue;
                    }
                    N::F(v) => v,
                };
                let t = f + v;
                if f.abs() >= v.abs() {
                    c += (f - t) + v;
                } else {
                    c += (v - t) + f;
                }
                f = t;
            }
        }
    }
    if !float_mode {
        N::I(isum)
    } else {
        if c != 0.0 && c.is_finite() {
            f += c;
        }
        N::F(f)
    }
}
