# SPDX-License-Identifier: GPL-3.0-or-later
"""①の具体例を、コンパイルしたコードで実際に動かして確かめる。
具体例の表から Rust の実行用 bin を生成し、cargo で動かして、結果・変化・外部作用を①の期待と比べる。"""
import shutil
import subprocess
from pathlib import Path

from .ast import Type
from .check import Checker, phys
from .plan import EXAMPLE, UNSPEC, Finding, n, split_top
from .rustgen import cargo_toml, rust_type


class ExampleError(Exception):
    pass


def lit(t: Type, text: str, ck: Checker) -> str:
    text = text.strip()
    k = t.kind
    if k in ("ID", "参照", "文字列"):
        return f'"{text}".to_string()'
    if k == "バイト列":
        h = text[2:] if text.startswith("0x") else None
        if h is None or len(h) % 2 or any(c not in "0123456789abcdefABCDEF" for c in h):
            raise ExampleError(f"バイト列は 0x に続く16進数で書きます（「{text}」）")
        return "vec![" + ", ".join(f"0x{h[i:i + 2]}u8" for i in range(0, len(h), 2)) + "]"
    if k == "日時":
        if not text.lstrip("-").isdigit():
            raise ExampleError(f"日時は秒の数で書きます（「{text}」）")
        return f"{text}i64"
    if k == "集合":
        if not (text.startswith("{") and text.endswith("}")):
            raise ExampleError(f"集合は {{要素, …}} で書きます（「{text}」）")
        items = [x.strip() for x in split_top(text[1:-1], ",")]
        return "BTreeSet::from([" + ", ".join(f'"{x}".to_string()' for x in items) + "])"
    if k in ("整数", "金額"):
        if not text.lstrip("-").isdigit():
            raise ExampleError(f"数のはずが「{text}」です")
        return f"{text}{phys(t)}"
    if k == "割合":
        a, _, b = text.partition("/")
        if not (a.strip().isdigit() and b.strip().isdigit()):
            raise ExampleError(f"割合は 分子/分母 で書きます（「{text}」）")
        return f"有理 {{ 分子: {a.strip()}, 分母: {b.strip()} }}"
    if k == "名前":
        if text not in [x for s in ck.p.states if s.name == t.target for x in [s.initial, *s.middle, *s.final]]:
            raise ExampleError(f"状態 {t.target} に「{text}」はありません")
        return f"{t.target}::{text}"
    if k == "写像":
        if text in ("{}", ""):
            return "BTreeMap::new()"
        if not (text.startswith("{") and text.endswith("}")):
            raise ExampleError(f"写像は {{キー:値, …}} で書きます（「{text}」）")
        pairs = []
        for item in split_top(text[1:-1], ","):
            kk, _, vv = item.partition(":")
            pairs.append(f'("{kk.strip()}".to_string(), {lit(t.value, vv, ck)})')
        return f"BTreeMap::from([{', '.join(pairs)}])"
    raise ExampleError(f"型 {k} の値は書けません")


def kv(text: str) -> dict[str, str]:
    out = {}
    for item in split_top(text, ","):
        k, eq, v = item.partition("=")
        if not eq:
            raise ExampleError(f"「名前=値」の形のはずが「{item}」です")
        out[k.strip()] = v.strip()
    return out


