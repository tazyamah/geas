// SPDX-License-Identifier: GPL-3.0-or-later
//! 構文・モジュール・字句のエラー。Python 版の ParseError・ModuleError・LexError に当たる。
use crate::lexer::{file_name, LexError};

/// 構文エラー。line・col は位置（col は 1 から。分からなければ 0）、file は use で取り込んだファイルのとき、
/// hint は他の言語の書き方らしいときなどの直し方（文言のあとに続けて出す）
#[derive(Debug, Clone, Default)]
pub struct ParseError {
    pub msg: String,
    pub line: i64,
    pub col: i64,
    pub file: String,
    pub hint: String,
}

impl ParseError {
    pub fn new(msg: impl Into<String>, line: i64) -> ParseError {
        ParseError { msg: msg.into(), line, ..Default::default() }
    }
    pub fn at(msg: impl Into<String>, line: i64, col: i64, file: &str, hint: impl Into<String>) -> ParseError {
        ParseError { msg: msg.into(), line, col, file: file.to_string(), hint: hint.into() }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let head = (if self.file.is_empty() { String::new() } else { format!("{} の ", file_name(&self.file)) }) + &format!("{}行目", self.line);
        write!(f, "{}: {}{}", head, self.msg, if self.hint.is_empty() { String::new() } else { format!("。{}", self.hint) })
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModuleError {
    pub msg: String,
    pub line: i64,
    pub file: String,
}

impl ModuleError {
    pub fn new(msg: impl Into<String>, line: i64) -> ModuleError {
        ModuleError { msg: msg.into(), line, file: String::new() }
    }
}

impl std::fmt::Display for ModuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let head = (if self.file.is_empty() { String::new() } else { format!("{} の ", file_name(&self.file)) })
            + &(if self.line != 0 { format!("{}行目", self.line) } else { String::new() });
        if head.is_empty() {
            write!(f, "{}", self.msg)
        } else {
            write!(f, "{}: {}", head, self.msg)
        }
    }
}

/// 構文解析までの段階で起きるエラー（字句・構文・モジュール）。CLI はどれも「構文エラー」として出す
#[derive(Debug, Clone)]
pub enum Error {
    Lex(LexError),
    Parse(ParseError),
    Module(ModuleError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Lex(e) => write!(f, "{}", e),
            Error::Parse(e) => write!(f, "{}", e),
            Error::Module(e) => write!(f, "{}", e),
        }
    }
}

impl From<LexError> for Error {
    fn from(e: LexError) -> Self {
        Error::Lex(e)
    }
}
impl From<ParseError> for Error {
    fn from(e: ParseError) -> Self {
        Error::Parse(e)
    }
}
impl From<ModuleError> for Error {
    fn from(e: ModuleError) -> Self {
        Error::Module(e)
    }
}
