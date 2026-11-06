import { useEffect, useRef, useState } from "react";
import {
  ChevronLeft,
  ChevronRight,
  File,
  FileImage,
  Film,
  FolderSearch,
  LayoutGrid,
  List,
  LoaderCircle,
  Search,
  Square,
  Shield,
  ShieldOff,
  SlidersHorizontal,
  Trash2,
  X,
} from "lucide-react";
import { api, bytes, date, kinds, message, risks } from "../services/api";
import type {
  DeleteResult,
  DeleteProgress,
  FilePage,
  FileQuery,
  FileSort,
  IndexedFile,
  MediaKind,
  Session,
} from "../types";
import { Button } from "../components/ui/button";
import { MediaTile, MediaViewer } from "../components/MediaBrowser";
const PAGE_SIZE = 200;

export function Files({
  session,
  kind,
  large,
  onError,
  onDeleted,
  onWorkingChange,
  deleteProgress,
  disabled = false,
}: {
  session?: Session;
  kind?: MediaKind;
  large?: boolean;
  onError: (s: string) => void;
  onDeleted?: (session: Session) => void;
  deleteProgress?: DeleteProgress;
  onWorkingChange?: (value: boolean) => void;
  disabled?: boolean;
}) {
  const [search, setSearch] = useState("");
  const [minSize, setMinSize] = useState(large ? "100" : "");
  const [maxSize, setMaxSize] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [sort, setSort] = useState<FileSort>("SizeDesc");
  const [offset, setOffset] = useState(0);
  const [page, setPage] = useState<FilePage>({ total: 0, files: [] });
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState<IndexedFile | null>(null);
  const [selection, setSelection] = useState<Map<string, boolean>>(new Map());
  const [selectAll, setSelectAll] = useState(false);
  const [excluded, setExcluded] = useState<Map<string, boolean>>(new Map());
  const selectRef = useRef<HTMLInputElement>(null);
  const [stopping, setStopping] = useState(false);
  const [grid, setGrid] = useState(!!kind);
  const [viewer, setViewer] = useState<number | null>(null);
  const [revision, setRevision] = useState(0);
  const [working, setWorking] = useState(false);
  const [filters, setFilters] = useState(false);
  const [result, setResult] = useState<DeleteResult | null>(null);
  const busy = working || disabled;
  const selectedCount = selectAll
    ? Math.max(0, page.total - excluded.size)
    : selection.size;
  const allProtected =
    selectedCount > 0 &&
    (selectAll
      ? (page.protectedCount ?? 0) -
          [...excluded.values()].filter(Boolean).length ===
        selectedCount
      : [...selection.values()].every(Boolean));
  const isSelected = (path: string) =>
    selectAll ? !excluded.has(path) : selection.has(path);
  const clearSelection = () => {
    setSelection(new Map());
    setSelectAll(false);
    setExcluded(new Map());
  };
  useEffect(() => {
    setSelection(new Map());
    setSelectAll(false);
    setExcluded(new Map());
    setViewer(null);
  }, [search, minSize, maxSize, from, to, session?.id]);
  useEffect(() => {
    setViewer(null);
  }, [offset]);
  useEffect(() => {
    if (selectRef.current)
      selectRef.current.indeterminate =
        selectedCount > 0 && selectedCount < page.total;
  }, [selectedCount, page.total]);
  useEffect(() => {
    setOffset(0);
  }, [search, minSize, maxSize, from, to, sort, session?.id]);
  const toggle = (file: IndexedFile) => {
    const set = selectAll ? setExcluded : setSelection;
    set((current) => {
      const next = new Map(current);
      if (next.has(file.relativePath)) next.delete(file.relativePath);
      else next.set(file.relativePath, file.userProtected);
      return next;
    });
  };
  const query: FileQuery = {
    sessionId: session?.id ?? "",
    kind,
    search,
    sort,
    offset,
    limit: PAGE_SIZE,
    minSize:
      minSize === "" ? undefined : Math.floor(Number(minSize) * 1024 ** 2),
    maxSize:
      maxSize === "" ? undefined : Math.floor(Number(maxSize) * 1024 ** 2),
    modifiedFrom: from ? Date.parse(`${from}T00:00:00Z`) / 1000 : undefined,
    modifiedTo: to ? Date.parse(`${to}T00:00:00Z`) / 1000 + 86400 : undefined,
  };
  const protect = async () => {
    if (!session) return;
    setWorking(true);
    onWorkingChange?.(true);
    try {
      if (selectAll)
        await api.protectMatching(query, [...excluded.keys()], !allProtected);
      else await api.protect(session.id, [...selection.keys()], !allProtected);
      setRevision((v) => v + 1);
      clearSelection();
    } catch (error) {
      onError(message(error));
    } finally {
      setWorking(false);
      onWorkingChange?.(false);
    }
  };
  const deleteSelected = async () => {
    if (!session || !selectedCount || busy) return;
    setWorking(true);
    onWorkingChange?.(true);
    setResult(null);
    setStopping(false);
    try {
      const outcome = selectAll
        ? await api.deleteMatching(query, [...excluded.keys()])
        : await api.deleteFiles(session.id, [...selection.keys()]);
      setLoading(true);
      setPage((current) => ({
        ...current,
        total: Math.max(
          0,
          current.total - outcome.deletedCount - outcome.retainedCount,
        ),
        files: [],
      }));
      clearSelection();
      setResult(outcome);
      if (outcome.session) onDeleted?.(outcome.session);
      setRevision((value) => value + 1);
    } catch (error) {
      onError(message(error));
      setRevision((value) => value + 1);
      try {
        onDeleted?.(await api.session(session.id));
      } catch {
        /* Keep the original failure visible. */
      }
    } finally {
      setWorking(false);
      onWorkingChange?.(false);
    }
  };
  const invalidSize =
    [minSize, maxSize].some(
      (v) => v !== "" && (!Number.isFinite(Number(v)) || Number(v) < 0),
    ) ||
    (minSize !== "" && maxSize !== "" && Number(minSize) > Number(maxSize));
  const invalidDate = from !== "" && to !== "" && from > to;
  useEffect(() => {
    if (!session || invalidSize || invalidDate) {
      setPage({ total: 0, files: [] });
      return;
    }
    let active = true;
    setLoading(true);
    const timer = setTimeout(() => {
      void api
        .files({
          sessionId: session.id,
          kind,
          search,
          sort,
          offset,
          limit: PAGE_SIZE,
          minSize:
            minSize === ""
              ? undefined
              : Math.floor(Number(minSize) * 1024 ** 2),
          maxSize:
            maxSize === ""
              ? undefined
              : Math.floor(Number(maxSize) * 1024 ** 2),
          modifiedFrom: from
            ? Date.parse(`${from}T00:00:00Z`) / 1000
            : undefined,
          modifiedTo: to
            ? Date.parse(`${to}T00:00:00Z`) / 1000 + 86400
            : undefined,
        })
        .then((data) => {
          if (active) {
            if (offset > 0 && offset >= data.total) {
              setOffset(
                Math.max(
                  0,
                  Math.floor((data.total - 1) / PAGE_SIZE) * PAGE_SIZE,
                ),
              );
            } else setPage(data);
          }
        })
        .catch((error) => {
          if (active) onError(message(error));
        })
        .finally(() => {
          if (active) setLoading(false);
        });
    }, 180);
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [
    session,
    kind,
    search,
    minSize,
    maxSize,
    from,
    to,
    sort,
    offset,
    onError,
    invalidSize,
    invalidDate,
    revision,
  ]);
  const reveal = async (file: IndexedFile) => {
    if (session)
      try {
        await api.reveal(session.id, file.relativePath);
      } catch (error) {
        onError(message(error));
      }
  };
  return (
    <>
      <div className="files-heading">
        <h1>
          {kind === "Image"
            ? "图片"
            : kind === "Video"
              ? "视频"
              : large
                ? "大文件"
                : "文件"}
        </h1>
        <span>{session ? `${page.total.toLocaleString()} 项` : "未扫描"}</span>
      </div>
      {result && (
        <div className="delete-result" role="status">
          <span>
            {result.deletedCount
              ? `已删除 ${result.deletedCount} 项 · ${bytes(result.bytes)}`
              : result.failures[0]?.reason.includes("QQ")
                ? "请先退出 QQ 后重试"
                : "没有文件被删除"}
            {result.retainedCount
              ? ` · ${result.retainedCount} 项保留在恢复区`
              : ""}
          </span>
          {result.remaining > 0 && (
            <span>
              {result.cancelled ? "已停止" : (result.stoppedReason ?? "未完成")}{" "}
              · 剩余 {result.remaining.toLocaleString()} 项
            </span>
          )}
          {result.failedCount > 0 && (
            <details>
              <summary>{result.failedCount} 项未删除</summary>
              {result.failures.map((item) => (
                <p key={item.path}>
                  <code>{item.path}</code>：{item.reason}
                </p>
              ))}
            </details>
          )}
          <Button
            size="icon"
            variant="ghost"
            aria-label="关闭删除结果"
            onClick={() => setResult(null)}
          >
            <X size={15} />
          </Button>
        </div>
      )}
      <section
        className="card file-card simple-file-card"
        data-session-id={session?.id}
      >
        <div className="file-actions">
          <label className="select-all">
            <input
              ref={selectRef}
              type="checkbox"
              aria-label="全选筛选结果"
              title="全选当前筛选结果（包含其他页）"
              disabled={busy || !page.files.length}
              checked={page.total > 0 && selectedCount === page.total}
              onChange={(event) => {
                setSelection(new Map());
                setExcluded(new Map());
                setSelectAll(event.target.checked);
              }}
            />
            <span>
              {selectedCount
                ? `已选 ${selectedCount.toLocaleString()} 项`
                : "全选"}
            </span>
          </label>
          <label className="compact-search">
            <Search size={16} />
            <input
              aria-label="搜索文件"
              placeholder="搜索文件"
              value={search}
              disabled={busy}
              onChange={(event) => setSearch(event.target.value)}
            />
          </label>
          <select
            aria-label="排序"
            value={sort}
            disabled={busy}
            onChange={(event) => setSort(event.target.value as FileSort)}
          >
            <option value="SizeDesc">大小 ↓</option>
            <option value="SizeAsc">大小 ↑</option>
            <option value="ModifiedDesc">最新优先</option>
            <option value="ModifiedAsc">最早优先</option>
            <option value="Path">名称</option>
          </select>
          <Button
            size="icon"
            variant="ghost"
            aria-label="筛选"
            aria-expanded={filters}
            onClick={() => setFilters(!filters)}
          >
            <SlidersHorizontal size={17} />
          </Button>
          <Button
            size="icon"
            variant="ghost"
            aria-label={grid ? "切换列表" : "切换网格"}
            onClick={() => setGrid(!grid)}
          >
            {grid ? <List size={17} /> : <LayoutGrid size={17} />}
          </Button>
          {selectedCount > 0 && (
            <Button
              size="icon"
              variant="ghost"
              disabled={busy}
              title={allProtected ? "取消保护" : "保护"}
              aria-label={allProtected ? "取消保护" : "保护"}
              onClick={() => void protect()}
            >
              {allProtected ? <ShieldOff size={17} /> : <Shield size={17} />}
            </Button>
          )}
          <Button
            size="sm"
            className="delete-button"
            disabled={!selectedCount || busy || loading}
            title="永久删除所选文件"
            onClick={() => void deleteSelected()}
          >
            {working ? (
              <LoaderCircle size={15} className="spin" />
            ) : (
              <Trash2 size={15} />
            )}
            删除
          </Button>
        </div>
        {working && deleteProgress && (
          <div className="bulk-progress" role="status">
            <progress
              max={Math.max(deleteProgress.total, 1)}
              value={deleteProgress.processed}
            />
            <span>
              {deleteProgress.processed.toLocaleString()} /{" "}
              {deleteProgress.total.toLocaleString()} · 已删除{" "}
              {deleteProgress.deleted.toLocaleString()} 项
            </span>
            <Button
              size="sm"
              variant="ghost"
              disabled={stopping}
              onClick={() => {
                setStopping(true);
                void api
                  .cancelDelete()
                  .catch((error) => onError(message(error)));
              }}
            >
              <Square size={12} />
              {stopping ? "正在停止…" : "停止"}
            </Button>
          </div>
        )}
        {selectedCount > 0 && (
          <div className="selection-note">删除后不可恢复</div>
        )}
        {filters && (
          <div className="filters secondary-filters compact-filters">
            <label>
              最小 MiB
              <input
                type="number"
                min="0"
                value={minSize}
                disabled={busy}
                onChange={(event) => setMinSize(event.target.value)}
              />
            </label>
            <label>
              最大 MiB
              <input
                type="number"
                min="0"
                value={maxSize}
                disabled={busy}
                onChange={(event) => setMaxSize(event.target.value)}
              />
            </label>
            <label>
              日期从
              <input
                type="date"
                value={from}
                disabled={busy}
                onChange={(event) => setFrom(event.target.value)}
              />
            </label>
            <label>
              至
              <input
                type="date"
                value={to}
                disabled={busy}
                onChange={(event) => setTo(event.target.value)}
              />
            </label>
          </div>
        )}
        {(invalidSize || invalidDate) && (
          <div className="notice">筛选范围无效</div>
        )}
        {grid ? (
          <div className="media-grid">
            {page.files.map((file, i) => (
              <MediaTile
                key={file.relativePath}
                file={file}
                sessionId={session?.id ?? ""}
                checked={isSelected(file.relativePath)}
                disabled={busy}
                onCheck={() => toggle(file)}
                onOpen={() =>
                  file.kind === "Other" ? setSelected(file) : setViewer(i)
                }
              />
            ))}
          </div>
        ) : (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>名称</th>
                  <th>大小</th>
                  <th>修改时间</th>
                  <th>状态</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {page.files.map((file) => {
                  const Icon =
                    file.kind === "Image"
                      ? FileImage
                      : file.kind === "Video"
                        ? Film
                        : File;
                  return (
                    <tr key={file.relativePath}>
                      <td>
                        <input
                          className="row-check"
                          type="checkbox"
                          disabled={busy}
                          aria-label={`选择 ${file.relativePath}`}
                          checked={isSelected(file.relativePath)}
                          onChange={() => toggle(file)}
                        />
                        <button
                          className="file-name"
                          onClick={() =>
                            file.kind === "Other"
                              ? setSelected(file)
                              : setViewer(page.files.indexOf(file))
                          }
                          title={file.relativePath}
                        >
                          <span className="file-type-icon">
                            <Icon size={19} />
                          </span>
                          <span>
                            <strong>
                              {file.relativePath.split("/").pop()}
                            </strong>
                            <small>{file.relativePath}</small>
                          </span>
                        </button>
                      </td>
                      <td className="tabular">{bytes(file.size)}</td>
                      <td className="file-time">{date(file.modified)}</td>
                      <td>
                        {file.risk === "Protected" ? (
                          <span
                            className="protected-mark"
                            title={file.evidence.join("\n")}
                          >
                            <Shield size={13} />
                            已保护
                          </span>
                        ) : file.risk === "Unknown" ? (
                          <span className="muted">未知</span>
                        ) : null}
                      </td>
                      <td>
                        <Button
                          variant="ghost"
                          size="icon"
                          aria-label={`定位 ${file.relativePath}`}
                          title="打开文件位置"
                          onClick={() => void reveal(file)}
                        >
                          <FolderSearch size={16} />
                        </Button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
        {!page.files.length && (
          <div className="list-empty">
            <FolderSearch size={34} />
            <h3>
              {!session
                ? "选择目录，开始浏览"
                : loading
                  ? "加载中…"
                  : "没有匹配的文件"}
            </h3>
          </div>
        )}
        {page.total > PAGE_SIZE && (
          <div className="pagination">
            <span>
              {Math.floor(offset / PAGE_SIZE) + 1} /{" "}
              {Math.max(1, Math.ceil(page.total / PAGE_SIZE))}
            </span>
            <div>
              <Button
                size="icon"
                variant="ghost"
                aria-label="上一页"
                disabled={offset === 0 || loading}
                onClick={() => setOffset(Math.max(0, offset - PAGE_SIZE))}
              >
                <ChevronLeft size={16} />
              </Button>
              <Button
                size="icon"
                variant="ghost"
                aria-label="下一页"
                disabled={offset + PAGE_SIZE >= page.total || loading}
                onClick={() => setOffset(offset + PAGE_SIZE)}
              >
                <ChevronRight size={16} />
              </Button>
            </div>
          </div>
        )}
      </section>
      {viewer !== null && session && (
        <MediaViewer
          files={page.files}
          index={viewer}
          sessionId={session.id}
          onIndex={setViewer}
          onClose={() => setViewer(null)}
        />
      )}
      {selected && (
        <div className="modal-backdrop" onClick={() => setSelected(null)}>
          <section
            className="detail-modal card"
            role="dialog"
            aria-modal="true"
            aria-label="文件详情"
            onClick={(event) => event.stopPropagation()}
            onKeyDown={(event) => {
              if (event.key === "Escape") setSelected(null);
            }}
            tabIndex={-1}
          >
            <div className="section-title">
              <h2>文件详情</h2>
              <Button
                autoFocus
                variant="ghost"
                size="icon"
                aria-label="关闭详情"
                onClick={() => setSelected(null)}
              >
                <X size={18} />
              </Button>
            </div>
            <code>{selected.relativePath}</code>
            <dl>
              <dt>大小</dt>
              <dd>{bytes(selected.size)}</dd>
              <dt>修改时间</dt>
              <dd>{date(selected.modified)}</dd>
              <dt>类型</dt>
              <dd>{kinds[selected.kind]}</dd>
              <dt>状态</dt>
              <dd>{risks[selected.risk]}</dd>
            </dl>
            <details>
              <summary>详细信息</summary>
              <ul>
                {selected.evidence.map((item) => (
                  <li key={item}>{item}</li>
                ))}
              </ul>
            </details>
            <Button variant="outline" onClick={() => void reveal(selected)}>
              <FolderSearch size={16} />
              打开文件位置
            </Button>
          </section>
        </div>
      )}
    </>
  );
}
