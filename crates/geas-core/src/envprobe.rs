// SPDX-License-Identifier: GPL-3.0-or-later
//! 動いている機械の上で、環境の事実を読む（`geas env-probe`）。
//! 出す形は env-facts（terraform の plan から作る事実）と同じで、照合は env-verify が担う。
//! 読めなかった事実は null（合格にしない）。Linux の /proc を読むので、ほかの OS では ports 等が null になる。
use serde_json::{json, Value};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// /proc/net/tcp・tcp6 の本文から、待ち受け（LISTEN）中でループバック以外の口を返す
pub fn listening_ports(tcp: &str) -> Vec<u16> {
    let mut out: Vec<u16> = vec![];
    for line in tcp.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 4 || f[3] != "0A" {
            continue;
        }
        let Some((addr, port)) = f[1].rsplit_once(':') else { continue };
        let loopback = addr == "0100007F" || addr == "00000000000000000000000001000000" || addr.starts_with("0000000000000000FFFF00000100007F");
        if loopback {
            continue;
        }
        if let Ok(p) = u16::from_str_radix(port, 16) {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// /proc/self/mountinfo の本文から、path を含む一番深いマウントの（マウント先, 種類）
pub fn mount_of(path: &str, mountinfo: &str) -> Option<(String, String)> {
    let mut best: Option<(String, String)> = None;
    for line in mountinfo.lines() {
        let Some((pre, post)) = line.split_once(" - ") else { continue };
        let mp = pre.split_whitespace().nth(4)?.to_string();
        let fstype = post.split_whitespace().next()?.to_string();
        let under = path == mp || mp == "/" || path.starts_with(&format!("{}/", mp.trim_end_matches('/')));
        if under && best.as_ref().map_or(true, |(b, _)| mp.len() >= b.len()) {
            best = Some((mp, fstype));
        }
    }
    best
}

/// ファイルシステムの種類が、再起動や入れ替えで消えないか。分からなければ None
pub fn persistent(fstype: &str) -> Option<bool> {
    match fstype {
        "tmpfs" | "ramfs" | "devtmpfs" | "overlay" | "aufs" | "squashfs" | "proc" | "sysfs" => Some(false),
        "ext2" | "ext3" | "ext4" | "xfs" | "btrfs" | "zfs" | "f2fs" | "nfs" | "nfs4" | "ceph" | "glusterfs" | "cifs" => Some(true),
        _ => None,
    }
}

fn reaches(target: &str, secs: u64) -> bool {
    let t = if target.contains(':') { target.to_string() } else { format!("{}:443", target) };
    t.to_socket_addrs().ok().map_or(false, |mut it| it.any(|sa| TcpStream::connect_timeout(&sa, Duration::from_secs(secs)).is_ok()))
}

pub struct Options {
    pub db: Option<String>,
    pub egress: Vec<String>,
}

/// この機械の事実。読めない項目は null、手がかりは notes
pub fn collect(o: &Options) -> (Value, Vec<String>) {
    let mut notes: Vec<String> = vec![];
    let mut facts = serde_json::Map::new();
    for k in ["instances_max", "rollout_max", "shared_host", "storage_persistent", "internet_ingress_ports", "egress_https"] {
        facts.insert(k.into(), Value::Null);
    }
    // 口：全部の宛先で待ち受けているもの。前段のファイアウォール・LB は見えないので「機械として届きうる口」
    let mut tcp = String::new();
    for f in ["/proc/net/tcp", "/proc/net/tcp6"] {
        tcp += &std::fs::read_to_string(f).unwrap_or_default();
    }
    if tcp.is_empty() {
        notes.push("/proc/net/tcp を読めないので、開いている口は確かめられない".into());
    } else {
        facts.insert("internet_ingress_ports".into(), json!(listening_ports(&tcp).iter().map(|p| p.to_string()).collect::<Vec<_>>()));
        notes.push("口は、この機械で待ち受けている（ループバック以外の）ものを読んだ。前段のファイアウォールや LB は見ていない".into());
    }
    // 領域：保存先のファイルが載っているマウントの種類
    if let Some(db) = &o.db {
        let real = std::fs::canonicalize(db).or_else(|_| std::fs::canonicalize(std::path::Path::new(db).parent().unwrap_or(std::path::Path::new(".")))).map(|p| p.display().to_string());
        match (real, std::fs::read_to_string("/proc/self/mountinfo")) {
            (Ok(real), Ok(mi)) => match mount_of(&real, &mi) {
                Some((mp, fs)) => {
                    let p = persistent(&fs);
                    notes.push(format!("保存先 {} は {} の上（種類 {}）{}", real, mp, fs, if fs.starts_with("nfs") { "。NFS 上の sqlite はロックが当てにならない" } else { "" }));
                    if let Some(p) = p {
                        facts.insert("storage_persistent".into(), json!(p));
                    }
                }
                None => notes.push("保存先のマウントを特定できなかった".into()),
            },
            _ => notes.push(format!("保存先 {} の場所を読めなかった", db)),
        }
    }
    // 到達：宛先に TCP でつながるか。1 つでもつながれば true、全部だめなら false
    if !o.egress.is_empty() {
        let ok: Vec<&String> = o.egress.iter().filter(|t| reaches(t, 5)).collect();
        facts.insert("egress_https".into(), json!(!ok.is_empty()));
        notes.push(format!("外向きの到達：{}／{} 件につながった", ok.len(), o.egress.len()));
    }
    (Value::Object(facts), notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TCP: &str = "  sl  local_address rem_address   st\n   0: 00000000:1F90 00000000:0000 0A 0 0\n   1: 0100007F:0050 00000000:0000 0A 0 0\n   2: 0A00000A:01BB 00000000:0000 01 0 0\n   3: 0A00000A:0016 00000000:0000 0A 0 0\n";

    #[test]
    fn 待ち受けの口はループバックと接続中を除く() {
        assert_eq!(listening_ports(TCP), vec![22, 8080]);
    }

    const MI: &str = "22 1 8:1 / / rw - ext4 /dev/sda1 rw\n30 22 0:25 / /data rw - xfs /dev/sdb rw\n31 22 0:26 / /tmp rw - tmpfs tmpfs rw\n";

    #[test]
    fn 一番深いマウントを選ぶ() {
        assert_eq!(mount_of("/data/app/x.sqlite", MI), Some(("/data".into(), "xfs".into())));
        assert_eq!(mount_of("/tmp/x", MI), Some(("/tmp".into(), "tmpfs".into())));
        assert_eq!(mount_of("/database/x", MI), Some(("/".into(), "ext4".into())));
    }

    #[test]
    fn 消える種類と消えない種類() {
        assert_eq!(persistent("overlay"), Some(false));
        assert_eq!(persistent("ext4"), Some(true));
        assert_eq!(persistent("fuse.foo"), None);
    }
}
