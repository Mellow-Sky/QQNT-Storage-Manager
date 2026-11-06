const { ipcMain, shell } = require("electron");
const fs = require("node:fs/promises");
const path = require("node:path");
const OPEN = "qqnt-storage-manager:open";
const STATUS = "qqnt-storage-manager:status";

async function status() {
  const info = { supported: process.platform === "win32", loaderVersion: globalThis.LiteLoader?.versions?.liteloader ?? "unknown", compatibility: "实验性 manifest v4 入口；未宣称适配所有 QQNT 版本", summary: null };
  if (!info.supported || !process.env.LOCALAPPDATA) return info;
  try {
    const location = path.join(process.env.LOCALAPPDATA, "io.github.qqnt-storage-manager", "public-summary.json");
    const stat = await fs.lstat(location);
    if (!stat.isFile() || stat.isSymbolicLink() || stat.size > 65536) return info;
    const value = JSON.parse(await fs.readFile(location, "utf8"));
    if (value.schema === 1 && Number.isSafeInteger(value.totalBytes) && value.totalBytes >= 0 && Number.isSafeInteger(value.files) && value.files >= 0 && Number.isSafeInteger(value.scannedAt)) {
      info.summary = { totalBytes: value.totalBytes, files: value.files, scannedAt: value.scannedAt, recognized: value.recognized === true };
    }
  } catch { /* No summary is normal before the standalone manager's first scan. */ }
  return info;
}

ipcMain.handle(STATUS, status);
ipcMain.handle(OPEN, async () => {
  if (process.platform !== "win32") throw new Error("此入口仅支持 Windows");
  // Fixed URI, no renderer-supplied path, executable, shell command, or arguments.
  await shell.openExternal("qqnt-storage-manager://open");
  return true;
});
