// SPDX-License-Identifier: GPL-3.0-or-later
//! コーパスの突き合わせ：同じ入力に対する出力の指紋が、記録した正解（tests/golden/hashes.json）と一致すること。
//!
//! 正解は、Rust 化の前の Python 版（git のタグ python-final）の出力から作った。入力は tests/corpus.tar.xz（テストに直書きされた
//! .geas と、そこから作られた変種）と tests/fixtures/ の実物。比べるもの：astdump、check --json、spec、review（HTML）、
//! rust --host --runtime --standalone の生成物の木（実物だけ）。
//! 正解を作り直すときは、タグ python-final を取り出して `python tools/golden.py make` を走らせる。
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

const BIN: &str = env!("CARGO_BIN_EXE_geas");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn sha(b: &[u8]) -> String {
    Sha256::digest(b).iter().map(|x| format!("{:02x}", x)).collect::<String>()[..20].to_string()
}

fn run(args: &[&str], cwd: &Path) -> (i32, Vec<u8>, Vec<u8>) {
    let r = Command::new(BIN).env("GEAS_LANG", "ja").args(args).current_dir(cwd).output().unwrap();
    (r.status.code().unwrap_or(-1), r.stdout, r.stderr)
}

/// ファイルの木の指紋（相対パスの要素ごとの並びで並べる。Python 版の pathlib の並びと同じ）
fn tree_hash(d: &Path) -> String {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(Vec<String>, PathBuf)>) {
        for e in std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else if p.is_file() {
                let rel: Vec<String> = p.strip_prefix(base).unwrap().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                out.push((rel, p));
            }
        }
    }
    let mut files = vec![];
    walk(d, d, &mut files);
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut h = Sha256::new();
    for (rel, p) in files {
        h.update(rel.join("/").as_bytes());
        h.update([0u8]);
        h.update(std::fs::read(p).unwrap());
        h.update([0u8]);
    }
    h.finalize().iter().map(|x| format!("{:02x}", x)).collect::<String>()[..20].to_string()
}

fn entry(rc: i32, out: &str, err: &[u8]) -> Value {
    serde_json::json!([rc, out, sha(err)])
}

fn one(workdir: &Path, name: &str, full: bool) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    let (rc, o, e) = run(&["astdump", name], workdir);
    // 構文木は JSON の書き方（空白・末尾の改行）の違いをならして比べる。こちらは最初から詰めて書くので、末尾の改行だけを除く
    let o = o.strip_suffix(b"\n").unwrap_or(&o).to_vec();
    out.insert("ast".into(), entry(rc, &sha(&o), &e));
    let (rc, o, e) = run(&["check", name, "--json"], workdir);
    out.insert("check".into(), entry(rc, &sha(&o), &e));
    if rc == 0 {
        let (rc, o, e) = run(&["spec", name], workdir);
        out.insert("spec".into(), entry(rc, &sha(&o), &e));
        let html = workdir.join(format!("{}.review.html", name));
        let hname = format!("{}.review.html", name);
        let (rc, _, e) = run(&["review", name, "-o", &hname], workdir);
        out.insert("review".into(), entry(rc, &if html.exists() { sha(&std::fs::read(&html).unwrap()) } else { String::new() }, &e));
        let _ = std::fs::remove_file(&html);
        if full {
            let dname = format!("{}.rust", name);
            let dest = workdir.join(&dname);
            let (rc, _, e) = run(&["rust", name, "-o", &dname, "--host", "--runtime", "--standalone"], workdir);
            out.insert("rust".into(), entry(rc, &if dest.exists() { tree_hash(&dest) } else { String::new() }, &e));
            let _ = std::fs::remove_dir_all(&dest);
        }
    }
    out
}

fn extract_corpus(dest: &Path) {
    let xz = std::fs::read(root().join("tests").join("corpus.tar.xz")).unwrap();
    let mut tar = vec![];
    lzma_rs::xz_decompress(&mut std::io::Cursor::new(xz), &mut tar).unwrap();
    tar::Archive::new(std::io::Cursor::new(tar)).unpack(dest).unwrap();
}

fn fixtures(dir: &Path, base: &Path, out: &mut Vec<(Vec<String>, PathBuf)>) {
    for e in std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()) {
        let p = e.path();
        if p.is_dir() {
            fixtures(&p, base, out);
        } else if p.extension().map_or(false, |x| x == "geas") {
            out.push((p.strip_prefix(base).unwrap().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect(), p));
        }
    }
}

#[test]
fn コーパスの出力が正解の指紋と一致する() {
    let tmp = std::env::temp_dir().join(format!("geas-golden-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let corpus = tmp.join("corpus");
    extract_corpus(&corpus);
    // (キー, 作業フォルダ, ファイル名, 実物か)
    let mut items: Vec<(String, PathBuf, String, bool)> = vec![];
    let mut names: Vec<String> = std::fs::read_dir(&corpus).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".geas")).collect();
    names.sort();
    for n in names {
        items.push((format!("corpus/{}", n.trim_end_matches(".geas")), corpus.clone(), n, false));
    }
    let fx = root().join("tests").join("fixtures");
    let mut found = vec![];
    fixtures(&fx, &fx, &mut found);
    found.sort_by(|a, b| a.0.cmp(&b.0));
    for (rel, p) in found {
        items.push((format!("fixtures/{}", rel.join("/")), p.parent().unwrap().to_path_buf(), p.file_name().unwrap().to_string_lossy().into_owned(), true));
    }
    let results: Mutex<BTreeMap<String, BTreeMap<String, Value>>> = Mutex::new(BTreeMap::new());
    let next = Mutex::new(0usize);
    let jobs = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        for _ in 0..jobs {
            s.spawn(|| loop {
                let i = {
                    let mut g = next.lock().unwrap();
                    let i = *g;
                    *g += 1;
                    i
                };
                let Some((key, wd, name, full)) = items.get(i) else { break };
                let r = one(wd, name, *full);
                results.lock().unwrap().insert(key.clone(), r);
            });
        }
    });
    let got = results.into_inner().unwrap();
    // 指紋を取り直すとき：GEAS_GOLDEN_DUMP=出力先 を付けると、得た指紋を JSON で書き出す
    if let Ok(path) = std::env::var("GEAS_GOLDEN_DUMP") {
        std::fs::write(path, serde_json::to_string(&got).unwrap()).unwrap();
    }
    let want: BTreeMap<String, BTreeMap<String, Value>> = serde_json::from_str(&std::fs::read_to_string(root().join("tests").join("golden").join("hashes.json")).unwrap()).unwrap();
    let mut bad: Vec<String> = vec![];
    // corpus/29774ce42e3c は、Python 版の置き場所を絶対パスで指す（/home/user/geas/geas/std/num.geas）。
    // Python 版を消したあとはそのファイルが無く、その環境にも依らないので、突き合わせから外す。
    for (k, v) in want.iter().filter(|(k, _)| k.as_str() != "corpus/29774ce42e3c") {
        for (c, x) in v {
            if got.get(k).and_then(|g| g.get(c)) != Some(x) {
                bad.push(format!("{}  {}", k, c));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(bad.is_empty(), "{} 件中 {} か所に食い違い:\n{}", want.len(), bad.len(), bad.iter().take(30).cloned().collect::<Vec<_>>().join("\n"));
}
