"use strict";
// 開発用：模擬の設定・時計・外部作用の記録（/api/test/…）。アプリの画面ではない。汎用：defs の mock と test/state で動く。
const $ = (s) => document.querySelector(s);
const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
async function api(method, path, body) {
  const r = await fetch(path.startsWith("/") ? path : "/api/" + path, { method, headers: { "Content-Type": "application/json", Accept: "application/json" }, body: body ? JSON.stringify(body) : undefined });
  const j = await r.json().catch(() => ({ error: "応答が読めません" }));
  if (!r.ok) throw new Error(j.error || r.statusText);
  return j;
}
function notice(msg) { const n = $("#notice"); n.textContent = msg; n.hidden = !msg; clearTimeout(notice.t); notice.t = setTimeout(() => (n.hidden = true), 5000); }
let D, built = false;
const effLabel = (n) => D.mock.effects[n]?.label || n;
function build(s) {
  const reps = Object.entries(D.mock.replies);
  $("#t-settings").innerHTML = reps.map(([name, r]) => {
    const eff = Object.entries(D.mock.effects).filter(([, e]) => e.reply === name).map(([, e]) => e.label).join("、");
    return `<fieldset class="group" data-reply="${esc(name)}"><legend>${esc(eff)} → ${esc(r.label)}</legend>` +
      (r.outcomes.length > 1 ? `<label class="f">返事の結果の種類<select data-outcome>${r.outcomes.map((o) => `<option value="${esc(o.name)}">${esc(o.label)}</option>`).join("")}</select></label>` : "") +
      `<label class="f"><input type="checkbox" data-drop> 返事を落とす（届かないまま）</label></fieldset>`;
  }).join("") + `<label class="f">返事までの秒数<input type="number" id="t-delay" min="0" max="600"></label><button class="primary" id="t-save">設定を反映</button>`;
  $("#t-actions").innerHTML = D.mock.events.map((e) => `<button class="primary" data-event="${esc(e.name)}">${esc(e.label)}（出来事）</button>`).join("") + `<button class="primary" id="t-advance">時計を30分進める</button>`;
  $("#t-save").addEventListener("click", async () => {
    const replies = {};
    document.querySelectorAll("[data-reply]").forEach((g) => { replies[g.dataset.reply] = { drop: g.querySelector("[data-drop]").checked, ...(g.querySelector("[data-outcome]") ? { outcome: g.querySelector("[data-outcome]").value } : {}) }; });
    await api("POST", "test/settings", { delay_secs: +$("#t-delay").value, replies });
    notice("設定を反映しました。"); load();
  });
  $("#t-advance").addEventListener("click", async () => { await api("POST", "test/advance", { secs: 1800 }); notice("時計を30分進めました。"); load(); });
  document.querySelectorAll("[data-event]").forEach((b) => b.addEventListener("click", async () => {
    const r = D.routes.find((x) => x.target === b.dataset.event && x.kind === "create");
    await api("POST", r.path, {}); notice(`${b.textContent}を起こしました。`); load();
  }));
  built = true;
}
async function load() {
  const s = await api("GET", "test/state");
  if (!built) build(s);
  if (document.activeElement?.closest(".group") == null) {
    $("#t-delay").value = s.settings.delay_secs;
    for (const [name, v] of Object.entries(s.settings.replies)) {
      const g = document.querySelector(`[data-reply="${name}"]`);
      if (!g) continue;
      g.querySelector("[data-drop]").checked = v.drop;
      if (g.querySelector("[data-outcome]")) g.querySelector("[data-outcome]").value = v.outcome;
    }
  }
  const row = (cells) => "<tr>" + cells.map((c) => `<td>${esc(c)}</td>`).join("") + "</tr>";
  const args = (a) => Object.entries(a || {}).map(([k, v]) => `${k}=${typeof v === "object" ? JSON.stringify(v) : v}`).join(" ");
  $("#t-waits").innerHTML = s.waits.length
    ? `<table><tr><th>トリガ</th><th>引数</th><th>届いた返事</th><th>期限まで</th></tr>${s.waits.map((w) => row([w.trigger, args(w.args), Object.keys(w.collected).join("、") || "なし", Math.max(0, w.deadline - s.now) + " 秒"])).join("")}</table>` : `<p class="hint">ありません。</p>`;
  $("#t-effects").innerHTML = `<table><tr><th>作用</th><th>引数</th><th>返事の行き先</th></tr>${s.effects.map((e) => row([effLabel(e.name), args(e.args), e.trigger || "待たない"])).join("")}</table>`;
}
setInterval(() => { if (!document.hidden) load().catch(() => {}); }, 2000);
api("GET", "defs").then((d) => { D = d; return load(); }).catch((e) => notice(e.message));
