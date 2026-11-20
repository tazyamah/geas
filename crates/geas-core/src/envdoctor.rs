// SPDX-License-Identifier: GPL-3.0-or-later
//! `geas doctor`：動いている機械（ホストやコンテナの中）の上で、`.geas` の要求を一度に確かめる。
//! env-probe の事実の収集と env-verify の照合に、機械の上でしか見えない確かめ（領域の書き込みと空き、
//! 台数、時刻同期、シークレットの有無）を足し、最後に「人の保証」の一覧を出す。判定は 4 つ。
//! シークレットの値は読まず、出力にも出さない（有無だけ）。
use crate::envcheck::{self, Res, FAIL, PASS, TRUST, UNKNOWN};
use crate::envprobe;
use crate::ast::Program;
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

/// env_file の「名前=値」の行から、値が入っている名前の一覧（値は返さない）
pub fn env_file_names(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.starts_with('#') {
                return None;
            }
            let (k, v) = l.split_once('=')?;
            if v.trim().trim_matches('"').is_empty() {
                None
            } else {
                Some(k.trim().to_string())
            }
        })
        .collect()
}

fn check_secret_in_file(item: &Res, file: &str, text: &Option<String>) -> Res {
    let var = secret_var(item.id.trim_start_matches("secret."));
    match text {
        None => res(&item.id, &item.aspect, &item.text, UNKNOWN, format!("環境ファイル {} を読めなかった", file)),
        Some(t) if env_file_names(t).contains(&var) => res(&item.id, &item.aspect, &item.text, PASS, format!("環境ファイル {} に {} の値がある（値は見ていない）", file, var)),
        Some(_) => res(&item.id, &item.aspect, &item.text, FAIL, format!("環境ファイル {} に {} の値が無い", file, var)),
    }
}

/// 宣言（container・infra）から導く確かめ。コンテナの中とホストで、見える物が違う
fn declared(p: &Program, in_box: bool) -> Vec<Res> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let (Some(c), Some(i)) = (p.container.as_ref(), p.infra.as_ref()) else { return vec![] };
    let mut out: Vec<Res> = vec![];
    let tcp = ["/proc/net/tcp", "/proc/net/tcp6"].iter().map(|f| std::fs::read_to_string(f).unwrap_or_default()).collect::<String>();
    let port = c.port as u16;
    let listen = envprobe::listening_any(&tcp).into_iter().filter(|(q, _)| *q == port).collect::<Vec<_>>();
    let t = format!("宣言した口 {} で待ち受けている", c.port);
    out.push(if listen.is_empty() {
        res("declared.port", "到達性", &t, FAIL, "待ち受けていない（サーバーが起動していない、または別の口で動いている）")
    } else if in_box && listen.iter().all(|(_, lo)| *lo) {
        res("declared.port", "到達性", &t, FAIL, "ループバックだけで待ち受けている（コンテナの外から届かない。--host 0.0.0.0 が要る）")
    } else {
        res("declared.port", "到達性", &t, PASS, format!("{} で待ち受けている", c.port))
    });
    if !listen.is_empty() {
        let t = "サーバーの /api/health が 200 を返す";
        out.push(match crate::envlive::fetch_default(&format!("http://127.0.0.1:{}", c.port), "/api/health") {
            Ok(200) => res("declared.health", "到達性", t, PASS, "GET /api/health → 200"),
            Ok(s) => res("declared.health", "到達性", t, FAIL, format!("GET /api/health → {}", s)),
            Err(e) => res("declared.health", "到達性", t, UNKNOWN, format!("届かなかった：{}", e)),
        });
    }
    if in_box {
        return out;
    }
    // ホストの側：データフォルダ・環境ファイル・Docker
    let t = format!("ホストのデータフォルダ {} が、番号 {} のユーザーに書ける", i.volume_host, crate::containergen::UID);
    out.push(match std::fs::metadata(&i.volume_host) {
        Err(e) => res("declared.volume", "安全性", &t, FAIL, format!("読めない：{}", e)),
        Ok(m) if !m.is_dir() => res("declared.volume", "安全性", &t, FAIL, "フォルダではない"),
        Ok(m) if m.uid() == crate::containergen::UID || m.permissions().mode() & 0o002 != 0 => res("declared.volume", "安全性", &t, PASS, format!("所有者 {}、権限 {:o}", m.uid(), m.permissions().mode() & 0o777)),
        Ok(m) => res("declared.volume", "安全性", &t, FAIL, format!("所有者 {}、権限 {:o} で、番号 {} は書けない（chown するか、権限を見直す）", m.uid(), m.permissions().mode() & 0o777, crate::containergen::UID)),
    });
    if !i.env_file.is_empty() {
        let t = format!("環境ファイル {} が、所有者以外に読めない", i.env_file);
        out.push(match std::fs::metadata(&i.env_file) {
            Err(e) => res("declared.env_file", "機密", &t, FAIL, format!("読めない：{}", e)),
            Ok(m) if m.permissions().mode() & 0o077 != 0 => res("declared.env_file", "機密", &t, FAIL, format!("権限 {:o} で、ほかのユーザーにも読める（chmod 600 にする）", m.permissions().mode() & 0o777)),
            Ok(m) => res("declared.env_file", "機密", &t, PASS, format!("権限 {:o}", m.permissions().mode() & 0o777)),
        });
    }
    let t = "docker compose が使える";
    let dv = std::process::Command::new("docker").args(["compose", "version", "--short"]).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    out.push(match dv {
        Some(v) => res("declared.docker", "安全性", t, PASS, format!("docker compose {}", v)),
        None => res("declared.docker", "安全性", t, UNKNOWN, "docker compose を実行できなかった（入っていない、または権限が無い）"),
    });
    out
}

