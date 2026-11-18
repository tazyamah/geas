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


def one(cmd, workdir: Path, name: str, full: bool) -> dict:
    """1 つの .geas（workdir/name）に、各コマンドを走らせた結果の指紋"""
    out = {}
    r = run(cmd, ["check", name, "--json"], workdir)
    out["check"] = [r.returncode, sha(r.stdout), sha(r.stderr)]
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


def collect(cmd, jobs):
    with tempfile.TemporaryDirectory() as t:
        items = inputs(Path(t))
        with ThreadPoolExecutor(jobs) as ex:
            res = list(ex.map(lambda it: (it[0], one(cmd, it[1], it[2], it[3])), items))
    return dict(res)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("mode", choices=["make", "verify", "corpus-record"])
    ap.add_argument("--cmd", default=f"{sys.executable} -m geas")
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
    # 正解（Python 版）は、いつもリポジトリの直下から動かす
    env_cwd = ROOT
    os.environ["PYTHONPATH"] = str(ROOT) + os.pathsep + os.environ.get("PYTHONPATH", "")
    os.environ["PYTHONUTF8"] = "1"
    got = collect(shlex.split(a.cmd), a.jobs)
    if a.mode == "make":
        HASHES.parent.mkdir(parents=True, exist_ok=True)
        HASHES.write_text(json.dumps(got, ensure_ascii=False, indent=0, sort_keys=True) + "\n", encoding="utf-8")
        print(f"{len(got)} 件の指紋を {HASHES} に書きました")
        return 0
    want = json.loads(HASHES.read_text(encoding="utf-8"))
    bad = [(k, c) for k, v in want.items() for c, x in v.items() if got.get(k, {}).get(c) != x]
    missing = [k for k in want if k not in got]
    print(f"{len(want)} 件中 {len({k for k, _ in bad})} 件に食い違い（{len(bad)} か所）")
    for k, c in bad[:30]:
        print(f"  {k}  {c}")
    return 1 if bad or missing else 0


if __name__ == "__main__":
    sys.exit(main())
