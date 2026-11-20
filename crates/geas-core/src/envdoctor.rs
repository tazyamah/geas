// SPDX-License-Identifier: GPL-3.0-or-later
//! `geas doctor`：動いている機械（ホストやコンテナの中）の上で、`.geas` の要求を一度に確かめる。
//! env-probe の事実の収集と env-verify の照合に、機械の上でしか見えない確かめ（領域の書き込みと空き、
//! 台数、時刻同期、シークレットの有無）を足し、最後に「人の保証」の一覧を出す。判定は 4 つ。
//! シークレットの値は読まず、出力にも出さない（有無だけ）。
use crate::envcheck::{self, Res, FAIL, PASS, TRUST, UNKNOWN};
use crate::envprobe;
use crate::envreq::EnvItem;
use std::path::Path;

pub struct Options {
    pub db: Option<String>,
    pub egress: Vec<String>,
    pub secrets_file: Option<String>,
}

fn res(id: &str, aspect: &str, text: &str, status: &'static str, detail: impl Into<String>) -> Res {
    Res { id: id.into(), aspect: aspect.into(), text: text.into(), status, detail: detail.into() }
}

/// 生成サーバーが読む環境変数の名前（外部.名前 → GEAS_SECRET_外部_名前、大文字）
pub fn secret_var(target: &str) -> String {
    format!("GEAS_SECRET_{}", target.replace('.', "_").to_ascii_uppercase())
}

fn check_secret(item: &Res, secrets_file: &Option<String>) -> Res {
    let target = item.id.trim_start_matches("secret.");
    let var = secret_var(target);
    if std::env::var(&var).map_or(false, |v| !v.is_empty()) {
        return res(&item.id, &item.aspect, &item.text, PASS, format!("環境変数 {} がある（値は見ていない）", var));
    }
    if let Some(f) = secrets_file {
        return match std::fs::read_to_string(f).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
            Some(v) if v.get(target).and_then(|x| x.as_str()).map_or(false, |s| !s.is_empty()) => res(&item.id, &item.aspect, &item.text, PASS, format!("シークレットのファイルに {} がある（値は見ていない）", target)),
            Some(_) => res(&item.id, &item.aspect, &item.text, FAIL, format!("環境変数 {} も、シークレットのファイルの {} も無い", var, target)),
            None => res(&item.id, &item.aspect, &item.text, UNKNOWN, format!("環境変数 {} は無く、シークレットのファイル {} も読めなかった", var, f)),
        };
    }
    res(&item.id, &item.aspect, &item.text, FAIL, format!("環境変数 {} が無い（シークレットのファイルを使うなら --secrets で渡す）", var))
}

/// 保存先のファイルを開いているプロセスの数（/proc を読む。読めなければ None）
pub fn holders(db: &Path) -> Option<usize> {
    let real = std::fs::canonicalize(db).ok()?;
    let mut n = 0;
    for e in std::fs::read_dir("/proc").ok()?.flatten() {
        if !e.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(fds) = std::fs::read_dir(e.path().join("fd")) else { continue };
        if fds.flatten().any(|f| std::fs::read_link(f.path()).map_or(false, |t| t == real)) {
            n += 1;
        }
    }
    Some(n)
}

fn free_kb(dir: &Path) -> Option<u64> {
    let out = std::process::Command::new("df").args(["-Pk"]).arg(dir).output().ok()?;
    let t = String::from_utf8_lossy(&out.stdout).to_string();
    t.lines().nth(1)?.split_whitespace().nth(3)?.parse().ok()
}

fn data_dir(db: &str) -> std::path::PathBuf {
    let p = Path::new(db);
    match p.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => std::path::PathBuf::from("."),
    }
}

fn in_container() -> bool {
    Path::new("/.dockerenv").exists() || std::fs::read_to_string("/proc/1/cgroup").map_or(false, |t| t.contains("docker") || t.contains("containerd") || t.contains("kubepods"))
}

