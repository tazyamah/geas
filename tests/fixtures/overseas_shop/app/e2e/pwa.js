// 画面（Geas の screen から作った汎用の画面）の通し確認（Playwright）。使い方: node pwa.js http://localhost:8787
// 商品一覧から注文し、決済と倉庫の返事がそろって「完了」になるまでを、画面の操作で確かめる。
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
  await p.waitForSelector('[data-screen="ProductList"] [data-row]');
  assert.strictEqual(await p.locator("#screen [data-row]").count(), 8, "商品一覧には全部の商品が並ぶ");
  assert.strictEqual(await p.evaluate(() => document.body.dataset.layout), "grid", "design の配置が画面に効く");
  assert.strictEqual(await p.evaluate(() => document.querySelector('meta[name="theme-color"]').content), "#0b5cad", "design の色が画面に効く");

  // 商品を選ぶと注文の画面が開き、その商品の通貨になる
  await p.click('[data-row="tea-jp"] [data-open="OrderForm"]');
  await p.waitForSelector('[data-screen="OrderForm"] [data-row]');
  assert.ok(p.url().endsWith("/orders/new#product_id=tea-jp"), "画面の URL はリソースの URL（受け取る値はフラグメント）: " + p.url());
  assert.strictEqual(await p.locator('[data-slot="currency"] button.on').getAttribute("data-slotval"), "JPY");
  assert.strictEqual(await p.locator('#screen [data-row]').count(), 4, "円の商品は4つ");
  assert.strictEqual(await p.locator('[data-qty="tea-jp"]').innerText(), "1", "選んだ商品は1個で始まる");

  // 通貨を切り替えると、その通貨の商品だけになる
  const rows = async (n) => p.waitForFunction((n) => document.querySelectorAll("#screen [data-row]").length === n, n, { timeout: 5000 });
  // URL を直接開いても同じ画面になる（形式 html）
  const q = await ctx.newPage();
  await q.goto(base + "/orders/new#product_id=tea-jp");
  await q.waitForSelector('[data-screen="OrderForm"] [data-row]');
  assert.strictEqual(await q.locator('[data-qty="tea-jp"]').innerText(), "1");
  await q.close();
  await p.click('[data-slotval="USD"]');
  await rows(3); // ドルの商品は3つ
  assert.ok((await p.locator('#screen [data-col="price"]').first().innerText()).includes("$"));
  await p.click('[data-slotval="JPY"]');
  await rows(4);

  await p.click('[data-row="tea-jp"] [data-inc]');
  await p.click('[data-row="knife-jp"] [data-inc]');
  await p.fill('[data-field="ship_to.country"]', "JP");
  await p.fill('[data-field="ship_to.postal_code"]', "100-0001");
  await p.fill('[data-field="ship_to.line"]', "東京都千代田区千代田1-1");
  await p.click('[data-action="place_order"]');
  await p.waitForSelector('[data-screen="MyOrders"] [data-row]');
  await p.waitForFunction(() => document.querySelector('[data-col="status"][data-value="Completed"]'), null, { timeout: 10000 });
  const order = await p.locator("#screen").innerText();
  assert.ok(order.includes("11,676"), "税と送料を足した合計: " + order); // 10160 + 税10% 1016 + 送料500

  // 注文のあとのポイントは、選んだ顧客の下に出る
  assert.ok((await p.locator('[data-picked="customer"]').innerText()).includes("15"));

  // サービスワーカーが殻を覚え、オフラインでも画面が開く
  await p.evaluate(() => navigator.serviceWorker.ready);
  await ctx.setOffline(true);
  await p.reload();
  await p.waitForSelector("#tabs button");
  assert.ok(!(await p.locator("#offline").isHidden()), "オフラインの表示");
  await ctx.setOffline(false);
  assert.deepStrictEqual(errors, []);
  await b.close();
  console.log("pwa ok");
})().catch((e) => { console.error(e); process.exit(1); });
