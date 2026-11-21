// SPDX-License-Identifier: GPL-3.0-or-later
//! Kani による証明の実行（通常のテスト＝cargo test には入れない。`cargo install --locked kani-verifier && cargo kani setup` が済んでいること）。
//!
//! - `proofs`：生成物の前置き（crates/geas-core/src/assets/rust_prelude.txt）に proofs/prelude_proofs.rs を連結した crate を作り、証明を 1 つずつ回す
//! - `std-proofs`：標準部品（crates/geas-core/assets/std/num.geas）を、引数の型の全部の値について示す。部品ごとに 2 段：
//!   式どおり（戻り値が、桁の限りを気にしない計算 i128 の式と全部の入力で等しい）と、panic しない（panic も計算のあふれも起きない）。
//!   時間内に終わらない証明は成功に数えない。`--record` で、境界値の検査も回して台帳（num.parts.json）に証明した内容を書く（仕様書の「人の保証」に出る）。
//!   geas の実行ファイルは、環境変数 GEAS か PATH から探す。
use crate::repo_root;
use regex::Regex;
use serde_json::Value;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

const CARGO_PRELUDE: &str = "[package]\nname = \"prelude_proofs\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[lints.rust]\nunexpected_cfgs = { level = \"warn\", check-cfg = ['cfg(kani)'] }\n";
const CARGO_STD: &str = "[package]\nname = \"std_proofs\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[lints.rust]\nunexpected_cfgs = { level = \"warn\", check-cfg = ['cfg(kani)'] }\n";

fn tmpdir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("geas-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("src")).unwrap();
    d
}

/// cargo kani を、上限の時間つきで回す。(終了コード, 標準出力, 時間内に終わったか)
fn run_with_timeout(mut cmd: Command, limit: Duration) -> Result<(i32, String, bool), String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("cargo kani を起動できません（{}）", e))?;
    let (mut so, mut se) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let t1 = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = so.read_to_string(&mut s);
        s
    });
    let t2 = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = se.read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            break Some(st);
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let out = t1.join().unwrap();
    let err = t2.join().unwrap();
    Ok((status.and_then(|s| s.code()).unwrap_or(-1), out + &err, status.is_some()))
}

/// 前置きの証明を 1 つずつ実行する。戻り値は、失敗した（または時間内に終わらなかった）証明の数
pub fn prelude(timeout: u64) -> Result<usize, String> {
    let root = repo_root();
    let prelude_src = std::fs::read_to_string(root.join("crates/geas-core/src/assets/rust_prelude.txt")).map_err(|e| e.to_string())?;
    let proofs = std::fs::read_to_string(root.join("proofs/prelude_proofs.rs")).map_err(|e| e.to_string())?;
    let d = tmpdir("prelude-proofs");
    std::fs::write(d.join("Cargo.toml"), CARGO_PRELUDE).unwrap();
    std::fs::write(d.join("src/lib.rs"), format!("{}\n{}", prelude_src, proofs)).unwrap();
    let re = Regex::new(r"#\[kani::proof\](?:\s*#\[[^\]]*\])*\s*fn (\w+)").unwrap();
    let mut bad = 0;
    for c in re.captures_iter(&proofs) {
        let h = &c[1];
        let mut cmd = Command::new("cargo");
        cmd.args(["kani", "--harness", h]).current_dir(&d);
        let (code, out, finished) = run_with_timeout(cmd, Duration::from_secs(timeout))?;
        let ok = finished && code == 0 && out.contains("VERIFICATION:- SUCCESSFUL");
        let note = if ok {
            "成功".to_string()
        } else if !finished {
            format!("{} 秒で終わらず", timeout)
        } else {
            let lines: Vec<&str> = out.lines().filter(|x| x.contains("Failed Checks") || x.contains("VERIFICATION")).collect();
            format!("失敗\n{}", lines.join("\n"))
        };
        println!("{}: {}", h, note);
        if !ok {
            bad += 1;
        }
    }
    let _ = std::fs::remove_dir_all(&d);
    println!("前置きの証明: {}", if bad == 0 { "すべて成功".to_string() } else { format!("{} 件が成功していません", bad) });
    Ok(bad)
}

// ---- 標準部品 ----
const EQ: &str = "式どおりで、panic しない";
const SAFE: &str = "panic しない";

fn wrap(v: &str) -> String {
    format!("(({}) - LO).rem_euclid(M) + LO", v)
}

