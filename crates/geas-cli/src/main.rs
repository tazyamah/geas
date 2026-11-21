// SPDX-License-Identifier: GPL-3.0-or-later
// 表示は、英語モードのときだけ、出す直前に英語にする（geas_core::i18n）。
macro_rules! println {
    () => { std::println!() };
    ($($t:tt)*) => { std::println!("{}", geas_core::i18n::tr(&format!($($t)*))) };
}
macro_rules! print {
    ($($t:tt)*) => { std::print!("{}", geas_core::i18n::tr(&format!($($t)*))) };
}
macro_rules! eprintln {
    () => { std::eprintln!() };
    ($($t:tt)*) => { std::eprintln!("{}", geas_core::i18n::tr(&format!($($t)*))) };
}

/// 文章のファイルを書く（英語モードなら英語にして）
fn write_text(path: impl AsRef<std::path::Path>, text: impl AsRef<str>) -> std::io::Result<()> {
    std::fs::write(path, geas_core::i18n::tr(text.as_ref()))
}

/// --lang と GEAS_LANG から言語を決める。--lang は取り除く。どちらも無ければ英語（ライブラリの既定は日本語）
fn take_lang(argv: &mut Vec<String>) -> Result<geas_core::i18n::Lang, String> {
    use geas_core::i18n::{parse_lang, Lang};
    let mut chosen: Option<Lang> = None;
    let mut i = 0;
    while i < argv.len() {
        let a = argv[i].clone();
        let val = if a == "--lang" {
            argv.remove(i);
            if i >= argv.len() {
                return Err("argument --lang: expected one argument (ja or en)".into());
            }
            Some(argv.remove(i))
        } else if let Some(v) = a.strip_prefix("--lang=") {
            argv.remove(i);
            Some(v.to_string())
        } else {
            i += 1;
            None
        };
        if let Some(v) = val {
            chosen = Some(parse_lang(&v).ok_or_else(|| format!("argument --lang: invalid choice: '{}' (choose from ja, en)", v))?);
        }
    }
    if let Some(l) = chosen {
        return Ok(l);
    }
    if let Ok(v) = std::env::var("GEAS_LANG") {
        if !v.trim().is_empty() {
            return parse_lang(&v).ok_or_else(|| format!("GEAS_LANG: invalid value '{}' (choose from ja, en)", v));
        }
    }
    Ok(Lang::En)
}
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
use geas_core::check::{Checker, ERROR, QUESTION, WARN};
use geas_core::plan::{read_plan, Finding, EXAMPLE, UNSPEC};
use std::collections::{HashMap, HashSet};

// ---- 引数（argparse の代わりの、必要なだけの読み取り） ----
struct Cmd {
    name: &'static str,
    pos: &'static [&'static str],
    /// (オプション名, 値を取るか)
    opts: &'static [(&'static str, bool)],
}

const CMDS: &[Cmd] = &[
    Cmd { name: "interview", pos: &["action", "ledger"], opts: &[("--title", true), ("-o", true), ("--force", false)] },
    Cmd { name: "astdump", pos: &["code"], opts: &[] },
    Cmd { name: "check", pos: &["code"], opts: &[("--json", false), ("--base", true), ("--base-git", true)] },
    Cmd { name: "parts", pos: &["code"], opts: &[("--all", false), ("--record", false), ("--proved", true), ("--describe", false)] },
    Cmd { name: "spec", pos: &["code"], opts: &[("--plan", true), ("-o", true)] },
    Cmd { name: "env-checks", pos: &["code"], opts: &[("--json", false)] },
    Cmd { name: "env-facts", pos: &["plan"], opts: &[("--adapter", true), ("--service", true)] },
    Cmd { name: "env-verify", pos: &["code"], opts: &[("--facts", true), ("--plan", true), ("--adapter", true), ("--service", true), ("--json", false), ("--strict", false)] },
    Cmd { name: "container", pos: &["code"], opts: &[("-o", true)] },
    Cmd { name: "doctor", pos: &["code"], opts: &[("--db", true), ("--egress", true), ("--secrets", true), ("--json", false), ("--strict", false)] },
    Cmd { name: "env-probe", pos: &[], opts: &[("--db", true), ("--egress", true)] },
    Cmd { name: "env-live", pos: &["code"], opts: &[("--url", true), ("--facts", true), ("--json", false), ("--strict", false)] },
    Cmd { name: "review", pos: &["code"], opts: &[("--plan", true), ("--diff-o", true), ("--base", true), ("--base-git", true), ("-o", true)] },
    Cmd { name: "rust", pos: &["code"], opts: &[("-o", true), ("--standalone", false), ("--target", true), ("--plan", true), ("--host", false), ("--runtime", false), ("--seed", true), ("--server", true)] },
    Cmd { name: "compare", pos: &["code", "plan"], opts: &[("--no-run", false), ("-o", true)] },
    Cmd { name: "mutate", pos: &["code"], opts: &[("--base", true), ("--base-git", true), ("--run", false), ("--plan", true), ("-o", true)] },
    Cmd { name: "receipt", pos: &["code"], opts: &[("--base", true), ("--base-git", true), ("--plan", true), ("-o", true)] },
    Cmd { name: "trace", pos: &["code", "log"], opts: &[("--plan", true), ("-o", true)] },
    Cmd { name: "mcp-bridge", pos: &["tools", "url"], opts: &[] },
    Cmd { name: "bench", pos: &["cases", "runs"], opts: &[("--no-run", false), ("--baseline", true), ("--json", true), ("-o", true)] },
];

