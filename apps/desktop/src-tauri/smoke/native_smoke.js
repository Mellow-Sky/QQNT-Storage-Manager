(async () => {
  const until = async (check, label) => {
    const deadline = performance.now() + 25000;
    while (performance.now() < deadline) {
      const value = await check();
      if (value) return value;
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    throw new Error(`Timed out: ${label}`);
  };
  const assert = (condition, message) => { if (!condition) throw new Error(message); };
  try {
    await until(() => document.querySelector("h1")?.textContent === "文件", "React page render");
    const invoke = (command, args) => window.__TAURI_INTERNALS__.invoke(command, args).catch((error) => { throw new Error(`${command}: ${error}`); });
    assert((await invoke("list_sessions")).length === 0, "temporary index should begin empty");
    const scan = async ({ files = 3, bytes = 1883 } = {}) => {
      const id = await invoke("start_scan", { grantId: __GRANT_ID_JSON__ });
      const status = await until(async () => {
        const next = await invoke("scan_status");
        return next.state !== "running" ? next : null;
      }, "native scan completion");
      assert(status.state === "completed", "native scan failed");
      assert(status.progress.files === files && status.progress.bytes === bytes, "incorrect filesystem totals");
      return { id, status };
    };
    const { id: sessionId } = await scan();
    const page = await invoke("list_files", { query: { sessionId, limit: 60, offset: 0 } });
    assert(page.total === 3, "incorrect indexed file page");
    const filtered = await invoke("list_files", { query: { sessionId, kind: "Image", minSize: 134, maxSize: 134, limit: 60 } });
    assert(filtered.total === 1 && filtered.files[0].relativePath === "Pic/中文.png", "native Unicode/type/size filtering failed");
    assert(page.files.find((file) => file.relativePath === "nt_db/chat.db")?.risk === "Protected", "database protection failed");
    const rendered = await until(() => {
      const values = ["total-size", "image-size", "video-size", "other-size"].map((id) => document.querySelector(`[data-testid="${id}"]`)?.textContent);
      return JSON.stringify(values) === JSON.stringify(["1.84 KiB", "134 B", "1.69 KiB", "18 B"]) ? values : null;
    }, "frontend statistics update from Rust");
    const preview = await invoke("media_preview", { sessionId, relativePath: "Pic/中文.png", full: true });
    assert(preview.originalWidth === 64 && preview.originalHeight === 32 && preview.dataUrl.startsWith("data:image/jpeg;base64,"), "bounded real image decode failed");
    await invoke("protect_files", { sessionId, paths: ["Pic/中文.png"], protected: true });
    const protectedPage = await invoke("list_files", { query: { sessionId, risk: "Protected", limit: 60 } });
    assert(protectedPage.total === 2 && protectedPage.files.some((f) => f.userProtected), "manual protection overlay failed");
    await invoke("protect_files", { sessionId, paths: ["Pic/中文.png"], protected: false });
    const plan = await invoke("plan_cleanup", { request: { sessionId, paths: ["Pic/中文.png", "nt_db/chat.db"], mode: "Standard", mediaReview: true } });
    assert(plan.files === 1 && plan.entries.find((e) => e.file.relativePath === "nt_db/chat.db").allowed === false, "dry run must exclude databases");
    assert(await invoke("cleanup_enabled") === false, "cleanup must default off");
    let blocked = false;
    try { await invoke("execute_cleanup", { planId: plan.id, confirmation: "隔离 1 个文件" }); } catch { blocked = true; }
    assert(blocked, "default read-only switch failed");
    const token = await invoke("video_token", { sessionId, relativePath: "Video/模拟.mp4" });
    const video = document.createElement("video");
    video.muted = true;
    video.preload = "auto";
    video.src = window.__TAURI_INTERNALS__.convertFileSrc(token, "qqnt-media");
    document.body.append(video);
    await until(() => {
      if (video.error) throw new Error(`Native video codec/protocol error ${video.error.code}: ${video.error.message}`);
      return video.readyState >= 2;
    }, "WebView2 decodes H264 from authorized range protocol");
    assert(video.videoWidth === 64 && video.videoHeight === 36 && video.duration >= 1, "real video metadata incorrect");
    video.remove();
    const { id: latestSession, status } = await scan();
    assert(status.progress.reusedFiles === 3, "incremental fingerprint reuse failed");
    assert(document.querySelectorAll("nav button").length === 4, "navigation did not render");
    assert(!document.body.textContent.includes("浏览器预览"), "native app incorrectly treated as browser preview");
    await until(() => document.querySelector("[data-session-id]")?.dataset.sessionId === latestSession, "latest scan rendered in file list");
    const sessionsBeforeDelete = (await invoke("list_sessions")).length;
    const checkbox = await until(() => { const node = document.querySelector('input[aria-label="选择 Pic/中文.png"]'); return node && !node.disabled ? node : null; }, "selectable file list");
    checkbox.click();
    const deleteButton = await until(() => Array.from(document.querySelectorAll("button")).find((node) => node.textContent.trim() === "删除" && !node.disabled), "direct delete button");
    deleteButton.click();
    await until(() => document.body.textContent.includes("已删除 1 项") || document.body.textContent.includes("请先退出 QQ"), "direct delete or live QQ guard");
    const actualDelete = document.body.textContent.includes("已删除 1 项");
    assert(!document.querySelector("dialog[open]"), "delete must not open a confirmation workflow");
    assert(await invoke("cleanup_enabled") === false, "direct delete must not enable background cleanup");
    assert((await invoke("list_sessions")).length === sessionsBeforeDelete, "delete must not create a new scan");
    const updated = await invoke("get_session", { sessionId: latestSession });
    assert(updated.summary.progress.files === (actualDelete ? 2 : 3), "known changes must update file totals without rescanning");
    if (actualDelete) await until(() => document.querySelector('[data-testid="total-size"]')?.textContent === "1.71 KiB", "statistics update without rescan");
    window.__qqntNativeSmokeResult = { ok: true, directDeleteButton: true, noRescan: true, actualDelete, qqGuardBlocked: !actualDelete, files: 3, bytes: 1883, rendered, unicodeFilter: true, databaseProtected: true, nativeIpc: true, imageDecode: true, h264Video: true, manualProtection: true, dryRun: true, defaultReadOnly: true, incrementalReuse: 3 };
  } catch (error) {
    window.__qqntNativeSmokeResult = { ok: false, error: String(error) };
  }
})();
