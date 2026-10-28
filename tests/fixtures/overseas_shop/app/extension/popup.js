const $ = (s) => document.querySelector(s);
(async () => {
  const { serverUrl, asinTable } = await chrome.storage.local.get(["serverUrl", "asinTable"]);
  $("#url").value = serverUrl || "";
  $("#table").value = asinTable ? JSON.stringify(asinTable, null, 1) : "";
})();
$("#save").addEventListener("click", async () => {
  let table = {};
  try { table = $("#table").value.trim() ? JSON.parse($("#table").value) : {}; } catch { $("#msg").textContent = "対応表が JSON になっていません"; return; }
  await chrome.storage.local.set({ serverUrl: $("#url").value.trim(), asinTable: table });
  $("#msg").textContent = "保存しました（ページを開き直すと反映されます）";
});
