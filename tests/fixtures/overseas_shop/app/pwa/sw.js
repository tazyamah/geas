// 画面の殻（HTML・JS・CSS・アイコン）だけを覚えておく。API は覚えない（古い在庫や注文を見せないため）。
const SHELL = "shop-shell-v1";
const FILES = ["./", "index.html", "app.js", "style.css", "manifest.webmanifest", "icon.svg", "icon-192.png", "icon-512.png"];

self.addEventListener("install", (e) => {
  e.waitUntil(caches.open(SHELL).then((c) => c.addAll(FILES)).then(() => self.skipWaiting()));
});
self.addEventListener("activate", (e) => {
  e.waitUntil(caches.keys().then((ks) => Promise.all(ks.filter((k) => k !== SHELL).map((k) => caches.delete(k)))).then(() => self.clients.claim()));
});
self.addEventListener("fetch", (e) => {
  const u = new URL(e.request.url);
  if (e.request.method !== "GET" || u.origin !== location.origin || u.pathname.startsWith("/api/")) return;
  // 殻は、ネットワークを先に試し、つながらなければ覚えたものを返す
  e.respondWith(fetch(e.request).then((r) => {
    const copy = r.clone();
    caches.open(SHELL).then((c) => c.put(e.request, copy));
    return r;
  }).catch(() => caches.match(e.request).then((r) => r || caches.match("index.html"))));
});