pub fn run(p: &Program, items: &[EnvItem], o: &Options) -> (Vec<Res>, Vec<String>) {
    let in_box = in_container();
    // 宣言があれば、保存先は宣言から決まる（コンテナの中はコンテナ側のパス、ホストはホスト側のパス）
    let mut o = Options { db: o.db.clone(), egress: o.egress.clone(), secrets_file: o.secrets_file.clone() };
    if o.db.is_none() {
        if let (Some(c), Some(i)) = (p.container.as_ref(), p.infra.as_ref()) {
            o.db = Some(format!("{}/app.sqlite", if in_box { c.data.trim_end_matches('/') } else { i.volume_host.trim_end_matches('/') }));
        }
    }
    let o = &o;
    let env_file = p.infra.as_ref().filter(|i| !in_box && !i.env_file.is_empty()).map(|i| i.env_file.clone());
    let env_text = env_file.as_ref().and_then(|f| std::fs::read_to_string(f).ok());
    let (facts, mut notes) = envprobe::collect(&envprobe::Options { db: o.db.clone(), egress: o.egress.clone() });
    notes.insert(0, format!("実行場所：{}", if in_box { "コンテナの中" } else { "ホスト（コンテナの外）" }));
    if let Some(db) = &o.db {
        if o.secrets_file.is_none() && p.infra.is_some() {
            notes.push(format!("保存先は宣言から {} とした", db));
        }
    }
    let mut out: Vec<Res> = envcheck::verify(items, &facts)
        .into_iter()
        .map(|r| match (&env_file, r.id.starts_with("secret.")) {
            (Some(f), true) => check_secret_in_file(&r, f, &env_text),
            (None, true) => check_secret(&r, &o.secrets_file),
            _ => r,
        })
        .collect();
    out.extend(declared(p, in_box));
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
    fn 環境ファイルは値のある名前だけ数える() {
        let t = "# c\nGEAS_SECRET_A_B=xyz\nGEAS_SECRET_C_D=\nGEAS_SECRET_E_F=\"\"\n";
        assert_eq!(env_file_names(t), vec!["GEAS_SECRET_A_B".to_string()]);
    }

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
