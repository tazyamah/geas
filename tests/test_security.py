# SPDX-License-Identifier: GPL-3.0-or-later
"""生成器が、.geas や計画書の文字列を生成コードに埋めるときに、コードを差し込ませないこと。
人は .geas も生成コードも読まないので、表示名や計画書のセルに書いた文字列が、仕様書に出ないコードとして動いてはいけない。"""
import json
import re
import shutil
import subprocess
from pathlib import Path

import pytest

from geas.check import check
from geas.examples import runner_source, write_crate
from geas.hostgen import host_source
from geas.mutate import read_git
from geas.parser import parse
from geas.plan import read_plan
from geas.rustgen import doc_line, generate, rust_str

ROOT = Path(__file__).resolve().parent.parent
SHOP = ROOT / "tests" / "fixtures" / "shop"
OVERSEAS = ROOT / "tests" / "fixtures" / "overseas_shop" / "overseas_shop.geas"

needs_cargo = pytest.mark.skipif(shutil.which("cargo") is None, reason="cargo が無い")

# 文字列の外へ出ようとする文字の並び：生の文字列の終わり（"####）、行コメント、改行、波かっこ（書式の文字列）、逆斜線
NASTY = ['"####; pub fn injected() -> i32 { 6 * 7 } //', '"); panic!("x"); ("', "a\\", "x\ny", "{}{{", "\\u{41}", "\t\r\x00\x7f", "日本語 "]


def literal_value(src: str, start: int) -> tuple[str, int]:
    """src[start] から始まる Rust の文字列リテラル（"…"）を読み、(値, 閉じた " の次の位置) を返す"""
    assert src[start] == '"'
    out, i = [], start + 1
    simple = {"n": "\n", "r": "\r", "t": "\t", "\\": "\\", '"': '"', "0": "\0", "'": "'"}
    while src[i] != '"':
        if src[i] == "\\":
            if src[i + 1] == "u":
                j = src.index("}", i)
                out.append(chr(int(src[i + 3:j], 16)))
                i = j + 1
                continue
            out.append(simple[src[i + 1]])
            i += 2
            continue
        assert src[i] != "\r", "Rust の文字列リテラルに生の CR は書けない"
        out.append(src[i])
        i += 1
    return "".join(out), i + 1


def test_rust_str_はどんな文字列も_リテラルの外へ出さずに元の値で読み戻せる():
    for s in NASTY + ["", "ふつうの文字列", '"', "\\\\\""]:
        lit = rust_str(s)
        value, end = literal_value(lit, 0)
        assert value == s and end == len(lit), (s, lit)
        assert "\n" not in lit and "\r" not in lit
    for s in NASTY:
        assert "\n" not in doc_line(s) and "\r" not in doc_line(s) and " " not in doc_line(s)


def injected_overseas() -> str:
    """お店の例の、画面の表示名に、生の文字列を抜けてコードを書く文字列を入れたもの"""
    src = OVERSEAS.read_text()
    assert "/// 商品一覧の画面\n" in src
    return src.replace("/// 商品一覧の画面\n", "/// 商品一覧の画面" + NASTY[0] + "\n", 1)


def test_表示名に生の文字列の終わりを書いても_DEFS_JSON_は1つの文字列のまま():
    ck = check(parse(injected_overseas()), strict_names=True)
    assert ck.ready, ck.diags
    host = host_source(ck)
    head = "pub const DEFS_JSON: &str = "
    at = host.index(head) + len(head)
    value, end = literal_value(host, at)
    assert host[end:end + 2] == ";\n"  # リテラルのあとは、文の終わりだけ（差し込まれたコードは無い）
    defs = json.loads(value)
    assert defs["screens"]["screens"]["ProductList"]["label"] == "商品一覧の画面" + NASTY[0]
    assert "pub fn injected" not in host[:at] + host[end:]  # 差し込もうとした関数は、リテラルの中の文字としてだけある


@needs_cargo
def test_表示名に生の文字列の終わりを書いても_ビルドしたアプリに差し込んだ関数は無い(tmp_path):
    ck = check(parse(injected_overseas()), strict_names=True)
    main = 'fn main() { println!("{}", geas_app::host::DEFS_JSON); }\n'
    write_crate(ck, generate(ck), main, tmp_path, host=True)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline", "--bin", "plan_examples"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    assert json.loads(r.stdout)["screens"]["screens"]["ProductList"]["label"].endswith(NASTY[0])
    probe = tmp_path / "src" / "bin" / "probe.rs"
    probe.write_text("fn main() { println!(\"{}\", geas_app::host::injected()); }\n")
    r = subprocess.run(["cargo", "build", "--quiet", "--offline", "--bin", "probe"], cwd=tmp_path, capture_output=True, text=True)
    assert r.returncode != 0 and "injected" in r.stderr  # 差し込んだつもりの関数は、どこにも無い


def evil_plan(tmp_path) -> Path:
    """計画書の具体例の値に、Rust の文字列を抜けてファイルを書くコードを入れたもの。ID にも書式の文字（{}）と " を入れる"""
    plan = (SHOP / "shop_plan.md").read_text()
    payload = 'C9".to_string() + &{ std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/PWNED"), "x").ok(); String::new() } + &"'
    assert "顧客ID=C9," in plan and "| EX3 |" in plan
    plan = plan.replace("顧客ID=C9,", "顧客ID=" + payload + ",", 1).replace("| EX3 |", '| EX3"{} |', 1)
    out = tmp_path / "plan.md"
    out.write_text(plan)
    return out


def test_計画書の具体例の値は_生成した_Rust_の文字列から出ない(tmp_path):
    ck = check(parse((SHOP / "shop.geas").read_text()), strict_names=True)
    rows = read_plan(evil_plan(tmp_path).read_text())["具体例"]
    src, problems = runner_source(ck, rows)
    line = next(x for x in src.splitlines() if "PWNED" in x)
    m = re.search(r"customer_id: (\")", line)
    value, end = literal_value(line, m.start(1))
    assert value.startswith('C9".to_string()') and line[end:].startswith(".to_string(), items:")  # 値は文字列のまま、次の項目へ続く
    assert 'println!("{}\\t' in src and '"EX3\\"{}"' in src  # 具体例の ID は、書式の文字列ではなく引数で渡る


@needs_cargo
def test_計画書の具体例の値を動かしても_コードとしては動かない(tmp_path):
    ck = check(parse((SHOP / "shop.geas").read_text()), strict_names=True)
    rows = read_plan(evil_plan(tmp_path).read_text())["具体例"]
    src, problems = runner_source(ck, rows)
    crate = tmp_path / "crate"
    write_crate(ck, generate(ck), src, crate)
    r = subprocess.run(["cargo", "run", "--quiet", "--offline", "--bin", "plan_examples"], cwd=crate, capture_output=True, text=True)
    assert r.returncode == 0, r.stderr[-2500:]
    assert not (crate / "PWNED").exists()
    assert 'EX3"{}\tNoSuchCustomer' in r.stdout  # 長い ID の顧客はいない（値として比べられた）


def test_前の版の指定が_ハイフンで始まると_git_を呼ばずに断る(tmp_path):
    for rev in ("--output=/tmp/x", "-p", ""):
        with pytest.raises(ValueError, match="前の版の指定"):
            read_git(tmp_path / "a.geas", rev)
