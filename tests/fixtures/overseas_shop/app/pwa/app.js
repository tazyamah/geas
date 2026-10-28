"use strict";
// 海外にも売るお店の画面。サーバーの JSON の口だけを使う（/api/queries, /api/deeds, /api/events）。
const $ = (s) => document.querySelector(s);
const CURRENCIES = ["JPY", "USD", "EUR"];
const STATUS = { Received: "受付", PaymentRequested: "決済の返事待ち", Completed: "完了", Canceled: "キャンセル" };
const MINOR = { JPY: 0, USD: 2, EUR: 2 }; // 金額は最小単位（円・セント）で届く

const st = {
  customer: localStorage.getItem("customer") || "",
  currency: localStorage.getItem("currency") || "JPY",
  customers: [], products: [], orders: [],
  cart: JSON.parse(localStorage.getItem("cart") || "{}"), // {通貨: {商品ID: 数量}}
  tab: "products",
};

const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const money = (n, cur) => new Intl.NumberFormat("ja-JP", { style: "currency", currency: cur, minimumFractionDigits: MINOR[cur], maximumFractionDigits: MINOR[cur] }).format(n / 10 ** MINOR[cur]);

async function api(method, path, body) {
  const r = await fetch("/api/" + path, { method, headers: { "Content-Type": "application/json" }, body: body ? JSON.stringify(body) : undefined });
  const j = await r.json().catch(() => ({ error: "応答が読めません" }));
  if (!r.ok) throw new Error(j.error || r.statusText);
  return j;
}
const query = (name, args) => api("GET", "queries/" + name + (args ? "?" + new URLSearchParams(args) : ""));

function notice(msg, kind) {
  const n = $("#notice");
  n.textContent = msg; n.hidden = !msg; n.className = "banner " + (kind || "info");
  if (msg) clearTimeout(notice.t), notice.t = setTimeout(() => (n.hidden = true), 6000);
}

// ---- 読み込み ----
async function loadCustomers() {
  st.customers = (await query("customer_list")).rows;
  if (!st.customers.some((c) => c.customer_id === st.customer)) st.customer = st.customers[0]?.customer_id || "";
  $("#customer").innerHTML = st.customers.map((c) => `<option value="${esc(c.customer_id)}">${esc(c.customer_id)}</option>`).join("");
  $("#customer").value = st.customer;
  const me = st.customers.find((c) => c.customer_id === st.customer);
  $("#points").textContent = me ? `ポイント ${me.points}` : "";
}
async function loadProducts() { st.products = (await query("product_list")).rows; renderProducts(); renderCart(); }
async function loadOrders() {
  if (!st.customer) return;
  st.orders = (await query("orders_of", { customer: st.customer })).rows;
  renderOrders();
}
async function refresh() {
  try {
    await loadCustomers(); await Promise.all([loadProducts(), loadOrders()]);
    $("#offline").hidden = true;
  } catch (e) { if (!navigator.onLine) $("#offline").hidden = false; else notice("読み込めません: " + e.message, "warn"); }
}

// ---- 商品 ----
function renderProducts() {
  $("#currency-switch").innerHTML = CURRENCIES.map((c) => `<button data-cur="${c}" class="${c === st.currency ? "on" : ""}">${c}</button>`).join("");
  const list = st.products.filter((p) => p.currency === st.currency);
  $("#product-list").innerHTML = list.map((p) => `
    <div class="card" data-id="${esc(p.product_id)}">
      <div class="name">${esc(p.name)}</div>
      <div class="price">${money(p.price, p.currency)}</div>
      <div class="meta">在庫 ${p.stock}　JAN ${esc(p.jan)}</div>
      <button ${p.stock < 1 ? "disabled" : ""} data-add="${esc(p.product_id)}">${p.stock < 1 ? "品切れ" : "カートに入れる"}</button>
    </div>`).join("") || `<p class="hint">この通貨の商品はありません。</p>`;
}
const cartOf = () => (st.cart[st.currency] ||= {});
function saveCart() { localStorage.setItem("cart", JSON.stringify(st.cart)); renderCart(); }

// ---- カート ----
function renderCart() {
  const c = cartOf();
  const lines = Object.entries(c).map(([id, q]) => ({ p: st.products.find((p) => p.product_id === id), q })).filter((x) => x.p);
  const count = lines.reduce((a, x) => a + x.q, 0);
  $("#cart-count").textContent = count || "";
  const sub = lines.reduce((a, x) => a + x.p.price * x.q, 0);
  $("#cart-lines").innerHTML = lines.length
    ? lines.map(({ p, q }) => `<div class="line"><div>${esc(p.name)}<div class="meta hint">${money(p.price, p.currency)}</div></div>
        <div class="qty"><button data-dec="${esc(p.product_id)}" aria-label="減らす">−</button><span>${q}</span><button data-inc="${esc(p.product_id)}" aria-label="増やす">＋</button></div></div>`).join("") +
      `<div class="line"><strong>小計（${st.currency}）</strong><strong>${money(sub, st.currency)}</strong></div>`
    : `<p class="hint">カートは空です（${st.currency}）。</p>`;
  $("#place").disabled = !lines.length;
}

