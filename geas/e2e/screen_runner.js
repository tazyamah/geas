// 画面の通しテストの汎用の実行部（Playwright）。アプリの名前も画面も知らない。
// 使い方: node screen_runner.js 手順.json サーバーのURL 作業フォルダ [--no-ext]
// 手順.json は、geas が計画書（ユースケースの手順と画面の具体例）から作る。部品の見つけ方は、画面の汎用の実行部（geas/ui）の約束：
//   data-screen＝画面、data-row＝行、data-field＝入力欄、data-qty/data-inc/data-dec＝個数、data-slot/data-slotval＝slot、data-action＝処理、data-open＝行から開く画面
// 待ち時間は決め打ちにしない：最初に、サーバーの応答時間・返事までの秒数・画面の引き直しの間隔を実測し、そこから作る。
const assert = require("assert");
const fs = require("fs");
const path = require("path");
const { chromium } = require("playwright");

const [casesFile, base, work, flag] = process.argv.slice(2);
const cases = JSON.parse(fs.readFileSync(casesFile, "utf8"));
const EXT = path.resolve(path.dirname(casesFile), "../ext");

async function getJson(p) { return (await fetch(base + p)).json(); }

// 較正：待ちの上限を実測から作る
async function calibrate(browser, defs) {
  const lat = [];
  const time = async (p) => { const t = Date.now(); const r = await fetch(base + p); await r.text(); lat.push(Date.now() - t); };
  for (let i = 0; i < 5; i++) await time("/api/health");
  for (const r of defs.routes) if (r.method === "GET" && !r.path.includes(":")) await time(r.path);
  const L = Math.max(...lat);
  const state = await getJson("/api/test/state").catch(() => ({}));
  const S = ((state.settings && state.settings.delay_secs) || 0) * 1000;
  // 画面が見せている問い合わせを引き直す間隔：同じ GET が2回目に出るまでを測る
  const ctx = await browser.newContext();
  const p = await ctx.newPage();
  const seen = new Map();
  let P = 3000;
  const got = new Promise((ok) => p.on("request", (rq) => {
    if (rq.method() !== "GET" || !new URL(rq.url()).pathname.startsWith("/") || /\.(js|css|png|svg|json|html)$/.test(rq.url())) return;
    const u = rq.url(), t = Date.now();
    if (seen.has(u) && t - seen.get(u) > 200) ok(t - seen.get(u));
    else if (!seen.has(u)) seen.set(u, t);
  }));
  await p.goto(base + "/");
  P = await Promise.race([got, new Promise((ok) => setTimeout(() => ok(P), 8000))]);
  await ctx.close();
  const budget = S + P + 5 * L + 1000;
  console.log(`較正: 応答 最大${L}ms / 返事まで${S}ms / 画面の引き直し${P}ms → 待ちの上限 ${budget}ms`);
  return { L, S, P, budget };
}

const replaceId = (rt, st) => (rt.id_input ? (st.replace.find((r) => r.field === rt.id_input) || {}).value : undefined);
const routeOf = (defs, deed) => defs.routes.find((r) => r.target === deed && r.kind !== "index" && r.kind !== "show");
const pathRe = (pat) => new RegExp("^" + pat.replace(/[.*+?^${}()|[\]\\]/g, "\\$&").replace(/:[A-Za-z0-9_]+/g, "[^/]+") + "/?$");

