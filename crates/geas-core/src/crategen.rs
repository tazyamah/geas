// SPDX-License-Identifier: GPL-3.0-or-later
//! 生成物（crate・サーバー・窓・画面・拡張・MCP）を書き出す（Python 版の examples.write_crate・servergen・windowgen・devgen・extgen・screengen.write_ui・mcp.write_mcp の移し）。
use crate::ast::*;
use crate::check::Checker;
use crate::hostgen::host_source;
use crate::mcp::tool_defs;
use crate::pynum::escape;
use crate::rawexpr::{requires_json, requires_tree};
use crate::rest::routes;
use crate::rustgen::{cargo_toml, generate, library_deps};
use crate::screengen::{design_of, query_def, CURRENCY_MINOR, DEFAULT_ACCENT};
use include_dir::{include_dir, Dir};
use serde_json::{json, Map as JMap, Value};
use std::fs;
use std::io::Write;
use std::path::Path;

pub static ASSETS: Dir = include_dir!("$CARGO_MANIFEST_DIR/assets");

pub const HOST_MOD: &str = "\n// 窓口（JSON と WASM の境界）は、外の値を読む所なので、生成規則の束縛の外に置く\n#[allow(unsafe_code, clippy::not_unsafe_ptr_arg_deref, clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects)]\npub mod host;\n";
pub const DEFAULT_SERVER: &str = "http://localhost:8787";

type R = Result<(), String>;

fn io<T>(r: std::io::Result<T>) -> Result<T, String> {
    r.map_err(|e| e.to_string())
}

fn write(path: &Path, text: &str) -> R {
    io(fs::write(path, crate::i18n::tr_bytes(text.as_bytes())))
}

fn mkdirs(p: &Path) -> R {
    io(fs::create_dir_all(p))
}

/// assets の dir（相対）の直下のファイルを、out へ複写する
fn copy_dir(dir: &str, out: &Path) -> R {
    mkdirs(out)?;
    let d = ASSETS.get_dir(dir).ok_or_else(|| format!("同梱のフォルダがありません: {}", dir))?;
    for f in d.files() {
        let name = f.path().file_name().unwrap();
        io(fs::write(out.join(name), crate::i18n::tr_bytes(f.contents())))?;
    }
    Ok(())
}

fn asset_text(path: &str) -> Result<&'static str, String> {
    ASSETS.get_file(path).and_then(|f| f.contents_utf8()).ok_or_else(|| format!("同梱のファイルがありません: {}", path))
}

/// Python の json.dumps(ensure_ascii=False, indent=n)
pub fn dumps_indent(v: &Value, indent: usize) -> String {
    let pad = vec![b' '; indent];
    let fmt = serde_json::ser::PrettyFormatter::with_indent(&pad);
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    serde::Serialize::serialize(v, &mut ser).unwrap();
    String::from_utf8(buf).unwrap()
}

// ---- server ----
const SERVER_CARGO: &str = "[package]\nname = \"geas_server\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n# 生成した Geas のクレート（ひとつ上）\n[dependencies]\ngeas_app = { path = \"..\" }\nrusqlite = { version = \"0.37\", features = [\"bundled\"] }\nserde_json = \"1\"\ntiny_http = \"0.12\"\n\n[features]\n{NET_FEATURES}\n# 整数の溢れの確認は、リリースビルドでも付ける（値の範囲の保証の網）。Cargo は組み立ての起点（このクレート）の\n# [profile] しか使わず、geas_app の Cargo.toml に書いた設定はここでは効かないので、ここにも書く\n[profile.release]\noverflow-checks = true\n";
const NET_DEP: &str = "ureq = { version = \"2\", optional = true, default-features = false, features = [\"tls\"] }\n";

