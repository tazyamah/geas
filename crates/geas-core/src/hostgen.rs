// SPDX-License-Identifier: GPL-3.0-or-later
//! ブラウザ（WASM）・サーバーから【処理】を呼ぶための橋渡し host.rs の生成（Python 版 hostgen.py の移し）。
//! JSON の文字列で（名前, 文脈, 保存, 入力）を受け取り、処理を実行して書込を適用し、（結果, 外部作用, 新しい保存）を JSON で返す。
use crate::ast::*;
use crate::check::defs::Step;
use crate::check::{phys, Checker, ProcInfo};
use crate::mcp::tool_defs;
use crate::rest::{first_by_name, internal_targets, path_inputs, post_queries, q_inputs, q_types, route_actors, routes as rest_routes, versioned, ANYONE, PATH_INT};
use crate::rustgen::{column_type, deed_inputs_proc, deed_inputs_query, opt, rust_str, value_checks};
use crate::screengen::{screens_def, jint};
use indexmap::IndexMap;
use serde_json::{json, Map as JMap, Value};

const JSON_PRELUDE: &str = include_str!("assets/host_json_prelude.txt");
const CALL_WASM: &str = include_str!("assets/host_call_wasm.txt");

fn no_opt(t: &Type) -> Type {
    Type { optional: false, ..t.clone() }
}

/// 処理と問い合わせの入力。無いかもしれない入力は、キーが無いのも null も同じ「無い」
pub fn input_j(x: &str, t: &Type) -> String {
    if t.optional {
        return format!("match Input.get_opt(\"{}\") {{ Some(v) => Some({}), None => None }}", x, from_j(&no_opt(t), "v"));
    }
    from_j(t, &format!("Input.get(\"{}\")?", x))
}

/// 保存の行を確かめるときの型。型に結んだ変換（read）は、外から受け取るときに 1 回だけ通すものなので外す
pub fn stored(t: &Type) -> Type {
    let mut t = t.clone();
    if t.kind == "文字列" && t.read.is_some() {
        t.read = None;
    }
    if t.kind == "合成" {
        t.parts = t.parts.map(|ps| ps.iter().map(|(pn, pt)| (pn.clone(), stored(pt))).collect());
    }
    t
}

/// JSON 値 j から Rust の値へ（エラーは ? で伝える）
pub fn from_j(t: &Type, j: &str) -> String {
    if t.optional {
        return format!("match {} {{ J::Null => None, v => Some({}) }}", j, from_j(&no_opt(t), "v"));
    }
    let k = t.kind.as_str();
    match k {
        "ID" | "参照" | "文字列" | "生" => format!("{}.as_str()?.to_string()", j),
        "整数" | "金額" => format!("{}::try_from({}.as_int()?).map_err(|_| \"範囲外の数です\".to_string())?", phys(t), j),
        "日時" => format!("({}.as_int()? as i64)", j),
        "真偽" => format!("{}.as_bool()?", j),
        "割合" => {
            let r = if t.trust.is_some() { None } else { t.range.as_ref() };
            format!("read_ratio({}.as_str()?, {}, {})?", j, opt(r.and_then(|r| r.lo)), opt(r.and_then(|r| r.hi)))
        }
        "バイト列" => format!("b64_decode({}.as_str()?)?", j),
        "集合" => format!("{}.as_set()?", j),
        "名前" => format!("{}::from_name({}.as_str()?).ok_or_else(|| \"未知の状態です\".to_string())?", t.target.clone().unwrap_or_default(), j),
        "合成" => {
            let comp = |pn: &str, pt: &Type| -> String {
                if pt.optional {
                    format!("match {}.get_opt(\"{}\") {{ Some(v) => Some({}), None => None }}", j, pn, from_j(&no_opt(pt), "v"))
                } else {
                    from_j(pt, &format!("{}.get(\"{}\")?", j, pn))
                }
            };
            let parts: Vec<String> = t.parts.as_deref().unwrap_or(&[]).iter().map(|(pn, pt)| format!("{}: {}", pn, comp(pn, pt))).collect();
            format!("{} {{ {} }}", t.target.clone().unwrap_or_default(), parts.join(", "))
        }
        "写像" => format!(
            "{}.as_obj()?.iter().map(|(k, v)| Ok((k.clone(), {}))).collect::<Result<BTreeMap<String, _>, String>>()?",
            j,
            from_j(t.value.as_deref().unwrap(), "v")
        ),
        _ => panic!("JSON への対応がない型: {}", k),
    }
}

