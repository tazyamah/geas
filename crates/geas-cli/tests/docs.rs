// SPDX-License-Identifier: GPL-3.0-or-later
//! はじめかたの例（tests/fixtures/tutorial/memo.geas）：文書に載せた中身が実物と同じで、検査を通る
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_geas");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

#[test]
fn はじめかたの例は検査を通り_日英の文書に載せた中身と一致する() {
    let memo_path = root().join("tests").join("fixtures").join("tutorial").join("memo.geas");
    let memo = std::fs::read_to_string(&memo_path).unwrap();
    for doc in ["docs/getting-started.md", "docs/en/getting-started.md"] {
        let text = std::fs::read_to_string(root().join(doc)).unwrap();
        assert!(text.contains(memo.trim_end_matches('\n')), "{} に載せた memo.geas が実物と違います", doc);
    }
    let r = Command::new(BIN).args(["check", memo_path.to_str().unwrap()]).output().unwrap();
    let out = format!("{}{}", String::from_utf8_lossy(&r.stdout), String::from_utf8_lossy(&r.stderr));
    assert!(r.status.success() && out.contains("検査: 通りました"), "{}", out);
}
