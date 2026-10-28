// サーバーへの問い合わせは、ここ（service worker）から行う。汎用：defs.js（Geas の screen から生成）で動く。
// ページ（https のサイト）から http の localhost を直接呼ぶとブラウザに止められるため、
// manifest の host_permissions で許可したオリジンにだけ、拡張が代わりに問い合わせる。
importScripts("defs.js");
const X = self.GEAS_EXT;

async function base() {
  const { serverUrl } = await chrome.storage.local.get("serverUrl");
  return (serverUrl || X.server).replace(/\/+$/, "");
}

chrome.runtime.onMessage.addListener((msg, _sender, send) => {
  if (msg && msg.type === "query" && typeof msg.value === "string") {
    (async () => {
      try {
        const b = await base();
        const r = await fetch(b + X.route.path.replace(/:[A-Za-z0-9_]+/, encodeURIComponent(msg.value)), { headers: { Accept: "application/json" } });
        const j = await r.json();
        send(r.ok ? { ok: true, rows: j.rows || [], shop: b } : { ok: false, error: j.error || r.statusText });
      } catch (e) {
        send({ ok: false, error: X.texts.offline, offline: true });
      }
    })();
    return true; // 非同期で返事をする
  }
});