async function place(ev) {
  ev.preventDefault();
  if (!navigator.onLine) return notice("オフラインのため、注文できません。", "warn");
  const f = new FormData($("#checkout"));
  const items = cartOf();
  $("#place").disabled = true;
  try {
    const r = await api("POST", "deeds/place_order", {
      input: { customer_id: st.customer, items, ship_to: { country: f.get("country").toUpperCase(), postal_code: f.get("postal_code"), line: f.get("line") } },
      slots: { currency: st.currency },
    });
    const b = r.body || {};
    const name = (id) => st.products.find((p) => p.product_id === id)?.name || id;
    const msg = {
      Received: () => `注文を受け付けました（${b.order_id}）。`,
      OutOfStock: () => `在庫が足りません: ${(b.short || []).map(name).join("、")}`,
      NoSuchProduct: () => `登録のない商品があります: ${(b.unregistered || []).join("、")}`,
      OtherCurrency: () => `${st.currency} でない商品が入っています: ${(b.mismatched || []).map(name).join("、")}`,
      NoSuchCustomer: () => "お客さまが見つかりません。",
    }[r.result]?.() || r.result;
    notice(msg, r.result === "Received" ? "info" : "warn");
    if (r.result === "Received") { st.cart[st.currency] = {}; saveCart(); switchTab("orders"); }
    await refresh();
  } catch (e) { notice("注文できません: " + e.message, "warn"); }
  finally { $("#place").disabled = false; renderCart(); }
}

// ---- 注文 ----
function renderOrders() {
  $("#order-list").innerHTML = st.orders.length
    ? st.orders.slice().reverse().map((o) => `<div class="order"><div><strong>${esc(o.order_id)}</strong><div class="meta hint">${o.item_count} 点　${money(o.total, o.currency)}</div></div>
        <span class="status ${esc(o.status)}">${STATUS[o.status] || esc(o.status)}</span></div>`).join("")
    : `<p class="hint">注文はまだありません。</p>`;
}

// ---- テスト操作 ----
async function loadTest() {
  const s = await api("GET", "test/state");
  if (document.activeElement?.closest("#test .panel") == null) {
    $("#t-drop").checked = s.settings.drop_payment_reply; $("#t-short").checked = s.settings.warehouse_short;
    $("#t-decline").value = s.settings.decline_from; $("#t-delay").value = s.settings.delay_secs;
  }
  const row = (cells) => "<tr>" + cells.map((c) => `<td>${esc(c)}</td>`).join("") + "</tr>";
  $("#t-waits").innerHTML = s.waits.length
    ? `<table><tr><th>トリガ</th><th>注文</th><th>届いた返事</th><th>期限まで</th></tr>${s.waits.map((w) => row([w.trigger, w.args.order_id, Object.keys(w.collected).join("、") || "なし", Math.max(0, w.deadline - s.now) + " 秒"])).join("")}</table>` : `<p class="hint">ありません。</p>`;
  $("#t-effects").innerHTML = `<table><tr><th>作用</th><th>注文</th><th>返事の行き先</th></tr>${s.effects.map((e) => row([e.name, e.args.order_id, e.trigger || "待たない"])).join("")}</table>`;
}
async function saveSettings() {
  await api("POST", "test/settings", { drop_payment_reply: $("#t-drop").checked, warehouse_short: $("#t-short").checked, decline_from: +$("#t-decline").value, delay_secs: +$("#t-delay").value });
  notice("設定を反映しました。"); loadTest();
}

// ---- つながりが戻った（出来事） ----
async function reconnected() {
  $("#offline").hidden = true;
  try {
    const r = await api("POST", "events/Reconnected", {});
    if (r.runs.length) notice(`つながりが戻りました。返事待ちの ${r.runs.length} 件を確かめ直しています。`);
  } catch (e) { notice("確かめ直せません: " + e.message, "warn"); }
  refresh();
}

// ---- 画面の動き ----
function switchTab(t) {
  st.tab = t;
  document.querySelectorAll("#tabs button").forEach((b) => b.classList.toggle("on", b.dataset.tab === t));
  document.querySelectorAll(".tab").forEach((s) => (s.hidden = s.id !== t));
  if (t === "test") loadTest().catch((e) => notice(e.message, "warn"));
}
document.addEventListener("click", (e) => {
  const t = e.target.closest("button"); if (!t) return;
  if (t.dataset.tab) switchTab(t.dataset.tab);
  else if (t.dataset.cur) { st.currency = t.dataset.cur; localStorage.setItem("currency", st.currency); renderProducts(); renderCart(); }
  else if (t.dataset.add) { const c = cartOf(); c[t.dataset.add] = Math.min(99, (c[t.dataset.add] || 0) + 1); saveCart(); notice("カートに入れました。"); }
  else if (t.dataset.inc) { const c = cartOf(); c[t.dataset.inc] = Math.min(99, c[t.dataset.inc] + 1); saveCart(); }
  else if (t.dataset.dec) { const c = cartOf(); if (--c[t.dataset.dec] < 1) delete c[t.dataset.dec]; saveCart(); }
});
$("#customer").addEventListener("change", (e) => { st.customer = e.target.value; localStorage.setItem("customer", st.customer); refresh(); });
$("#checkout").addEventListener("submit", place);
$("#t-save").addEventListener("click", saveSettings);
$("#t-reconnect").addEventListener("click", reconnected);
$("#t-advance").addEventListener("click", async () => { await api("POST", "test/advance", { secs: 1800 }); notice("時計を30分進めました。"); loadTest(); loadOrders(); });
window.addEventListener("online", reconnected);
window.addEventListener("offline", () => ($("#offline").hidden = false));
document.addEventListener("visibilitychange", () => { if (!document.hidden) refresh(); });

// 返事待ちの注文があるあいだ、状態を引き直す
setInterval(() => {
  if (document.hidden || !navigator.onLine) return;
  if (st.orders.some((o) => o.status === "Received" || o.status === "PaymentRequested")) { loadOrders().catch(() => {}); loadCustomers().catch(() => {}); }
  if (st.tab === "test") loadTest().catch(() => {});
}, 2000);

if ("serviceWorker" in navigator) navigator.serviceWorker.register("sw.js").catch(() => {});
if (!navigator.onLine) $("#offline").hidden = false;
renderProducts(); renderCart();
refresh();
