// お店のサーバーへの問い合わせは、ここ（service worker）から行う。
// ページ（https の Amazon）から http の localhost を直接呼ぶとブラウザに止められるため、
// manifest の host_permissions で許可したオリジンにだけ、拡張が代わりに問い合わせる。
const DEFAULT_BASE = "http://localhost:8787";

async function base() {
  const { serverUrl } = await chrome.storage.local.get("serverUrl");
  return (serverUrl || DEFAULT_BASE).replace(/\/+$/, "");
}

chrome.runtime.onMessage.addListener((msg, _sender, send) => {
  if (msg && msg.type === "price" && typeof msg.jan === "string") {
    (async () => {
      try {
        const b = await base();
        const r = await fetch(`${b}/product_prices/${encodeURIComponent(msg.jan)}`);
        const j = await r.json();
        send(r.ok ? { ok: true, rows: j.rows || [], shop: b } : { ok: false, error: j.error || r.statusText });
      } catch (e) {
        send({ ok: false, error: "お店のサーバーにつながりません", offline: true });
      }
    })();
    return true; // 非同期で返事をする
  }
});
