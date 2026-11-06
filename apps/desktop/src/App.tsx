import { useEffect, useState } from "react";
import {
  FolderOpen,
  LoaderCircle,
  Moon,
  RefreshCw,
  Settings,
  Square,
  Sun,
  X,
} from "lucide-react";
import { useManager } from "./hooks/useManager";
import { Button } from "./components/ui/button";
import { Files } from "./pages/Files";
import { SettingsPage } from "./pages/SettingsPage";
import { api, bytes, desktopAvailable } from "./services/api";
import appIcon from "./assets/app-icon.png";

export default function App() {
  const manager = useManager();
  const [filter, setFilter] = useState("all");
  const [settings, setSettings] = useState(false);
  const [processing, setProcessing] = useState(false);
  const [dark, setDark] = useState(
    () => localStorage.getItem("qqnt-theme") === "dark",
  );
  const [lowDisk, setLowDisk] = useState<number | null>(null);
  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark);
    localStorage.setItem("qqnt-theme", dark ? "dark" : "light");
  }, [dark]);
  useEffect(() => {
    if (!desktopAvailable || !manager.grantId) return;
    let live = true;
    const read = () =>
      void api
        .diskStatus(manager.grantId)
        .then((value) => {
          if (live) setLowDisk(value.lowDisk ? value.availableBytes : null);
        })
        .catch(() => {
          if (live) setLowDisk(null);
        });
    read();
    const timer = setInterval(read, 60000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [manager.grantId]);
  const running = manager.status?.state === "running";
  const mutating = [
    "cleaning",
    "restoring",
    "deleting",
    "configuring",
  ].includes(manager.status?.state ?? "");
  const busy = manager.busy || running || mutating || processing;
  const summary = manager.session?.summary;
  const rescan = async () => {
    const grant = manager.grants.find(
      (item) => item.path === manager.session?.root,
    );
    if (grant) await manager.start(grant.id);
  };
  return (
    <div className="simple-app">
      <header className="simple-header">
        <div className="simple-brand">
          <img
            className="app-icon"
            src={appIcon}
            alt=""
            width={34}
            height={34}
          />
          <strong>QQNT 存储管理</strong>
        </div>
        <div className="simple-header-actions">
          <Button
            size="icon"
            variant="ghost"
            aria-label="切换明暗主题"
            onClick={() => setDark(!dark)}
          >
            {dark ? <Sun size={18} /> : <Moon size={18} />}
          </Button>
          <Button
            size="icon"
            variant="ghost"
            aria-label="设置"
            disabled={busy}
            onClick={() => setSettings(true)}
          >
            <Settings size={18} />
          </Button>
        </div>
      </header>
      <main className="simple-main">
        <div className="simple-directory">
          <FolderOpen size={19} />
          {manager.grants.length ? (
            <select
              aria-label="数据目录"
              disabled={busy}
              value={manager.grantId}
              onChange={(event) => {
                const id = event.target.value;
                manager.setGrantId(id);
                void manager.start(id);
              }}
            >
              {manager.grants.map((grant) => (
                <option key={grant.id} value={grant.id}>
                  {grant.path}
                </option>
              ))}
            </select>
          ) : (
            <span className="directory-placeholder">
              {manager.session?.root ?? "选择 QQNT 数据目录"}
            </span>
          )}
          <Button
            size="sm"
            variant="outline"
            disabled={busy || !desktopAvailable}
            onClick={() => void manager.choose()}
          >
            选择目录
          </Button>
          {running ? (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => void manager.cancel()}
            >
              <Square size={13} />
              停止
            </Button>
          ) : (
            <Button
              size="icon"
              variant="ghost"
              title="重新扫描"
              aria-label="重新扫描"
              disabled={!manager.grantId || busy || !desktopAvailable}
              onClick={() => void manager.start()}
            >
              <RefreshCw size={17} />
            </Button>
          )}
        </div>
        {!desktopAvailable && (
          <div className="preview-note">
            浏览器预览 · 本地文件请使用桌面程序
          </div>
        )}
        {manager.error && (
          <div className="error-banner" role="alert">
            <span>{manager.error}</span>
            <Button
              size="icon"
              variant="ghost"
              aria-label="关闭错误提示"
              onClick={() => manager.setError("")}
            >
              <X size={16} />
            </Button>
          </div>
        )}
        {lowDisk !== null && (
          <div className="notice">磁盘剩余 {bytes(lowDisk)}</div>
        )}
        {running ? (
          <div className="simple-progress" role="status">
            <LoaderCircle size={15} className="spin" />
            扫描中 · {manager.status!.progress.files.toLocaleString()} 项 ·{" "}
            {bytes(manager.status!.progress.bytes)}
          </div>
        ) : (
          summary && (
            <div className="simple-summary">
              <span>
                {summary.progress.files.toLocaleString()} 个文件
                {manager.session?.status !== "completed" ? " · 部分结果" : ""}
              </span>
              <strong data-testid="total-size">
                {bytes(summary.progress.bytes)}
              </strong>
              <span>
                图片{" "}
                <strong data-testid="image-size">
                  {bytes(summary.byKind.Image?.bytes ?? 0)}
                </strong>
              </span>
              <span>
                视频{" "}
                <strong data-testid="video-size">
                  {bytes(summary.byKind.Video?.bytes ?? 0)}
                </strong>
              </span>
              <span>
                其他{" "}
                <strong data-testid="other-size">
                  {bytes(summary.byKind.Other?.bytes ?? 0)}
                </strong>
              </span>
            </div>
          )
        )}
        <nav className="file-tabs" aria-label="文件类型">
          {[
            ["all", "全部"],
            ["images", "图片"],
            ["videos", "视频"],
            ["large", "大文件"],
          ].map(([id, label]) => (
            <button
              key={id}
              className={filter === id ? "active" : ""}
              aria-current={filter === id ? "page" : undefined}
              disabled={mutating || processing}
              onClick={() => setFilter(id)}
            >
              {label}
            </button>
          ))}
        </nav>
        <Files
          key={`${filter}:${manager.session?.id ?? "empty"}`}
          session={manager.session}
          kind={
            filter === "images"
              ? "Image"
              : filter === "videos"
                ? "Video"
                : undefined
          }
          large={filter === "large"}
          onError={manager.setError}
          onDeleted={manager.applySession}
          deleteProgress={
            manager.status?.state === "deleting"
              ? (manager.status.deleteProgress ?? undefined)
              : undefined
          }
          onWorkingChange={setProcessing}
          disabled={busy}
        />
      </main>
      {settings && (
        <SettingsPage
          grantId={manager.grantId}
          sessions={manager.sessions}
          sessionId={manager.sessionId}
          onSession={manager.setSessionId}
          onClose={() => setSettings(false)}
          onDiscover={() => manager.choose(true)}
          onError={manager.setError}
          onRestored={rescan}
        />
      )}
    </div>
  );
}
