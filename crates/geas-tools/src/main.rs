// SPDX-License-Identifier: GPL-3.0-or-later
//! リポジトリの保守用の道具。geas 本体（検査器・生成器）の依存ではない。
//!
//! - `geas-tools site 出力先`：docs/・skills/・見本の計画書の Markdown から、文書サイト（GitHub Pages 用）を作る
//! - `geas-tools proofs [--timeout 秒]`：生成物の前置きの証明（proofs/prelude_proofs.rs）を Kani で実行する
//! - `geas-tools std-proofs [--timeout 秒] [--jobs 数] [--record]`：標準部品の証明を Kani で実行する
mod proofs;
mod site;

use std::path::{Path, PathBuf};

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(|s| s.as_str()) {
        Some("site") if args.len() == 2 => match site::run(&repo_root(), Path::new(&args[1])) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{}", e);
                1
            }
        },
        Some(c @ ("proofs" | "std-proofs")) => {
            let opt = |name: &str, dflt: u64| -> u64 { args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(dflt) };
            let r = if c == "proofs" {
                proofs::prelude(opt("--timeout", 300)).map(|bad| if bad == 0 { 0 } else { 1 })
            } else {
                proofs::std_main(opt("--timeout", 60), opt("--jobs", 4) as u32, args.iter().any(|a| a == "--record"), args.iter().any(|a| a == "--print-source"))
            };
            match r {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{}", e);
                    1
                }
            }
        }
        _ => {
            eprintln!("使い方: geas-tools site 出力先 | proofs [--timeout 秒] | std-proofs [--timeout 秒] [--jobs 数] [--record]");
            2
        }
    };
    std::process::exit(code);
}