/// メソッド名 → 戻り値 R が満たす式（Rust）。A・B は引数、R は戻り値で、どれも i128（真偽の戻り値は bool のまま）。
/// LO・HI は self の型の範囲、M は 2 のビット数乗。Rust の整数のメソッドとは別の書き方（桁の限りを気にしない計算）で書く
fn spec(method: &str) -> Option<String> {
    let s: String = match method {
        "abs_diff" => "R == (A - B).abs()".into(),
        "cast_signed" => "R == if A >= M / 2 { A - M } else { A }".into(),
        "cast_unsigned" => "R == A.rem_euclid(M)".into(),
        "is_negative" => "R == (A < 0)".into(),
        "is_positive" => "R == (A > 0)".into(),
        "is_power_of_two" => "R == (A > 0 && (A & (A - 1)) == 0)".into(),
        "isqrt" => "R >= 0 && R * R <= A && (R + 1) * (R + 1) > A".into(),
        "midpoint" => "R == (A + B) / 2".into(), // i128 の / は 0 の側へ丸める
        "saturating_abs" => "R == A.abs().clamp(LO, HI)".into(),
        "saturating_add" | "saturating_add_signed" | "saturating_add_unsigned" => "R == (A + B).clamp(LO, HI)".into(),
        "saturating_sub" | "saturating_sub_signed" | "saturating_sub_unsigned" => "R == (A - B).clamp(LO, HI)".into(),
        "saturating_mul" => "R == (A * B).clamp(LO, HI)".into(),
        "saturating_neg" => "R == (-A).clamp(LO, HI)".into(),
        "signum" => "R == if A > 0 { 1 } else if A < 0 { -1 } else { 0 }".into(),
        "to_le" => "R == A || cfg!(target_endian = \"big\")".into(),
        "wrapping_abs" => format!("R == {}", wrap("A.abs()")),
        "wrapping_add" | "wrapping_add_signed" | "wrapping_add_unsigned" => format!("R == {}", wrap("A + B")),
        "wrapping_sub" | "wrapping_sub_signed" | "wrapping_sub_unsigned" => format!("R == {}", wrap("A - B")),
        "wrapping_mul" => format!("R == {}", wrap("A * B")),
        "wrapping_neg" => format!("R == {}", wrap("-A")),
        _ => return None,
    };
    Some(s)
}

pub struct Part {
    pub name: String,
    params: Vec<(String, String)>, // (名前, Rust の型)
    ret_kind: String,
    ret_ty: String,
    expr: String,
}

fn geas_bin() -> String {
    std::env::var("GEAS").unwrap_or_else(|_| "geas".to_string())
}

fn num_geas() -> std::path::PathBuf {
    repo_root().join("crates/geas-core/assets/std/num.geas")
}

pub fn std_parts() -> Result<Vec<Part>, String> {
    let r = Command::new(geas_bin()).arg("parts").arg(num_geas()).arg("--describe").output().map_err(|e| format!("geas を起動できません（{}）。環境変数 GEAS に実行ファイルを指定してください", e))?;
    if !r.status.success() {
        return Err(String::from_utf8_lossy(&r.stderr).into_owned());
    }
    let v: Value = serde_json::from_slice(&r.stdout).map_err(|e| e.to_string())?;
    let mut out = vec![];
    for d in v.as_array().ok_or("describe の出力が配列ではありません")? {
        let s = |x: &Value| x.as_str().unwrap_or("").to_string();
        out.push(Part {
            name: s(&d["name"]),
            params: d["params"].as_array().unwrap().iter().map(|p| (s(&p[0]), s(&p[2]))).collect(),
            ret_kind: s(&d["ret"][0]),
            ret_ty: s(&d["ret"][1]),
            expr: s(&d["expr"]),
        });
    }
    Ok(out)
}

fn name_re() -> Regex {
    Regex::new(r"^Num_([iu])(8|16|32|64)_(\w+)$").unwrap()
}

/// 証明の Rust。部品ごとに safe_名前（panic しない）と、式があれば eq_名前（式どおり）
pub fn source(parts: &[Part]) -> String {
    let re = name_re();
    let mut l: Vec<String> = vec!["#![allow(warnings)]".into(), "#[cfg(kani)]".into(), "mod proofs {".into()];
    for pt in parts {
        let m = re.captures(&pt.name).expect("標準部品の名前が Num_i8_… の形ではありません");
        let bits: u32 = m[2].parse().unwrap();
        let signed = &m[1] == "i";
        let method = m[3].to_string();
        let (lo, hi): (i128, i128) = if signed { (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1) } else { (0, (1i128 << bits) - 1) };
        let anys: Vec<String> = pt.params.iter().map(|(n, t)| format!("let {}: {} = kani::any();", n, t)).collect();
        let anys = anys.join(" ");
        let call = format!("let r: {} = {{ {} }};", pt.ret_ty, pt.expr);
        l.push(format!("    #[kani::proof]\n    fn safe_{}() {{ {} {} }}", pt.name, anys, call));
        if let Some(sp) = spec(&method) {
            let names: Vec<&String> = pt.params.iter().map(|(n, _)| n).collect();
            let mut binds = format!("let A = {} as i128;", names[0]);
            if names.len() > 1 {
                binds.push_str(&format!(" let B = {} as i128;", names[1]));
            }
            let ret = if pt.ret_kind == "真偽" { "let R = r;" } else { "let R = r as i128;" };
            l.push(format!(
                "    #[kani::proof]\n    fn eq_{}() {{ {} {} {} {} let LO: i128 = {}; let HI: i128 = {}; let M: i128 = {}; assert!({}); }}",
                pt.name,
                anys,
                call,
                binds,
                ret,
                lo,
                hi,
                1i128 << bits,
                sp
            ));
        }
    }
    l.push("}".into());
    l.join("\n") + "\n"
}