const USAGE: &str = "usage: geas [-h] [--lang {ja,en}]\n            {interview,astdump,check,parts,spec,env-checks,env-facts,env-verify,env-probe,env-live,doctor,container,review,rust,compare,mutate,receipt,trace,bench,mcp-bridge} ...";

struct Args {
    cmd: &'static str,
    pos: HashMap<&'static str, String>,
    vals: HashMap<String, String>,
    flags: HashSet<String>,
}

impl Args {
    fn v(&self, k: &str) -> Option<&str> {
        self.vals.get(k).map(|s| s.as_str()).or_else(|| self.pos.get(k).map(|s| s.as_str()))
    }
    fn f(&self, k: &str) -> bool {
        self.flags.contains(k)
    }
}

fn arg_error(cmd: &str, msg: &str) -> i32 {
    eprintln!("{}\ngeas {}: error: {}", USAGE, cmd, msg);
    2
}

fn parse_args(argv: &[String]) -> Result<Args, i32> {
    let Some(name) = argv.first() else {
        eprintln!("{}", USAGE);
        return Err(2);
    };
    let Some(cmd) = CMDS.iter().find(|c| c.name == name) else {
        eprintln!("{}\ngeas: error: argument cmd: invalid choice: '{}'", USAGE, name);
        return Err(2);
    };
    let mut a = Args { cmd: cmd.name, pos: HashMap::new(), vals: HashMap::new(), flags: HashSet::new() };
    let mut positional: Vec<String> = vec![];
    let mut i = 1;
    let mut only_pos = false;
    while i < argv.len() {
        let x = &argv[i];
        if !only_pos && x == "--" {
            only_pos = true;
        } else if !only_pos && x.starts_with('-') && x.len() > 1 && !(x.chars().nth(1).map_or(false, |c| c.is_ascii_digit()) && false) {
            let (key, inline) = match x.split_once('=') {
                Some((k, v)) if k.starts_with("--") => (k.to_string(), Some(v.to_string())),
                _ => (x.clone(), None),
            };
            let Some(&(opt, takes)) = cmd.opts.iter().find(|(o, _)| *o == key) else {
                // 「-」だけの位置引数（ログを標準入力から読む）は上で除いてある
                return Err(arg_error(cmd.name, &format!("unrecognized arguments: {}", x)));
            };
            if takes {
                let v = match inline {
                    Some(v) => v,
                    None => {
                        i += 1;
                        match argv.get(i) {
                            Some(v) => v.clone(),
                            None => return Err(arg_error(cmd.name, &format!("argument {}: expected one argument", opt))),
                        }
                    }
                };
                a.vals.insert(opt.trim_start_matches('-').replace('-', "_"), v);
            } else {
                a.flags.insert(opt.trim_start_matches('-').replace('-', "_"));
            }
        } else {
            positional.push(x.clone());
        }
        i += 1;
    }
    if positional.len() > cmd.pos.len() {
        return Err(arg_error(cmd.name, &format!("unrecognized arguments: {}", positional[cmd.pos.len()..].join(" "))));
    }
    if positional.len() < cmd.pos.len() {
        let miss: Vec<&str> = cmd.pos[positional.len()..].to_vec();
        return Err(arg_error(cmd.name, &format!("the following arguments are required: {}", miss.join(", "))));
    }
    for (k, v) in cmd.pos.iter().zip(positional) {
        a.pos.insert(k, v);
    }
    if cmd.name == "interview" && !["init", "status", "render"].contains(&a.pos["action"].as_str()) {
        return Err(arg_error(cmd.name, &format!("argument action: invalid choice: '{}' (choose from 'init', 'status', 'render')", a.pos["action"])));
    }
    if cmd.name == "rust" && a.vals.get("o").is_none() {
        return Err(arg_error(cmd.name, "the following arguments are required: -o"));
    }
    if cmd.name == "review" && a.vals.get("o").is_none() {
        return Err(arg_error(cmd.name, "the following arguments are required: -o"));
    }
    if cmd.name == "env-verify" && a.vals.get("facts").is_none() == a.vals.get("plan").is_none() {
        return Err(arg_error(cmd.name, if a.vals.get("facts").is_some() { "argument --plan: not allowed with argument --facts" } else { "one of the arguments --facts --plan is required" }));
    }
    if cmd.name == "env-live" && a.vals.get("url").is_none() {
        return Err(arg_error(cmd.name, "the following arguments are required: --url"));
    }
    if cmd.name == "rust" {
        if let Some(t) = a.vals.get("target") {
            if !["windows", "macos", "linux"].contains(&t.as_str()) {
                return Err(arg_error(cmd.name, &format!("argument --target: invalid choice: '{}' (choose from 'windows', 'macos', 'linux')", t)));
            }
        }
    }
    Ok(a)
}

/// Python の Path(v).parent の表示
fn py_parent(v: &str) -> String {
    match Path::new(v).parent() {
        Some(p) if p.as_os_str().is_empty() => ".".into(),
        Some(p) => p.display().to_string(),
        None => v.to_string(),
    }
}

