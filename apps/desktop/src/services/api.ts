import { convertFileSrc, invoke, isTauri } from "@tauri-apps/api/core";
import type { FilePage, FileQuery, Grant, Session, Status } from "../types";
export const desktopAvailable = isTauri();
function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!desktopAvailable)
    return Promise.reject(new Error("请在桌面程序中使用此功能。"));
  return invoke<T>(command, args);
}
export const api = {
  session: (sessionId: string) => call<Session>("get_session", { sessionId }),
  deleteMatching: (query: FileQuery, excludedPaths: string[]) =>
    call<import("../types").DeleteResult>("delete_matching", {
      query,
      excludedPaths,
    }),
  protectMatching: (
    query: FileQuery,
    excludedPaths: string[],
    protectedValue: boolean,
  ) =>
    call<void>("protect_matching", {
      query,
      excludedPaths,
      protected: protectedValue,
    }),
  cancelDelete: () => call<void>("cancel_delete"),
  deleteFiles: (sessionId: string, paths: string[]) =>
    call<import("../types").DeleteResult>("delete_files", { sessionId, paths }),
  restoreFiles: (ids: string[]) =>
    call<import("../types").CleanupOperation>("restore_files", { ids }),
  importQuarantine: (grantId: string) =>
    call<number>("import_quarantine", { grantId }),
  policySettings: () =>
    call<import("../types").PolicySettings>("policy_settings"),
  savePolicy: (settings: import("../types").PolicySettings) =>
    call<void>("save_policy", { settings }),
  policyReport: (sessionId: string) =>
    call<import("../types").PolicyReport>("policy_report", { sessionId }),
  cleanAppCache: (all: boolean, confirmation: string) =>
    call<import("../types").CleanupOperation>("clean_app_cache", {
      all,
      confirmation,
    }),
  diskStatus: (grantId: string) =>
    call<{ availableBytes: number; thresholdBytes: number; lowDisk: boolean }>(
      "disk_status",
      { grantId },
    ),
  cleanupEnabled: () => call<boolean>("cleanup_enabled"),
  enableCleanup: (enabled: boolean) =>
    call<void>("enable_cleanup", { enabled }),
  plan: (request: {
    sessionId: string;
    paths: string[];
    mode: import("../types").CleanupMode;
    mediaReview: boolean;
    rules?: import("../types").CleanupRule;
  }) => call<import("../types").CleanupPlan>("plan_cleanup", { request }),
  execute: (planId: string, confirmation: string) =>
    call<import("../types").CleanupOperation>("execute_cleanup", {
      planId,
      confirmation,
    }),
  quarantine: (offset = 0) =>
    call<{ total: number; items: import("../types").QuarantineItem[] }>(
      "quarantine_page",
      { offset },
    ),
  quarantineAction: (
    ids: string[],
    action: "restore" | "delete",
    confirmation: string,
  ) =>
    call<import("../types").CleanupOperation>("quarantine_action", {
      ids,
      action,
      confirmation,
    }),
  cleanupHistory: () =>
    call<import("../types").CleanupOperation[]>("cleanup_history"),
  choose: () => call<Grant[]>("choose_directory"),
  discover: () => call<Grant[]>("discover_directories"),
  start: (grantId: string) => call<string>("start_scan", { grantId }),
  cancel: (sessionId: string) => call<void>("cancel_scan", { sessionId }),
  status: () => call<Status>("scan_status"),
  sessions: () => call<Session[]>("list_sessions"),
  files: (query: FileQuery) => call<FilePage>("list_files", { query }),
  reveal: (sessionId: string, relativePath: string) =>
    call<void>("reveal_file", { sessionId, relativePath }),
  preview: (sessionId: string, relativePath: string, full = false) =>
    call<import("../types").MediaPreview>("media_preview", {
      sessionId,
      relativePath,
      full,
    }),
  video: async (sessionId: string, relativePath: string) =>
    convertFileSrc(
      await call<string>("video_token", { sessionId, relativePath }),
      "qqnt-media",
    ),
  protect: (sessionId: string, paths: string[], protectedValue: boolean) =>
    call<void>("protect_files", {
      sessionId,
      paths,
      protected: protectedValue,
    }),
};
export function message(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
export function bytes(value: number) {
  if (value === 0) return "0 B";
  const i = Math.min(Math.floor(Math.log(value) / Math.log(1024)), 4);
  return `${(value / 1024 ** i).toLocaleString("zh-CN", { maximumFractionDigits: i ? 2 : 0 })} ${["B", "KiB", "MiB", "GiB", "TiB"][i]}`;
}
export function date(value: number | null | undefined) {
  return value == null ? "—" : new Date(value * 1000).toLocaleString("zh-CN");
}
export const kinds = { Image: "图片", Video: "视频", Other: "其他文件" };
export const risks = {
  SafeCandidate: "低风险候选",
  ReviewRequired: "需要审核",
  Protected: "受保护",
  Unknown: "未知 · 只读",
};
export const states: Record<string, string> = {
  idle: "等待扫描",
  running: "扫描中",
  completed: "已完成",
  cancelled: "已取消 · 部分结果",
  interrupted: "意外中断 · 部分结果",
  failed: "失败 · 部分结果",
};
