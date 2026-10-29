"use strict";
// Geas の screen 宣言（/api/defs の screens）から画面を作る汎用の実行部。アプリの名前も項目も知らない。
// 規則：show＝問い合わせの行を design の layout で並べる／open＝行を選ぶと開く／field x from q＝q の行から選ぶ（写像は行ごとに数）／
//       slot＝利用者に選ばせる値（同じ名前の列を持つ行だけを出す）／action＝処理を呼び、結果の種類ごとの行き先へ進む。
const $ = (s, r = document) => r.querySelector(s);
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const store = { get: (k) => { try { return localStorage.getItem(k); } catch { return null; } }, set: (k, v) => { try { localStorage.setItem(k, v); } catch {} } };

let D, S, Q;                       // 定義全体、画面、問い合わせ
const rowsOf = {};                 // 問い合わせ名 → 直近の行
let cur = null;                    // いまの画面 {name, params}
const slotVal = {};                // slot 名 → 選んだ値
let qty = {};                      // 写像の入力欄 → {行のID: 数}

let ROUTE = {};                    // 処理・問い合わせ・出来事の名前 → resource の URL（defs の routes）
async function api(method, path, body, keep) {
  const r = await fetch(path.startsWith("/") ? path : "/api/" + path, { method, headers: { "Content-Type": "application/json", Accept: "application/json" }, body: body ? JSON.stringify(body) : undefined });
  const j = await r.json().catch(() => ({ error: "応答が読めません" }));
  if (!r.ok && !(keep && j.result)) throw new Error(j.error || r.statusText);  // 処理の結果の種類は、HTTP の状態が 4xx でも結果として読む
  return j;
}
const urlOf = (name, args) => ROUTE[name].path.replace(/:[a-z_]+/, encodeURIComponent(Object.values(args || {})[0] ?? ""));  // 条件はパスで渡す（クエリ文字列は使わない）
const query = async (name, args) => {
  const j = await api("GET", urlOf(name, args));
  return (rowsOf[name] = j.rows || (j.ids || []).map((id) => ({ id })));
};

function notice(msg, kind) {
  const n = $("#notice");
  n.textContent = msg || ""; n.hidden = !msg; n.className = "banner " + (kind || "info");
  if (msg) clearTimeout(notice.t), notice.t = setTimeout(() => (n.hidden = true), 6000);
}

// ---- 値の見せ方 ----
function fmt(col, row) {
  const v = row[col.name];
  if (v === null || v === undefined) return "";
  if (col.kind === "money") {
    const cur = col.currency || row[col.unit], m = D.screens.minor[cur] ?? 0;
    try { return new Intl.NumberFormat("ja-JP", { style: "currency", currency: cur, minimumFractionDigits: m, maximumFractionDigits: m }).format(v / 10 ** m); } catch { return String(v); }
  }
  if (col.kind === "enum") return col.values.find((x) => x.value === v)?.label ?? v;
  return v;
}
const mainCol = (qd) => qd.columns.find((c) => c.kind === "text") || qd.columns[0];
const idCol = (qd) => qd.columns.find((c) => c.kind === "id") || qd.columns[0];
// 結果の中身（IDなど）を、読み込んである問い合わせの行の見出しに置き換えて見せる
function nameOf(id) {
  for (const [q, rows] of Object.entries(rowsOf)) {
    const qd = Q[q], r = rows.find((x) => x[idCol(qd).name] === id);
    if (r) return String(r[mainCol(qd).name]);
  }
  return String(id);
}
const bodyText = (v) => (Array.isArray(v) ? v.map(nameOf).join("、") : typeof v === "object" && v ? JSON.stringify(v) : nameOf(v));

