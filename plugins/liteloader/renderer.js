export async function onSettingWindowCreated(view) {
  const section = document.createElement("section");
  section.style.cssText = "padding:24px;display:grid;gap:16px;line-height:1.7";
  const title = document.createElement("h2"); title.textContent = "QQNT Storage Manager";
  const description = document.createElement("p"); description.textContent = "扫描、审核、保护、隔离和恢复都由独立管理器执行。本插件只读取不含账号、路径和媒体内容的扫描摘要。";
  const compatibility = document.createElement("p");
  const summary = document.createElement("p"); summary.textContent = "正在读取独立管理器摘要…";
  const error = document.createElement("p"); error.style.color = "#d97706";
  const open = document.createElement("button"); open.textContent = "打开存储管理器"; open.style.cssText = "padding:9px 18px;border-radius:6px;cursor:pointer";
  const refresh = document.createElement("button"); refresh.textContent = "刷新扫描摘要"; refresh.style.cssText = open.style.cssText;
  const read = async () => {
    try {
      if (!window.qqntStorageManager) throw new Error("LiteLoader 预加载 API 不可用，请检查版本和插件加载状态。");
      const value = await window.qqntStorageManager.getStatus();
      compatibility.textContent = `LiteLoader ${value.loaderVersion} · ${value.compatibility}`;
      open.disabled = !value.supported;
      summary.textContent = value.summary ? `${(value.summary.totalBytes / 1024 ** 3).toFixed(2)} GiB · ${value.summary.files.toLocaleString()} 个文件 · ${new Date(value.summary.scannedAt * 1000).toLocaleString()} · ${value.summary.recognized ? "候选布局匹配" : "未知布局，只读"}` : "尚无扫描摘要。请先在独立管理器中选择目录并完成扫描。";
      error.textContent = "";
    } catch (reason) { error.textContent = String(reason); }
  };
  refresh.addEventListener("click", read);
  open.addEventListener("click", async () => {
    try { await window.qqntStorageManager.openManager(); error.textContent = ""; }
    catch { error.textContent = "无法打开。请安装 Windows 安装包以注册入口协议，或从开始菜单手动启动独立管理器。"; }
  });
  section.append(title, description, compatibility, summary, open, refresh, error); view.append(section); await read();
}