fn parent_dir_exists(v: &str) -> bool {
    let p = std::fs::canonicalize(Path::new(v).parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."))).ok();
    match p {
        Some(p) => p.is_dir(),
        None => {
            // 親が無い（まだ作られていない）なら、絶対にした親の有無を見る
            let abs = std::env::current_dir().unwrap_or_default().join(v);
            abs.parent().map_or(false, |p| p.is_dir())
        }
    }
}

fn check_paths(a: &Args) -> Vec<String> {
    let mut bad = vec![];
    for k in ["code", "plan", "base", "seed", "facts", "baseline"] {
        if let Some(v) = a.v(k) {
            if !v.is_empty() && !Path::new(v).is_file() {
                bad.push(if !Path::new(v).exists() { format!("ファイルがありません: {}", v) } else { format!("ファイルではありません: {}", v) });
            }
        }
    }
    if let Some(l) = a.v("log") {
        if l != "-" && !Path::new(l).is_file() {
            bad.push(format!("ログのファイルがありません: {}", l));
        }
    }
    if a.cmd == "bench" {
        if let Some(j) = a.v("json") {
            if !parent_dir_exists(j) {
                bad.push(format!("書き出す先のフォルダがありません: {}", py_parent(j)));
            }
        }
    }
    for k in ["o", "diff_o"] {
        if let Some(v) = a.v(k) {
            if !v.is_empty() && a.cmd != "rust" && a.cmd != "interview" && !parent_dir_exists(v) {
                bad.push(format!("書き出す先のフォルダがありません: {}", py_parent(v)));
            }
        }
    }
    if a.cmd == "rust" {
        if a.f("runtime") && !a.f("host") {
            bad.push("--runtime は --host と一緒に使います（サーバーの実行部は、画面の窓口と組みます）".into());
        }
        if a.f("standalone") && !(a.f("host") && a.f("runtime")) {
            bad.push("--standalone は --host --runtime と一緒に使います（窓つき exe は、サーバーの実行部と生成した画面を使います）".into());
        }
        if a.v("seed").map_or(false, |s| !s.is_empty()) && !a.f("runtime") {
            bad.push("--seed は --host --runtime と一緒に使います（最初のデータはサーバーが入れます）".into());
        }
    }
    bad
}

// ---- 読み込み ----
fn fname(p: &str) -> String {
    Path::new(p).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// コードを読んで構文木にする。読めなければ、構文エラーを出して終了コード 2
fn load(path: &str) -> Result<geas_core::ast::Program, i32> {
    let src = std::fs::read_to_string(path).map_err(|e| {
        eprintln!("{}: {}", path, e);
        2
    })?;
    parse(&src, Some(Path::new(path))).map_err(|e| {
        eprintln!("{}", syntax_error_text(&e, path));
        2
    })
}

fn print_diags(ck: &Checker, to_err: bool) {
    for d in &ck.diags {
        if to_err { eprintln!("{}", d.for_code()) } else { println!("{}", d.for_code()) }
    }
}

fn counts_line(ck: &Checker) -> String {
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

fn write_or_print(out: Option<&str>, text: &str) {
    match out {
        Some(o) => write_text(o, text).unwrap(),
        None => println!("{}", text),
    }
}

/// 前の版のコードの文字列と、その名前（--base のファイル、または --base-git の版）
fn read_base(a: &Args) -> Result<(String, String), String> {
    if let Some(b) = a.v("base") {
        return Ok((std::fs::read_to_string(b).map_err(|e| e.to_string())?, fname(b)));
    }
    let rev = a.v("base_git").unwrap_or("");
    Ok((geas_core::mutate::read_git(Path::new(a.v("code").unwrap()), rev)?, format!("git {}", rev)))
}

fn has_base(a: &Args) -> bool {
    a.v("base").is_some() || a.v("base_git").is_some()
}

fn base_path<'a>(a: &'a Args) -> &'a str {
    a.v("base").or_else(|| a.v("code")).unwrap()
}

fn plan_text(a: &Args) -> Option<String> {
    a.v("plan").map(|p| std::fs::read_to_string(p).unwrap_or_default())
}

fn plan_usecases(md: Option<&str>) -> Option<Vec<geas_core::plan::Row>> {
    md.map(|m| read_plan(m).get("ユースケース").cloned().unwrap_or_default())
}

// ---- 各コマンド ----
fn cmd_check(a: &Args) -> i32 {
    let path = a.v("code").unwrap();
    let json = a.f("json");
    let mut prog = if json {
        let src = std::fs::read_to_string(path).unwrap_or_default();
        match parse(&src, Some(Path::new(path))) {
            Ok(p) => p,
            Err(e) => {
                print!("{}", geas_core::hints::syntax_json(path, &e));
                return 2;
            }
        }
    } else {
        match load(path) { Ok(p) => p, Err(c) => return c }
    };
    let mut ck = geas_core::check::defs::check(&mut prog, true);
    ck.code_path = Some(path.to_string());
    if has_base(a) {
        let r: Result<(), String> = (|| {
            let (text, _) = read_base(a)?;
            let mut bp = parse(&text, Some(Path::new(base_path(a)))).map_err(|e| e.to_string())?;
            let bck = geas_core::check::defs::check(&mut bp, true);
            ck.check_against(&bck);
            Ok(())
        })();
        if let Err(e) = r {
            eprintln!("前の版を読めません: {}", e);
            return 2;
        }
    }
    if json {
        print!("{}", geas_core::hints::to_json(ck.ok(), ck.ready(), &ck.diags, path));
        return if ck.ok() { 0 } else { 1 };
    }
    print_diags(&ck, false);
    println!("{}", counts_line(&ck));
    if ck.ok() { 0 } else { 1 }
}

fn cmd_spec(a: &Args) -> i32 {
    let path = a.v("code").unwrap();
    let mut prog = match load(path) { Ok(p) => p, Err(c) => return c };
    let mut ck = geas_core::check::defs::check(&mut prog, true);
    ck.code_path = Some(path.to_string());
    let spec = geas_core::spec::build(&ck);
    let mut text = geas_core::spec::render(&ck, &spec, &format!("（{}）", fname(path)));
    if let Some(md) = plan_text(a) {
        text += &geas_core::plan::coverage_markdown(&spec, &read_plan(&md)).join("\n");
        text += "\n";
    }
    write_or_print(a.v("o"), &text);
    if ck.ok() { 0 } else { 1 }
}

fn cmd_review(a: &Args) -> i32 {
    let path = a.v("code").unwrap();
    let out = a.v("o").unwrap();
    let mut prog = match load(path) { Ok(p) => p, Err(c) => return c };
    let mut ck = geas_core::check::defs::check(&mut prog, true);
    ck.code_path = Some(path.to_string());
    let md = plan_text(a);
    let findings = match &md {
        Some(m) => geas_core::plan::compare_static(&geas_core::spec::build(&ck), &read_plan(m)),
        None => vec![],
    };
    let (mut base_prog, mut base_label, mut base_text) = (None, String::new(), None);
    if has_base(a) {
        let r: Result<(), String> = (|| {
            let (t, l) = read_base(a)?;
            base_prog = Some(parse(&t, Some(Path::new(base_path(a)))).map_err(|e| e.to_string())?);
            base_label = l;
            base_text = Some(t);
            Ok(())
        })();
        if let Err(e) = r {
            eprintln!("前の版を読めません: {}", e);
            return 2;
        }
    }
    let base_ck = base_prog.as_mut().map(|p| geas_core::check::defs::check(p, true));
    if let Some(b) = &base_ck {
        ck.check_against(b);
    }
    let receipt_html = match (&base_ck, a.v("diff_o")) {
        (Some(b), Some(_)) => {
            let rows = plan_usecases(md.as_deref());
            let r = geas_core::receipt::build_receipt(&ck, b, path, base_text.as_deref(), &base_label, rows.as_deref());
            Some(geas_core::receipt::to_html(&r))
        }
        _ => None,
    };
    let label = format!("（{}）", fname(path));
    let html = match geas_core::review::review(&ck, md.as_deref(), &findings, &label, false) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("{}", e);
            return 1;
        }
    };
    write_text(out, html).unwrap();
    if let Some(d) = a.v("diff_o") {
        write_text(d, geas_core::review::review_changes(&ck, &findings, &label, base_ck.as_ref(), &base_label, false, receipt_html.as_deref())).unwrap();
        println!("生成しました: {}", d);
    } else if base_ck.is_some() {
        eprintln!("前の版は、--diff-o で出力先を渡したときだけ使います（仕様書には載せません）。");
    }
    println!("生成しました: {}", out);
    if ck.ok() { 0 } else { 1 }
}

