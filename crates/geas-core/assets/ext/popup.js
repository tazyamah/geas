const $ = (s) => document.querySelector(s);
$("#h").textContent = self.GEAS_EXT.title;
$("#about").textContent = self.GEAS_EXT.about;
$("#url").placeholder = self.GEAS_EXT.server;
(async () => {
  const { serverUrl, readerConfig } = await chrome.storage.local.get(["serverUrl", "readerConfig"]);
  $("#url").value = serverUrl || "";
  $("#config").value = readerConfig ? JSON.stringify(readerConfig, null, 1) : "";
})();
$("#save").addEventListener("click", async () => {
  let cfg = {};
  try { cfg = $("#config").value.trim() ? JSON.parse($("#config").value) : {}; } catch { $("#msg").textContent = "読み取りの設定が JSON になっていません"; return; }
  await chrome.storage.local.set({ serverUrl: $("#url").value.trim(), readerConfig: cfg });
  $("#msg").textContent = "保存しました（ページを開き直すと反映されます）";
});
