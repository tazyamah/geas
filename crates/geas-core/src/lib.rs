// SPDX-License-Identifier: GPL-3.0-or-later
//! geas 本体（字句・構文・検査・仕様書・生成）の Rust 版。いまは Python 版（geas/）を移している途中。
pub mod ast;
pub mod error;
pub mod foreign;
pub mod lexer;
pub mod modules;
pub mod parser;