fn cmd_rust(a: &Args) -> i32 {
    let path = a.v("code").unwrap();
    let out = a.v("o").unwrap();
    let (host, runtime, standalone) = (a.f("host"), a.f("runtime"), a.f("standalone"));
    let server = a.v("server").unwrap_or(geas_core::crategen::DEFAULT_SERVER).to_string();
    let mut prog = match load(path) { Ok(p) => p, Err(c) => return c };
    let mut ck = geas_core::check::defs::check(&mut prog, true);
    ck.code_path = Some(path.to_string());
    if !ck.ready() {
        print_diags(&ck, false);
        eprintln!("{}", counts_line(&ck));
        eprintln!("検査にエラーか問いが残っているので、続けません（問いに答えて、コードか計画書に反映してください）。");
        return 1;
    }
    if standalone && !ck.p.actors.is_empty() {
        eprintln!("役割（actor）のあるアプリは、窓つき exe（--standalone）にできません。窓つき exe には、役割を確かめるプロキシが前にいないためです。");
        return 1;
    }
    if standalone && !ck.p.paths.is_empty() {
        let tgt = a.v("target").map(|s| s.to_string()).unwrap_or_else(|| if cfg!(windows) { "windows".into() } else if cfg!(target_os = "macos") { "macos".into() } else { "linux".into() });
        let lack: Vec<&str> = ck.p.paths.iter().filter(|(_, k, es, _)| k == "absolute" && !es.iter().any(|(o, _, _)| o == &tgt)).map(|(n, _, _, _)| n.as_str()).collect();
        if !lack.is_empty() {
            eprintln!("パス {} に、組む OS（{}）の行がありません。`{} \"…\"` を書くか、--target で組む OS を指定してください", lack.join("・"), tgt, tgt);
            return 1;
        }
    }
    let md = plan_text(a);
    let dest: std::path::PathBuf = Path::new(out).components().filter(|c| !matches!(c, std::path::Component::CurDir)).collect();
    let mut msgs = vec![];
    let r = geas_core::crategen::generate_tree(&ck, &dest, host, runtime, standalone, a.v("seed"), &server, md.as_deref(), &mut msgs);
    for m in msgs {
        println!("{}", m);
    }
    if let Err(e) = r {
        eprintln!("{}", e);
        return 1;
    }
    println!("生成しました: {}", out);
    0
}

fn report(findings: &[Finding]) -> String {
    if findings.is_empty() {
        return "突合の結果: 計画とコードに食い違いはありません。\n".into();
    }
    let mut out: Vec<String> = vec!["突合の結果".into(), String::new(), "| 種別 | 場所 | 内容 |".into(), "|---|---|---|".into()];
    out.extend(findings.iter().map(|f| format!("| {} | {} | {} |", f.kind, f.where_, f.msg)));
    out.join("\n") + "\n"
}