pub fn run(items: &[EnvItem], o: &Options) -> (Vec<Res>, Vec<String>) {
    let (facts, mut notes) = envprobe::collect(&envprobe::Options { db: o.db.clone(), egress: o.egress.clone() });
    notes.insert(0, format!("実行場所：{}", if in_container() { "コンテナの中" } else { "ホスト（コンテナの外）" }));
    let mut out: Vec<Res> = envcheck::verify(items, &facts).into_iter().map(|r| if r.id.starts_with("secret.") { check_secret(&r, &o.secrets_file) } else { r }).collect();
    // 領域：書き込みと空き
    if let Some(db) = &o.db {
        let dir = data_dir(db);
        let probe = dir.join(format!(".geas-doctor-{}", std::process::id()));
        let w = std::fs::write(&probe, b"x");
        let _ = std::fs::remove_file(&probe);
        out.push(match w {
            Ok(_) => res("doctor.writable", "安全性", "保存先のフォルダに、サーバーが書ける", PASS, format!("{} に書けた（確かめのための一時ファイルは消した）", dir.display())),
            Err(e) => res("doctor.writable", "安全性", "保存先のフォルダに、サーバーが書ける", FAIL, format!("{} に書けない：{}", dir.display(), e)),
        });
        out.push(match free_kb(&dir) {
            Some(kb) if kb >= 102_400 => res("doctor.space", "資源", "保存先の空きが 100 MiB 以上ある", PASS, format!("空き {} MiB", kb / 1024)),
            Some(kb) => res("doctor.space", "資源", "保存先の空きが 100 MiB 以上ある", FAIL, format!("空き {} MiB しかない", kb / 1024)),
            None => res("doctor.space", "資源", "保存先の空きが 100 MiB 以上ある", UNKNOWN, "df で空きを読めなかった"),
        });
        // 台数：同じ DB を開いているプロセスの数と、要求の上限
        let limit = items.iter().find(|i| i.id == "instances").and_then(|i| i.cond.get("at_most").and_then(|v| v.as_i64()));
        if let (Some(n), Some(lim)) = (holders(Path::new(db)), limit) {
            out.push(if n as i64 > lim {
                res("doctor.instances", "安全性", &format!("同じ DB を開いているサーバーは {} 台以下", lim), FAIL, format!("いま {} 個のプロセスが {} を開いている", n, db))
            } else {
                res("doctor.instances", "安全性", &format!("同じ DB を開いているサーバーは {} 台以下", lim), PASS, format!("いま {} 個のプロセスが開いている（この機械の分だけ。ほかの機械は見えない）", n))
            });
        }
    }
    // 時刻同期
    let ntp = std::process::Command::new("timedatectl").args(["show", "-p", "NTPSynchronized", "--value"]).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    out.push(match ntp.as_deref() {
        Some("yes") => res("doctor.time", "安全性", "時刻が同期している", PASS, "NTP と同期している"),
        Some("no") => res("doctor.time", "安全性", "時刻が同期している", FAIL, "NTP と同期していない"),
        _ => res("doctor.time", "安全性", "時刻が同期している", UNKNOWN, "timedatectl で読めなかった（コンテナの中ではホストの時刻に従う）"),
    });
    // 人の保証：geas の外。確認してもらう一覧
    for (id, text) in [
        ("human.host_os", "ホストの OS の更新・SSH・ディスクの管理"),
        ("human.docker", "Docker の導入と版、ホストのファイアウォール"),
        ("human.front", "前段の TLS と DNS"),
        ("human.backup", "DB ファイルのバックアップと退避"),
        ("human.secret_values", "シークレットの値そのものの管理"),
    ] {
        out.push(res(id, "人の保証", text, TRUST, "geas の外。手で用意し、そうなっていることを人が引き受ける"));
    }
    (out, notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn シークレットの環境変数名() {
        assert_eq!(secret_var("Payment.api_key"), "GEAS_SECRET_PAYMENT_API_KEY");
    }

    #[test]
    fn 無いシークレットは不合格_値は出さない() {
        let r = res("secret.Zzz.k", "機密", "t", UNKNOWN, "");
        let got = check_secret(&r, &None);
        assert_eq!(got.status, FAIL);
        assert!(got.detail.contains("GEAS_SECRET_ZZZ_K"));
    }

    #[test]
    fn 自分のプロセスが開いているファイルを数えられる() {
        let p = std::env::temp_dir().join(format!("geas-doctor-t-{}", std::process::id()));
        let _f = std::fs::File::create(&p).unwrap();
        assert!(holders(&p).unwrap() >= 1);
        let _ = std::fs::remove_file(&p);
    }
}