// ---- 行の並び ----
function rowHtml(qd, row, opts) {
  const id = row[idCol(qd).name], main = mainCol(qd);
  const cols = qd.columns.filter((c) => c !== main && !(c.kind === "id" && qd.columns.length > 1));
  const parts = cols.map((c) => {
    if (c.kind === "money") return `<div class="money" data-col="${esc(c.name)}">${esc(fmt(c, row))}</div>`;
    if (c.kind === "enum" && c.name !== (opts.slot && opts.slot.name)) return `<span class="tag" data-col="${esc(c.name)}" data-value="${esc(row[c.name])}">${esc(fmt(c, row))}</span>`;
    if (c.kind === "enum") return "";
    return `<div class="col" data-col="${esc(c.name)}">${esc(c.label)} <b>${esc(fmt(c, row))}</b></div>`;
  }).join("");
  const opens = (opts.opens || []).map((t) => `<button type="button" data-open="${esc(t)}" data-id="${esc(id)}">${esc(S[t].label)}</button>`).join("");
  const step = opts.qtyField
    ? `<div class="qty"><button type="button" data-dec="${esc(id)}" aria-label="減らす">−</button><span data-qty="${esc(id)}">${qty[opts.qtyField][id] || 0}</span><button type="button" data-inc="${esc(id)}" aria-label="増やす">＋</button></div>` : "";
  return `<div class="row" data-row="${esc(id)}"><div class="main">${esc(fmt(main, row))}</div>${parts}${opens}${step}</div>`;
}
function visible(rows, slots) {  // slot と同じ名前の列を持つ行は、選んだ値のものだけ
  return rows.filter((r) => slots.every((s) => !(s.name in r) || r[s.name] === slotVal[s.name]));
}

// ---- 問い合わせの入力 ----
function fieldValue(sc, f) {
  const el = $(`[data-field="${f.name}"]`);
  if (f.type.kind === "map") return Object.fromEntries(Object.entries(qty[f.name] || {}).filter(([, n]) => n > 0));
  if (f.type.kind === "group") return Object.fromEntries(f.type.parts.map((p) => [p.name, $(`[data-field="${f.name}.${p.name}"]`).value]));
  return el ? el.value : (cur.params[f.name] ?? "");
}
function showArgs(sc, qname) {
  const args = {};
  for (const n of Q[qname].inputs) {
    const f = sc.fields.find((x) => x.name === n.name);
    const v = f ? fieldValue(sc, f) : cur.params[n.name];
    if (v === undefined || v === "") return null;
    args[n.name] = v;
  }
  return args;
}
async function loadShow(sc, qname) {
  const box = $(`[data-show="${qname}"] .rows`);
  if (!box) return;
  const args = showArgs(sc, qname);
  if (!args) { box.innerHTML = `<p class="hint">入力すると表示します。</p>`; return; }
  const rows = await query(qname, args), qd = Q[qname];
  box.innerHTML = rows.length ? rows.map((r) => rowHtml(qd, r, { opens: sc.opens, slot: sc.slots[0] })).join("") : `<p class="hint">ありません。</p>`;
}