/// Rust の値 x から JSON 文字列へ（式。String を返す）。None は null
pub fn to_j(t: &Type, x: &str) -> String {
    if t.optional {
        return format!("match &{} {{ None => \"null\".to_string(), Some(v) => {} }}", x, to_j(&no_opt(t), "v"));
    }
    let k = t.kind.as_str();
    match k {
        "ID" | "参照" | "文字列" | "生" => format!("js_str(&{})", x),
        "整数" | "金額" | "日時" => format!("{}.to_string()", x),
        "真偽" => format!("format!(\"{{}}\", {})", x),
        "バイト列" => format!("js_str(&b64_encode(&{}))", x),
        "割合" => format!("js_str(&{}.to_string())", x),
        "集合" => format!("js_set(&{})", x),
        "名前" => format!("js_str({}.name())", x),
        "合成" => {
            let items = t.parts.as_deref().unwrap_or(&[]).iter().map(|(pn, pt)| format!("format!(\"{{}}:{{}}\", js_str(\"{}\"), {})", pn, to_j(pt, &format!("{}.{}", x, pn)))).collect::<Vec<_>>().join(", ");
            format!("format!(\"{{{{{{}}}}}}\", vec![{}].join(\",\"))", items)
        }
        "写像" => format!(
            "format!(\"{{{{{{}}}}}}\", {}.iter().map(|(k, v)| format!(\"{{}}:{{}}\", js_str(k), {})).collect::<Vec<_>>().join(\",\"))",
            x,
            to_j(t.value.as_deref().unwrap(), "v")
        ),
        _ => panic!("JSON への対応がない型: {}", k),
    }
}

/// Rust の値 x を JSON にして、文字列 o の後ろに書く文。to_j と同じ JSON になる
pub fn put_j(t: &Type, x: &str) -> String {
    if t.optional {
        return format!("match &{} {{ None => o.push_str(\"null\"), Some(v) => {{ {} }} }}", x, put_j(&no_opt(t), "v"));
    }
    match t.kind.as_str() {
        "ID" | "参照" | "文字列" | "生" => format!("js_str_into(o, &{});", x),
        "整数" | "金額" | "日時" | "真偽" => format!("let _ = write!(o, \"{{}}\", {});", x),
        "名前" => format!("js_str_into(o, {}.name());", x),
        _ => format!("o.push_str(&{});", to_j(t, x)),
    }
}

/// 処理が読み書きしうる entity の名前。サーバーは、保存からこれだけを読み込んで処理に渡す
pub fn touched_data(info: &ProcInfo) -> Vec<String> {
    let mut names: Vec<String> = vec![];
    let mut add = |n: String| {
        if !names.contains(&n) {
            names.push(n);
        }
    };
    for r in info.reads() {
        add(r.split('.').next().unwrap_or("").to_string());
    }
    for w in &info.writes {
        add(w.split('.').next().unwrap_or("").to_string());
    }
    for d in &info.deletes {
        add(d.clone());
    }
    for d in &info.new_ids {
        add(d.clone());
    }
    for d in info.lookup_vars.values() {
        add(d.clone());
    }
    for t in &info.transitions {
        add(t.0.clone());
    }
    names.sort();
    names
}

/// 開発用の模擬：外部作用ごとに返る応答と、応答の結果の種類（outcome）
pub fn mock_def(ck: &Checker) -> Value {
    let p = ck.p;
    let show = |s: &str| p.show_names(s);
    let kind_of = |k: &str| -> &'static str {
        match k {
            "ID" | "参照" => "id",
            "文字列" | "生" => "text",
            "整数" => "int",
            "真偽" => "bool",
            "集合" => "set",
            _ => "text",
        }
    };
    let mut effects = JMap::new();
    for e in &p.effects {
        if let Some(r) = &e.replies {
            if p.replies.iter().any(|x| &x.name == r) {
                effects.insert(e.name.clone(), json!({"label": show(&e.name), "reply": r}));
            }
        }
    }
    let mut replies = JMap::new();
    for r in &p.replies {
        if p.effects.iter().any(|e| e.replies.as_deref() == Some(r.name.as_str())) {
            let outcomes: Vec<(String, Vec<String>)> = if r.outcomes.is_empty() { vec![("Normal".to_string(), r.params.iter().map(|(n, _)| n.clone()).collect())] } else { r.outcomes.clone() };
            replies.insert(
                r.name.clone(),
                json!({
                    "label": show(&r.name),
                    "fields": r.params.iter().map(|(n, t)| json!({"name": n, "kind": kind_of(&t.kind)})).collect::<Vec<_>>(),
                    "outcomes": outcomes.iter().map(|(on, g)| json!({"name": on, "label": show(on), "gives": g})).collect::<Vec<_>>(),
                }),
            );
        }
    }
    json!({
        "effects": effects,
        "replies": replies,
        "events": p.events.iter().filter(|ev| !ev.on.is_empty()).map(|ev| json!({"name": ev.name, "label": show(&ev.name)})).collect::<Vec<_>>(),
    })
}

/// データ用フォルダの定義：OS ごとのパス。relative は OS なしの行
pub fn data_folder_def(ck: &Checker) -> Value {
    if ck.p.data_folders.is_empty() {
        return Value::Null;
    }
    let mut m = JMap::new();
    for (os_name, path, _) in &ck.p.data_folders {
        m.insert(if os_name.is_empty() { "relative".to_string() } else { os_name.clone() }, json!(path));
    }
    Value::Object(m)
}