fn cmd_compare(a: &Args) -> i32 {
    let path = a.v("code").unwrap();
    let mut prog = match load(path) { Ok(p) => p, Err(c) => return c };
    let mut ck = geas_core::check::defs::check(&mut prog, true);
    ck.code_path = Some(path.to_string());
    if !ck.ready() {
        print_diags(&ck, false);
        eprintln!("{}", counts_line(&ck));
        eprintln!("検査にエラーか問いが残っているので、続けません（問いに答えて、コードか計画書に反映してください）。");
        return 1;
    }
    let lib = geas_core::rustgen::generate(&ck);
    let plan = read_plan(&plan_text(a).unwrap_or_default());
    let mut findings = geas_core::plan::compare_static(&geas_core::spec::build(&ck), &plan);
    let rows: Vec<geas_core::plan::Row> = plan.get("具体例").cloned().unwrap_or_default();
    let (src, problems) = geas_core::examples::runner_source(&ck, &rows);
    findings.extend(problems);
    if !a.f("no_run") && !rows.is_empty() {
        let tmp = std::env::temp_dir().join(format!("geas-compare-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let r = geas_core::crategen::write_crate(&ck, &lib, &src, &tmp, false).and_then(|_| geas_core::examples::run_crate(&tmp, &|s| ck.p.show_names(s)));
        let _ = std::fs::remove_dir_all(&tmp);
        match r {
            Ok(got) => findings.extend(geas_core::examples::compare_examples(&rows, &got)),
            Err(e) => findings.push(Finding { kind: EXAMPLE.into(), where_: "具体例の実行".into(), msg: e }),
        }
    }
    let text = report(&findings);
    println!("{}", text);
    if let Some(o) = a.v("o") {
        write_text(o, &text).unwrap();
    }
    if findings.iter().any(|f| f.kind != UNSPEC) { 1 } else { 0 }
}

fn cmd_mutate(a: &Args) -> i32 {
    use geas_core::mutate;
    let path = a.v("code").unwrap();
    let r: Result<(Vec<mutate::Mutant>, Option<String>, Option<String>, String), (String, i32)> = (|| {
        let (base, scope) = if let Some(b) = a.v("base") {
            (Some(std::fs::read_to_string(b).map_err(|e| (e.to_string(), 2))?), format!("{} から変わった行だけ", fname(b)))
        } else if let Some(g) = a.v("base_git") {
            (Some(mutate::read_git(Path::new(path), g).map_err(|e| (e, 2))?), format!("git {} から変わった行だけ", g))
        } else {
            (None, "すべての行".to_string())
        };
        let (mut ms, why) = mutate::run(path, base.as_deref()).map_err(|e| (e, 2))?;
        let mut run_note = None;
        if a.f("run") && why.is_none() {
            let Some(p) = a.v("plan") else {
                return Err(("--run には --plan 計画書.md が要ります".to_string(), -2));
            };
            run_note = mutate::run_examples(path, &mut ms, &std::fs::read_to_string(p).unwrap_or_default()).map_err(|e| (e, 2))?;
        }
        Ok((ms, why, run_note, scope))
    })();
    let (ms, why, run_note, scope) = match r {
        Ok(x) => x,
        Err((m, -2)) => {
            eprintln!("{}", m);
            return 2;
        }
        Err((m, c)) => {
            eprintln!("エラー: {}", m);
            return c;
        }
    };
    if let Some(w) = why {
        eprintln!("{}", w);
        return 1;
    }
    let scope = format!("{}{}", scope, if a.f("run") && run_note.is_none() { "（具体例も動かして確認）" } else { "" });
    let text = mutate::report(&ms, &fname(path), &scope);
    if let Some(n) = run_note {
        eprintln!("{}", n);
    }
    write_or_print(a.v("o"), &text);
    0
}

fn cmd_receipt(a: &Args) -> i32 {
    let path = a.v("code").unwrap();
    if !has_base(a) {
        eprintln!("前の版が要ります（--base 前のコード.geas、または --base-git REV）。");
        return 2;
    }
    let (base_text, label) = match read_base(a) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("前の版を読めません: {}", e);
            return 2;
        }
    };
    let mut prog = match load(path) { Ok(p) => p, Err(c) => return c };
    let mut ck = geas_core::check::defs::check(&mut prog, true);
    ck.code_path = Some(path.to_string());
    let mut bp = match parse(&base_text, Some(Path::new(base_path(a)))) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("前の版を読めません: {}", e);
            return 2;
        }
    };
    let bck = geas_core::check::defs::check(&mut bp, true);
    let rows = plan_usecases(plan_text(a).as_deref());
    let r = geas_core::receipt::build_receipt(&ck, &bck, path, Some(&base_text), &label, rows.as_deref());
    write_or_print(a.v("o"), &geas_core::receipt::to_markdown(&r));
    0
}

fn cmd_trace(a: &Args) -> i32 {
    use std::io::Read;
    let path = a.v("code").unwrap();
    let mut prog = match load(path) { Ok(p) => p, Err(c) => return c };
    let mut ck = geas_core::check::defs::check(&mut prog, true);
    ck.code_path = Some(path.to_string());
    let log = a.v("log").unwrap();
    let text = if log == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s).ok();
        s
    } else {
        match std::fs::read(log) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) => {
                eprintln!("ログを読めません: {}", e);
                return 2;
            }
        }
    };
    let entries = geas_core::trace::read_log(&text);
    if entries.is_empty() {
        eprintln!("geas のルートのログ（\"log\": \"geas.route\" の行）が見つかりません。");
        return 2;
    }
    let rows = plan_usecases(plan_text(a).as_deref());
    let t = geas_core::trace::trace(&ck, &entries, rows.as_deref());
    write_or_print(a.v("o"), &geas_core::trace::to_markdown(&t, &fname(path)));
    if t.unknown.is_empty() { 0 } else { 1 }
}