pub fn write_server(dest: &Path, seed: Option<&str>, net: bool) -> R {
    let out = dest.join("server");
    mkdirs(&out.join("src"))?;
    copy_dir("server/src", &out.join("src"))?;
    io(fs::write(out.join("Cargo.lock"), ASSETS.get_file("server/Cargo.lock").ok_or("Cargo.lock")?.contents()))?;
    let mut cargo = SERVER_CARGO.replacen("tiny_http = \"0.12\"\n", &format!("tiny_http = \"0.12\"\n{}", if net { NET_DEP } else { "" }), 1);
    cargo = cargo.replacen("{NET_FEATURES}\n", if net { "net = [\"dep:ureq\"]\ndefault = [\"net\"]\n" } else { "net = []\n" }, 1);
    write(&out.join("Cargo.toml"), &cargo)?;
    write(&out.join(".gitignore"), "target/\n*.sqlite\n*.sqlite-*\n")?;
    let seed_text = match seed {
        Some(p) => io(fs::read_to_string(p))?,
        None => "{}".to_string(),
    };
    write(&out.join("src").join("seed.json"), &seed_text)
}

// ---- window ----
const WINDOW_CARGO: &str = "[package]\nname = \"geas_window\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n# 生成した Geas のクレート（ひとつ上）と、サーバーの実行部（経路の処理・保存・処理を動かす層。HTTP の待ち受けは使わない）\n[dependencies]\ngeas_app = { path = \"..\" }\ngeas_server = { path = \"../server\" }\nserde_json = \"1\"\ntao = \"0.34\"\nwry = \"0.54\"\n{RESIDENT_DEP}\n[features]\n{RESIDENT_FEATURES}\n# 整数の溢れの確認は、リリースビルドでも付ける（server と同じ理由。起点のクレートの [profile] だけが使われる）\n[profile.release]\noverflow-checks = true\n";

pub fn write_window(dest: &Path, resident: bool) -> R {
    let out = dest.join("window");
    mkdirs(&out.join("src"))?;
    io(fs::write(out.join("src").join("main.rs"), &crate::i18n::tr_bytes(ASSETS.get_file("window/src/main.rs").ok_or("window/src/main.rs")?.contents())))?;
    if let Some(l) = ASSETS.get_file("window/Cargo.lock") {
        io(fs::write(out.join("Cargo.lock"), l.contents()))?;
    }
    let mut cargo = WINDOW_CARGO.replacen("{RESIDENT_DEP}\n", if resident { "tray-icon = { version = \"0.26\", optional = true }\n" } else { "" }, 1);
    cargo = cargo.replacen("{RESIDENT_FEATURES}", if resident { "resident = [\"dep:tray-icon\"]\ndefault = [\"resident\"]\n" } else { "resident = []\n" }, 1);
    write(&out.join("Cargo.toml"), &cargo)?;
    write(&out.join(".gitignore"), "target/\n*.sqlite\n*.sqlite-*\n")
}

pub fn write_dev(dest: &Path) -> R {
    copy_dir("dev", &dest.join("dev"))
}

// ---- ui ----
fn png(rgb: [u8; 3], n: u32) -> Vec<u8> {
    let mut row = vec![0u8];
    for _ in 0..n {
        row.extend_from_slice(&rgb);
    }
    let mut raw_in: Vec<u8> = vec![];
    for _ in 0..n {
        raw_in.extend_from_slice(&row);
    }
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
    enc.write_all(&raw_in).unwrap();
    let raw = enc.finish().unwrap();
    fn chunk(t: &[u8], d: &[u8]) -> Vec<u8> {
        let mut o = (d.len() as u32).to_be_bytes().to_vec();
        o.extend_from_slice(t);
        o.extend_from_slice(d);
        let mut c = flate2::Crc::new();
        c.update(t);
        c.update(d);
        o.extend_from_slice(&c.sum().to_be_bytes());
        o
    }
    let mut ihdr = n.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&n.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(chunk(b"IDAT", &raw));
    out.extend(chunk(b"IEND", b""));
    out
}