// ---- 画面 ----
let gen = 0;  // 続けて開かれたとき、古い描画を途中でやめる
async function render() {
  const my = ++gen, sc = S[cur.name];
  document.body.dataset.mode = sc.design.mode;
  document.body.dataset.layout = sc.design.layout;
  document.documentElement.style.setProperty("--acc", sc.design.accent);
  $('meta[name="theme-color"]').content = sc.design.accent;
  $("#screen-title").textContent = sc.label;
  document.querySelectorAll("#tabs button").forEach((b) => b.classList.toggle("on", b.dataset.screen === cur.name));
  const main = $("#screen");
  main.dataset.screen = cur.name;
  main.innerHTML = "";
  for (const s of sc.slots) {
    const known = s.values.some((v) => v.value === slotVal[s.name]);
    if (!known) slotVal[s.name] = store.get("slot:" + s.name) || s.values[0].value;
  }
  // 画面に渡された値で、選ぶ行と slot の初期値を決める
  for (const f of sc.fields) if (f.from && f.type.kind === "map" && !(f.name in qty)) {
    const q = (qty[f.name] = {});
    const rows = rowsOf[f.from] || await query(f.from, {}), qd = Q[f.from];
    if (my !== gen) return;
    for (const v of Object.values(cur.params)) {
      const r = rows.find((x) => x[idCol(qd).name] === v);
      if (r) { q[v] = 1; for (const s of sc.slots) if (s.name in r) slotVal[s.name] = r[s.name]; }
    }
  }
  main.innerHTML = [
    ...sc.slots.map((s) => `<div class="pills" role="group" aria-label="${esc(s.label)}" data-slot="${esc(s.name)}">${s.values.map((v) => `<button type="button" data-slotval="${esc(v.value)}" class="${v.value === slotVal[s.name] ? "on" : ""}">${esc(v.label)}</button>`).join("")}</div>`),
    ...sc.fields.map((f) => fieldHtml(sc, f)),
    ...sc.shows.map((q) => `<section data-show="${esc(q)}"><h2>${esc(Q[q].label)}</h2><div class="rows ${esc(sc.design.layout)}"></div></section>`),
    sc.actions.length ? `<div class="actions">${sc.actions.map((a) => `<button type="button" class="primary" data-action="${esc(a.deed)}">${esc(a.label)}</button>`).join("")}</div>` : "",
  ].join("");
  for (const f of sc.fields) { await fillField(sc, f); if (my !== gen) return; }
  await Promise.all(sc.shows.map((q) => loadShow(sc, q).catch((e) => notice("読み込めません: " + e.message, "warn"))));
}
function fieldHtml(sc, f) {
  const t = f.type;
  if (t.kind === "map") return `<h2>${esc(f.label)}</h2><div class="rows ${esc(sc.design.layout === "form" ? "stack" : sc.design.layout)}" data-field="${esc(f.name)}"></div>`;
  if (t.kind === "group") return `<fieldset class="group"><legend>${esc(f.label)}</legend>${t.parts.map((p) => inputHtml(`${f.name}.${p.name}`, p.label, p)).join("")}</fieldset>`;
  if (f.from) return `<label class="f">${esc(f.label)}<select data-field="${esc(f.name)}"></select></label><div class="picked" data-picked="${esc(f.name)}"></div>`;
  return inputHtml(f.name, f.label, t);
}
function inputHtml(name, label, t) {
  const attrs = t.kind === "int" ? `type="number" ${t.lo != null ? `min="${Number(t.lo)}"` : ""} ${t.hi != null ? `max="${Number(t.hi)}"` : ""}` : `type="text" ${t.max ? `maxlength="${Number(t.max)}"` : ""}`;
  return `<label class="f">${esc(label)}<input data-field="${esc(name)}" ${attrs} autocomplete="off"></label>`;
}
async function fillField(sc, f) {
  if (!f.from) return;
  const qd = Q[f.from], rows = await query(f.from, {});
  if (f.type.kind === "map") {
    const list = visible(rows, sc.slots), box = $(`[data-field="${f.name}"]`);
    if (!box) return;
    box.innerHTML = list.map((r) => rowHtml(qd, r, { qtyField: f.name, slot: sc.slots[0] })).join("") || `<p class="hint">ありません。</p>`;
    return;
  }
  const sel = $(`[data-field="${f.name}"]`), idc = idCol(qd).name;
  if (!sel) return;
  sel.innerHTML = rows.map((r) => `<option value="${esc(r[idc])}">${esc(r[mainCol(qd).name])}</option>`).join("");
  const saved = store.get("pick:" + f.from);
  sel.value = rows.some((r) => String(r[idc]) === saved) ? saved : (rows[0] ? rows[0][idc] : "");
  const show = () => {
    const r = rows.find((x) => String(x[idc]) === sel.value);
    $(`[data-picked="${f.name}"]`).textContent = r ? qd.columns.filter((c) => c.name !== idc && c !== mainCol(qd)).map((c) => `${c.label} ${fmt(c, r)}`).join("　") : "";
  };
  show();
  sel.addEventListener("change", () => { store.set("pick:" + f.from, sel.value); show(); sc.shows.forEach((q) => loadShow(sc, q).catch(() => {})); });
}