/// MCP の橋渡し：標準入出力の JSON-RPC を、動いているサーバーの REST に中継する
fn cmd_mcp_bridge(a: &Args) -> i32 {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    match geas_core::mcpbridge::serve(a.v("tools").unwrap(), a.v("url").unwrap(), &mut stdin.lock(), &mut stdout.lock()) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{}", e);
            1
        }
    }
}

fn cmd_bench(a: &Args) -> i32 {
    use geas_core::bench;
    let (cases, runs) = (Path::new(a.v("cases").unwrap()), Path::new(a.v("runs").unwrap()));
    if !cases.is_dir() || !runs.is_dir() {
        eprintln!("ケース集と結果の置き場は、どちらもフォルダで渡してください。");
        return 2;
    }
    let scores = bench::run_bench(cases, runs, !a.f("no_run"));
    if scores.is_empty() {
        eprintln!("ケース集に plan.md を持つフォルダがありません。");
        return 2;
    }
    let base: Option<serde_json::Value> = a.v("baseline").and_then(|b| serde_json::from_str(&std::fs::read_to_string(b).unwrap_or_default()).ok());
    if let Some(j) = a.v("json") {
        std::fs::write(j, bench::to_json(&scores)).unwrap();
    }
    write_or_print(a.v("o"), &bench::to_markdown(&scores, base.as_ref()));
    if scores.iter().all(|s| s.passed()) { 0 } else { 1 }
}

fn cmd_interview(a: &Args) -> i32 {
    use geas_core::interview as iv;
    let (action, ledger) = (a.v("action").unwrap(), a.v("ledger").unwrap());
    let save = |led: &serde_json::Value| std::fs::write(ledger, geas_core::crategen::dumps_indent(led, 2) + "\n").unwrap();
    if action == "init" {
        if Path::new(ledger).exists() && !a.f("force") {
            eprintln!("{} は既にあります（上書きするなら --force）", ledger);
            return 2;
        }
        save(&iv::new_ledger(a.v("title").unwrap_or("（題名）")));
        println!("台帳を作りました: {}", ledger);
        return 0;
    }
    let mut led: serde_json::Value = match std::fs::read_to_string(ledger).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("台帳を読めません: {}", e);
            return 2;
        }
    };
    iv::fill_defaults(&mut led);
    if action == "status" {
        let (todo, ready) = iv::status(&led);
        if todo.is_empty() {
            println!("- 聞くことは残っていません");
        } else {
            println!("{}", todo.iter().map(|t| format!("- {}", t)).collect::<Vec<_>>().join("\n"));
        }
        println!("コードを書き始めてよい: {}", if ready { "はい" } else { "いいえ（動作要件 R1〜R7 とユースケースが揃うまで）" });
        return if ready { 0 } else { 1 };
    }
    write_or_print(a.v("o"), &iv::render(&led));
    0
}

fn facts_from_plan(a: &Args) -> Result<(serde_json::Value, Vec<String>), String> {
    let ad = a.v("adapter").unwrap_or("aws-ecs");
    if !geas_core::adapter::ADAPTER_NAMES.contains(&ad) {
        return Err(format!("アダプタ「{}」はありません（ある：{}）", ad, geas_core::adapter::ADAPTER_NAMES.join("・")));
    }
    let plan: serde_json::Value = std::fs::read_to_string(a.v("plan").unwrap()).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())).map_err(|e| format!("plan を読めません: {}", e))?;
    Ok(geas_core::adapter::facts_from_plan(&plan, a.v("service").unwrap_or("")))
}

fn cmd_env_facts(a: &Args) -> i32 {
    match facts_from_plan(a) {
        Err(e) => {
            eprintln!("{}", e);
            2
        }
        Ok((facts, notes)) => {
            println!("{}", geas_core::crategen::dumps_indent(&serde_json::json!({"facts": facts, "notes": notes}), 2));
            0
        }
    }
}

fn cmd_env_checks(a: &Args) -> i32 {
    let path = a.v("code").unwrap();
    let mut prog = match load(path) { Ok(p) => p, Err(c) => return c };
    let ck = geas_core::check::defs::check(&mut prog, true);
    if !ck.ok() {
        print_diags(&ck, true);
        eprintln!("検査: エラーがあります（直してから一覧を出してください）");
        return 1;
    }
    print!("{}", if a.f("json") { geas_core::envreq::to_json(ck.p) } else { geas_core::envreq::to_text(ck.p) });
    0
}

fn cmd_env_verify(a: &Args) -> i32 {
    use geas_core::envcheck as ec;
    let path = a.v("code").unwrap();
    let mut prog = match load(path) { Ok(p) => p, Err(c) => return c };
    let ck = geas_core::check::defs::check(&mut prog, true);
    if !ck.ok() {
        for d in &ck.diags {
            eprintln!("{}", d);
        }
        eprintln!("検査: エラーがあります（直してから照合してください）");
        return 1;
    }
    let (facts, notes) = if let Some(f) = a.v("facts") {
        match std::fs::read_to_string(f).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).map_err(|e| e.to_string())) {
            Ok(v) => (v.get("facts").cloned().unwrap_or(v), vec![]),
            Err(e) => {
                eprintln!("事実を読めません: {}", e);
                return 2;
            }
        }
    } else {
        match facts_from_plan(a) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("{}", e);
                return 2;
            }
        }
    };
    let results = ec::verify(&geas_core::envreq::items(ck.p), &facts);
    if a.f("json") {
        println!("{}", geas_core::crategen::dumps_indent(&serde_json::json!({"results": ec::results_json(&results), "counts": ec::counts_json(&results), "notes": notes}), 2));
    } else {
        print!("{}", ec::to_text(&results));
        for n in &notes {
            println!("メモ：{}", n);
        }
    }
    let c = ec::counts(&results);
    let n = |k: &str| c.iter().find(|(x, _)| *x == k).map_or(0, |(_, v)| *v);
    if n(ec::FAIL) > 0 || (a.f("strict") && n(ec::UNKNOWN) > 0) { 1 } else { 0 }
}

