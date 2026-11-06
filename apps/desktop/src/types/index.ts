export type MediaKind = "Image" | "Video" | "Other";
export type Risk = "SafeCandidate" | "ReviewRequired" | "Protected" | "Unknown";
export interface Detection {
  adapter: string;
  version: string;
  recognized: boolean;
  readOnly: boolean;
  evidence: string[];
}
export interface Grant {
  id: string;
  path: string;
  detection: Detection;
}
export interface Bucket {
  count: number;
  bytes: number;
}
export interface Progress {
  files: number;
  bytes: number;
  directories: number;
  skippedLinks: number;
  errors: number;
  elapsedMs: number;
  reusedFiles: number;
}
export interface PolicySettings {
  retentionDays: number | null;
  scopeLimitBytes: number | null;
  appCacheLimitBytes: number;
  lowDiskBytes: number;
  autoCleanup: boolean;
  autoScan: boolean;
  scanIntervalMinutes: number;
  diskCacheEnabled: boolean;
}
export interface PolicyReport {
  reviewCount: number;
  reviewBytes: number;
  reviewPaths: string[];
  scopeBytes: number;
  scopeOverLimit: boolean;
  availableBytes: number | null;
  lowDisk: boolean;
  cacheCount: number;
  cacheBytes: number;
  cacheReclaimableBytes: number;
  warning: string;
}
export interface Summary {
  progress: Progress;
  byKind: Partial<Record<MediaKind, Bucket>>;
  byRisk: Partial<Record<Risk, Bucket>>;
  byDirectory: Record<string, Bucket>;
  byMonth: Record<string, Bucket>;
  warnings: string[];
  cancelled: boolean;
}
export interface Session {
  id: string;
  root: string;
  started: number;
  finished: number | null;
  status: string;
  detection: Detection;
  summary: Summary | null;
}
export interface Status {
  deleteProgress?: DeleteProgress | null;
  sessionId: string | null;
  state: string;
  progress: Progress;
  summary: Summary | null;
  error: string | null;
}
export interface IndexedFile {
  relativePath: string;
  directory: string;
  size: number;
  created: number | null;
  modified: number | null;
  kind: MediaKind;
  risk: Risk;
  evidence: string[];
  userProtected: boolean;
  fingerprint?: { identity: string; size: number } | null;
}

export interface MediaPreview {
  dataUrl: string;
  width: number;
  height: number;
  originalWidth: number;
  originalHeight: number;
}

export type CleanupMode = "Conservative" | "Standard" | "Advanced";
export interface CleanupRule {
  minAgeDays?: number;
  minSize?: number;
  extensions: string[];
}
export interface CleanupPlan {
  id: string;
  sessionId: string;
  root: string;
  created: number;
  expires: number;
  mode: CleanupMode;
  mediaReview: boolean;
  files: number;
  bytes: number;
  state: string;
  entries: {
    file: IndexedFile;
    allowed: boolean;
    reason: string;
    digest: string | null;
  }[];
}
export interface QuarantineItem {
  id: string;
  planId: string;
  root: string;
  relativePath: string;
  storedName: string;
  size: number;
  quarantinedAt: number;
  state: string;
  error: string | null;
}
export interface CleanupOperation {
  status?: string;
  id: string;
  planId: string;
  action: string;
  started: number;
  finished: number | null;
  succeeded: number;
  failed: number;
  logicalBytes: number;
  files: {
    relativePath: string;
    state: string;
    reason: string;
    quarantineId: string | null;
  }[];
}
export type FileSort =
  "SizeDesc" | "SizeAsc" | "ModifiedDesc" | "ModifiedAsc" | "Path";
export interface FileQuery {
  sessionId: string;
  kind?: MediaKind;
  risk?: Risk;
  search?: string;
  minSize?: number;
  maxSize?: number;
  modifiedFrom?: number;
  modifiedTo?: number;
  sort?: FileSort;
  offset: number;
  limit: number;
}
export interface FilePage {
  total: number;
  protectedCount?: number;
  files: IndexedFile[];
}
export type Page =
  | "dashboard"
  | "storage"
  | "images"
  | "videos"
  | "large"
  | "cleanup"
  | "history"
  | "settings";

export interface DeleteResult {
  deleted: string[];
  retained: string[];
  failures: { path: string; reason: string }[];
  bytes: number;
  total: number;
  processed: number;
  deletedCount: number;
  retainedCount: number;
  failedCount: number;
  remaining: number;
  cancelled: boolean;
  stoppedReason: string | null;
  session: Session | null;
}
export interface DeleteProgress {
  total: number;
  processed: number;
  deleted: number;
  failed: number;
  bytes: number;
}