// ---- 処理を呼ぶ ----
async function act(sc, a) {
  if (!navigator.onLine) return notice("オフラインのため、操作できません。", "warn");
  const input = {};
  for (const i of a.inputs) {
    const f = sc.fields.find((x) => x.name === i.name);
    if (!f) { input[i.name] = cur.params[i.name]; continue; }
    const v = fieldValue(sc, f);
    if (i.kind === "group" ? Object.values(v).some((x) => x === "") : v === "") return notice(`「${f.label}」を入れてください。`, "warn");
    if (i.kind === "map") {  // slot と同じ名前の列で絞られて見えている行だけを送る
      const rows = visible(rowsOf[f.from] || [], sc.slots), idc = idCol(Q[f.from]).name, ok = new Set(rows.map((r) => r[idc]));
      input[i.name] = Object.fromEntries(Object.entries(v).filter(([k]) => ok.has(k)));
      if (!Object.keys(input[i.name]).length) return notice(`「${f.label}」を1つ以上選んでください。`, "warn");
    } else input[i.name] = i.kind === "int" ? Number(v) : v;  // 数の欄は数で送る
  }
  const slots = Object.fromEntries(sc.slots.map((s) => [s.name, slotVal[s.name]]));
  const btn = $(`[data-action="${a.deed}"]`); btn.disabled = true;
  try {
    const rt0 = ROUTE[a.deed], idv = rt0.id_input ? input[rt0.id_input] : undefined;  // update／destroy は /…/:id の ID を URL に載せる
    if (rt0.id_input) delete input[rt0.id_input];
    const r = await api(rt0.method, rt0.path.replace(/:[A-Za-z0-9_]+/, encodeURIComponent(idv ?? "")), { ...input, slots }, true);
    const rt = a.routes.find((x) => x.result === r.result);
    const b = r.body || {};
    const detail = (rt?.body || []).filter((p) => b[p.name] !== undefined && !(Array.isArray(b[p.name]) && !b[p.name].length)).map((p) => `${p.label} ${bodyText(b[p.name])}`).join("　");
    const to = rt ? rt.to : "stay";
    notice((rt ? rt.label : r.result) + (detail ? "：" + detail : ""), to === "stay" ? "warn" : "info");
    if (to === "back") history.back();
    else if (to !== "stay") { qty = {}; go(to, {}); }
    else await Promise.all([...sc.shows.map((q) => loadShow(sc, q)), ...sc.fields.filter((f) => f.from && f.type.kind !== "map").map((f) => fillField(sc, f))]);  // 選ぶ元が変わる（取り下げなど）ので引き直す
  } catch (e) { notice("できませんでした: " + e.message, "warn"); }
  finally { btn.disabled = false; }
}

