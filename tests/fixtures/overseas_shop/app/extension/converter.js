// ASIN → JAN の変換は、この拡張（クライアント）の仕事。サーバーは JAN だけを受け取り、最後の1桁もサーバーが確かめる。
// 順番：(1) ページに書いてある JAN/EAN を読む　(2) 読めなければ、下の対応表（ポップアップで直せる）で ASIN から引く。
(function (root) {
  "use strict";

  // 試すための対応表（ASIN → JAN）。本物の変換（商品データの API など）に替えるときは、asinToJan だけを替える。
  const SAMPLE_TABLE = {
    B0SHOPTEA1: "4901234567894",
    B0SHOPKNF1: "4909876543214",
    B0SHOPBWL1: "4911112222333",
    B0SHOPVSE1: "4922223333443",
  };

  // JAN・EAN・UPC の最後の1桁が合っているか（サーバーと同じ決まり：右から2桁目を3倍、次を1倍…の合計が10の倍数）
  function validGtin(s) {
    if (!/^\d+$/.test(s) || ![8, 12, 13, 14].includes(s.length)) return false;
    let sum = 0;
    [...s].reverse().forEach((c, i) => (sum += Number(c) * (i % 2 ? 3 : 1)));
    return sum % 10 === 0;
  }

  function asinOf(url, doc) {
    const m = /\/(?:dp|gp\/product|gp\/aw\/d)\/([A-Z0-9]{10})(?:[/?]|$)/.exec(url);
    if (m) return m[1];
    const el = doc && doc.querySelector("input#ASIN");
    return el && /^[A-Z0-9]{10}$/.test(el.value) ? el.value : null;
  }

  // 商品情報の表から「JAN」「EAN」「GTIN」の行を探す
  function janFromPage(doc) {
    const rows = doc.querySelectorAll("tr, li");
    for (const r of rows) {
      const t = r.textContent.replace(/\s+/g, " ");
      if (!/(JAN|EAN|GTIN)/i.test(t)) continue;
      const m = /(\d{8,14})/.exec(t.replace(/[‎‏]/g, ""));
      if (m && validGtin(m[1])) return m[1];
    }
    return null;
  }

  function asinToJan(asin, table) {
    const jan = (table || SAMPLE_TABLE)[asin];
    return jan && validGtin(jan) ? jan : null;
  }

  function resolve(url, doc, table) {
    const asin = asinOf(url, doc);
    if (!asin) return { asin: null, jan: null };
    const jan = janFromPage(doc) || asinToJan(asin, table);
    return { asin, jan, source: jan && janFromPage(doc) === jan ? "page" : "table" };
  }

  root.ShopConverter = { SAMPLE_TABLE, validGtin, asinOf, janFromPage, asinToJan, resolve };
})(typeof self !== "undefined" ? self : this);
