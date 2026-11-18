#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Rust 版との突き合わせの道具（Rust 化の移行用）。

Python 版（python -m geas）を正解にして、同じ入力に対する出力を記録し、別の実装（Rust 版）と比べる。
  python tools/golden.py make                    正解の出力のハッシュを tests/golden/hashes.json に作り直す
  python tools/golden.py verify [--cmd "コマンド"]  コマンド（既定は python -m geas）の出力を hashes.json と比べる
  python tools/golden.py corpus-record           pytest を回して、テストが parse() に渡した .geas を採り直す（tests/corpus.tar.xz）
入力は、tests/corpus.tar.xz（テストに直書きされた .geas と、そこから作られた変種）と tests/fixtures/ の実物。
比べるもの：check --json、spec、review（HTML）、rust --host --runtime --standalone の生成物の木（実物だけ）。
"""
import argparse
import hashlib
import json
import os
import shlex
import shutil
import subprocess
import sys
import tarfile
import tempfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CORPUS = ROOT / "tests" / "corpus.tar.xz"
HASHES = ROOT / "tests" / "golden" / "hashes.json"
FIXTURES = ROOT / "tests" / "fixtures"


def sha(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()[:20]


def run(cmd, args, cwd):
    r = subprocess.run(cmd + args, cwd=cwd, capture_output=True, timeout=300)
    return r


def tree_hash(d: Path) -> str:
    h = hashlib.sha256()
    for p in sorted(x for x in d.rglob("*") if x.is_file()):
        h.update(str(p.relative_to(d)).encode() + b"\0" + p.read_bytes() + b"\0")
    return h.hexdigest()[:20]


def norm_json(b: bytes) -> bytes:
    """JSON の書き方の違い（空白）をならす。項目の並びと辞書の順は、そのまま比べる"""
    try:
        return json.dumps(json.loads(b.decode("utf-8")), ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    except Exception:
        return b


def one(cmd, workdir: Path, name: str, full: bool, only=None) -> dict:
    """1 つの .geas（workdir/name）に、各コマンドを走らせた結果の指紋"""
    out = {}
    r = run(cmd, ["astdump", name], workdir)
    out["ast"] = [r.returncode, sha(norm_json(r.stdout)), sha(r.stderr)]
    if only == ["ast"]:
        return out
    r = run(cmd, ["check", name, "--json"], workdir)
    out["check"] = [r.returncode, sha(r.stdout), sha(r.stderr)]
    if only == ["ast", "check"] or only == ["check"]:
        return out
    if r.returncode == 0:
        r = run(cmd, ["spec", name], workdir)
        out["spec"] = [r.returncode, sha(r.stdout), sha(r.stderr)]
        html = workdir / (name + ".review.html")
        r = run(cmd, ["review", name, "-o", html.name], workdir)
        out["review"] = [r.returncode, sha(html.read_bytes()) if html.exists() else "", sha(r.stderr)]
        if html.exists():
            html.unlink()
        if full:
            dest = workdir / (name + ".rust")
            r = run(cmd, ["rust", name, "-o", dest.name, "--host", "--runtime", "--standalone"], workdir)
            out["rust"] = [r.returncode, tree_hash(dest) if dest.exists() else "", sha(r.stderr)]
            shutil.rmtree(dest, ignore_errors=True)
    return out


def inputs(tmp: Path):
    """(キー, 作業フォルダ, ファイル名, 実物か) の一覧。コーパスは tmp に展開する"""
    corpus = tmp / "corpus"
    with tarfile.open(CORPUS) as t:
        t.extractall(corpus, filter="data") if sys.version_info >= (3, 12) else t.extractall(corpus)
    items = [(f"corpus/{p.stem}", corpus, p.name, False) for p in sorted(corpus.glob("*.geas"))]
    # 実物は、隣のファイル（use・計画書）を見るので、リポジトリのフォルダのまま走らせる
    for p in sorted(FIXTURES.rglob("*.geas")):
        items.append((f"fixtures/{p.relative_to(FIXTURES)}", p.parent, p.name, True))
    return items


def collect(cmd, jobs, only=None):
    with tempfile.TemporaryDirectory() as t:
        items = inputs(Path(t))
        with ThreadPoolExecutor(jobs) as ex:
            res = list(ex.map(lambda it: (it[0], one(cmd, it[1], it[2], it[3], only)), items))
    return dict(res)


def first_diff(a, b, path="$"):
    """2 つの JSON の最初の違いの場所と中身"""
    if type(a) != type(b):
        return path, a, b
    if isinstance(a, dict):
        for k in a:
            if k not in b:
                return f"{path}.{k}", a[k], "（無い）"
        for k in b:
            if k not in a:
                return f"{path}.{k}", "（無い）", b[k]
        if list(a) != list(b):
            return path + "（キーの並び）", list(a), list(b)
        for k in a:
            d = first_diff(a[k], b[k], f"{path}.{k}")
            if d:
                return d
        return None
    if isinstance(a, list):
        for i, (x, y) in enumerate(zip(a, b)):
            d = first_diff(x, y, f"{path}[{i}]")
            if d:
                return d
        if len(a) != len(b):
            return path + "（件数）", len(a), len(b)
        return None
    return None if a == b else (path, a, b)


def astdiff(cmd_b, jobs, limit):
    """構文木のダンプを、Python 版（正解）と別の実装で比べて、違いの場所を出す"""
    cmd_a = shlex.split(f"{sys.executable} -m geas")
    cmd_b = shlex.split(cmd_b)
    with tempfile.TemporaryDirectory() as t:
        items = inputs(Path(t))

        def work(it):
            key, wd, name, _ = it
            ra = run(cmd_a, ["astdump", name], wd)
            rb = run(cmd_b, ["astdump", name], wd)
            if ra.returncode != rb.returncode:
                return key, ("終了コード", ra.returncode, rb.returncode)
            try:
                return key, first_diff(json.loads(ra.stdout), json.loads(rb.stdout))
            except Exception as e:
                return key, ("読めない", str(e)[:100], rb.stdout[:200].decode("utf-8", "replace"))

        with ThreadPoolExecutor(jobs) as ex:
            res = list(ex.map(work, items))
    bad = [(k, d) for k, d in res if d]
    print(f"{len(res)} 件中 {len(bad)} 件に食い違い")
    for k, d in bad[:limit]:
        print(f"  {k}: {d[0]}\n      python: {str(d[1])[:160]}\n      rust  : {str(d[2])[:160]}")
    return 1 if bad else 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("mode", choices=["make", "verify", "corpus-record", "astdiff"])
    ap.add_argument("--cmd", default=f"{sys.executable} -m geas")
    ap.add_argument("--only", help="比べる項目を絞る（ast、check）")
    ap.add_argument("--limit", type=int, default=10)
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    a = ap.parse_args()
    if a.mode == "corpus-record":
        with tempfile.TemporaryDirectory() as t:
            env = dict(os.environ, GEAS_RECORD_CORPUS=t)
            subprocess.run([sys.executable, "-m", "pytest", "tests", "-q", "-p", "no:cacheprovider", "--deselect", "tests/test_proofs.py"], cwd=ROOT, env=env)
            names = sorted(p.name for p in Path(t).iterdir())
            with tarfile.open(CORPUS, "w:xz") as tar:
                for n in names:
                    ti = tar.gettarinfo(Path(t) / n, n)
                    ti.mtime, ti.uid, ti.gid, ti.uname, ti.gname = 0, 0, 0, "", ""
                    with open(Path(t) / n, "rb") as f:
                        tar.addfile(ti, f)
        print(f"{len(names)} 件を {CORPUS} に書きました")
        return 0
    if a.mode == "astdiff":
        os.environ["PYTHONPATH"] = str(ROOT) + os.pathsep + os.environ.get("PYTHONPATH", "")
        os.environ["PYTHONUTF8"] = "1"
        return astdiff(a.cmd, a.jobs, a.limit)
    # 正解（Python 版）は、いつもリポジトリの直下から動かす
    env_cwd = ROOT
    os.environ["PYTHONPATH"] = str(ROOT) + os.pathsep + os.environ.get("PYTHONPATH", "")
    os.environ["PYTHONUTF8"] = "1"
    only = a.only.split(",") if a.only else None
    got = collect(shlex.split(a.cmd), a.jobs, only)
    if a.mode == "make":
        HASHES.parent.mkdir(parents=True, exist_ok=True)
        HASHES.write_text(json.dumps(got, ensure_ascii=False, indent=0, sort_keys=True) + "\n", encoding="utf-8")
        print(f"{len(got)} 件の指紋を {HASHES} に書きました")
        return 0
    want = json.loads(HASHES.read_text(encoding="utf-8"))
    bad = [(k, c) for k, v in want.items() for c, x in v.items() if (only is None or c in only) and got.get(k, {}).get(c) != x]
    missing = [k for k in want if k not in got]
    print(f"{len(want)} 件中 {len({k for k, _ in bad})} 件に食い違い（{len(bad)} か所）")
    for k, c in bad[:30]:
        print(f"  {k}  {c}")
    return 1 if bad or missing else 0


if __name__ == "__main__":
    sys.exit(main())