pub fn write_ui(ck: &Checker, dest: &Path) -> R {
    let out = dest.join("ui");
    copy_dir("ui", &out)?;
    let p = ck.p;
    let accent = if let Some(s) = p.screens.first() { design_of(s).1 } else { DEFAULT_ACCENT.to_string() };
    let title = if !p.title.is_empty() { p.title.clone() } else if let Some(s) = p.screens.first() { p.show_names(&s.name) } else { String::new() };
    let first: String = title.chars().take(1).collect();
    write(
        &out.join("icon.svg"),
        &format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 512 512\"><rect width=\"512\" height=\"512\" rx=\"96\" fill=\"{}\"/><text x=\"256\" y=\"350\" font-size=\"300\" text-anchor=\"middle\" fill=\"#fff\" font-family=\"sans-serif\">{}</text></svg>\n",
            accent,
            escape(&first)
        ),
    )?;
    let hex = |i: usize| u8::from_str_radix(&accent[i..i + 2], 16).unwrap_or(0);
    let rgb = [hex(1), hex(3), hex(5)];
    for n in [192u32, 512] {
        io(fs::write(out.join(format!("icon-{}.png", n)), png(rgb, n)))?;
    }
    Ok(())
}

// ---- ブラウザ拡張 ----
pub fn ext_screens<'a>(ck: &Checker<'a>) -> Vec<&'a Screen> {
    ck.p.screens.iter().filter(|s| !s.pages.is_empty()).collect()
}

fn defs_of(ck: &Checker, sc: &Screen, server: &str) -> Value {
    let p = ck.p;
    let show = |s: &str| p.show_names(s);
    let qn = &sc.shows[0];
    let q = ck.queries[qn.as_str()];
    let qd = query_def(ck, q, &show);
    let route = routes(p).into_iter().find(|r| &r.target == qn).map(|r| (r.method, r.path)).expect("拡張の問い合わせの URL");
    let title = if !p.title.is_empty() { p.title.clone() } else { show(&sc.name) };
    let mut input = match &qd["inputs"][0] {
        Value::Object(m) => m.clone(),
        _ => JMap::new(),
    };
    let iname = input.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    input.insert("label".into(), json!(show(&iname)));
    let part = q.inputs[0].1.read.as_ref().and_then(|r| ck.parts.get(r.as_str()).copied());
    input.insert("requires".into(), requires_tree(part).map(|r| requires_json(&r)).unwrap_or_else(|| json!([])));
    let rd = sc.read.as_ref().expect("拡張の read");
    let mut lookup = JMap::new();
    for (k, v) in &rd.lookup.0 {
        lookup.insert(k.clone(), json!(v));
    }
    let mut minor = JMap::new();
    for (k, v) in CURRENCY_MINOR {
        minor.insert(k.into(), json!(v));
    }
    json!({
        "title": title, "label": show(&sc.name), "about": format!("ページを開くと、{}の「{}」を表示します。", title, show(qn)),
        "server": server, "route": {"method": route.0, "path": route.1},
        "input": input,
        "read": {"subject": rd.subject, "urls": rd.urls, "elements": rd.elements.iter().map(|(a, b)| json!([a, b])).collect::<Vec<_>>(), "rows": rd.value_rows, "lookup": lookup},
        "columns": qd["columns"], "minor": minor,
        "texts": {"loading": "調べています…", "unknown": "この商品の{input}が分かりません（{subject}）。",
                  "none": format!("この商品は、{}では扱っていません（{{input}} {{value}}）。", title), "open": format!("{}を開く", title),
                  "failed": "調べられません", "offline": format!("{}のサーバーにつながりません", title)},
    })
}

pub fn write_ext(ck: &Checker, dest: &Path, server: &str) -> R {
    let sc = ext_screens(ck)[0];
    let out = dest.join("ext");
    copy_dir("ext", &out)?;
    let d = defs_of(ck, sc, server);
    write(&out.join("defs.js"), &format!("self.GEAS_EXT = {};\n", dumps_indent(&d, 1)))?;
    let origin = format!("{}/*", server.trim_end_matches('/'));
    let other = if origin.contains("localhost") { Some(origin.replace("localhost", "127.0.0.1")) } else { None };
    let mut hosts = vec![json!(origin)];
    if let Some(o) = other {
        hosts.push(json!(o));
    }
    let manifest = json!({
        "manifest_version": 3, "name": d["label"], "version": "0.1.0",
        "description": format!("{}：{}", d["title"].as_str().unwrap_or(""), d["label"].as_str().unwrap_or("")),
        "permissions": ["storage"],
        "host_permissions": hosts,
        "background": {"service_worker": "background.js"},
        "content_scripts": [{"matches": sc.pages, "js": ["defs.js", "reader.js", "content.js"], "css": ["content.css"], "run_at": "document_idle"}],
        "action": {"default_title": d["label"], "default_popup": "popup.html"},
        "content_security_policy": {"extension_pages": "script-src 'self'; object-src 'self'"},
    });
    write(&out.join("manifest.json"), &(dumps_indent(&manifest, 2) + "\n"))
}

