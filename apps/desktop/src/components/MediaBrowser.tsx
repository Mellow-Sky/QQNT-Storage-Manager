import { useEffect, useRef, useState } from "react";
import {
  ChevronLeft,
  ChevronRight,
  ImageOff,
  LoaderCircle,
  Shield,
  Video,
  X,
} from "lucide-react";
import type { IndexedFile, MediaPreview } from "../types";
import { api, bytes, date, message, risks } from "../services/api";
import { Button } from "./ui/button";

// At most two IPC requests in flight; backend serializes image decoding.
const queue: Array<() => Promise<void>> = [];
let active = 0;
function dispatch() {
  while (active < 2 && queue.length) {
    const job = queue.shift()!;
    active++;
    void job().finally(() => {
      active--;
      dispatch();
    });
  }
}
export function MediaTile({
  file,
  sessionId,
  checked,
  onCheck,
  onOpen,
  disabled = false,
}: {
  file: IndexedFile;
  sessionId: string;
  checked: boolean;
  onCheck: () => void;
  onOpen: () => void;
  disabled?: boolean;
}) {
  const ref = useRef<HTMLElement>(null);
  const [preview, setPreview] = useState<MediaPreview | null>(null);
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  useEffect(() => {
    let live = true;
    let scheduled = false;
    setPreview(null);
    setError("");
    const observer = new IntersectionObserver(
      (entries) => {
        if (
          !entries.some((e) => e.isIntersecting) ||
          scheduled ||
          file.kind !== "Image"
        )
          return;
        scheduled = true;
        setLoading(true);
        queue.push(async () => {
          if (!live) return;
          try {
            const value = await api.preview(sessionId, file.relativePath);
            if (live) setPreview(value);
          } catch (e) {
            if (live) setError(message(e));
          } finally {
            if (live) setLoading(false);
          }
        });
        dispatch();
        observer.disconnect();
      },
      { rootMargin: "150px" },
    );
    if (ref.current) observer.observe(ref.current);
    return () => {
      live = false;
      observer.disconnect();
    };
  }, [file.relativePath, file.kind, sessionId]);
  return (
    <article ref={ref} className={`media-tile ${checked ? "selected" : ""}`}>
      <label className="media-check">
        <input
          type="checkbox"
          aria-label={`选择 ${file.relativePath}`}
          checked={checked}
          disabled={disabled}
          onChange={onCheck}
        />
      </label>
      <button
        className="media-visual"
        onClick={onOpen}
        title={error || file.relativePath}
        aria-label={`预览 ${file.relativePath}`}
      >
        {preview ? (
          <img
            src={preview.dataUrl}
            width={preview.width}
            height={preview.height}
            alt={file.relativePath.split("/").pop()}
            loading="lazy"
          />
        ) : file.kind === "Video" ? (
          <Video size={36} />
        ) : loading ? (
          <LoaderCircle className="spin" size={24} />
        ) : (
          <ImageOff size={30} />
        )}
        <span>
          {error ? "无法预览" : file.kind === "Video" ? "预览视频" : ""}
        </span>
      </button>
      <div className="media-caption">
        <strong title={file.relativePath}>
          {file.relativePath.split("/").pop()}
        </strong>
        <small>
          {bytes(file.size)} ·{" "}
          {file.modified
            ? new Date(file.modified * 1000).toISOString().slice(0, 10)
            : "日期未知"}
        </small>
        {file.risk === "Protected" && (
          <span className="protected-mark">
            <Shield size={11} />
            已保护
          </span>
        )}
      </div>
    </article>
  );
}

export function MediaViewer({
  files,
  index,
  sessionId,
  onIndex,
  onClose,
}: {
  files: IndexedFile[];
  index: number;
  sessionId: string;
  onIndex: (i: number) => void;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const file = files[index];
  const [preview, setPreview] = useState<MediaPreview | null>(null);
  const [url, setUrl] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [metadata, setMetadata] = useState("");
  useEffect(() => {
    const d = dialog.current;
    if (d && !d.open) d.showModal();
    return () => {
      d?.close();
    };
  }, []);
  useEffect(() => {
    if (!file) return;
    let live = true;
    setPreview(null);
    setUrl("");
    setError("");
    setMetadata("");
    setLoading(true);
    void (async () => {
      try {
        if (file.kind === "Video") {
          const source = await api.video(sessionId, file.relativePath);
          if (live) setUrl(source);
        } else {
          const value = await api.preview(sessionId, file.relativePath, true);
          if (live) setPreview(value);
        }
      } catch (e) {
        if (live) setError(message(e));
      } finally {
        if (live) setLoading(false);
      }
    })();
    return () => {
      live = false;
    };
  }, [file, sessionId]);
  if (!file) return null;
  return (
    <dialog
      ref={dialog}
      className="media-viewer"
      onCancel={onClose}
      onKeyDown={(e) => {
        if (e.key === "ArrowLeft" && index > 0) onIndex(index - 1);
        if (e.key === "ArrowRight" && index + 1 < files.length)
          onIndex(index + 1);
      }}
    >
      <header>
        <div>
          <strong>{file.relativePath}</strong>
          <small>
            {bytes(file.size)} · {date(file.modified)} · {risks[file.risk]}
          </small>
        </div>
        <Button
          variant="ghost"
          size="icon"
          aria-label="关闭预览"
          onClick={onClose}
        >
          <X size={20} />
        </Button>
      </header>
      <div className="viewer-stage">
        {loading ? (
          <LoaderCircle size={32} className="spin" />
        ) : error ? (
          <div>
            <ImageOff size={42} />
            <p>{error}</p>
          </div>
        ) : preview ? (
          <img src={preview.dataUrl} alt={file.relativePath} />
        ) : url ? (
          <video
            src={url}
            controls
            preload="metadata"
            onError={() =>
              setError("视频损坏或系统不支持该编码。可在资源管理器中定位原件。")
            }
            onLoadedMetadata={(e) => {
              const v = e.currentTarget;
              setMetadata(
                `${v.videoWidth} × ${v.videoHeight} · ${Number.isFinite(v.duration) ? v.duration.toFixed(1) + " 秒" : "时长未知"}`,
              );
            }}
          />
        ) : null}
      </div>
      <footer>
        <Button
          variant="outline"
          disabled={index === 0}
          onClick={() => onIndex(index - 1)}
        >
          <ChevronLeft size={16} />
          上一项
        </Button>
        <span>
          {index + 1}/{files.length} ·{" "}
          {preview
            ? `${preview.originalWidth} × ${preview.originalHeight} · 预览最长边 2048px`
            : metadata}
        </span>
        <Button
          variant="outline"
          disabled={index + 1 === files.length}
          onClick={() => onIndex(index + 1)}
        >
          下一项
          <ChevronRight size={16} />
        </Button>
      </footer>
    </dialog>
  );
}