/// 部品名 → 証明できた内容（EQ か SAFE）。どちらも示せなかった部品は入らない
pub fn std_run(parts: &[Part], timeout: u64, jobs: u32) -> Result<Vec<(String, &'static str)>, String> {
    let d = tmpdir("std-proofs");
    std::fs::write(d.join("Cargo.toml"), CARGO_STD).unwrap();
    std::fs::write(d.join("src/lib.rs"), source(parts)).unwrap();
    let r = Command::new("cargo")
        .args(["kani", "-j", &jobs.to_string(), "--output-format", "terse", "-Z", "unstable-options", "--harness-timeout", &format!("{}s", timeout)])
        .current_dir(&d)
        .output()
        .map_err(|e| format!("cargo kani を起動できません（{}）", e))?;
    let _ = std::fs::remove_dir_all(&d);
    let stdout = String::from_utf8_lossy(&r.stdout).into_owned();
    let all = stdout.clone() + &String::from_utf8_lossy(&r.stderr);
    if !Regex::new(r"Complete - (\d+) successfully verified harnesses, (\d+) failures, (\d+) total").unwrap().is_match(&stdout) {
        let tail: String = all.chars().rev().take(3000).collect::<Vec<_>>().into_iter().rev().collect();
        return Err(format!("Kani が証明を実行できませんでした:\n{}", tail));
    }
    let failed: std::collections::HashSet<String> = Regex::new(r"Verification failed for - proofs::(\w+)").unwrap().captures_iter(&stdout).map(|c| c[1].to_string()).collect();
    let re = name_re();
    let mut out = vec![];
    for pt in parts {
        let method = re.captures(&pt.name).unwrap()[3].to_string();
        let safe = !failed.contains(&format!("safe_{}", pt.name));
        if safe && spec(&method).is_some() && !failed.contains(&format!("eq_{}", pt.name)) {
            out.push((pt.name.clone(), EQ));
        } else if safe {
            out.push((pt.name.clone(), SAFE));
        }
    }
    Ok(out)
}

pub fn report(proved: &[(String, &'static str)], parts: &[Part]) -> String {
    let re = name_re();
    let get = |n: &str| proved.iter().find(|(k, _)| k == n).map(|(_, v)| *v);
    let eq = proved.iter().filter(|(_, v)| *v == EQ).count();
    let mut rest: Vec<&str> = parts.iter().filter(|p| get(&p.name).is_none()).map(|p| p.name.as_str()).collect();
    rest.sort();
    let mut l = vec![format!("標準部品の証明：{} 個のうち、式どおりと示せたもの {} 個、panic しないとだけ示せたもの {} 個、示せなかったもの {} 個", parts.len(), eq, proved.len() - eq, rest.len())];
    for n in &rest {
        l.push(format!("  － {}：時間内に示せませんでした（境界値の検査だけが根拠）", n));
    }
    let mut lost: Vec<&str> = parts.iter().filter(|p| spec(&re.captures(&p.name).unwrap()[3]).is_some() && get(&p.name) == Some(SAFE)).map(|p| p.name.as_str()).collect();
    lost.sort();
    for n in lost {
        l.push(format!("  － {}：式どおりかは、時間内に示せませんでした（panic しないことは示した）", n));
    }
    l.join("\n")
}

/// 標準部品の証明の入口。戻り値は終了コード
pub fn std_main(timeout: u64, jobs: u32, record: bool, print_source: bool) -> Result<i32, String> {
    let parts = std_parts()?;
    if print_source {
        print!("{}", source(&parts));
        return Ok(0);
    }
    let proved = std_run(&parts, timeout, jobs)?;
    println!("{}", report(&proved, &parts));
    if !record {
        return Ok(0);
    }
    let check = Command::new(geas_bin()).arg("parts").arg(num_geas()).output().map_err(|e| e.to_string())?;
    if !check.status.success() {
        print!("{}", String::from_utf8_lossy(&check.stdout));
        eprintln!("境界値の検査で食い違いがあるので、台帳は書きません");
        return Ok(1);
    }
    let mut obj = serde_json::Map::new();
    for (k, v) in &proved {
        obj.insert(k.clone(), Value::String(v.to_string()));
    }
    let f = std::env::temp_dir().join(format!("geas-proved-{}.json", std::process::id()));
    std::fs::write(&f, serde_json::to_string(&Value::Object(obj)).unwrap()).map_err(|e| e.to_string())?;
    let r = Command::new(geas_bin()).arg("parts").arg(num_geas()).arg("--record").arg("--proved").arg(&f).output().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&f);
    let so = String::from_utf8_lossy(&r.stdout);
    println!("{}", so.trim().lines().last().unwrap_or(""));
    Ok(r.status.code().unwrap_or(1))
}

