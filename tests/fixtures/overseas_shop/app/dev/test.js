"use strict";
// 開発用：模擬の設定・時計・外部作用の記録（/api/test/…）。アプリの画面ではない。
const $ = (s) => document.querySelector(s);
const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
async function api(method, path, body) {
  const r = await fetch("/api/" + path, { method, headers: { "Content-Type": "application/json" }, body: body ? JSON.stringify(body) : undefined });
  const j = await r.json().catch(() => ({ error: "応答が読めません" }));
  if (!r.ok) throw new Error(j.error || r.statusText);
  return j;
}
function notice(msg) { const n = $("#notice"); n.textContent = msg; n.hidden = !msg; clearTimeout(notice.t); notice.t = setTimeout(() => (n.hidden = true), 5000); }
async function load() {
  const s = await api("GET", "test/state");
  if (document.activeElement?.closest(".group") == null) {
    $("#t-drop").checked = s.settings.drop_payment_reply; $("#t-short").checked = s.settings.warehouse_short;
    $("#t-decline").value = s.settings.decline_from; $("#t-delay").value = s.settings.delay_secs;
  }
  const row = (cells) => "<tr>" + cells.map((c) => `<td>${esc(c)}</td>`).join("") + "</tr>";
  $("#t-waits").innerHTML = s.waits.length
    ? `<table><tr><th>トリガ</th><th>注文</th><th>届いた返事</th><th>期限まで</th></tr>${s.waits.map((w) => row([w.trigger, w.args.order_id, Object.keys(w.collected).join("、") || "なし", Math.max(0, w.deadline - s.now) + " 秒"])).join("")}</table>` : `<p class="hint">ありません。</p>`;
  $("#t-effects").innerHTML = `<table><tr><th>作用</th><th>注文</th><th>返事の行き先</th></tr>${s.effects.map((e) => row([e.name, e.args.order_id, e.trigger || "待たない"])).join("")}</table>`;
}
$("#t-save").addEventListener("click", async () => {
  await api("POST", "test/settings", { drop_payment_reply: $("#t-drop").checked, warehouse_short: $("#t-short").checked, decline_from: +$("#t-decline").value, delay_secs: +$("#t-delay").value });
  notice("設定を反映しました。"); load();
});
$("#t-reconnect").addEventListener("click", async () => {
  const d = await api("GET", "defs");
  for (const [name, on] of Object.entries(d.screens.events_on)) if (on === "online") await api("POST", d.routes.find((r) => r.target === name).path, {});
  notice("つながりが戻った出来事を起こしました。"); load();
});
$("#t-advance").addEventListener("click", async () => { await api("POST", "test/advance", { secs: 1800 }); notice("時計を30分進めました。"); load(); });
setInterval(() => { if (!document.hidden) load().catch(() => {}); }, 2000);
load().catch((e) => notice(e.message));
