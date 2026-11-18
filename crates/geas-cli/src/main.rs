// SPDX-License-Identifier: GPL-3.0-or-later
use geas_core::error::Error;
use geas_core::parser::parse;
use std::path::Path;

fn error_json(e: &Error) -> serde_json::Value {
    let (kind, msg, line, col, file, hint) = match e {
        Error::Lex(e) => ("Lex", e.msg.clone(), e.line, e.col, e.file.clone(), String::new()),
        Error::Parse(e) => ("Parse", e.msg.clone(), e.line, e.col, e.file.clone(), e.hint.clone()),
        Error::Module(e) => ("Module", e.msg.clone(), e.line, 0, e.file.clone(), String::new()),
    };
    let name = Path::new(&file).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    serde_json::json!({"_error": kind, "text": e.to_string(), "msg": msg, "line": line, "col": col, "file": name, "hint": hint})
}

fn astdump(path: &str) -> i32 {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}: {}", path, e);
            return 2;
        }
    };
    match parse(&src, Some(Path::new(path))) {
        Ok(p) => {
            println!("{}", serde_json::to_string(&p).unwrap());
            0
        }
        Err(e) => {
            println!("{}", error_json(&e));
            2
        }
    }
}

fn check_json(path: &str) -> i32 {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}: {}", path, e);
            return 2;
        }
    };
    match parse(&src, Some(Path::new(path))) {
        Err(e) => {
            print!("{}", geas_core::hints::syntax_json(path, &e));
            2
        }
        Ok(mut prog) => {
            let ck = geas_core::check::defs::check(&mut prog, true);
            print!("{}", geas_core::hints::to_json(ck.ok(), ck.ready(), &ck.diags, path));
            if ck.ok() { 0 } else { 1 }
        }
    }
}

/// 構文エラーの表示：位置と文言、その行の抜き出しと、字句の位置の印
fn syntax_error_text(e: &Error, path: &str) -> String {
    use unicode_width::UnicodeWidthChar;
    let (line, col, file) = match e {
        Error::Lex(x) => (x.line, x.col, x.file.clone()),
        Error::Parse(x) => (x.line, x.col, x.file.clone()),
        Error::Module(x) => (x.line, 0, x.file.clone()),
    };
    let file = if file.is_empty() { path.to_string() } else { file };
    let mut out = vec![format!("構文エラー: {}", e)];
    if line != 0 {
        let src: Vec<String> = std::fs::read_to_string(&file).map(|s| s.split('\n').map(|x| x.to_string()).collect()).unwrap_or_default();
        if line > 0 && (line as usize) <= src.len() && !src[line as usize - 1].trim().is_empty() {
            let text = src[line as usize - 1].trim_end().to_string();
            out.push(format!("  {:>4} | {}", line, text));
            let n = text.chars().count() as i64;
            if col > 0 && col <= n + 1 {
                let pad: String = text.chars().take(col as usize - 1).map(|c| if c == '\t' { "\t".to_string() } else { " ".repeat(if c.width().unwrap_or(1) == 2 { 2 } else { 1 }) }).collect();
                out.push(format!("  {:>4} | {}^", "", pad));
            }
        }
    }
    out.join("\n")
}

fn spec_cmd(path: &str, out: Option<&str>) -> i32 {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}: {}", path, e);
            return 2;
        }
    };
    let mut prog = match parse(&src, Some(Path::new(path))) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", syntax_error_text(&e, path));
            return 2;
        }
    };
    let mut ck = geas_core::check::defs::check(&mut prog, true);
    ck.code_path = Some(path.to_string());
    let name = Path::new(path).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let spec = geas_core::spec::build(&ck);
    let text = geas_core::spec::render(&ck, &spec, &format!("（{}）", name));
    match out {
        Some(o) => std::fs::write(o, &text).unwrap(),
        None => println!("{}", text),
    }
    if ck.ok() { 0 } else { 1 }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let code = match args.get(1).map(|s| s.as_str()) {
        Some("astdump") if args.len() == 3 => astdump(&args[2]),
        Some("spec") if args.len() == 3 => spec_cmd(&args[2], None),
        Some("spec") if args.len() == 5 && args[3] == "-o" => spec_cmd(&args[2], Some(&args[4])),
        Some("check") if args.len() == 4 && args[3] == "--json" => check_json(&args[2]),
        _ => {
            eprintln!("geas-rs: まだ astdump だけが動きます（Python 版を移している途中）");
            2
        }
    };
    std::process::exit(code);
}