// 見えることの1項目が成り立つまで待つ（上限は較正から）
async function expectOn(p, st, e, cal, scopeSel) {
  const fn = ({ e, scopeSel }) => {
    const norm = (s) => String(s).replace(/\s+/g, " ").trim();
    const scope = document.querySelector(scopeSel);
    const text = scope ? norm(scope.innerText) : "";
    switch (e.as) {
      case "含む": return text.includes(e.value);
      case "通知": { const n = document.getElementById("notice"); return !!n && !n.hidden && norm(n.innerText).includes(e.value); }
      case "含まない": return !!scope && !text.includes(e.value);
      case "行数": return document.querySelectorAll("#screen [data-row]").length === Number(e.value);
      case "行": return !!document.querySelector(`#screen [data-row="${e.value}"]`);
      case "行なし": return !document.querySelector(`#screen [data-row="${e.value}"]`) && !!document.querySelector("#screen[data-screen]");
      case "選択": return document.querySelector(`[data-slot="${e.slot}"] button.on`)?.getAttribute("data-slotval") === e.value;
      case "列": {
        const rows = e.row === "*" ? [...document.querySelectorAll("#screen [data-row]")] : [document.querySelector(`#screen [data-row="${e.row}"]`)].filter(Boolean);
        return rows.some((r) => { const c = r.querySelector(`[data-col="${e.col}"]`); return c && norm((c.querySelector("b") || c).innerText) === e.value; });
      }
    }
    return false;
  };
  try {
    await p.waitForFunction(fn, { e, scopeSel }, { timeout: cal.budget, polling: 100 });
  } catch (err) {
    const body = await p.evaluate((s) => String((document.querySelector(s) || document.body).innerText).replace(/\s+/g, " "), scopeSel).catch(() => "");
    throw new Error(`${st.text}: 見えること「${e.as} ${e.value ?? ""}」が ${cal.budget}ms 待っても成り立ちません。画面: ${body.slice(0, 400)}`);
  }
}

