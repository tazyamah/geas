// ページから値を読む（汎用）。読み方は defs.js の read（Geas の screen の read）で決まり、このファイルはお店を知らない。
// 契約：self.GeasReader.read(url, document, config) → { subject（いまのページの何か。無ければ null）, value（問い合わせる値。読めなければ null）, source（どこから読んだか） }
// config.lookup（ポップアップで直せる）は、Geas の lookup に足す・上書きする対応表。
(function (root) {
  "use strict";
  const R = root.GEAS_EXT.read;

  // 入力に check gtin があるとき、JAN・EAN・UPC の最後の1桁が合っているか（サーバーと同じ決まり）
  function valid(s) {
    if (root.GEAS_EXT.input.check !== "gtin") return true;
    if (!/^\d+$/.test(s) || ![8, 12, 13, 14].includes(s.length)) return false;
    let sum = 0;
    [...s].reverse().forEach((c, i) => (sum += Number(c) * (i % 2 ? 3 : 1)));
    return sum % 10 === 0;
  }

  function subjectOf(url, doc) {
    for (const u of R.urls) {
      const m = new RegExp(u).exec(url);
      if (m) return m[1];
    }
    for (const [sel, pat] of R.elements) {
      const el = doc && doc.querySelector(sel);
      if (el && (!pat || new RegExp("^(?:" + pat + ")$").test(el.value))) return el.value;
    }
    return null;
  }

  // 表の行や一覧の項目のうち、見出しの語を含むものから、数字（8〜14桁）を読む
  function valueFromRows(doc) {
    if (!R.rows.length) return null;
    const re = new RegExp(R.rows.map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|"), "i");
    for (const r of doc.querySelectorAll("tr, li")) {
      const t = r.textContent.replace(/\s+/g, " ").replace(/[‎‏]/g, "");
      if (!re.test(t)) continue;
      const m = /(\d{8,14})/.exec(t);
      if (m && valid(m[1])) return m[1];
    }
    return null;
  }

  root.GeasReader = {
    read(url, doc, config) {
      const subject = subjectOf(url, doc);
      if (!subject) return { subject: null, value: null };
      const label = R.subject + " " + subject;
      const page = valueFromRows(doc);
      if (page) return { subject: label, value: page, source: "ページから" };
      const table = Object.assign({}, R.lookup, (config && config.lookup) || {});
      const v = table[subject];
      return v && valid(v) ? { subject: label, value: v, source: "対応表から" } : { subject: label, value: null, source: "" };
    },
  };
})(self);