// ---- 移動 ----
// 画面の URL は、リソースの URL（defs の screens[].url。例 /orders/new）。渡す値は、パスの :名前 か URL のフラグメント（#名前=値）に載せる（クエリ文字列は使わない）。
// リソースに付かない画面だけ、/#/画面名 を使う。
const pathNames = (url) => [...url.matchAll(/:([A-Za-z0-9_]+)/g)].map((m) => m[1]);
const patOf = (url) => new RegExp("^" + url.split("/").map((p) => (p.startsWith(":") ? "([^/]+)" : p.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))).join("/") + "/?$");
function parseLocation() {
  const frag = location.hash.replace(/^#/, "");
  if (frag.startsWith("/")) {
    const [n, qs] = frag.slice(1).split("?");
    if (S[n]) return { name: n, params: Object.fromEntries(new URLSearchParams(qs || "")) };
  }
  for (const [name, sc] of Object.entries(S)) {
    const m = sc.url && patOf(sc.url).exec(location.pathname);
    if (!m) continue;
    const params = Object.fromEntries(new URLSearchParams(frag));
    pathNames(sc.url).forEach((n, i) => { const v = decodeURIComponent(m[i + 1]); if (v !== ":" + n) params[n] = v; });  // 値が入っていないパス（:名前 のまま）は、渡された値なし
    return { name, params };
  }
  return { name: D.screens.first, params: {} };
}
function urlFor(name, params) {
  const sc = S[name], rest = { ...params };
  if (!sc.url) { const qs = new URLSearchParams(rest).toString(); return `/#/${name}` + (qs ? "?" + qs : ""); }
  const path = sc.url.replace(/:([A-Za-z0-9_]+)/g, (m, n) => { if (!(n in rest)) return m; const v = rest[n]; delete rest[n]; return encodeURIComponent(v); });
  const frag = new URLSearchParams(rest).toString();
  return path + (frag ? "#" + frag : "");
}
function go(name, params) { history.pushState(null, "", urlFor(name, params)); return route(); }
async function route() {
  const next = parseLocation();
  if (!cur || cur.name !== next.name) qty = {};
  cur = next;
  await render().catch((e) => notice("画面を作れません: " + e.message, "warn"));
}

document.addEventListener("click", (e) => {
  const t = e.target.closest("button"); if (!t) return;
  const sc = cur && S[cur.name];
  if (t.dataset.screen) go(t.dataset.screen, {});
  else if (t.dataset.open) {
    const target = S[t.dataset.open], p = target.params[0];
    qty = {}; go(t.dataset.open, p ? { [p.name]: t.dataset.id } : {});
  } else if (t.dataset.slotval !== undefined) {
    const n = t.closest("[data-slot]").dataset.slot;
    slotVal[n] = t.dataset.slotval; store.set("slot:" + n, slotVal[n]); render();
  } else if (t.dataset.inc || t.dataset.dec) {
    const f = sc.fields.find((x) => x.type.kind === "map"), q = qty[f.name], id = t.dataset.inc || t.dataset.dec, hi = f.type.value.hi ?? 99;
    q[id] = Math.max(0, Math.min(hi, (q[id] || 0) + (t.dataset.inc ? 1 : -1)));
    $(`[data-qty="${id}"]`).textContent = q[id];
  } else if (t.dataset.action) act(sc, sc.actions.find((a) => a.deed === t.dataset.action));
});

// ---- 出来事・つながり ----
async function fire(name) {
  try { await api("POST", ROUTE[name].path, {}); } catch (e) { notice("確かめ直せません: " + e.message, "warn"); }
  if (cur) await Promise.all(S[cur.name].shows.map((q) => loadShow(S[cur.name], q).catch(() => {})));
}
const eventsOn = (when) => Object.entries(D.screens.events_on).filter(([, on]) => on === when).map(([n]) => n);
window.addEventListener("online", () => { $("#offline").hidden = true; eventsOn("online").forEach(fire); });
window.addEventListener("offline", () => ($("#offline").hidden = false));
document.addEventListener("visibilitychange", () => { if (!document.hidden) eventsOn("opened").forEach(fire); });
window.addEventListener("popstate", route);

// 見せている問い合わせは、状態が変わる（返事が届く）ので引き直す。入力中の欄には触らない
setInterval(() => {
  if (!cur || document.hidden || !navigator.onLine) return;
  const sc = S[cur.name];
  sc.shows.forEach((q) => loadShow(sc, q).catch(() => {}));
}, 2000);

(async function start() {
  // 定義は覚えておき、オフラインでも画面を開けるようにする
  try { D = await api("GET", "defs"); store.set("defs", JSON.stringify(D)); }
  catch (e) { const c = store.get("defs"); if (!c) throw e; D = JSON.parse(c); }
  S = D.screens.screens; Q = D.screens.queries;
  ROUTE = Object.fromEntries(D.routes.map((r) => [r.target, r]));
  document.title = D.screens.title;
  $("#app-title").textContent = D.screens.title;
  $("#tabs").innerHTML = D.screens.order.map((n) => [n, S[n]]).filter(([, s]) => !s.params.length && s.entry === (s === S[D.screens.first])).map(([n, s]) => `<button data-screen="${n}">${esc(s.label)}</button>`).join("");
  if ("serviceWorker" in navigator) navigator.serviceWorker.register("/sw.js").catch(() => {});
  if (!navigator.onLine) $("#offline").hidden = false;
  eventsOn("opened").forEach(fire);
  await route();
})().catch((e) => notice("定義を読めません: " + e.message, "warn"));