fn cmd_container(a: &Args) -> i32 {
    let path = a.v("code").unwrap();
    let mut prog = match load(path) { Ok(p) => p, Err(c) => return c };
    let ck = geas_core::check::defs::check(&mut prog, true);
    if !ck.ok() {
        for d in &ck.diags {
            eprintln!("{}", d);
        }
        eprintln!("検査: エラーがあります（直してから生成してください）");
        return 1;
    }
    let files = match geas_core::containergen::generate(ck.p) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{}（`container {{ … }}` と `infra vm_docker \"名前\" {{ … }}` を書きます）", e);
            return 2;
        }
    };
    let Some(dest) = a.v("o") else {
        for (n, body) in &files {
            println!("==== {} ====\n{}", n, body);
        }
        return 0;
    };
    let dest = Path::new(dest);
    if let Err(e) = std::fs::create_dir_all(dest) {
        eprintln!("{}: {}", dest.display(), e);
        return 2;
    }
    for (n, body) in &files {
        if let Err(e) = std::fs::write(dest.join(n), body) {
            eprintln!("{}: {}", n, e);
            return 2;
        }
        println!("生成しました: {}", dest.join(n).display());
    }
    0
}

fn cmd_doctor(a: &Args) -> i32 {
    use geas_core::envcheck as ec;
    let path = a.v("code").unwrap();
    let mut prog = match load(path) { Ok(p) => p, Err(c) => return c };
    let ck = geas_core::check::defs::check(&mut prog, true);
    if !ck.ok() {
        for d in &ck.diags {
            eprintln!("{}", d);
        }
        eprintln!("検査: エラーがあります（直してから診断してください）");
        return 1;
    }
    let o = geas_core::envdoctor::Options {
        db: a.v("db").map(|s| s.to_string()),
        egress: a.v("egress").map(|s| s.split(',').filter(|x| !x.is_empty()).map(|x| x.to_string()).collect()).unwrap_or_default(),
        secrets_file: a.v("secrets").map(|s| s.to_string()),
    };
    let (results, notes) = geas_core::envdoctor::run(ck.p, &geas_core::envreq::items(ck.p), &o);
    if a.f("json") {
        println!("{}", geas_core::crategen::dumps_indent(&serde_json::json!({"results": ec::results_json(&results), "counts": ec::counts_json(&results), "notes": notes}), 2));
    } else {
        print!("{}", ec::to_text(&results));
        for n in &notes {
            println!("メモ：{}", n);
        }
    }
    let c = ec::counts(&results);
    let n = |k: &str| c.iter().find(|(x, _)| *x == k).map_or(0, |(_, v)| *v);
    if n(ec::FAIL) > 0 || (a.f("strict") && n(ec::UNKNOWN) > 0) { 1 } else { 0 }
}

fn cmd_env_probe(a: &Args) -> i32 {
    let o = geas_core::envprobe::Options {
        db: a.v("db").map(|s| s.to_string()),
        egress: a.v("egress").map(|s| s.split(',').filter(|x| !x.is_empty()).map(|x| x.to_string()).collect()).unwrap_or_default(),
    };
    let (facts, notes) = geas_core::envprobe::collect(&o);
    println!("{}", geas_core::crategen::dumps_indent(&serde_json::json!({"facts": facts, "notes": notes}), 2));
    0
}

fn cmd_env_live(a: &Args) -> i32 {
    use geas_core::envcheck as ec;
    let path = a.v("code").unwrap();
    let mut prog = match load(path) { Ok(p) => p, Err(c) => return c };
    let ck = geas_core::check::defs::check(&mut prog, true);
    if !ck.ok() {
        for d in &ck.diags {
            eprintln!("{}", d);
        }
        eprintln!("検査: エラーがあります（直してから確かめてください）");
        return 1;
    }
    let mut results: Vec<ec::Res> = vec![];
    if let Some(f) = a.v("facts") {
        match std::fs::read_to_string(f).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<serde_json::Value>(&t).map_err(|e| e.to_string())) {
            Ok(v) => results.extend(ec::verify(&geas_core::envreq::items(ck.p), &v.get("facts").cloned().unwrap_or(v))),
            Err(e) => {
                eprintln!("事実を読めません: {}", e);
                return 2;
            }
        }
    }
    let probes = geas_core::envlive::probes(ck.p);
    results.extend(geas_core::envlive::run(&probes, a.v("url").unwrap(), &geas_core::envlive::fetch_default));
    if a.f("json") {
        println!("{}", geas_core::crategen::dumps_indent(&serde_json::json!({"results": ec::results_json(&results), "counts": ec::counts_json(&results)}), 2));
    } else {
        print!("{}", ec::to_text(&results));
    }
    let c = ec::counts(&results);
    let n = |k: &str| c.iter().find(|(x, _)| *x == k).map_or(0, |(_, v)| *v);
    if n(ec::FAIL) > 0 || (a.f("strict") && n(ec::UNKNOWN) > 0) { 1 } else { 0 }
}

