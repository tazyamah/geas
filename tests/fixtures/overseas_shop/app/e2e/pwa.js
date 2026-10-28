// PWA の通し確認（Playwright）。使い方: node pwa.js http://localhost:8787
// カートに入れて注文し、決済と倉庫の返事がそろって「完了」になるまでを、画面の操作で確かめる。
const assert = require("assert");
const { chromium } = require("playwright");
const base = process.argv[2] || "http://localhost:8787";

(async () => {
  const b = await chromium.launch();
  const ctx = await b.newContext({ viewport: { width: 420, height: 900 } });
  const p = await ctx.newPage();
  const errors = [];
  p.on("pageerror", (e) => errors.push(e.message));
  await p.goto(base + "/");
  await p.waitForSelector(".card");
  assert.strictEqual(await p.locator(".card").count(), 4, "円の商品は4つ");

  // 通貨を切り替えると、その通貨の商品だけになる
  await p.click('[data-cur="USD"]');
  assert.strictEqual(await p.locator(".card").count(), 3, "ドルの商品は3つ");
  assert.ok((await p.locator(".card .price").first().innerText()).includes("$"));
  await p.click('[data-cur="JPY"]');

  await p.click('[data-add="tea-jp"]');
  await p.click('[data-add="tea-jp"]');
  await p.click('[data-add="knife-jp"]');
  await p.click('[data-tab="cart"]');
  assert.ok((await p.locator("#cart-lines").innerText()).includes("10,160"), "小計");
  await p.click("#place");
  await p.waitForSelector(".order");
  await p.waitForFunction(() => document.querySelector(".status.Completed"), null, { timeout: 10000 });
  const order = await p.locator("#order-list").innerText();
  assert.ok(order.includes("11,676"), "税と送料を足した合計: " + order); // 10160 + 税10% 1016 + 送料500

  // 注文のあとのポイントは、顧客の表示に出る
  assert.ok((await p.locator("#points").innerText()).includes("15"));

  // サービスワーカーが殻を覚え、オフラインでも画面が開く
  await p.evaluate(() => navigator.serviceWorker.ready);
  await ctx.setOffline(true);
  await p.reload();
  await p.waitForSelector("#tabs");
  assert.ok(!(await p.locator("#offline").isHidden()), "オフラインの表示");
  await ctx.setOffline(false);
  assert.deepStrictEqual(errors, []);
  await b.close();
  console.log("pwa ok");
})().catch((e) => { console.error(e); process.exit(1); });
