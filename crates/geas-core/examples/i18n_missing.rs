// SPDX-License-Identifier: GPL-3.0-or-later
//! 対訳表に無くて、英語モードでも日本語のまま残る文を挙げる。標準入力は 1 行 1 文（`\n` はそのまま 2 文字）。
//! 使い方：... | cargo run -q -p geas-core --example i18n_missing
//! `--show` を付けると、全部の文を英語にした結果を出す（確かめ用）
use std::io::BufRead;

fn main() {
    let show = std::env::args().any(|a| a == "--show");
    for l in std::io::stdin().lock().lines() {
        let l = l.unwrap();
        let s = l.replace("\\n", "\n");
        if show {
            println!("{}", geas_core::i18n::to_en(&s).replace('\n', "\\n"));
            continue;
        }
        if !geas_core::i18n::untranslated(&s).is_empty() {
            println!("{}", l);
        }
    }
}
