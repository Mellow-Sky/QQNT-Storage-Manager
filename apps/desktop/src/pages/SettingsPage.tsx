import { useEffect, useRef, useState } from "react";
import { ChevronLeft, ChevronRight, X } from "lucide-react";
import { Button } from "../components/ui/button";
import { api, bytes, date, desktopAvailable, message } from "../services/api";
import type { PolicySettings, QuarantineItem, Session } from "../types";

export function SettingsPage({
  grantId,
  sessions,
  sessionId,
  onSession,
  onClose,
  onDiscover,
  onError,
  onRestored,
}: {
  grantId: string;
  sessions: Session[];
  sessionId: string;
  onSession: (id: string) => void;
  onClose: () => void;
  onDiscover: () => Promise<unknown>;
  onError: (text: string) => void;
  onRestored: () => Promise<unknown>;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const [policy, setPolicy] = useState<PolicySettings | null>(null);
  const [items, setItems] = useState<QuarantineItem[]>([]);
  const [offset, setOffset] = useState(0);
  const [total, setTotal] = useState(0);
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState("");
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    const dialog = ref.current;
    dialog?.showModal();
    return () => dialog?.close();
  }, []);
  useEffect(() => {
    if (!desktopAvailable) return;
    let live = true;
    void api
      .policySettings()
      .then((value) => {
        if (live) setPolicy(value);
      })
      .catch((error) => onError(message(error)));
    return () => {
      live = false;
    };
  }, [onError]);
  useEffect(() => {
    if (!desktopAvailable) return;
    let live = true;
    void api
      .quarantine(offset)
      .then((page) => {
        if (live) {
          setItems(page.items.filter((item) => item.state === "quarantined"));
          setTotal(page.total);
        }
      })
      .catch((error) => onError(message(error)));
    return () => {
      live = false;
    };
  }, [offset, revision, onError]);
  const perform = async (action: () => Promise<void>) => {
    setBusy(true);
    setFeedback("");
    try {
      await action();
    } catch (error) {
      setFeedback(message(error));
    } finally {
      setBusy(false);
    }
  };
  const patch = (value: Partial<PolicySettings>) =>
    setPolicy((current) => (current ? { ...current, ...value } : null));
  return (
    <dialog
      ref={ref}
      className="preferences-dialog"
      onCancel={(event) => {
        if (busy) event.preventDefault();
        else onClose();
      }}
    >
      <div className="preferences-title">
        <h2>设置</h2>
        <Button
          autoFocus
          size="icon"
          variant="ghost"
          aria-label="关闭设置"
          disabled={busy}
          onClick={onClose}
        >
          <X size={18} />
        </Button>
      </div>
      <Button
        variant="outline"
        disabled={busy || !desktopAvailable}
        onClick={() =>
          void perform(async () => {
            await onDiscover();
            onClose();
          })
        }
      >
        查找 QQNT 目录
      </Button>
      {policy && (
        <section className="preferences-section">
          <label className="preference-line">
            <span>定时扫描</span>
            <input
              type="checkbox"
              checked={policy.autoScan}
              disabled={busy}
              onChange={(event) => patch({ autoScan: event.target.checked })}
            />
          </label>
          {policy.autoScan && (
            <label className="preference-line">
              <span>扫描间隔（分钟）</span>
              <input
                type="number"
                min="5"
                max="1440"
                value={policy.scanIntervalMinutes}
                disabled={busy}
                onChange={(event) =>
                  patch({ scanIntervalMinutes: Number(event.target.value) })
                }
              />
            </label>
          )}
          <label className="preference-line">
            <span>保存预览缓存</span>
            <input
              type="checkbox"
              checked={policy.diskCacheEnabled}
              disabled={busy}
              onChange={(event) =>
                patch({ diskCacheEnabled: event.target.checked })
              }
            />
          </label>
          <label className="preference-line">
            <span>自动清理预览缓存</span>
            <input
              type="checkbox"
              checked={policy.autoCleanup}
              disabled={busy}
              onChange={(event) => patch({ autoCleanup: event.target.checked })}
            />
          </label>
          <details className="preferences-more">
            <summary>更多选项</summary>
            <label className="preference-line">
              <span>预览缓存保留（天）</span>
              <input
                type="number"
                min="1"
                max="36500"
                placeholder="永久"
                value={policy.retentionDays ?? ""}
                disabled={busy}
                onChange={(event) =>
                  patch({
                    retentionDays: event.target.value
                      ? Number(event.target.value)
                      : null,
                  })
                }
              />
            </label>
            <label className="preference-line">
              <span>预览缓存上限（MiB）</span>
              <input
                type="number"
                min="1"
                max="10240"
                value={policy.appCacheLimitBytes / 1024 ** 2}
                disabled={busy}
                onChange={(event) =>
                  patch({
                    appCacheLimitBytes: Number(event.target.value) * 1024 ** 2,
                  })
                }
              />
            </label>
            <label className="preference-line">
              <span>磁盘提醒阈值（GiB）</span>
              <input
                type="number"
                min="0"
                step="0.5"
                value={policy.lowDiskBytes / 1024 ** 3}
                disabled={busy}
                onChange={(event) =>
                  patch({
                    lowDiskBytes: Number(event.target.value) * 1024 ** 3,
                  })
                }
              />
            </label>
          </details>
          <Button
            size="sm"
            disabled={busy}
            onClick={() =>
              void perform(async () => {
                if (policy.autoCleanup) await api.enableCleanup(true);
                await api.savePolicy(policy);
                setFeedback("已保存");
              })
            }
          >
            保存
          </Button>
        </section>
      )}
      {sessions.length > 0 && (
        <details className="preferences-section">
          <summary>扫描历史</summary>
          <select
            className="history-select"
            aria-label="扫描历史"
            value={sessionId}
            onChange={(event) => {
              onSession(event.target.value);
              onClose();
            }}
          >
            {sessions.map((session) => (
              <option key={session.id} value={session.id}>
                {date(session.started)} · {session.root}
              </option>
            ))}
          </select>
        </details>
      )}
      <details className="preferences-section" id="recovery">
        <summary>恢复区{items.length ? ` · ${items.length}` : ""}</summary>
        {items.map((item) => (
          <div className="recovery-row" key={item.id}>
            <div>
              <strong>{item.relativePath}</strong>
              <small title={item.root}>
                {bytes(item.size)} · {item.root}
              </small>
            </div>
            <Button
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() =>
                void perform(async () => {
                  const result = await api.restoreFiles([item.id]);
                  if (result.failed)
                    throw new Error(
                      result.files.find((file) => file.state === "failed")
                        ?.reason ?? "恢复失败",
                    );
                  setRevision((value) => value + 1);
                  setFeedback("已恢复");
                  await onRestored();
                })
              }
            >
              恢复
            </Button>
          </div>
        ))}
        {!items.length && <p className="muted">暂无文件</p>}
        {total > 100 && (
          <div className="pagination">
            <Button
              size="icon"
              variant="ghost"
              aria-label="上一页恢复记录"
              disabled={busy || offset === 0}
              onClick={() => setOffset(offset - 100)}
            >
              <ChevronLeft size={16} />
            </Button>
            <Button
              size="icon"
              variant="ghost"
              aria-label="下一页恢复记录"
              disabled={busy || offset + 100 >= total}
              onClick={() => setOffset(offset + 100)}
            >
              <ChevronRight size={16} />
            </Button>
          </div>
        )}
        <Button
          size="sm"
          variant="ghost"
          disabled={!grantId || busy}
          onClick={() =>
            void perform(async () => {
              const count = await api.importQuarantine(grantId);
              setRevision((value) => value + 1);
              setFeedback(`已导入 ${count} 项`);
            })
          }
        >
          导入恢复记录
        </Button>
      </details>
      {feedback && (
        <p className="preferences-feedback" role="status">
          {feedback}
        </p>
      )}
    </dialog>
  );
}