/// 常駐の定義
pub fn resident_def(ck: &Checker, screens: &Value) -> Value {
    let r = match &ck.p.resident {
        None => return Value::Null,
        Some(r) => r,
    };
    let mut menu: Vec<Value> = vec![];
    for (label, kind, target, _) in &r.menu {
        if kind == "separator" {
            menu.push(json!({"kind": "separator"}));
        } else if kind == "screen" {
            menu.push(json!({"kind": "screen", "label": label, "screen": target, "url": screens["screens"][target]["url"]}));
        } else {
            menu.push(json!({"kind": "event", "label": label, "event": target}));
        }
    }
    json!({"tooltip": if r.tooltip.is_empty() { screens["title"].clone() } else { json!(r.tooltip) }, "start": r.start, "click": r.click, "menu": menu})
}

pub fn file_def(e: &EffectDecl) -> Value {
    let f = e.file.as_ref().unwrap();
    let mut rows = JMap::new();
    for (k, (o, t)) in &f.rows {
        let mut take = JMap::new();
        for (n, src) in t {
            take.insert(n.clone(), json!(src));
        }
        rows.insert(k.clone(), json!({"outcome": o, "take": take}));
    }
    json!({"op": f.op, "dir": f.dir, "rest": f.rest, "from": f.src, "rows": rows})
}

fn http_row(o: &str, tk: &[(String, String)]) -> JMap<String, Value> {
    let mut take = JMap::new();
    for (n, path) in tk {
        take.insert(n.clone(), json!(path));
    }
    let mut m = JMap::new();
    m.insert("outcome".into(), json!(o));
    m.insert("take".into(), Value::Object(take));
    m
}

pub fn http_def(e: &EffectDecl) -> Value {
    let h = e.http.as_ref().unwrap();
    let statuses: Vec<Value> = h
        .statuses
        .iter()
        .map(|(lo, hi, o, tk)| {
            let mut m = JMap::new();
            m.insert("lo".into(), jint(*lo));
            m.insert("hi".into(), jint(*hi));
            for (k, v) in http_row(o, tk) {
                m.insert(k, v);
            }
            Value::Object(m)
        })
        .collect();
    let other = match &h.other {
        Some((o, tk)) => Value::Object(http_row(o, tk)),
        None => Value::Null,
    };
    json!({"method": h.method, "url": h.url, "bearer": h.bearer, "body": h.body.iter().map(|(k, n)| json!([k, n])).collect::<Vec<_>>(), "statuses": statuses, "other": other})
}