// ---- MCP ----
pub fn write_mcp(ck: &Checker, dest: &Path, server: &str) -> R {
    let out = dest.join("mcp");
    mkdirs(&out)?;
    write(&out.join("tools.json"), &(dumps_indent(&Value::Array(tool_defs(ck)?), 1) + "\n"))?;
    write(&out.join("bridge.py"), asset_text("mcp_bridge.py")?)?;
    let reg = json!({"mcpServers": {"geas": {"command": "python3", "args": ["bridge.py", "tools.json", server]}}});
    write(&out.join("mcp.json"), &(dumps_indent(&reg, 2) + "\n"))
}

/// 具体例の実行用 bin（計画書の具体例が無いときは、空の main）
pub fn runner_source_empty() -> String {
    "use geas_app::*;\nuse std::collections::{BTreeMap, BTreeSet};\n\n#[allow(unused_mut, non_snake_case, unused_variables)]\nfn main() {\n".to_string() + "\n}\n"
}

pub fn write_crate(ck: &Checker, lib_src: &str, runner_src: &str, dest: &Path, host: bool) -> R {
    mkdirs(&dest.join("src").join("bin"))?;
    write(&dest.join("Cargo.toml"), &cargo_toml("geas_app", host, &library_deps(ck)?))?;
    write(&dest.join("src").join("lib.rs"), &(lib_src.to_string() + if host { HOST_MOD } else { "" }))?;
    if host {
        write(&dest.join("src").join("host.rs"), &host_source(ck)?)?;
        write_ui(ck, dest)?;
        write_dev(dest)?;
    }
    write(&dest.join("src").join("bin").join("plan_examples.rs"), runner_src)
}

/// `rust` コマンドの生成本体。plan_md があれば、具体例の実行用 bin と、URL・画面の具体例のテストも作る。
/// 標準出力に出す行は msgs に積む（途中で失敗しても、そこまでの行は出す）
pub fn generate_tree(ck: &Checker, dest: &Path, host: bool, runtime: bool, standalone: bool, seed: Option<&str>, server: &str, plan_md: Option<&str>, msgs: &mut Vec<String>) -> R {
    let lib = generate(ck);
    let plan = plan_md.map(crate::plan::read_plan);
    let (src, problems) = match &plan {
        Some(p) => crate::examples::runner_source(ck, p.get("具体例").map(|v| v.as_slice()).unwrap_or(&[])),
        None => (runner_source_empty(), vec![]),
    };
    for p in problems {
        msgs.push(format!("{}: {}: {}", p.kind, p.where_, p.msg));
    }
    write_crate(ck, &lib, &src, dest, host)?;
    if host && runtime {
        write_server(dest, seed, ck.p.effects.iter().any(|e| e.http.is_some()))?;
        if standalone {
            write_window(dest, ck.p.resident.is_some())?;
        }
        if let Some(plan) = &plan {
            match crate::testgen::write_url_tests(ck, plan, dest) {
                Err(e) => return Err(format!("URL の具体例: {}", e)),
                Ok(Some(f)) => msgs.push(format!("URL の具体例のテストを生成しました: {}", f.display())),
                Ok(None) => {}
            }
            match crate::testgen::write_screen_tests(ck, plan, dest) {
                Err(e) => return Err(format!("画面の具体例: {}", e)),
                Ok(Some(f)) => msgs.push(format!("画面の具体例のテストを生成しました: {}", f.display())),
                Ok(None) => {}
            }
        }
    }
    if host {
        write_mcp(ck, dest, server)?;
    }
    if host && !ext_screens(ck).is_empty() {
        write_ext(ck, dest, server)?;
    }
    Ok(())
}
