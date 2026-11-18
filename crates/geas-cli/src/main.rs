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

fn spec_cmd(path: &str, out: Option<&str>, plan: Option<&str>) -> i32 {
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
    let mut text = geas_core::spec::render(&ck, &spec, &format!("（{}）", name));
    if let Some(pp) = plan {
        let md = match std::fs::read_to_string(pp) { Ok(s) => s, Err(e) => { eprintln!("{}: {}", pp, e); return 2; } };
        text += &geas_core::plan::coverage_markdown(&spec, &geas_core::plan::read_plan(&md)).join("\n");
        text += "\n";
    }
    match out {
        Some(o) => std::fs::write(o, &text).unwrap(),
        None => println!("{}", text),
    }
    if ck.ok() { 0 } else { 1 }
}

/// 検査まで済ませた Checker を作る（読めなければ終了コードを返す）
fn load(path: &str, prog: &mut Option<geas_core::ast::Program>) -> Result<(), i32> {
    let src = std::fs::read_to_string(path).map_err(|e| {
        eprintln!("{}: {}", path, e);
        2
    })?;
    match parse(&src, Some(Path::new(path))) {
        Ok(p) => {
            *prog = Some(p);
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", syntax_error_text(&e, path));
            Err(2)
        }
    }
}

fn review_cmd(args: &[String]) -> i32 {
    let (mut file, mut out, mut plan, mut diff_o) = (None, None, None, None);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => { i += 1; out = args.get(i).cloned(); }
            "--plan" => { i += 1; plan = args.get(i).cloned(); }
            "--diff-o" => { i += 1; diff_o = args.get(i).cloned(); }
            a if file.is_none() => file = Some(a.to_string()),
            _ => { eprintln!("geas-rs: 解釈できない引数: {}", args[i]); return 2; }
        }
        i += 1;
    }
    let (Some(path), Some(out)) = (file, out) else { eprintln!("geas-rs: review FILE -o OUT"); return 2; };
    let mut prog = None;
    if let Err(c) = load(&path, &mut prog) { return c; }
    let mut prog = prog.unwrap();
    let mut ck = geas_core::check::defs::check(&mut prog, true);
    ck.code_path = Some(path.clone());
    let name = Path::new(&path).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let plan_md = match &plan {
        Some(p) => match std::fs::read_to_string(p) { Ok(s) => Some(s), Err(e) => { eprintln!("{}: {}", p, e); return 2; } },
        None => None,
    };
    let findings = match &plan_md {
        Some(m) => geas_core::plan::compare_static(&geas_core::spec::build(&ck), &geas_core::plan::read_plan(m)),
        None => vec![],
    };
    let label = format!("（{}）", name);
    let html = match geas_core::review::review(&ck, plan_md.as_deref(), &findings, &label, false) {
        Ok(h) => h,
        Err(e) => { eprintln!("{}", e); return 1; }
    };
    std::fs::write(&out, html).unwrap();
    if let Some(d) = diff_o {
        std::fs::write(&d, geas_core::review::review_changes(&ck, &findings, &label, None, "", false, None)).unwrap();
        println!("生成しました: {}", d);
    }
    println!("生成しました: {}", out);
    if ck.ok() { 0 } else { 1 }
}

fn counts_line(ck: &geas_core::check::Checker) -> String {
    use geas_core::check::{ERROR, QUESTION, WARN};
    let n = |lv: &str| ck.diags.iter().filter(|d| d.level == lv).count();
    let tally = format!("エラー {} 件・問い {} 件・警告 {} 件", n(ERROR), n(QUESTION), n(WARN));
    if n(ERROR) > 0 {
        return format!("検査: エラーがあります（{}）", tally);
    }
    if n(QUESTION) > 0 {
        return format!("検査: 通りました（{}）。問いは人に1問ずつ聞いて決めてください（答えるまで rust と compare は進みません）", tally);
    }
    format!("検査: 通りました{}", if n(WARN) > 0 { format!("（{}）", tally) } else { String::new() })
}