/// migration の手順を、サーバーが起動時に当てる形にする
pub fn migration_defs(ck: &Checker) -> Value {
    let steps = ck.migration_steps();
    let norm = ck.migration_norm();
    let norm_steps: Vec<Step> = norm.iter().map(|(s, d)| Step { dn: d.clone().unwrap_or_default(), ..s.clone() }).collect();
    let mut out: IndexMap<i128, Vec<Value>> = IndexMap::new();
    for (i, s) in steps.iter().enumerate() {
        let mut st = JMap::new();
        st.insert("op".into(), json!(s.op));
        st.insert("entity".into(), json!(s.dn));
        st.insert("field".into(), json!(s.fname));
        if s.op == "rename" || s.op == "rename_entity" {
            if let Some(MigVal::To(t)) = &s.arg {
                st.insert("to".into(), json!(t));
            }
        } else if s.op == "add" {
            let fin = Checker::migration_final(&norm_steps, i);
            let nd = norm[i].1.as_deref();
            let f = match (&fin, nd) {
                (Some(fin), Some(nd)) if ck.data.contains_key(nd) => ck.data[nd].fields.iter().find(|x| &x.name == fin),
                _ => None,
            };
            let (kind, val): (&str, Value) = match &s.arg {
                Some(MigVal::Int(n)) => ("int", jint(*n)),
                Some(MigVal::Str(v)) => ("str", json!(v)),
                Some(MigVal::Bool(b)) => ("bool", json!(b)),
                Some(MigVal::Null) | None => ("null", Value::Null),
                Some(MigVal::Name(v)) => ("name", json!(v)),
                Some(MigVal::Ratio(v)) => ("ratio", json!(v)),
                Some(MigVal::To(v)) => ("to", json!(v)),
            };
            let value = if kind == "int" && f.map_or(false, |f| f.r#type.kind == "割合") { json!(format!("{}/1", s.arg.as_ref().map(|a| if let MigVal::Int(n) = a { n.to_string() } else { String::new() }).unwrap_or_default())) } else { val };
            st.insert("value".into(), value);
        }
        out.entry(s.ver).or_default().push(Value::Object(st));
    }
    Value::Array(ck.p.migrations.iter().map(|m| json!({"version": jint(m.version), "steps": out.get(&m.version).cloned().unwrap_or_default()})).collect())
}

pub fn definitions(ck: &Checker) -> Result<Value, String> {
    let p = ck.p;
    let mut flows: JMap<String, Value> = JMap::new();
    for fl in &p.flows {
        for (proc, routes) in &fl.entries {
            let e = flows.entry(proc.clone()).or_insert_with(|| Value::Object(JMap::new()));
            if let Value::Object(m) = e {
                for (v, ds) in route_map(routes) {
                    m.insert(v, if ds.len() == 1 { json!(ds[0]) } else { json!(ds) });
                }
            }
        }
    }
    let who = route_actors(p);
    let by_res = first_by_name(p);
    let posts = post_queries(p);
    let rows_of = versioned(p);
    let inner = internal_targets(p);
    let mut deeds = JMap::new();
    for x in &p.processes {
        let info = &ck.infos[&x.name];
        let mut ids: Vec<&String> = info.new_ids.iter().collect::<indexmap::IndexSet<_>>().into_iter().collect();
        ids.sort();
        let mut m = JMap::new();
        m.insert("inputs".into(), json!(x.inputs.iter().map(|(n, _)| n).collect::<Vec<_>>()));
        m.insert("refs".into(), json!(x.refs.iter().map(|(n, _)| n).collect::<Vec<_>>()));
        m.insert("new_ids".into(), json!(ids));
        m.insert("touches".into(), json!(touched_data(info)));
        if info.caller {
            m.insert("caller".into(), json!(true));
        }
        deeds.insert(x.name.clone(), Value::Object(m));
    }
    let mut queries = JMap::new();
    for q in &p.queries {
        let mut m = JMap::new();
        m.insert("inputs".into(), json!(q.inputs.iter().map(|(n, _)| n).collect::<Vec<_>>()));
        m.insert("refs".into(), json!(q.refs.iter().map(|(n, _)| n).collect::<Vec<_>>()));
        if uses_caller(p, q) {
            m.insert("caller".into(), json!(true));
        }
        if paged(q) {
            m.insert("paged".into(), json!(true));
        }
        queries.insert(q.name.clone(), Value::Object(m));
    }
    let mut events = JMap::new();
    for ev in &p.events {
        events.insert(ev.name.clone(), json!(ev.steps.iter().map(|(k, q, d)| json!({"kind": k, "query": q, "deed": d})).collect::<Vec<_>>()));
    }
    let mut effects = JMap::new();
    for e in &p.effects {
        let mut m = JMap::new();
        m.insert("args".into(), json!(e.params.iter().map(|(n, _)| n).collect::<Vec<_>>()));
        m.insert("joins".into(), json!(e.joins));
        m.insert("no_wait".into(), json!(e.no_wait.is_some()));
        if e.http.is_some() {
            m.insert("http".into(), http_def(e));
        }
        if e.file.is_some() {
            m.insert("file".into(), file_def(e));
        }
        effects.insert(e.name.clone(), Value::Object(m));
    }
    let mut replies = JMap::new();
    for r in &p.replies {
        replies.insert(r.name.clone(), json!(r.params.iter().map(|(n, t)| json!({"name": n, "required": !t.optional})).collect::<Vec<_>>()));
    }
    let mut rules = JMap::new();
    for f in &p.rules {
        let rows: Vec<Value> = f
            .rows
            .iter()
            .map(|(conds, val)| {
                json!({"when": conds.iter().map(|(a, b)| json!([a, b])).collect::<Vec<_>>(), "value": match val { IntOrStr::Int(n) => jint(*n), IntOrStr::Str(s) => json!(s) }})
            })
            .collect();
        rules.insert(f.slot.clone(), Value::Array(rows));
    }
    let screens = screens_def(ck);
    let resident = resident_def(ck, &screens);
    let mut routes: Vec<Value> = vec![];
    for r in rest_routes(p) {
        let mut m = JMap::new();
        m.insert("method".into(), json!(r.method));
        m.insert("path".into(), json!(r.path));
        m.insert("kind".into(), json!(r.kind));
        m.insert("target".into(), json!(r.target));
        m.insert("resource".into(), json!(r.resource));
        let mut st = JMap::new();
        for (k, v) in &r.statuses {
            st.insert(k.clone(), jint(*v));
        }
        m.insert("statuses".into(), Value::Object(st));
        let (kind, tgt, rn) = (r.kind.as_str(), r.target.as_str(), r.resource.as_str());
        if (kind == "update" || kind == "destroy") && ck.procs.contains_key(tgt) {
            let id_in = ck.procs[tgt].inputs.iter().find(|(_, t)| t.kind == "ID").map(|(n, _)| n.clone()).unwrap_or_default();
            m.insert("id_input".into(), json!(id_in));
        }
        if (kind == "update" || kind == "destroy" || kind == "show") && rows_of.contains_key(rn) {
            m.insert("row_entity".into(), json!(rows_of[rn]));
        }
        if inner.contains(tgt) {
            m.insert("internal".into(), json!(true));
        }
        if kind == "index" && posts.contains_key(tgt) {
            m.insert("post".into(), json!(true));
        }
        if (kind == "index" || kind == "show") && ck.queries.contains_key(tgt) {
            let q = ck.queries[tgt];
            let res = by_res[rn];
            let act = res.actions.iter().find(|a| a.target == tgt).unwrap();
            let ty = q_types(q);
            for (part, picked) in [("path_inputs", path_inputs(p, res, act, q)), ("q_inputs", q_inputs(p, res, act, q))] {
                let items: Vec<Value> = picked
                    .iter()
                    .map(|n| {
                        let mut e = JMap::new();
                        e.insert("name".into(), json!(n));
                        if PATH_INT.contains(&ty[n].kind.as_str()) {
                            e.insert("int".into(), json!(true));
                        }
                        if ty[n].optional {
                            e.insert("optional".into(), json!(true));
                        }
                        Value::Object(e)
                    })
                    .collect();
                m.insert(part.into(), Value::Array(items));
            }
        }
        if let Some(w) = who.get(tgt) {
            if w != &vec![ANYONE.to_string()] {
                m.insert("for".into(), json!(w));
            }
        }
        routes.push(Value::Object(m));
    }
    let mut out = JMap::new();
    out.insert("deeds".into(), Value::Object(deeds));
    out.insert("queries".into(), Value::Object(queries));
    out.insert("flows".into(), Value::Object(flows));
    out.insert("events".into(), Value::Object(events));
    out.insert("effects".into(), Value::Object(effects));
    out.insert("replies".into(), Value::Object(replies));
    out.insert("rules".into(), Value::Object(rules));
    out.insert("mock".into(), mock_def(ck));
    out.insert("screens".into(), screens);
    out.insert("resident".into(), resident);
    out.insert("data_folder".into(), data_folder_def(ck));
    out.insert("routes".into(), Value::Array(routes));
    out.insert("mcp".into(), Value::Array(tool_defs(ck)?));
    out.insert("schema_version".into(), jint(ck.schema_version()));
    out.insert("migrations".into(), migration_defs(ck));
    out.insert(
        "triggers".into(),
        Value::Array(
            p.triggers
                .iter()
                .map(|t| {
                    json!({"name": t.name, "args": t.params.iter().map(|(n, _)| n).collect::<Vec<_>>(),
                        "causes": t.causes.iter().map(|(k, key, d)| json!({
                            "kind": k, "key": key,
                            "secs": if k == "deadline" { key.parse::<i64>().map(|x| json!(x)).unwrap_or(Value::Null) } else { Value::Null },
                            "replies": if k == "all" { json!(key.split(',').collect::<Vec<_>>()) } else { Value::Null },
                            "dest": d})).collect::<Vec<_>>()})
                })
                .collect(),
        ),
    );
    Ok(Value::Object(out))
}

/// サーバーの実行部が使う口。保存をメモリに持ったまま処理を走らせ、変わった行だけを DB と行き来する
pub fn server_api(p: &Program, ck: &Checker) -> String {
    let load = p
        .data
        .iter()
        .map(|d| {
            let k = &ck.key_field(&d.name).unwrap().name;
            format!(
                "        \"{n}\" => {{ let r = {n}_from_json(&j)?; same_key(\"{n}\", key, &r.{k})?; store.{n}.insert(key.to_string(), r); Ok(()) }}",
                n = d.name,
                k = k
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let get = p.data.iter().map(|d| format!("        \"{n}\" => store.{n}.get(key).map({n}_to_json),", n = d.name)).collect::<Vec<_>>().join("\n");
    let parts: Vec<String> = vec![
        "/// サーバーの口：メモリ上の保存に直接、処理か問い合わせを走らせる（request は name・ctx・input。保存は渡さない）。\n".into(),
        "/// 応答は call から \"store\" を除いた形。変わった行は changed に足す（途中で失敗しても、それまでに変えた行は入っている）\n".into(),
        "pub fn call_store(request: &str, store: &mut Store, changed: &mut Changed) -> Result<String, String> {\n".into(),
        "    let r = parse(request)?;\n".into(),
        "    call_inner(r.get(\"name\")?.as_str()?, r.get(\"ctx\")?, r.get(\"input\")?, store, changed)\n}\n".into(),
        "/// DB の 1 行（実体の名前・キー・行の JSON）を、メモリ上の保存へ入れる\n".into(),
        "pub fn load_row(store: &mut Store, entity: &str, key: &str, row: &str) -> Result<(), String> {\n".into(),
        "    let j = parse(row)?;\n    match entity {\n".into(),
        format!("{}\n", load),
        "        _ => Err(format!(\"実体「{}」はありません\", entity)),\n    }\n}\n".into(),
        "/// メモリ上の保存の 1 行を、DB に書く JSON にする。行が無ければ None（消えた行）\n".into(),
        "pub fn row_json(store: &Store, entity: &str, key: &str) -> Option<String> {\n".into(),
        "    match entity {\n".into(),
        format!("{}\n", get),
        "        _ => None,\n    }\n}\n".into(),
        "/// メモリ上の保存の 1 行の版。行が無ければ None\n".into(),
        "pub fn row_version(store: &Store, entity: &str, key: &str) -> Option<String> {\n".into(),
        "    row_json(store, entity, key).map(|s| row_etag(&s))\n}".into(),
    ];
    parts.concat()
}

pub fn host_source(ck: &Checker) -> Result<String, String> {
    let p = ck.p;
    let etag_data: indexmap::IndexSet<String> = versioned(p).values().cloned().collect();
    let mut out: Vec<String> = vec![JSON_PRELUDE.to_string()];
    for d in &p.data {
        let key = ck.key_field(&d.name).unwrap().name.clone();
        let dn = &d.name;
        out.push(format!("fn {}_from_json(j: &J) -> Result<{}, String> {{\n    let r = {} {{", dn, dn, dn));
        for f in &d.fields {
            out.push(format!("        {}: {},", f.name, from_j(&f.r#type, &format!("j.get(\"{}\")?", f.name))));
        }
        out.push(format!("    }};\n    {}_row_ok(&r)?;\n    Ok(r)\n}}", dn));
        out.push(format!("fn {}_row_ok(r: &{}) -> Result<(), String> {{", dn, dn));
        for f in &d.fields {
            let t = stored(&f.r#type);
            let xs = if t.optional { "x".to_string() } else { format!("r.{}", f.name) };
            for c in value_checks(&format!("{}.{}", dn, f.name), &no_opt(&t), &xs, t.optional) {
                if t.optional {
                    out.push(format!("    if let Some(x) = &r.{} {{ {} }}", f.name, c));
                } else {
                    out.push(format!("    {}", c));
                }
            }
        }
        for rel in &ck.rel_invs {
            if &rel.data != dn {
                continue;
            }
            for (fd, c) in &rel.cons {
                let mut items: Vec<(&String, &i128)> = fd.iter().collect();
                items.sort_by(|a, b| a.0.cmp(b.0));
                let terms = if items.is_empty() { "0i128".to_string() } else { items.iter().map(|(n, k)| format!("({}i128) * (r.{} as i128)", k, n)).collect::<Vec<_>>().join(" + ") };
                let mut cond = format!("({}) + ({}i128) <= 0", terms, c);
                if let Some((wf, wv)) = &rel.when {
                    let tgt = d.fields.iter().find(|x| &x.name == wf).and_then(|x| x.r#type.target.clone()).unwrap_or_default();
                    cond = format!("r.{} != {}::{} || {}", wf, tgt, wv, cond);
                }
                out.push(format!("    if !({}) {{ return Err(format!(\"{{}} の行が、約束「{{}}」を守っていません\", \"{}\", {})); }}", cond, dn, rust_str(&rel.text)));
            }
        }
        out.push("    Ok(())\n}".into());
        out.push(format!("fn {}_to_json(r: &{}) -> String {{\n    let mut v: Vec<String> = Vec::new();", dn, dn));
        for f in &d.fields {
            out.push(format!("    v.push(format!(\"{{}}:{{}}\", js_str(\"{}\"), {}));", f.name, to_j(&f.r#type, &format!("r.{}", f.name))));
        }
        out.push("    format!(\"{{{}}}\", v.join(\",\"))\n}".into());
    }
    out.push("/// 行を置くキーと、行の中のキーの項目は同じ（違う行を通すと、D[k] で引いた行の ID が k でなくなり、書き戻しで別の行ができる）\nfn same_key(entity: &str, key: &str, id: &str) -> Result<(), String> {\n    if key == id { Ok(()) } else { Err(format!(\"{} のキー「{}」と、行の中の ID「{}」が違います\", entity, key, id)) }\n}".into());
    out.push("pub fn read_store(j: &J) -> Result<Store, String> {\n    let mut s = Store::default();".into());
    for d in &p.data {
        out.push(format!(
            "    if let Some(m) = j.get_opt(\"{n}\") {{ for (k, v) in m.as_obj()? {{ let r = {n}_from_json(v)?; same_key(\"{n}\", k, &r.{k2})?; s.{n}.insert(k.clone(), r); }} }}",
            n = d.name,
            k2 = ck.key_field(&d.name).unwrap().name
        ));
    }
    out.push("    Ok(s)\n}".into());
    out.push("pub fn write_store(s: &Store) -> String {\n    let mut v: Vec<String> = Vec::new();".into());
    for d in &p.data {
        out.push(format!(
            "    v.push(format!(\"{{}}:{{{{{{}}}}}}\", js_str(\"{n}\"), s.{n}.iter().map(|(k, r)| format!(\"{{}}:{{}}\", js_str(k), {n}_to_json(r))).collect::<Vec<_>>().join(\",\")));",
            n = d.name
        ));
    }
    out.push("    format!(\"{{{}}}\", v.join(\",\"))\n}".into());
    // 外部作用
    out.push(format!("fn write_effects(e: &Effect) -> String {{\n    match {} {{", if p.effects.is_empty() { "*e" } else { "e" }));
    for e in &p.effects {
        let names: Vec<&str> = e.params.iter().map(|(n, _)| n.as_str()).collect();
        let mut pat_items = names.clone();
        pat_items.push("idem_key");
        let pat = pat_items.join(", ");
        let args = names.iter().map(|_| "{}:{}").collect::<Vec<_>>().join(",");
        let vals = e.params.iter().map(|(n, t)| format!("js_str(\"{}\"), {}", n, to_j(t, n))).collect::<Vec<_>>().join(", ");
        out.push(format!(
            "        Effect::{} {{ {} }} => format!(\"{{{{\\\"name\\\":{{}},\\\"args\\\":{{{{{}}}}},\\\"idem_key\\\":{{}}}}}}\", js_str(\"{}\"), {}, js_str(idem_key)),",
            e.name.replace('.', "_"),
            pat,
            args,
            e.name,
            vals
        ));
    }
    out.push("    }\n}".into());
    for r in &p.results {
        out.push(format!("fn {}_body(r: &{}) -> String {{\n    match r {{", r.name, r.name));
        for (v, ps) in &r.variants {
            if !ps.is_empty() {
                let names: Vec<String> = (0..ps.len()).map(|i| format!("a{}", i)).collect();
                let fmt = ps.iter().map(|_| "{}:{}").collect::<Vec<_>>().join(",");
                let vals = ps.iter().zip(names.iter()).map(|((n, t), a)| format!("js_str(\"{}\"), {}", n, to_j(t, a))).collect::<Vec<_>>().join(", ");
                out.push(format!("        {}::{}({}) => format!(\"{{{{{}}}}}\", {}),", r.name, v, names.join(", "), fmt, vals));
            } else {
                out.push(format!("        {}::{} => \"{{}}\".to_string(),", r.name, v));
            }
        }
        out.push("    }\n}".into());
    }
    for q in &p.queries {
        if !q.select.is_empty() {
            let n = &q.name;
            let cols: Vec<(&Column, Type)> = q.select.iter().map(|c| (c, column_type(ck, q, c))).collect();
            let body_parts: Vec<String> = cols
                .iter()
                .enumerate()
                .map(|(k, (c, t))| {
                    let lit = format!("{}{}:", if k == 0 { "{" } else { "," }, serde_json::to_string(&c.name).unwrap());
                    format!("o.push_str({}); {}", rust_str(&lit), put_j(t, &format!("r.{}", c.name)))
                })
                .collect();
            let mut body = body_parts.join(" ");
            if etag_data.contains(&q.target) && !cols.is_empty() {
                body += &format!(
                    " if let Some(row) = Store.{t}.get(&r.row_key_) {{ o.push_str(\",\\\"_etag\\\":\\\"\"); o.push_str(&row_etag(&{t}_to_json(row))); o.push('\"'); }}",
                    t = q.target
                );
            }
            body = if !cols.is_empty() { body + " o.push('}');" } else { "o.push_str(\"{}\");".to_string() };
            out.push(format!("fn {}_row_json(o: &mut String, r: &{}Row, Store: &Store) {{\n    let _ = Store;\n    {}\n}}", n, n, body));
        }
    }
    out.push("fn write_result(name: &str, args: Vec<String>, body: String, effects: Vec<String>) -> String {\n    format!(\"{{\\\"result\\\":{},\\\"args\\\":{},\\\"body\\\":{},\\\"effects\\\":[{}]}}\", js_str(name), js_arr(&args), body, effects.join(\",\"))\n}".into());
    out.push("fn call_inner(name: &str, ctx: &J, Input: &J, Store: &mut Store, changed: &mut Changed) -> Result<String, String> {\n    match name {".into());
    for proc in &p.processes {
        let info = &ck.infos[&proc.name];
        let n = &proc.name;
        let mut ids: Vec<&String> = info.new_ids.iter().collect::<indexmap::IndexSet<_>>().into_iter().collect();
        ids.sort();
        let mut cf: Vec<String> = proc.refs.iter().map(|(x, t)| format!("{}: {}", x, from_j(t, &format!("ctx.get(\"{}\")?", x)))).collect();
        cf.extend(ids.iter().map(|d| format!("new_{}_id: ctx.get(\"new_{}_id\")?.as_str()?.to_string()", d, d)));
        let inf: Vec<String> = deed_inputs_proc(ck, proc).iter().map(|(x, t)| format!("{}: {}", x, input_j(x, t))).collect();
        out.push(format!("        \"{n}\" => {{\n            let c = {n}Ctx {{ {} }};\n            let i = {n}Input {{ {} }};\n            c.validate()?;\n            i.validate()?;", cf.join(", "), inf.join(", "), n = n));
        out.push(format!("            let o = {n}(&c, Store, &i).map_err(|f| f.to_string())?;\n            Store.apply_tracked(o.Write, changed).map_err(|f| f.to_string())?;", n = n));
        out.push(format!("            Ok(write_result(o.result.name(), o.result.args(), {}_body(&o.result), o.effects.iter().map(write_effects).collect()))\n        }}", proc.result));
    }
    for q in &p.queries {
        let n = &q.name;
        let cf: Vec<String> = q.refs.iter().map(|(x, t)| format!("{}: {}", x, from_j(t, &format!("ctx.get(\"{}\")?", x)))).collect();
        let inf: Vec<String> = gen_inputs(p, q).iter().map(|(x, t)| format!("{}: {}", x, input_j(x, t))).collect();
        out.push(format!("        \"{n}\" => {{\n            let c = {n}Ctx {{ {} }};\n            let i = {n}Input {{ {} }};\n            c.validate()?;\n            i.validate()?;", cf.join(", "), inf.join(", "), n = n));
        let choices = order_choices(q);
        if !choices.is_empty() {
            let names: Vec<&String> = choices.iter().map(|(name, _, _)| name).collect();
            out.push(format!(
                "            if let Some(o) = &i.{ORDER} {{ if ![{}].contains(&o.as_str()) {{ return Err(\"並び（q[{ORDER}]）は、{} のどれかです\".to_string()); }} }}",
                names.iter().map(|x| rust_str(x)).collect::<Vec<_>>().join(", "),
                names.iter().map(|x| x.as_str()).collect::<Vec<_>>().join("・"),
                ORDER = ORDER
            ));
        }
        let pg = paged(q);
        if pg {
            out.push(format!("            if let Some(k) = &i.{AFTER} {{ if !Store.{}.contains_key(k) {{ return Err(\"続きの位置（q[{AFTER}]）の行がありません。最初のページから読み直してください\".to_string()); }} }}", q.target, AFTER = AFTER));
        }
        let call = if pg { format!("{}_paged(&c, Store, &i)", n) } else { format!("({}(&c, Store, &i), None::<String>)", n) };
        let tail = "if let Some(k) = &next { o.push_str(\",\\\"after\\\":\"); o.push_str(&js_str(k)); }";
        if !q.select.is_empty() {
            out.push(format!(
                "            let (rows, next) = {};\n            let mut o = String::with_capacity(rows.len().saturating_mul(64).saturating_add(16));\n            o.push_str(\"{{\\\"rows\\\":[\");\n            for (k, r) in rows.iter().enumerate() {{ if k > 0 {{ o.push(','); }} {}_row_json(&mut o, r, Store); }}\n            o.push_str(\"]\");\n            {}\n            o.push('}}');\n            Ok(write_result(\"一覧\", Vec::new(), o, vec![]))\n        }}",
                call, n, tail
            ));
        } else {
            out.push(format!(
                "            let (ids, next) = {};\n            let mut o = String::from(\"{{\");\n            if let Some(k) = &next {{ o.push_str(\"\\\"after\\\":\"); o.push_str(&js_str(k)); }}\n            o.push('}}');\n            Ok(write_result(\"一覧\", ids, o, vec![]))\n        }}",
                call
            ));
        }
    }
    out.push("        _ => Err(format!(\"処理「{}」はありません\", name)),\n    }\n}".into());
    out.push(
        "/// 保存ごと JSON で受け渡す口（画面の WASM・試験）。書込を適用した後の保存を \"store\" に入れて返す\npub fn call(request: &str) -> Result<String, String> {\n    let r = parse(request)?;\n    let mut s = read_store(r.get(\"store\")?)?;\n    let out = call_inner(r.get(\"name\")?.as_str()?, r.get(\"ctx\")?, r.get(\"input\")?, &mut s, &mut Vec::new())?;\n    let head = out.strip_suffix('}').ok_or_else(|| \"応答の形が違います\".to_string())?;\n    Ok(format!(\"{},\\\"store\\\":{}}}\", head, write_store(&s)))\n}"
            .into(),
    );
    out.push(server_api(p, ck));
    let defs = serde_json::to_string(&definitions(ck)?).map_err(|e| e.to_string())?;
    out.push(format!("pub const DEFS_JSON: &str = {};", rust_str(&defs)));
    out.push(
        "\nfn ret(s: String) -> u64 {\n    let b = s.into_bytes().into_boxed_slice();\n    let l = b.len();\n    let p = Box::into_raw(b) as *mut u8;\n    ((p as u64) << 32) | (l as u64)\n}\n#[no_mangle]\npub extern \"C\" fn alloc(n: usize) -> *mut u8 { let mut v = Vec::<u8>::with_capacity(n); let p = v.as_mut_ptr(); std::mem::forget(v); p }\n#[no_mangle]\npub extern \"C\" fn dealloc(p: *mut u8, n: usize) { unsafe { drop(Vec::from_raw_parts(p, 0, n)); } }\n"
            .to_string()
            + CALL_WASM
            + "#[no_mangle]\npub extern \"C\" fn geas_defs() -> u64 { ret(DEFS_JSON.to_string()) }\n",
    );
    let _ = deed_inputs_query;
    Ok(out.join("\n") + "\n")
}
