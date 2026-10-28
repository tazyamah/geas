// Chrome 拡張の通し確認（Playwright）。使い方: node extension.js 作業フォルダ
// 本物の Amazon には行かず、amazon.co.jp への要求を差し替えた商品ページで、価格の表示を確かめる。
const assert = require("assert");
const path = require("path");
const { chromium } = require("playwright");

const EXT = path.resolve(__dirname, "../extension");
const work = process.argv[2] || require("os").tmpdir();

const html = (body) => `<!doctype html><html lang="ja"><head><meta charset="utf-8"></head><body><h1>商品</h1>${body}</body></html>`;
const PAGES = {
  B0SHOPTEA1: html("<p>ページに JAN は無い</p>"), // 対応表から引く
  B0PAGEJAN1: html('<table id="productDetails_techSpec_section_1"><tr><th>JANコード</th><td>4909876543214</td></tr></table>'), // ページから読む
  B0UNKNOWN1: html("<p>JAN も対応表も無い</p>"),
  B0BADJAN01: html("<table><tr><th>JAN</th><td>4901234567895</td></tr></table>"), // 最後の1桁が違う
  B0NOTSOLD1: html("<table><tr><th>EAN</th><td>4900000000009</td></tr></table>"), // 正しい JAN だが、お店に無い
};

(async () => {
  const ctx = await chromium.launchPersistentContext(path.join(work, "profile"), {
    headless: false,
    args: ["--headless=new", `--disable-extensions-except=${EXT}`, `--load-extension=${EXT}`, "--no-sandbox"],
  });
  const page = await ctx.newPage();
  await page.route("https://www.amazon.co.jp/**", (route) => {
    const m = /\/(?:dp|gp\/product)\/([A-Z0-9]{10})/.exec(route.request().url());
    route.fulfill({ contentType: "text/html; charset=utf-8", body: m && PAGES[m[1]] ? PAGES[m[1]] : html("<p>top</p>") });
  });
  const box = async (url) => {
    await page.goto(url);
    await page.waitForTimeout(1800);
    const b = page.locator("#shop-price-box");
    return (await b.count()) ? (await b.innerText()).replace(/\s+/g, " ") : null;
  };
  let t = await box("https://www.amazon.co.jp/dp/B0SHOPTEA1");
  assert.ok(t.includes("￥680") && t.includes("$6.99") && t.includes("対応表から"), t);
  t = await box("https://www.amazon.co.jp/gp/product/B0PAGEJAN1");
  assert.ok(t.includes("￥8,800") && t.includes("$79.00") && t.includes("ページから"), t);
  t = await box("https://www.amazon.co.jp/dp/B0UNKNOWN1");
  assert.ok(t.includes("JAN が分かりません"), t);
  t = await box("https://www.amazon.co.jp/dp/B0BADJAN01");
  assert.ok(t.includes("JAN が分かりません"), t); // 最後の1桁が合わない JAN は、拡張でも問い合わせない
  t = await box("https://www.amazon.co.jp/dp/B0NOTSOLD1");
  assert.ok(t.includes("扱っていません"), t);
  assert.strictEqual(await box("https://www.amazon.co.jp/"), null, "商品ページでなければ出さない");
  await ctx.close();
  console.log("extension ok");
})().catch((e) => { console.error(e); process.exit(1); });
