// SPDX-License-Identifier: GPL-3.0-or-later
//! 区間計算（check/num.rs）と、Lean で証明したモデル（proofs/lean/Geas/Arith.lean）の突き合わせ。
//! 乱数の入力（固定の種）で、同じ区間が返ることを確かめる。Lean（`lean` の実行ファイル）が無ければ飛ばす。
//! `lean` の場所は環境変数 GEAS_LEAN（無ければ PATH の lean）。
use geas_core::check::num::*;
use std::io::Write;
use std::process::{Command, Stdio};

fn next(s: &mut u64) -> u64 {
    *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *s >> 33
}

fn bound(s: &mut u64) -> Option<i128> {
    match next(s) % 12 {
        0 => None,
        1 => Some(i128::MAX / 3),
        2 => Some(i128::MIN / 3),
        3 => Some(0),
        _ => Some((next(s) % 41) as i128 - 20),
    }
}

fn show(b: &Option<Q>) -> String {
    b.as_ref().map_or("_".to_string(), q_show)
}

#[test]
fn lean_diff() {
    let lean = std::env::var("GEAS_LEAN").unwrap_or_else(|_| "lean".to_string());
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../proofs/lean");
    if Command::new(&lean).arg("--version").output().is_err() {
        eprintln!("lean が無いので飛ばす");
        return;
    }
    // Lean 側を、モデルの証明ごと組む
    let st = Command::new(&lean).current_dir(&root).args(["-o", "Geas/Arith.olean", "Geas/Arith.lean"]).status().unwrap();
    assert!(st.success(), "Arith.lean が通らない");
    let mut seed = 20261128u64;
    let ops = ["add", "neg", "mul", "min", "max", "abs"];
    let (mut input, mut expect) = (String::new(), Vec::new());
    for _ in 0..3000 {
        let op = ops[(next(&mut seed) % 6) as usize];
        // 下限 ≤ 上限 になるように、2 点から作る（None はそのまま）
        let mk = |s: &mut u64| -> (Option<i128>, Option<i128>) {
            let (x, y) = (bound(s), bound(s));
            match (x, y) {
                (Some(x), Some(y)) => (Some(x.min(y)), Some(x.max(y))),
                p => p,
            }
        };
        let (a, b) = (mk(&mut seed), mk(&mut seed));
        let ia: Iv = (a.0.map(q), a.1.map(q));
        let ib: Iv = (b.0.map(q), b.1.map(q));
        let r = match op {
            "add" => iv_add(&ia, &ib),
            "neg" => iv_neg(&ia),
            "mul" => iv_mul(&ia, &ib),
            "min" => iv_min(&ia, &ib),
            "max" => iv_max(&ia, &ib),
            _ => iv_abs(&ia),
        };
        input.push_str(&format!("{} {} {} {} {}\n", op, show(&ia.0), show(&ia.1), show(&ib.0), show(&ib.1)));
        expect.push(format!("{} {}", show(&r.0), show(&r.1)));
    }
    let mut child = Command::new(&lean)
        .current_dir(&root)
        .env("LEAN_PATH", &root)
        .args(["--run", "Geas/Diff.lean"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "Diff.lean が動かない");
    let got: Vec<String> = String::from_utf8(out.stdout).unwrap().lines().map(|l| l.to_string()).collect();
    assert_eq!(got.len(), expect.len());
    let lines: Vec<&str> = input.lines().collect();
    for (i, (g, e)) in got.iter().zip(&expect).enumerate() {
        assert_eq!(g, e, "Lean と Rust で区間が違う: {}", lines[i]);
    }
}