fn cmd_parts(a: &Args) -> i32 {
    use geas_core::partrun as pr;
    let path = a.v("code").unwrap();
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let first: Vec<String> = text.lines().map(|l| l.trim()).find(|l| !l.is_empty() && !l.starts_with("//")).map(|l| l.split_whitespace().map(|x| x.to_string()).collect()).unwrap_or_default();
    let is_module = first.first().map_or(false, |w| w == "module");
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| Path::new(path).to_path_buf());
    let mut prog = if is_module {
        let t = format!("use \"{}\"\n", abs.to_string_lossy().replace('\\', "/"));
        match parse(&t, Some(&abs.parent().unwrap().join("_parts.geas"))) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("{}", e);
                return 2;
            }
        }
    } else {
        match load(path) { Ok(p) => p, Err(c) => return c }
    };
    let mut ck = geas_core::check::defs::check(&mut prog, true);
    if !is_module {
        ck.code_path = Some(path.to_string());
    }
    let all = a.f("all");
    let parts: Vec<&geas_core::ast::Part> = ck.p.parts.iter().filter(|pt| pt.external() && (is_module || all || ck.used_libs.contains(&pt.name))).collect();
    if parts.is_empty() {
        println!("確かめる部品（library）がありません{}", if is_module || all { "" } else { "（呼ばれていない部品も確かめるには --all）" });
        return 0;
    }
    if a.f("describe") {
        // 開発用：確かめる部品の宣言を JSON で出す（proofs/std_proofs.py が読む）
        let ty = |t: &geas_core::ast::Type| if t.kind == "真偽" { "bool".to_string() } else { geas_core::check::phys(t).to_string() };
        let list: Vec<serde_json::Value> = parts.iter().map(|pt| serde_json::json!({
            "name": pt.name,
            "params": pt.params.iter().map(|(n, t)| serde_json::json!([n, t.kind, ty(t)])).collect::<Vec<_>>(),
            "ret": [pt.ret.kind, ty(&pt.ret)],
            "expr": pt.lib.as_ref().map(|l| l.2.clone()).unwrap_or_default(),
        })).collect();
        println!("{}", geas_core::crategen::dumps_indent(&serde_json::Value::Array(list), 1));
        return 0;
    }
    let tmp = std::env::temp_dir().join(format!("geas-parts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let rep = pr::check_parts(&parts, &tmp, &pr::reference(&parts));
    let _ = std::fs::remove_dir_all(&tmp);
    let rep = match rep {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}", e);
            return 2;
        }
    };
    println!("{}", pr::render(&rep));
    if a.f("record") {
        let out = Path::new(path).with_extension("parts.json");
        let proved: Option<serde_json::Map<String, serde_json::Value>> = a.v("proved").and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str(&t).ok());
        let led = pr::sort_keys(&pr::ledger_of(&rep, &parts, proved.as_ref()));
        std::fs::write(&out, geas_core::crategen::dumps_indent(&led, 1) + "\n").unwrap();
        println!("台帳を書きました: {}", out.display());
    }
    if rep.ok() { 0 } else { 1 }
}

/// 開発用（rustlib・rusthost）：生成物の一部だけを標準出力に出す。Python 版との突き合わせに使う
fn dev_cmd(argv: &[String]) -> Option<i32> {
    let (Some(c), Some(path)) = (argv.first(), argv.get(1)) else { return None };
    if argv.len() != 2 || (c != "rustlib" && c != "rusthost") {
        return None;
    }
    let mut prog = match load(path) { Ok(p) => p, Err(c) => return Some(c) };
    let ck = geas_core::check::defs::check(&mut prog, true);
    if c == "rustlib" {
        print!("{}", geas_core::rustgen::generate(&ck));
        return Some(0);
    }
    Some(match geas_core::hostgen::host_source(&ck) {
        Ok(s) => { print!("{}", s); 0 }
        Err(e) => { eprintln!("{}", e); 1 }
    })
}

fn main() {
    let mut argv: Vec<String> = std::env::args().skip(1).collect();
    let lang = match take_lang(&mut argv) {
        Ok(l) => l,
        Err(m) => {
            std::eprintln!("geas: error: {}", m);
            std::process::exit(2);
        }
    };
    // 構文木の出力（astdump）・開発用の出力・計画書の雛形（interview）は、他の道具が読むので日本語のまま
    let machine = matches!(argv.first().map(|s| s.as_str()), Some("astdump") | Some("interview") | Some("rustlib") | Some("rusthost"));
    geas_core::i18n::set_lang(if machine { geas_core::i18n::Lang::Ja } else { lang });
    if let Some(c) = dev_cmd(&argv) {
        std::process::exit(c);
    }
    let a = match parse_args(&argv) {
        Ok(a) => a,
        Err(c) => std::process::exit(c),
    };
    let bad = check_paths(&a);
    if !bad.is_empty() {
        for b in bad {
            eprintln!("{}", b);
        }
        std::process::exit(2);
    }
    let code = match a.cmd {
        "astdump" => astdump(a.v("code").unwrap()),
        "interview" => cmd_interview(&a),
        "mutate" => cmd_mutate(&a),
        "receipt" => cmd_receipt(&a),
        "trace" => cmd_trace(&a),
        "bench" => cmd_bench(&a),
        "mcp-bridge" => cmd_mcp_bridge(&a),
        "env-facts" => cmd_env_facts(&a),
        "env-checks" => cmd_env_checks(&a),
        "env-verify" => cmd_env_verify(&a),
        "env-probe" => cmd_env_probe(&a),
        "doctor" => cmd_doctor(&a),
        "container" => cmd_container(&a),
        "env-live" => cmd_env_live(&a),
        "parts" => cmd_parts(&a),
        "check" => cmd_check(&a),
        "spec" => cmd_spec(&a),
        "review" => cmd_review(&a),
        "rust" => cmd_rust(&a),
        "compare" => cmd_compare(&a),
        _ => 2,
    };
    std::process::exit(code);
}