async function runPwaCase(ctx, defs, cal, c) {
  const p = await ctx.newPage();
  const errors = [];
  p.on("pageerror", (e) => errors.push("pageerror: " + e.message));
  p.on("console", (m) => { if (m.type() === "error" && /Content Security Policy/i.test(m.text())) errors.push("CSP 違反: " + m.text()); });
  p.on("dialog", async (d) => { errors.push("ダイアログが出ました: " + d.message()); await d.dismiss(); });
  for (const [i, st] of c.steps.entries()) {
    const here = `#screen[data-screen="${st.screen}"]`;
    if (st.op === "look") {
      if (i === 0) await p.goto(base + st.url);
      await p.waitForSelector(here, { timeout: cal.budget });
      const d = st.design || {};
      if (d.layout) assert.strictEqual(await p.evaluate(() => document.body.dataset.layout), d.layout, `${st.text}: design の配置が画面に効く`);
      if (d.accent) assert.strictEqual(await p.evaluate(() => document.querySelector('meta[name="theme-color"]').content), d.accent, `${st.text}: design の色が画面に効く`);
      if (d.mode) assert.strictEqual(await p.evaluate(() => document.body.dataset.mode), d.mode, `${st.text}: design の明暗が画面に効く`);
    } else {
      await p.waitForSelector(here, { timeout: cal.budget });
    }
    if (st.op === "open") {
      await p.waitForSelector(`${here} [data-row="${st.row}"]`, { timeout: cal.budget });
      await p.click(`[data-row="${st.row}"] [data-open="${st.opens}"]`);
      await p.waitForSelector(`#screen[data-screen="${st.opens}"]`, { timeout: cal.budget });
      const target = defs.screens.screens[st.opens];
      assert.ok(pathRe(target.url).test(await p.evaluate(() => location.pathname)), `${st.text}: 画面の URL は、リソースの URL（${target.url}）`);
      assert.ok(decodeURIComponent(await p.evaluate(() => location.pathname + location.hash)).includes(st.row), `${st.text}: 選んだ行の ID が URL に載る`);
    }
    if (st.op === "act") {
      const post = (p, body) => fetch(base + p, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
      const drop = (on) => Promise.all((st.drop || []).map((n) => post("/api/test/settings", { replies: { [n]: { drop: on } } })));
      await drop(true);  // 返事を落とす：届かないまま待たせる
      for (const s of st.set) {
        if (s.as === "field") {
          const sel = `[data-field="${s.field}"]`;
          await p.waitForSelector(sel, { timeout: cal.budget });
          if ((await p.locator(sel).evaluate((el) => el.tagName)) === "SELECT") {
            await p.waitForFunction(([sel, v]) => [...document.querySelector(sel).options].some((o) => o.value === v), [sel, String(s.value)], { timeout: cal.budget });
            await p.selectOption(sel, String(s.value));
          } else await p.fill(sel, String(s.value));
        } else if (s.as === "slot") {
          await p.click(`[data-slot="${s.slot}"] [data-slotval="${s.value}"]`);
          await p.waitForSelector(`[data-slot="${s.slot}"] button.on[data-slotval="${s.value}"]`, { timeout: cal.budget });
        } else if (s.as === "count") {
          const q = `[data-qty="${s.row}"]`;
          await p.waitForSelector(q, { timeout: cal.budget });
          for (let k = 0; k < 120; k++) {
            const now = Number(await p.locator(q).innerText());
            if (now === s.value) break;
            await p.click(`[data-${now < s.value ? "inc" : "dec"}="${s.row}"]`);
          }
          assert.strictEqual(Number(await p.locator(q).innerText()), s.value, `${st.text}: 個数を ${s.value} にできません`);
        }
      }
      const rt = routeOf(defs, st.deed), re = pathRe(rt.path);
      const isCall = (rq) => rq.method() === rt.method && re.test(new URL(rq.url()).pathname);
      const urlIs = (u) => re.test(u.pathname);
      if (st.replace.length) {  // 画面からは作れない値（画面が出さない選択肢）を、送る直前に差し替えて、結果の扱いを確かめる
        await p.route(urlIs, async (route) => {
          if (route.request().method() !== rt.method) return route.continue();
          const body = JSON.parse(route.request().postData() || "{}");
          for (const r of st.replace) {
            if (rt.id_input && r.field === rt.id_input) continue;
            let v = r.value;
            if (/^[{\[]/.test(v)) v = JSON.parse(v);
            if (r.as === "slot") (body.slots = body.slots || {})[r.slot] = v;
            else if (r.field.includes(".")) { const [a, b] = r.field.split("."); (body[a] = body[a] || {})[b] = v; }
            else body[r.field] = v;
          }
          let url = route.request().url();
          const idr = replaceId(rt, st);  // update／destroy の ID は URL にある
          if (idr !== undefined) url = new URL(rt.path.replace(/:[A-Za-z0-9_]+/, encodeURIComponent(idr)), url).href;
          route.continue({ url, postData: JSON.stringify(body) });
        });
      }
      const sent = p.waitForResponse((r) => isCall(r.request()), { timeout: cal.budget });
      await p.click(`[data-action="${st.deed}"]`);
      const resp = await sent.catch(async (err) => {  // 送られなかったとき、画面の通知（入力の不足など）を添える
        throw new Error(`${st.text}: 処理の呼び出しが ${cal.budget}ms 待っても送られません。通知: ${await p.locator("#notice").innerText().catch(() => "")}`);
      });
      const j = await resp.json();
      if (st.result) assert.strictEqual(j.result, st.result, `${st.text}: 結果の種類（応答: ${JSON.stringify(j).slice(0, 200)}）`);
      if (st.replace.length) await p.unroute(urlIs);
      if (st.advance) {  // 模擬の時計を進める：待ちが登録されてから進める（実際には待たない）
        const t0 = Date.now();
        while (!((await getJson("/api/test/state")).waits || []).length) {
          if (Date.now() - t0 > cal.budget) throw new Error(`${st.text}: 返事待ちが ${cal.budget}ms たっても登録されません`);
          await new Promise((ok) => setTimeout(ok, 100));
        }
        const adv = await (await post("/api/test/advance", { secs: st.advance })).json();
        assert.ok((adv.fired || []).length, `${st.text}: 時計を ${st.advance} 秒進めても何も起きません`);
      }
      await drop(false);
      if (st.to === "stay") assert.ok(await p.locator(here).count(), `${st.text}: そのままの画面にとどまる`);
      else if (st.to !== "back") await p.waitForSelector(`#screen[data-screen="${st.to}"]`, { timeout: cal.budget });
    }
    for (const e of st.expect) await expectOn(p, st, e, cal, "#screen");
    console.log(`  ok ${st.text}`);
  }
  assert.deepStrictEqual(errors, [], `${c.name}: 画面でエラー`);
  await p.close();
}

const html = (body) => `<!doctype html><html lang="ja"><head><meta charset="utf-8"></head><body><h1>商品</h1>${body}</body></html>`;

async function runExtCases(extCases, cal) {
  const ctx = await chromium.launchPersistentContext(path.join(work, "profile"), {
    headless: false,
    args: ["--headless=new", `--disable-extensions-except=${EXT}`, `--load-extension=${EXT}`, "--no-sandbox"],
  });
  const page = await ctx.newPage();
  let current = html("");
  const origins = new Set(extCases.flatMap((c) => c.steps.map((s) => s.origin)));
  for (const o of origins) await page.route(o + "/**", (route) => route.fulfill({ contentType: "text/html; charset=utf-8", body: current }));
  for (const c of extCases) {
    console.log(c.name);
    for (const st of c.steps) {
      const pg = st.set.find((s) => s.as === "page");
      assert.ok(pg, `${st.text}: 拡張の画面には「ページ = 経路」が要ります`);
      current = html(st.set.filter((s) => s.as === "table").map((s) => { const [h, v] = s.value.split(/\s*[:：]\s*/); return `<table><tr><th>${h}</th><td>${v}</td></tr></table>`; }).join("") || "<p>top</p>");
      await page.goto(st.origin + pg.value);
      for (const e of st.expect) {
        if (e.as === "表示なし") {
          await page.waitForTimeout(4 * cal.L + 2000);  // 値を読み、問い合わせて表示するまでの目安（実測の応答時間から）
          assert.strictEqual(await page.locator("#geas-ext-box").count(), 0, `${st.text}: 何も表示しない`);
        } else await expectOn(page, st, e, cal, "#geas-ext-box");
      }
      console.log(`  ok ${st.text}`);
    }
  }
  await ctx.close();
}

(async () => {
  const defs = await getJson("/api/defs");
  const browser = await chromium.launch();
  const cal = await calibrate(browser, defs);
  const pwa = cases.filter((c) => !c.steps[0].ext), ext = cases.filter((c) => c.steps[0].ext);
  const ctx = await browser.newContext({ viewport: { width: 420, height: 900 } });
  for (const c of pwa) { console.log(c.name); await runPwaCase(ctx, defs, cal, c); }

  // 基盤の確認（計画書の手順にはない、汎用の画面の約束）：オフラインでも画面が開く／開発用の操作パネルが定義どおり動く
  const p = await ctx.newPage();
  await p.goto(base + "/");
  await p.waitForSelector("#tabs button");
  await p.evaluate(() => navigator.serviceWorker.ready);
  await ctx.setOffline(true);
  await p.reload();
  await p.waitForSelector("#tabs button");
  assert.ok(!(await p.locator("#offline").isHidden()), "オフラインの表示");
  await ctx.setOffline(false);
  const t = await ctx.newPage();
  await t.goto(base + "/test.html");
  const outcomeIs = (n, v) => t.waitForFunction(async ([n, v]) => (await (await fetch("/api/test/state")).json()).settings.replies[n].outcome === v, [n, v], { timeout: cal.budget });
  for (const [name, r] of Object.entries(defs.mock.replies)) {
    if (r.outcomes.length < 2) continue;
    const sel = `[data-reply="${name}"] [data-outcome]`, last = r.outcomes[r.outcomes.length - 1].name;
    await t.waitForSelector(sel);
    assert.deepStrictEqual(await t.locator(`[data-reply="${name}"] option`).allInnerTexts(), r.outcomes.map((o) => o.label));
    await t.selectOption(sel, last);
    await t.click("#t-save");
    await outcomeIs(name, last);
    await t.selectOption(sel, r.outcomes[0].name);
    await t.click("#t-save");
    await outcomeIs(name, r.outcomes[0].name);
  }
  console.log("  ok 基盤の確認（オフライン・開発用の操作パネル）");
  await browser.close();

  if (ext.length && flag !== "--no-ext") await runExtCases(ext, cal);
  else if (ext.length) console.log("拡張の手順は飛ばしました（ポート 8787 が使えない）");
  console.log("screens ok");
})().catch((e) => { console.error(e); process.exit(1); });
