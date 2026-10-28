// 画面の殻（HTML・JS・CSS・アイコン）だけを覚えておく。API は覚えない（古い在庫や注文を見せないため）。
const SHELL = "geas-shell-v2";
const FILES = ["/index.html", "/ui.js", "/ui.css", "/manifest.webmanifest", "/icon.svg", "/icon-192.png", "/icon-512.png"];

self.addEventListener("install", (e) => {
  e.waitUntil(caches.open(SHELL).then((c) => c.addAll(FILES)).then(() => self.skipWaiting()));
});
self.addEventListener("activate", (e) => {
  e.waitUntil(caches.keys().then((ks) => Promise.all(ks.filter((k) => k !== SHELL).map((k) => caches.delete(k)))).then(() => self.clients.claim()));
});
self.addEventListener("fetch", (e) => {
  const u = new URL(e.request.url);
  // 画面の URL（リソースの URL）は、API と同じ道筋なので、画面を開く操作（navigate）と殻のファイルだけを扱う。API の応答は覚えない
  const shell = FILES.includes(u.pathname);
  if (e.request.method !== "GET" || u.origin !== location.origin || !(shell || e.request.mode === "navigate")) return;
  // ネットワークを先に試し、つながらなければ覚えたものを返す（画面を開く操作は、どの URL でも同じ index.html）
  e.respondWith(fetch(e.request).then((r) => {
    if (shell) { const copy = r.clone(); caches.open(SHELL).then((c) => c.put(e.request, copy)); }
    return r;
  }).catch(() => caches.match(shell ? e.request : "/index.html")));
});