def example_code(ck: Checker, row: dict) -> str:
    q = ck.queries.get(row["処理"].strip())
    proc = q or ck.procs.get(row["処理"].strip())
    if proc is None:
        raise ExampleError(f"処理「{row['処理']}」がコードにありません")
    info = ck.infos[proc.name] if q is None else None
    L = ["    {", "        let mut 保存 = 保存::default();"]
    ctx = {}
    for item in split_top(row.get("前提", ""), ";"):
        if "(" in item.split("=")[0]:  # データ(ID, 項目=値, …)
            dname, _, rest = item.partition("(")
            dname = dname.strip()
            if dname not in ck.data:
                raise ExampleError(f"前提のデータ「{dname}」がコードにありません")
            args = split_top(rest.rstrip().rstrip(")"), ",")
            key = ck.key_field(dname)
            vals = {key.name: args[0]}
            vals.update(kv(",".join(args[1:])) if len(args) > 1 else {})
            fields = []
            for f in ck.data[dname].fields:
                if f.type.optional and vals.get(f.name, "null").strip() == "null":  # 書かないか null なら null
                    fields.append(f"{f.name}: None")
                    continue
                if f.name not in vals:
                    raise ExampleError(f"前提 {dname}({args[0]}) に項目「{f.name}」がありません")
                v = lit(f.type, vals[f.name], ck)
                fields.append(f"{f.name}: Some({v})" if f.type.optional else f"{f.name}: {v}")
            extra = set(vals) - {f.name for f in ck.data[dname].fields}
            if extra:
                raise ExampleError(f"データ {dname} に項目「{sorted(extra)[0]}」がありません")
            L.append(f'        保存.{dname}.insert("{args[0]}".to_string(), {dname} {{ {", ".join(fields)} }});')
        elif item:
            ctx.update(kv(item))
    fields = []
    for name, t in proc.refs:
        if name not in ctx:
            raise ExampleError(f"前提に 参照「{name}」がありません")
        fields.append(f"{name}: {lit(t, ctx[name], ck)}")
    for d in sorted(set(info.new_ids)) if info else []:
        if f"新{d}ID" not in ctx:
            raise ExampleError(f"前提に 採番「新{d}ID」がありません")
        fields.append(f'新{d}ID: "{ctx[f"新{d}ID"]}".to_string()')
    if info and info.parts:
        raise ExampleError(f"処理 {proc.name} は部品（{', '.join(sorted(info.parts))}）を呼ぶので、具体例では動かせません（部品の実装はアプリが持つため）")
    L.append(f"        let ctx = {proc.name}文脈 {{ {', '.join(fields)} }};")
    given = kv(row.get("入力", "")) if row.get("入力", "").strip() else {}
    ins = []
    for name, t in proc.inputs:
        if name not in given:
            if getattr(t, "optional", False):
                ins.append(f"{name}: None")
                continue
            raise ExampleError(f"入力に「{name}」がありません")
        v = lit(t, given[name], ck)
        ins.append(f"{name}: Some({v})" if getattr(t, "optional", False) else f"{name}: {v}")
    L.append(f"        let 入力 = {proc.name}入力 {{ {', '.join(ins)} }};")
    eid = row["ID"].strip()
    if q is not None:
        L += [
            "        match ctx.検査().and_then(|_| 入力.検査()) {",  # slot も入力も、境界で範囲を確かめる
            f'            Err(e) => println!("{eid}\\t入力不正({{}})\\t\\t", e),',
            f'            Ok(()) => println!("{eid}\\t一覧({{}})\\t\\t", {q.name}(&ctx, &保存, &入力){".iter().map(|x| x.表示()).collect::<Vec<_>>()" if q.select else ""}.join(",")),',
            "        }",
            "    }",
        ]
        return "\n".join(L)
    L += [
        "        match ctx.検査().and_then(|_| 入力.検査()) {",  # slot も入力も、境界で範囲を確かめる
        f'            Err(e) => println!("{eid}\\t入力不正({{}})\\t\\t", e),',
        "            Ok(()) => {",
        f"                let 出力 = {proc.name}(&ctx, &保存, &入力);",
        "                let 前 = 保存.clone();",
        "                保存.適用(出力.書込.clone());",
        f'                println!("{eid}\\t{{}}\\t{{}}\\t{{}}", 出力.結果.表示(), 前.差分(&保存).join(";"), '
        '出力.外部.iter().map(|e| e.表示()).collect::<Vec<_>>().join(";"));',
        "            }",
        "        }",
        "    }",
    ]
    return "\n".join(L)


def runner_source(ck: Checker, rows: list[dict]) -> tuple[str, list[Finding]]:
    problems, blocks = [], []
    for row in rows:
        eid = row["ID"].strip()
        if "未規定" in row["期待する結果"]:
            problems.append(Finding(UNSPEC, f"具体例 {eid}", "計画で未規定。コードがどう動くかは検証しない。問いとして残る"))
            continue
        try:
            blocks.append(example_code(ck, row))
        except ExampleError as e:
            problems.append(Finding(EXAMPLE, f"具体例 {eid}", f"実行できない形です: {e}"))
    src = "use geas_app::*;\nuse std::collections::{BTreeMap, BTreeSet};\n\n#[allow(unused_mut, non_snake_case, unused_variables)]\nfn main() {\n"
    return src + "\n".join(blocks) + "\n}\n", problems


HOST_MOD = "\npub mod host;\n"


def write_crate(ck: Checker, lib_src: str, runner_src: str, dest: Path, host: bool = False):
    (dest / "src" / "bin").mkdir(parents=True, exist_ok=True)
    (dest / "Cargo.toml").write_text(cargo_toml("geas_app", host))
    (dest / "src" / "lib.rs").write_text(lib_src + (HOST_MOD if host else ""))
    if host:
        from .hostgen import host_source
        (dest / "src" / "host.rs").write_text(host_source(ck))
    (dest / "src" / "bin" / "plan_examples.rs").write_text(runner_src)


def run_crate(dest: Path) -> dict[str, tuple[str, str, str]]:
    if shutil.which("cargo") is None:
        raise ExampleError("cargo が無いので具体例を実行できません")
    r = subprocess.run(["cargo", "run", "--quiet", "--offline", "--bin", "plan_examples"], cwd=dest,
                       capture_output=True, text=True)
    if r.returncode != 0:
        raise ExampleError("生成した Rust のビルドか実行に失敗しました:\n" + r.stderr[-2000:])
    out = {}
    for line in r.stdout.splitlines():
        parts = line.split("\t")
        if len(parts) == 4:
            out[parts[0]] = (parts[1], parts[2], parts[3])
    return out


def items(s: str) -> list[str]:
    return sorted(x for x in split_top(n(s), ";") if x and x != "なし")


def compare_examples(rows: list[dict], actual: dict) -> list[Finding]:
    out = []
    for row in rows:
        eid = row["ID"].strip()
        if "未規定" in row["期待する結果"]:
            continue
        if eid not in actual:
            continue
        res, chg, eff = actual[eid]
        for label, want, got in (
            ("結果", [n(row["期待する結果"])], [n(res)]),
            ("変化", items(row.get("期待する変化", "")), items(chg)),
            ("外部作用", items(row.get("期待する外部作用", "")), items(eff)),
        ):
            if want != got:
                out.append(Finding(EXAMPLE, f"具体例 {eid}（{row['処理']}）の{label}",
                                   f"計画: {'; '.join(want) or 'なし'} ／ 実行した結果: {'; '.join(got) or 'なし'}"))
    return out
