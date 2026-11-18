// SPDX-License-Identifier: GPL-3.0-or-later
//! モジュール（状態を持たない部品の集まり）の取り込み。Python 版 geas/modules.py の移し。
//! モジュールのファイルは `module 名前` で始まり、論理型・変換・部品・結果の形と、状態に触れない処理（deed）だけを書ける。
//! 取り込む側は `use "path"` と書き、モジュールの名前は `名前.定義名` で指す。
//! 内部では、モジュールの定義名を `名前_定義名` に付け替えて、1つの字句の列につなげる。
use crate::error::{Error, ModuleError};
use crate::lexer::{lex, Tok};
use indexmap::IndexMap;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// geas に同梱の標準モジュール。use "gtin.geas" は、取り込む側の隣に同じ名前のファイルが無ければ、ここから読む
pub fn std_source(rel: &str) -> Option<&'static str> {
    match rel {
        "gtin.geas" => Some(include_str!("../../../geas/std/gtin.geas")),
        "num.geas" => Some(include_str!("../../../geas/std/num.geas")),
        _ => None,
    }
}

const STD_DIR: &str = "/<geas-std>";

const MODULE_DECLS: [&str; 5] = ["ltype", "convert", "library", "result", "deed"];
const ALL_DECLS: [&str; 18] = [
    "entity", "ltype", "store", "state", "result", "effect", "part", "library", "convert", "invariant", "deed", "nexus", "flow", "trigger", "query", "event", "data", "timezone",
];

fn is_all_decl(s: &str) -> bool {
    ALL_DECLS.contains(&s) || s == "storage" || s == "needs"
}

/// 行頭（{ } の外）の字句の位置。宣言は、ここから始まる
pub fn statement_starts(toks: &[Tok]) -> Vec<usize> {
    let (mut depth, mut at_start) = (0i64, true);
    let mut out = vec![];
    for (i, t) in toks.iter().enumerate() {
        if t.kind == "NL" {
            at_start = true;
            continue;
        }
        if at_start && depth == 0 {
            out.push(i);
        }
        at_start = false;
        if t.kind == "OP" && t.val == "{" {
            depth += 1;
        } else if t.kind == "OP" && t.val == "}" {
            depth -= 1;
        }
    }
    out
}

fn mangle(module: &str, name: &str) -> String {
    format!("{}_{}", module, name)
}

/// 取り込んだモジュールの全体
#[derive(Default)]
pub struct Loaded {
    pub toks: Vec<Tok>,
    pub origin: IndexMap<String, (String, String)>,
    pub files: IndexMap<String, String>,
    pub names: IndexMap<String, IndexMap<String, String>>,
    pub versions: IndexMap<String, String>,
}

pub type Pins = (Option<String>, Option<String>);
pub type Visible = IndexMap<String, IndexMap<String, String>>;

/// `use "path" [version "1.2.0"] [sha256 "…"]` の行を取り出す
pub fn split_uses(toks: Vec<Tok>) -> Result<(Vec<(String, i64, Pins)>, Vec<Tok>), ModuleError> {
    let mut uses = vec![];
    let mut keep = vec![];
    let mut skip_to: usize = 0;
    let starts: std::collections::HashSet<usize> = statement_starts(&toks).into_iter().collect();
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        if i < skip_to {
            i += 1;
            continue;
        }
        if starts.contains(&i) && t.kind == "NAME" && t.val == "use" {
            if i + 1 >= toks.len() || toks[i + 1].kind != "STR" {
                return Err(ModuleError::new("use の後に、取り込むファイルを \"path\" の形で書きます", t.line));
            }
            let mut version = None;
            let mut sha = None;
            skip_to = i + 2;
            for key in ["version", "sha256"] {
                if skip_to < toks.len() && toks[skip_to].kind == "NAME" && toks[skip_to].val == key {
                    if skip_to + 1 >= toks.len() || toks[skip_to + 1].kind != "STR" {
                        return Err(ModuleError::new(format!("{} の後に、値を \"…\" の形で書きます", key), t.line));
                    }
                    let v = toks[skip_to + 1].val.clone();
                    if key == "version" {
                        version = Some(v);
                    } else {
                        sha = Some(v);
                    }
                    skip_to += 2;
                }
            }
            uses.push((toks[i + 1].val.clone(), t.line, (version, sha)));
            i += 1;
            continue;
        }
        keep.push(t.clone());
        i += 1;
    }
    Ok((uses, keep))
}

