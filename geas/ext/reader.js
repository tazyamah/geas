// ページから値を読む（汎用）。読み方は defs.js の read（Geas の screen の read）で決まり、このファイルはお店を知らない。
// 契約：self.GeasReader.read(url, document, config) → { subject（いまのページの何か。無ければ null）, value（問い合わせる値。読めなければ null）, source（どこから読んだか） }
// config.lookup（ポップアップで直せる）は、Geas の lookup に足す・上書きする対応表。
(function (root) {
  "use strict";
  const R = root.GEAS_EXT.read;

  // 入力の型に結んだ変換（Geas の convert）の拒否の条件を、送る前に確かめる（サーバーが境界で断るのと同じ条件。defs.js の input.requires）。
  // 条件は式の木。数が大きすぎて正確に計算できないときは、ここでは決めずに通す（サーバーが決める）
  class Unsure extends Error {}
  function val(t, s, env) {
    const k = t[0], cs = [...s];
    const num = (x) => { if (!Number.isSafeInteger(x)) throw new Unsure(); return x; };
    switch (k) {
      case "num": case "bool": return t[1];
      case "var": return env[t[1]];
      case "len": return cs.length;
      case "only": return cs.every((c) => t[1].some(([a, b]) => a <= c.codePointAt(0) && c.codePointAt(0) <= b));
      case "digit": { const i = val(t[1], s, env); return i >= 0 && i < cs.length && /^[0-9]$/.test(cs[i]) ? Number(cs[i]) : 0; }
      case "not": return !val(t[1], s, env);
      case "if": return val(val(t[1], s, env) ? t[2] : t[3], s, env);
      case "and": return val(t[1], s, env) && val(t[2], s, env);
      case "or": return val(t[1], s, env) || val(t[2], s, env);
      case "sum": { let acc = 0; for (let i = 0; i < cs.length; i++) acc = num(acc + val(t[2], s, { ...env, [t[1]]: i })); return acc; }
    }
    const xs = t.slice(1).map((a) => val(a, s, env)), [a, b] = xs;
    switch (k) {
      case "<": return a < b; case ">": return a > b; case "<=": return a <= b; case ">=": return a >= b; case "==": return a === b; case "!=": return a !== b;
      case "+": return num(a + b); case "-": return num(a - b); case "*": return num(a * b);
      case "/": return Math.floor(a / b); case "%": return ((a % b) + b) % b;
      case "min": return Math.min(...xs); case "max": return Math.max(...xs); case "abs": return Math.abs(a);
    }
    throw new Unsure();
  }
  function valid(s) {
    try {
      return (root.GEAS_EXT.input.requires || []).every(([t]) => val(t, s, {}));
    } catch (e) {
      if (e instanceof Unsure) return true;
      throw e;
    }
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
