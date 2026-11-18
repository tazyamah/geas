//! 外部作用のファイル操作（宣言した `over file`）。触れるのは、宣言した場所（path 型の引数が指す場所）の下の、宣言した相対パスのファイル 1 つだけ。
//! 書くのは新規作成だけ（すでにあれば書かない）。読む・書くは 1 MiB まで。場所は使う直前に、OS の書式で展開し直す（環境が変わっていても、黙って別の場所にしない）。
//! 結果は、ok・exists・not_found・denied・failed のどれかで返す（理由の詳細は標準エラーにだけ出し、パスや中身は載せない）。
use std::io::{Read, Write};

/// 1 件のファイル操作（宣言から決まる）
pub struct FileOp {
    /// write（新規作成だけ）・overwrite（作るか置き換える）・read・delete
    pub op: String,
    /// 場所（入力されたパス。展開前）
    pub dir: String,
    /// 場所の下の相対パス（/ 区切り）
    pub rest: String,
    /// write のとき、書く内容
    pub content: Option<String>,
}

pub struct Done {
    pub kind: &'static str,
    /// read の ok のとき、ファイルの中身
    pub content: Option<String>,
    /// 読んだ・書いたバイト数
    pub bytes: u64,
    /// 失敗の理由（標準エラーにだけ出す。パスは含めない）
    pub detail: String,
}

const LIMIT: u64 = 1 << 20;

fn failed(kind: &'static str, detail: impl Into<String>) -> Done {
    Done { kind, content: None, bytes: 0, detail: detail.into() }
}

fn kind_of(e: &std::io::Error) -> &'static str {
    match e.kind() {
        std::io::ErrorKind::NotFound => "not_found",
        std::io::ErrorKind::PermissionDenied => "denied",
        std::io::ErrorKind::AlreadyExists => "exists",
        _ => "failed",
    }
}

pub fn run(op: &FileOp) -> Done {
    let dir = match crate::ospath::os_path_expand(&op.dir) {
        Ok(d) => d,
        Err(e) => return failed("failed", e),
    };
    if op.rest.split('/').any(|s| s.is_empty() || s == "." || s == ".." || s.contains(['\\', ':', '\0'])) {
        return failed("failed", "ファイル名が正しくありません");
    }
    let mut path = std::path::PathBuf::from(&dir);
    for seg in op.rest.split('/') {
        path.push(seg);
    }
    if op.op == "delete" {
        // あるファイル（またはリンク）だけを消す。フォルダは消えない（remove_file は、フォルダには失敗する）
        return match std::fs::remove_file(&path) {
            Ok(()) => Done { kind: "ok", content: None, bytes: 0, detail: String::new() },
            Err(e) => failed(kind_of(&e), e.kind().to_string()),
        };
    }
    if op.op == "overwrite" {
        let body = op.content.clone().unwrap_or_default();
        if body.len() as u64 > LIMIT {
            return failed("failed", "書く内容が大きすぎます（1 MiB まで）");
        }
        // 同じフォルダの一時ファイルに書いてから、名前を付け替える（途中で止まっても、元のファイルは半端にならない）。
        // 付け替えは、元がリンクならリンクそのものを置き換える（リンク先は書き換えない）
        let tmp = path.with_file_name(format!(".geas-tmp-{}-{}", std::process::id(), op.rest.replace('/', "_")));
        let written = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp).and_then(|mut f| f.write_all(body.as_bytes()).and_then(|_| f.sync_all()));
        if let Err(e) = written {
            let _ = std::fs::remove_file(&tmp);
            return failed(kind_of(&e), e.kind().to_string());
        }
        return match std::fs::rename(&tmp, &path) {
            Ok(()) => Done { kind: "ok", content: None, bytes: body.len() as u64, detail: String::new() },
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                failed(kind_of(&e), e.kind().to_string())
            }
        };
    }
    if op.op == "write" {
        let body = op.content.clone().unwrap_or_default();
        if body.len() as u64 > LIMIT {
            return failed("failed", "書く内容が大きすぎます（1 MiB まで）");
        }
        // 新規作成だけ（すでにあれば exists。既存のファイルや、そこへ向くリンクを書き換えない）
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => match f.write_all(body.as_bytes()) {
                Ok(()) => Done { kind: "ok", content: None, bytes: body.len() as u64, detail: String::new() },
                Err(e) => failed(kind_of(&e), e.kind().to_string()),
            },
            Err(e) => failed(kind_of(&e), e.kind().to_string()),
        }
    } else {
        match std::fs::File::open(&path) {
            Ok(f) => {
                if !f.metadata().map(|m| m.is_file()).unwrap_or(false) {
                    return failed("failed", "ファイルではありません");
                }
                let mut text = String::new();
                match f.take(LIMIT + 1).read_to_string(&mut text) {
                    Ok(n) if n as u64 > LIMIT => failed("failed", "ファイルが大きすぎます（1 MiB まで）"),
                    Ok(n) => Done { kind: "ok", content: Some(text), bytes: n as u64, detail: String::new() },
                    Err(e) => failed("failed", e.kind().to_string()),
                }
            }
            Err(e) => failed(kind_of(&e), e.kind().to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("geas_fileio_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn write_creates_once_and_read_returns_it() {
        let d = tmp("rw");
        let w = |c: &str| run(&FileOp { op: "write".into(), dir: d.to_string_lossy().into_owned(), rest: "a/b.txt".into(), content: Some(c.into()) });
        assert_eq!(w("x").kind, "not_found");  // 場所の下の a/ が無い
        std::fs::create_dir_all(d.join("a")).unwrap();
        let first = w("hello");
        assert_eq!((first.kind, first.bytes), ("ok", 5));
        assert_eq!(w("again").kind, "exists");
        let r = run(&FileOp { op: "read".into(), dir: d.to_string_lossy().into_owned(), rest: "a/b.txt".into(), content: None });
        assert_eq!((r.kind, r.content.as_deref()), ("ok", Some("hello")));
        assert_eq!(run(&FileOp { op: "read".into(), dir: d.to_string_lossy().into_owned(), rest: "a/none.txt".into(), content: None }).kind, "not_found");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn overwrite_replaces_and_delete_removes_only_files() {
        let d = tmp("ow");
        let dir = d.to_string_lossy().into_owned();
        let op = |o: &str, rest: &str, c: Option<&str>| run(&FileOp { op: o.into(), dir: dir.clone(), rest: rest.into(), content: c.map(String::from) });
        assert_eq!(op("overwrite", "f.txt", Some("one")).kind, "ok");  // 無ければ作る
        assert_eq!(op("overwrite", "f.txt", Some("two2")).bytes, 4);  // あれば置き換える
        assert_eq!(std::fs::read_to_string(d.join("f.txt")).unwrap(), "two2");
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);  // 一時ファイルは残らない
        std::fs::create_dir(d.join("sub")).unwrap();
        assert_eq!(op("delete", "sub", None).kind, "failed");  // フォルダは消さない
        assert!(d.join("sub").is_dir());
        assert_eq!(op("delete", "f.txt", None).kind, "ok");
        assert_eq!(op("delete", "f.txt", None).kind, "not_found");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rejects_escape_and_relative_dir() {
        let d = tmp("esc");
        assert_eq!(run(&FileOp { op: "write".into(), dir: d.to_string_lossy().into_owned(), rest: "../x.txt".into(), content: Some("x".into()) }).kind, "failed");
        assert_eq!(run(&FileOp { op: "write".into(), dir: "relative".into(), rest: "x.txt".into(), content: Some("x".into()) }).kind, "failed");
        let _ = std::fs::remove_dir_all(&d);
    }
}
