// ページに、サーバーの問い合わせの結果を小さく表示する（読むだけ）。汎用：defs.js（Geas の screen から生成）と、
// ページから値を読む reader.js（手で書く部分。self.GeasReader.read）で動く。
(function () {
  "use strict";
  const X = self.GEAS_EXT, T = X.texts;
  const fmt = (n, cur) => { const m = X.minor[cur] ?? 2; return new Intl.NumberFormat("ja-JP", { style: "currency", currency: cur, minimumFractionDigits: m, maximumFractionDigits: m }).format(n / 10 ** m); };
  const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

  let box = null, lastKey = null;
  function ensureBox() {
    if (box && document.body.contains(box)) return box;
    box = document.createElement("div");
    box.id = "geas-ext-box";
    document.body.appendChild(box);
    return box;
  }
  function show(html, cls) {
    const b = ensureBox();
    b.className = cls || "";
    b.innerHTML = html;
  }
  // 行を見せる：金額の列は通貨つきで、ほかの列は値をそのまま
  function cell(col, row) {
    const v = row[col.name];
    if (v === null || v === undefined) return "";
    if (col.kind === "money") return `<span class="p" data-cur="${esc(row[col.unit] ?? "")}">${fmt(v, row[col.unit])}</span>`;
    if (col.kind === "enum") return `<span>${esc(col.values.find((x) => x.value === v)?.label ?? v)}</span>`;
    return `<span>${esc(v)}</span>`;
  }

  async function run() {
    const stored = await chrome.storage.local.get("readerConfig");
    const r = self.GeasReader.read(location.href, document, stored.readerConfig || {});
    const key = (r.subject || "") + "|" + (r.value || "");
    if (key === lastKey) return;
    lastKey = key;
    if (!r.subject) { if (box) box.remove(); return; }
    if (!r.value) { show(`<b>${esc(X.title)}</b><span>${esc(T.unknown.replace("{input}", X.input.label).replace("{subject}", r.subject))}</span>`, "dim"); return; }
    show(`<b>${esc(X.title)}</b><span>${esc(T.loading)}</span>`);
    const a = await chrome.runtime.sendMessage({ type: "query", value: r.value });
    if (!a || !a.ok) { show(`<b>${esc(X.title)}</b><span>${esc((a && a.error) || T.failed)}</span>`, "dim"); return; }
    if (!a.rows.length) { show(`<b>${esc(X.title)}</b><span>${esc(T.none.replace("{input}", X.input.label).replace("{value}", r.value))}</span>`, "dim"); return; }
    const rows = a.rows.map((row) => X.columns.filter((c) => c.kind === "money").map((c) => cell(c, row)).join("") || X.columns.map((c) => cell(c, row)).join(" ")).join("");
    show(`<b>${esc(X.title)}</b>${rows}<a href="${esc(a.shop)}/" target="_blank" rel="noopener">${esc(T.open)}</a><small>${esc(X.input.label)} ${esc(r.value)}${r.source ? "（" + esc(r.source) + "）" : ""}</small>`);
  }

  run();
  // ページを読み直さずに内容が変わることがある（商品が変わる、など）
  setInterval(() => { run().catch(() => {}); }, 1500);
})();