/// `モジュール名 . 定義名` を、付け替えた名前の1つの字句にする
pub fn replace_refs(toks: Vec<Tok>, visible: &Visible) -> Result<Vec<Tok>, ModuleError> {
    let mut out = vec![];
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        if t.kind == "NAME" && visible.contains_key(&t.val) && i + 2 < toks.len() && toks[i + 1].kind == "OP" && toks[i + 1].val == "." && toks[i + 2].kind == "NAME" {
            let (module, n) = (&t.val, &toks[i + 2].val);
            match visible[module].get(n) {
                None => return Err(ModuleError::new(format!("モジュール {} に「{}」はありません", module, n), t.line)),
                Some(m) => {
                    let mut nt = t.clone();
                    nt.kind = "NAME";
                    nt.val = m.clone();
                    out.push(nt);
                }
            }
            i += 3;
        } else {
            out.push(t.clone());
            i += 1;
        }
    }
    Ok(out)
}

fn resolve(p: &Path) -> PathBuf {
    if p.starts_with(STD_DIR) {
        return p.to_path_buf();
    }
    std::fs::canonicalize(p).unwrap_or_else(|_| {
        let abs = if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(p) };
        let mut out = PathBuf::new();
        for c in abs.components() {
            match c {
                std::path::Component::ParentDir => {
                    out.pop();
                }
                std::path::Component::CurDir => {}
                c => out.push(c),
            }
        }
        out
    })
}

fn module_exists(p: &Path) -> bool {
    if let Ok(rest) = p.strip_prefix(STD_DIR) {
        return std_source(&rest.to_string_lossy()).is_some();
    }
    p.exists()
}

fn read_module(p: &Path) -> String {
    if let Ok(rest) = p.strip_prefix(STD_DIR) {
        return std_source(&rest.to_string_lossy()).unwrap().to_string();
    }
    std::fs::read_to_string(p).unwrap_or_default()
}

fn set_file(e: Error, path: &str) -> Error {
    match e {
        Error::Lex(mut l) => {
            if l.file.is_empty() {
                l.file = path.to_string();
            }
            Error::Lex(l)
        }
        Error::Module(mut m) => {
            if m.file.is_empty() {
                m.file = path.to_string();
            }
            Error::Module(m)
        }
        e => e,
    }
}

/// モジュールのファイルを読み込み、モジュール名を返す。同じファイルは1度だけ読む
pub fn load_module(path: &Path, loaded: &mut Loaded, stack: &[PathBuf], line: i64) -> Result<String, Error> {
    let path = resolve(path);
    let key = path.to_string_lossy().into_owned();
    if let Some(m) = loaded.files.get(&key) {
        return Ok(m.clone());
    }
    if stack.contains(&path) {
        let chain: Vec<String> = stack.iter().chain(std::iter::once(&path)).map(|p| crate::lexer::file_name(&p.to_string_lossy())).collect();
        return Err(ModuleError::new(format!("モジュールが循環して取り込まれています: {}", chain.join(" → ")), line).into());
    }
    if !module_exists(&path) {
        return Err(ModuleError::new(format!("モジュールのファイル {} がありません", crate::lexer::file_name(&key)), line).into());
    }
    load_module_inner(&path, loaded, stack).map_err(|e| set_file(e, &key))
}

