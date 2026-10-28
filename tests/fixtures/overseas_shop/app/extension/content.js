// Amazon の商品ページに、その商品が「お店」でいくらかを小さく表示する（読むだけ）。
(function () {
  "use strict";
  const MINOR = { JPY: 0, USD: 2, EUR: 2 };
  const fmt = (n, cur) => new Intl.NumberFormat("ja-JP", { style: "currency", currency: cur, minimumFractionDigits: MINOR[cur] ?? 2, maximumFractionDigits: MINOR[cur] ?? 2 }).format(n / 10 ** (MINOR[cur] ?? 2));

  let box = null, lastKey = null;
  function ensureBox() {
    if (box && document.body.contains(box)) return box;
    box = document.createElement("div");
    box.id = "shop-price-box";
    document.body.appendChild(box);
    return box;
  }
  function show(html, cls) {
    const b = ensureBox();
    b.className = cls || "";
    b.innerHTML = html;
  }
  const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

  async function run() {
    const stored = await chrome.storage.local.get("asinTable");
    const table = Object.assign({}, self.ShopConverter.SAMPLE_TABLE, stored.asinTable || {});
    const { asin, jan, source } = self.ShopConverter.resolve(location.href, document, table);
    const key = asin + "|" + jan;
    if (key === lastKey) return;
    lastKey = key;
    if (!asin) { if (box) box.remove(); return; }
    if (!jan) { show(`<b>お店</b><span>この商品の JAN が分かりません（ASIN ${esc(asin)}）。</span>`, "dim"); return; }
    show(`<b>お店</b><span>調べています…</span>`);
    const r = await chrome.runtime.sendMessage({ type: "price", jan });
    if (!r || !r.ok) { show(`<b>お店</b><span>${esc((r && r.error) || "調べられません")}</span>`, "dim"); return; }
    if (!r.rows.length) { show(`<b>お店</b><span>この商品は、お店では扱っていません（JAN ${esc(jan)}）。</span>`, "dim"); return; }
    const prices = r.rows.map((x) => `<span class="p" data-cur="${esc(x.currency)}">${fmt(x.price, x.currency)}</span>`).join("");
    show(`<b>お店では</b>${prices}<a href="${esc(r.shop)}/" target="_blank" rel="noopener">お店を開く</a><small>JAN ${esc(jan)}（${source === "page" ? "ページから" : "対応表から"}）</small>`);
  }

  run();
  // Amazon の画面は、ページを読み直さずに商品が変わることがある
  setInterval(() => { run().catch(() => {}); }, 1500);
})();
