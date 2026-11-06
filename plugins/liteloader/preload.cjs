const { contextBridge, ipcRenderer } = require("electron");
contextBridge.exposeInMainWorld("qqntStorageManager", {
  getStatus: () => ipcRenderer.invoke("qqnt-storage-manager:status"),
  openManager: () => ipcRenderer.invoke("qqnt-storage-manager:open"),
});