fn load_module_inner(path: &Path, loaded: &mut Loaded, stack: &[PathBuf]) -> Result<String, Error> {
    let pstr = path.to_string_lossy().into_owned();
    let src = read_module(path);
    let mut toks: Vec<Tok> = lex(&src)?.into_iter().filter(|t| t.kind != "EOF").map(|mut t| {
        t.file = pstr.clone();
        t
    }).collect();
    let (uses, rest) = split_uses(toks)?;
    toks = rest;
    let mut st = stack.to_vec();
    st.push(path.to_path_buf());
    let visible = use_all(&uses, Some(path), loaded, &st)?;
    let starts = statement_starts(&toks);
    if starts.is_empty() || toks[starts[0]].val != "module" || toks.get(starts[0] + 1).map_or(true, |t| t.kind != "NAME") {
        return Err(ModuleError::new("モジュールのファイルは `module 名前` で始めます", toks.first().map_or(0, |t| t.line)).into());
    }
    let module = toks[starts[0] + 1].val.clone();
    if loaded.names.contains_key(&module) {
        return Err(ModuleError::new(format!("モジュール名「{}」が2つのファイルにあります", module), toks[starts[0]].line).into());
    }
    let mut declared: IndexMap<String, String> = IndexMap::new();
    for &i in &starts[1..] {
        let t = &toks[i];
        if t.kind == "NAME" && is_all_decl(&t.val) && !MODULE_DECLS.contains(&t.val.as_str()) {
            let mut sorted = MODULE_DECLS.to_vec();
            sorted.sort();
            return Err(ModuleError::new(format!("モジュールには {} を書けません（書けるのは {}。状態は持てません）", t.val, sorted.join("、")), t.line).into());
        }
        if t.kind == "NAME" && MODULE_DECLS.contains(&t.val.as_str()) && toks.get(i + 1).map_or(false, |n| n.kind == "NAME") {
            let n = toks[i + 1].val.clone();
            if declared.contains_key(&n) {
                return Err(ModuleError::new(format!("モジュール {} に「{}」が2つあります", module, n), t.line).into());
            }
            declared.insert(n.clone(), mangle(&module, &n));
        }
    }
    let mut head = 2;
    if toks.get(starts[0] + 2).map_or(false, |t| t.kind == "NAME" && t.val == "version") {
        match toks.get(starts[0] + 3) {
            Some(v) if v.kind == "STR" => {
                loaded.versions.insert(module.clone(), v.val.clone());
                head = 4;
            }
            _ => return Err(ModuleError::new("version の後に、\"1.0.0\" のような文字列を書きます", toks[starts[0]].line).into()),
        }
    }
    let mut body: Vec<Tok> = toks[..starts[0]].to_vec();
    body.extend_from_slice(&toks[(starts[0] + head).min(toks.len())..]);
    let body = replace_refs(body, &visible)?;
    let body: Vec<Tok> = body
        .into_iter()
        .map(|mut t| {
            if t.kind == "NAME" {
                if let Some(m) = declared.get(&t.val) {
                    t.val = m.clone();
                }
            }
            t
        })
        .collect();
    loaded.files.insert(pstr.clone(), module.clone());
    loaded.names.insert(module.clone(), declared.clone());
    for (n, m) in &declared {
        loaded.origin.insert(m.clone(), (module.clone(), n.clone()));
    }
    let last_line = body.last().map_or(0, |t| t.line);
    for t in body.into_iter().chain(std::iter::once(Tok::new("NL", "\n", last_line))) {
        if !(t.kind == "NL" && loaded.toks.last().map_or(false, |l| l.kind == "NL")) {
            loaded.toks.push(t);
        }
    }
    Ok(module)
}

pub fn use_all(uses: &[(String, i64, Pins)], base: Option<&Path>, loaded: &mut Loaded, stack: &[PathBuf]) -> Result<Visible, Error> {
    let mut visible: Visible = IndexMap::new();
    for (rel, line, (version, sha)) in uses {
        let mut target = match base {
            Some(b) => b.parent().unwrap_or(Path::new("")).join(rel),
            None => Path::new(STD_DIR).join(rel),
        };
        if !module_exists(&target) && std_source(rel).is_some() {
            target = Path::new(STD_DIR).join(rel);
        }
        let module = load_module(&target, loaded, stack, *line)?;
        if let Some(v) = version {
            let have = loaded.versions.get(&module).cloned().unwrap_or_default();
            if &have != v {
                return Err(ModuleError::new(format!("モジュール {} の version は「{}」で、use に書いた「{}」と違います", module, if have.is_empty() { "（無し）".to_string() } else { have }, v), *line).into());
            }
        }
        if let Some(h) = sha {
            let bytes = if module_exists(&target) {
                if target.starts_with(STD_DIR) { read_module(&target).into_bytes() } else { std::fs::read(&target).unwrap_or_default() }
            } else {
                vec![]
            };
            let hex: String = Sha256::digest(&bytes).iter().map(|b| format!("{:02x}", b)).collect();
            if &hex != h {
                return Err(ModuleError::new(format!("モジュール {} のソースのハッシュが、固定した値と違います（中身が変わっています）", module), *line).into());
            }
        }
        visible.insert(module.clone(), loaded.names[&module].clone());
    }
    Ok(visible)
}

/// 取り込む側の字句から、use を取り除き、モジュールの字句を前に足す。(字句の列, 付け替えた名前の対応)
pub fn expand(toks: Vec<Tok>, path: Option<&Path>) -> Result<(Vec<Tok>, IndexMap<String, (String, String)>), Error> {
    let (uses, toks) = split_uses(toks)?;
    if uses.is_empty() {
        return Ok((toks, IndexMap::new()));
    }
    if path.is_none() && !uses.iter().all(|(rel, _, _)| std_source(rel).is_some()) {
        return Err(ModuleError::new("use は、ファイルから読むときだけ使えます（geas に同梱の標準モジュールは除く）", toks.first().map_or(0, |t| t.line)).into());
    }
    let mut loaded = Loaded::default();
    let here = path.map(resolve);
    let stack: Vec<PathBuf> = here.iter().cloned().collect();
    let visible = use_all(&uses, here.as_deref(), &mut loaded, &stack)?;
    let mut out = std::mem::take(&mut loaded.toks);
    out.extend(replace_refs(toks, &visible)?);
    Ok((out, loaded.origin))
}
