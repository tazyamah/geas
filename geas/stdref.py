# SPDX-License-Identifier: GPL-3.0-or-later
"""標準部品（geas/std/num.geas）の手本：Rust の整数型のメソッドを、Python の整数（桁の限りがない）で書き直したもの。

標準部品の宣言は、Rust のドキュメントから機械で作る。その宣言（呼び出す式・型の写し方・範囲）が合っているかを、
Rust とは別に書いた計算と突き合わせて確かめるために置く（geas/partcheck.py が、範囲の端の値で呼んだ結果と比べる）。
新しいメソッドを標準部品に足したら、ここにも手本を足す（足し忘れは tests/test_parts.py が止める）。

to_be・to_le は実行環境のバイトの並びで結果が変わる。ここの手本は、リトルエンディアン（x86-64・aarch64）のもの。
"""
import math
import re


def wrap(v: int, bits: int, signed: bool) -> int:
    """桁あふれを切り捨てた値（2 の補数）"""
    v &= (1 << bits) - 1
    return v - (1 << bits) if signed and v >> (bits - 1) else v


def sat(v: int, bits: int, signed: bool) -> int:
    """範囲の端で止めた値"""
    lo, hi = (-(1 << (bits - 1)), (1 << (bits - 1)) - 1) if signed else (0, (1 << bits) - 1)
    return max(lo, min(hi, v))


def u(a: int, bits: int) -> int:
    """ビットの並びとして見た値（0 以上）"""
    return a & ((1 << bits) - 1)


def trailing_zeros(x: int, bits: int) -> int:
    return bits if x == 0 else (x & -x).bit_length() - 1


def rev(x: int, bits: int) -> int:
    return int(format(x, f"0{bits}b")[::-1], 2)


def swap(x: int, bits: int) -> int:
    return int.from_bytes(x.to_bytes(bits // 8, "little"), "big")


def rot(x: int, n: int, bits: int) -> int:
    n %= bits
    return ((x << n) | (x >> (bits - n))) & ((1 << bits) - 1)


def trunc_half(s: int) -> int:
    """2 で割って、0 の側へ丸める"""
    return -((-s) // 2) if s < 0 else s // 2


def sat_pow(bits: int, signed: bool, a: int, n: int) -> int:
    """a の n 乗を、範囲の端で止めた値。n は 2^32 近くまであるので、そのまま計算せず、桁が足りなくなる所で打ち切る"""
    if a in (-1, 0, 1):
        return 1 if n == 0 else a if n % 2 else a * a
    return sat(a ** min(n, bits + (n - bits) % 2 if n > bits else n), bits, signed)


# 名前 → 手本。引数は (ビット数, 符号つきか, 引数…)。戻り値の型が self と違うもの（個数・真偽・符号を変える型）は、そのままの値を返す
REF = {
    "abs_diff": lambda B, S, a, b: abs(a - b),
    "bit_width": lambda B, S, a: a.bit_length(),
    "cast_signed": lambda B, S, a: wrap(a, B, True),
    "cast_unsigned": lambda B, S, a: u(a, B),
    "count_ones": lambda B, S, a: bin(u(a, B)).count("1"),
    "count_zeros": lambda B, S, a: B - bin(u(a, B)).count("1"),
    "is_negative": lambda B, S, a: a < 0,
    "is_positive": lambda B, S, a: a > 0,
    "is_power_of_two": lambda B, S, a: a > 0 and a & (a - 1) == 0,
    "isolate_highest_one": lambda B, S, a: wrap(0 if u(a, B) == 0 else 1 << (u(a, B).bit_length() - 1), B, S),
    "isolate_lowest_one": lambda B, S, a: wrap(u(a, B) & -u(a, B), B, S),
    "isqrt": lambda B, S, a: math.isqrt(a),
    "leading_ones": lambda B, S, a: B - (u(~a, B)).bit_length(),
    "leading_zeros": lambda B, S, a: B - u(a, B).bit_length(),
    "trailing_ones": lambda B, S, a: trailing_zeros(u(~a, B), B),
    "trailing_zeros": lambda B, S, a: trailing_zeros(u(a, B), B),
    "midpoint": lambda B, S, a, b: trunc_half(a + b) if S else (a + b) // 2,
    "reverse_bits": lambda B, S, a: wrap(rev(u(a, B), B), B, S),
    "swap_bytes": lambda B, S, a: wrap(swap(u(a, B), B), B, S),
    "to_be": lambda B, S, a: wrap(swap(u(a, B), B), B, S),
    "to_le": lambda B, S, a: a,
    "rotate_left": lambda B, S, a, n: wrap(rot(u(a, B), n, B), B, S),
    "rotate_right": lambda B, S, a, n: wrap(rot(u(a, B), -n, B), B, S),
    "saturating_abs": lambda B, S, a: sat(abs(a), B, S),
    "saturating_add": lambda B, S, a, b: sat(a + b, B, S),
    "saturating_add_signed": lambda B, S, a, b: sat(a + b, B, S),
    "saturating_add_unsigned": lambda B, S, a, b: sat(a + b, B, S),
    "saturating_sub": lambda B, S, a, b: sat(a - b, B, S),
    "saturating_sub_signed": lambda B, S, a, b: sat(a - b, B, S),
    "saturating_sub_unsigned": lambda B, S, a, b: sat(a - b, B, S),
    "saturating_mul": lambda B, S, a, b: sat(a * b, B, S),
    "saturating_neg": lambda B, S, a: sat(-a, B, S),
    "saturating_pow": lambda B, S, a, n: sat_pow(B, S, a, n),
    "signum": lambda B, S, a: (a > 0) - (a < 0),
    "unbounded_shl": lambda B, S, a, n: 0 if n >= B else wrap(u(a, B) << n, B, S),
    "unbounded_shr": lambda B, S, a, n: (-1 if a < 0 else 0) if n >= B else a >> n,
    "wrapping_abs": lambda B, S, a: wrap(abs(a), B, S),
    "wrapping_add": lambda B, S, a, b: wrap(a + b, B, S),
    "wrapping_add_signed": lambda B, S, a, b: wrap(a + b, B, S),
    "wrapping_add_unsigned": lambda B, S, a, b: wrap(a + b, B, S),
    "wrapping_sub": lambda B, S, a, b: wrap(a - b, B, S),
    "wrapping_sub_signed": lambda B, S, a, b: wrap(a - b, B, S),
    "wrapping_sub_unsigned": lambda B, S, a, b: wrap(a - b, B, S),
    "wrapping_mul": lambda B, S, a, b: wrap(a * b, B, S),
    "wrapping_neg": lambda B, S, a: wrap(-a, B, S),
    "wrapping_pow": lambda B, S, a, n: wrap(pow(a, n, 1 << B), B, S),
}

NAME = re.compile(r"^Num_([iu])(8|16|32|64)_(\w+)$")


def reference(parts) -> dict:
    """部品名 → 手本の関数（引数は geas の値）。手本の無い部品は入らない"""
    out = {}
    for pt in parts:
        m = NAME.match(pt.name)
        if m and m.group(3) in REF:
            f, bits, signed = REF[m.group(3)], int(m.group(2)), m.group(1) == "i"
            out[pt.name] = lambda *args, f=f, bits=bits, signed=signed: f(bits, signed, *args)
    return out