fn rust_cmd(args: &[String]) -> i32 {
    let (mut file, mut out, mut plan, mut seed, mut target) = (None, None, None, None, None);
    let mut server = geas_core::crategen::DEFAULT_SERVER.to_string();
    let (mut host, mut runtime, mut standalone) = (false, false, false);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => { i += 1; out = args.get(i).cloned(); }
            "--plan" => { i += 1; plan = args.get(i).cloned(); }
            "--seed" => { i += 1; seed = args.get(i).cloned(); }
            "--target" => { i += 1; target = args.get(i).cloned(); }
            "--server" => { i += 1; if let Some(s) = args.get(i) { server = s.clone(); } }
            "--host" => host = true,
            "--runtime" => runtime = true,
            "--standalone" => standalone = true,
            a if file.is_none() => file = Some(a.to_string()),
            _ => { eprintln!("geas-rs: 解釈できない引数: {}", args[i]); return 2; }
        }
        i += 1;
    }
    let (Some(path), Some(out)) = (file, out) else { eprintln!("geas-rs: rust FILE -o OUT"); return 2; };
    let mut bad: Vec<String> = vec![];
    for (k, v) in [("コード", Some(&path)), ("計画書", plan.as_ref()), ("seed", seed.as_ref())] {
        let _ = k;
        if let Some(v) = v {
            let p = Path::new(v);
            if !p.is_file() {
                bad.push(if !p.exists() { format!("ファイルがありません: {}", v) } else { format!("ファイルではありません: {}", v) });
            }
        }
    }
    if runtime && !host { bad.push("--runtime は --host と一緒に使います（サーバーの実行部は、画面の窓口と組みます）".into()); }
    if standalone && !(host && runtime) { bad.push("--standalone は --host --runtime と一緒に使います（窓つき exe は、サーバーの実行部と生成した画面を使います）".into()); }
    if seed.is_some() && !runtime { bad.push("--seed は --host --runtime と一緒に使います（最初のデータはサーバーが入れます）".into()); }
    if !bad.is_empty() {
        for b in bad { eprintln!("{}", b); }
        return 2;
    }
    let mut prog = None;
    if let Err(c) = load(&path, &mut prog) { return c; }
    let mut prog = prog.unwrap();
    let mut ck = geas_core::check::defs::check(&mut prog, true);
    ck.code_path = Some(path.clone());
    if !ck.ready() {
        for d in &ck.diags { println!("{}", d.for_code()); }
        println!("{}", counts_line(&ck));
        eprintln!("検査にエラーか問いが残っているので、続けません（問いに答えて、コードか計画書に反映してください）。");
        return 1;
    }
    if standalone && !ck.p.actors.is_empty() {
        eprintln!("役割（actor）のあるアプリは、窓つき exe（--standalone）にできません。窓つき exe には、役割を確かめるプロキシが前にいないためです。");
        return 1;
    }
    if standalone && !ck.p.paths.is_empty() {
        let tgt = target.clone().unwrap_or_else(|| if cfg!(windows) { "windows".into() } else if cfg!(target_os = "macos") { "macos".into() } else { "linux".into() });
        let lack: Vec<&str> = ck.p.paths.iter().filter(|(_, k, es, _)| k == "absolute" && !es.iter().any(|(o, _, _)| o == &tgt)).map(|(n, _, _, _)| n.as_str()).collect();
        if !lack.is_empty() {
            eprintln!("パス {} に、組む OS（{}）の行がありません。`{} \"…\"` を書くか、--target で組む OS を指定してください", lack.join("・"), tgt, tgt);
            return 1;
        }
    }
    if plan.is_some() {
        eprintln!("geas-rs: --plan（具体例の実行用 bin）はまだ移していません");
        return 2;
    }
    let dest = Path::new(&out);
    if let Err(e) = geas_core::crategen::generate_tree(&ck, dest, host, runtime, standalone, seed.as_deref(), &server) {
        eprintln!("{}", e);
        return 1;
    }
    println!("生成しました: {}", out);
    0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let code = match args.get(1).map(|s| s.as_str()) {
        Some("astdump") if args.len() == 3 => astdump(&args[2]),
        Some("spec") => {
            let (mut f, mut o, mut p) = (None, None, None);
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "-o" => { i += 1; o = args.get(i).cloned(); }
                    "--plan" => { i += 1; p = args.get(i).cloned(); }
                    a if f.is_none() => f = Some(a.to_string()),
                    _ => { eprintln!("geas-rs: 解釈できない引数: {}", args[i]); std::process::exit(2); }
                }
                i += 1;
            }
            match f { Some(f) => spec_cmd(&f, o.as_deref(), p.as_deref()), None => 2 }
        }
        Some("rustlib") if args.len() == 3 => {
            let mut prog = None;
            if let Err(c) = load(&args[2], &mut prog) { std::process::exit(c); }
            let mut prog = prog.unwrap();
            let ck = geas_core::check::defs::check(&mut prog, true);
            print!("{}", geas_core::rustgen::generate(&ck));
            0
        }
        Some("rusthost") if args.len() == 3 => {
            let mut prog = None;
            if let Err(c) = load(&args[2], &mut prog) { std::process::exit(c); }
            let mut prog = prog.unwrap();
            let ck = geas_core::check::defs::check(&mut prog, true);
            match geas_core::hostgen::host_source(&ck) {
                Ok(s) => { print!("{}", s); 0 }
                Err(e) => { eprintln!("{}", e); 1 }
            }
        }
        Some("rust") => rust_cmd(&args[2..]),
        Some("review") => review_cmd(&args[2..]),
        Some("check") if args.len() == 4 && args[3] == "--json" => check_json(&args[2]),
        _ => {
            eprintln!("geas-rs: まだ astdump だけが動きます（Python 版を移している途中）");
            2
        }
    };
    std::process::exit(code);
}
