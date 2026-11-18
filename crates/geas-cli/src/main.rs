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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let code = match args.get(1).map(|s| s.as_str()) {
        Some("astdump") if args.len() == 3 => astdump(&args[2]),
        Some("check") if args.len() == 4 && args[3] == "--json" => check_json(&args[2]),
        _ => {
            eprintln!("geas-rs: まだ astdump だけが動きます（Python 版を移している途中）");
            2
        }
    };
    std::process::exit(code);
}
