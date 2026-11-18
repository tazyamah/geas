//! 実行時にユーザーが入力するパスの検査と展開。geas/rustgen.py の OS_PATH_RS（生成した crate の変換が使う関数）と同じ本文で、
//! 試験（tests/test_file_effect.py）が一致を確かめる。外部作用のファイル操作（fileio）が、使う直前にもう一度通す。
/// 実行時に入力されたパスを、実行中の OS の書式で展開し、絶対パスと使える文字を確かめる（失敗は入力不正）
pub fn os_path_expand(input: &str) -> Result<String, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("パスが空です".to_string());
    }
    if s.chars().any(|c| c == '\0' || c == '\n' || c == '\r') {
        return Err("パスに改行や NUL は使えません".to_string());
    }
    let env = |name: &str| -> Result<String, String> {
        match std::env::var(name) {
            Ok(v) if !v.is_empty() => Ok(v),
            _ => Err(format!("環境変数 {} が設定されていません", name)),
        }
    };
    let out = if cfg!(windows) {
        let mut out = String::new();
        let mut rest = s;
        while let Some((head, tail)) = rest.split_once('%') {
            out.push_str(head);
            let (name, after) = tail.split_once('%').ok_or_else(|| "% が閉じていません（%名前% の形で書きます）".to_string())?;
            out.push_str(&env(name)?);
            rest = after;
        }
        out.push_str(rest);
        out
    } else if s == "~" {
        env("HOME")?
    } else if let Some(t) = s.strip_prefix("~/") {
        format!("{}/{}", env("HOME")?, t)
    } else if s.starts_with('~') {
        return Err("~ のあとは / を書きます（~ユーザー名 は使えません）".to_string());
    } else {
        s.to_string()
    };
    if out.chars().count() > 4096 {
        return Err("パスが長すぎます（4096 文字まで）".to_string());
    }
    if cfg!(windows) {
        let drive = matches!(out.chars().collect::<Vec<char>>().as_slice(), [d, ':', sep, ..] if d.is_ascii_alphabetic() && (*sep == '\\' || *sep == '/'));
        if !(drive || out.starts_with("\\\\")) {
            return Err("絶対パス（C:\\… か \\\\サーバー\\…）で書いてください".to_string());
        }
        if out.chars().enumerate().any(|(i, c)| "<>\"|?*".contains(c) || (c == ':' && i != 1)) {
            return Err("Windows のパスに使えない文字があります（< > \" | ? * と、ドライブ以外の :）".to_string());
        }
    } else if !out.starts_with('/') {
        return Err("絶対パス（/ か ~ で始まる）で書いてください".to_string());
    }
    if out.split(['/', '\\']).any(|seg| seg == "..") {
        return Err("パスに .. は使えません".to_string());
    }
    Ok(out)
}
