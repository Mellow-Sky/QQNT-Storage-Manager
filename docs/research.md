# 实际源码研究记录

读取时间：2026-11-05。两个仓库均成功以只读 shallow clone 获取至工作区外 `/tmp`，已阅读 README、依赖、许可证与下列实际源码；未复制其业务实现。

## QQ-Group-Images-Cleaner

[仓库](https://github.com/steven-jianhao-li/QQ-Group-Images-Cleaner)，commit `1833dfbbc8bbe47103f01bb7747de7478b7bba0d`，MIT。依赖为 Python/Tkinter 与 Pillow。

核查 `lib/QQCleanerApp.py` 的 `scan_thread`、自动定位和删除线程，以及 `lib/ThumbnailViewerWindow.py` 的分页/排序/后台缩略图加载，`ConfirmationDialog.py` 的删除前随机预览。实际扫描使用 `os.scandir` 和修改时间分年月；默认位置是旧版 `Image/Group2`。程序有确认后的永久逐文件删除。

吸收：年月索引、后台工作、分页浏览、可见的文件详情。不会沿用旧版目录语义或删除逻辑；创建/访问时间不被推断为消息时间。

## LiteLoaderQQNT-QQCleaner

[仓库](https://github.com/MisaLiu/LiteLoaderQQNT-QQCleaner)，commit `694fc55d0fc0a9d0fa4e6472c7f9d0d8719bdb7f`，GPL-3.0-or-later。核查 `package.json` 的 manifest 和依赖，`main/index.ts`、`preload/index.ts`、`renderer/index.ts`、`main/qqnt/*`、`main/cleaner/*`、`common/utils/config.ts`。依赖涉及 Electron 29、electron-vite、TypeScript、UUID。

实际通过 main/preload/renderer、IPC、设置页生命周期和私有内核调用集成 QQNT；JSON 配置默认关闭自动清理。媒体清理由扫描结果的 fileKey 调用 QQ 内部接口。`cleaner/qqnt.ts` 的本体缓存函数立即抛错，调用在主清理链中被注释；README 同样保留修复本体缓存的 TODO，不能视为已实现。

吸收：独立配置、明确入口、兼容性隔离。私有 API 易受版本影响，没有在当前 QQNT 验证；不复制 GPL 实现进入本 MIT 项目。Phase 5 前不提供猜测的注入代码。

## 官方文档及实际依赖

按安装后的依赖核查 Tauri **2.12.5** / tauri-build **2.7.4** / dialog **2.8.4** 本地 crate 源码，JS API/CLI **2.12.4**。采用 Tauri 2 `invoke`、`Manager::path().app_local_data_dir()`、Rust `DialogExt`、阻塞任务池及 capabilities/CSP 配置，无 Tauri 1 API。版本以 Cargo.lock / package-lock.json 为准。

- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)
- [Calling Rust](https://v2.tauri.app/develop/calling-rust/)
- [Dialog](https://v2.tauri.app/plugin/dialog/)
- [Capabilities](https://v2.tauri.app/security/capabilities/)
- [Configuration](https://v2.tauri.app/reference/config/)
- [cap-std Dir 4.0.3](https://docs.rs/cap-std/4.0.3/cap_std/fs/struct.Dir.html)

shadcn/ui 的 Button 结构按照其 MIT registry 组件适配，保留 Radix Slot/CVA 和本地可编辑组件模式，调整为当前设计令牌。许可证及来源见第三方说明。

## 新布局与插件可用性补查

维护者项目 [qq-chat-image-compressor](https://github.com/mkmk233/qq-chat-image-compressor)（commit `9d0be37aa563824b87e03ac3e4794584501c9c6d`，MIT；已读 README、requirements、LICENSE、`qqcompress.py` 的账号/目录发现及 `qqstate.py` 的收藏路径）记录 `nt_qq/nt_db` 与 `nt_qq/nt_data` 同级，媒体位于 `Pic` 和 `Emoji`，收藏主库位于 `Emoji/personal_emoji`。这只提供另一种候选结构的源码证据，不证明所有新版 QQNT 相同，也不采用其压缩/替换逻辑。适配器 1.1.0 增加 Pic+Emoji 候选检测和收藏路径拒绝规则；只选 nt_data 时不访问父级数据库。

已查看 [LiteLoader 官方仓库](https://github.com/LiteLoaderQQNT/LiteLoaderQQNT)：页面标示 2026-05-17 归档；[官方插件文档](https://liteloaderqqnt.github.io/docs/new-plugin.html) 描述 manifest v4 和设置窗口生命周期，但文档存在不等于当前 QQNT 运行验证。保持独立运行，插件跳过构建。

## 发布构建补充（2026-11-06）

实际交叉构建使用 cargo-xwin 0.23.1、LLVM 21、Microsoft SDK/CRT 和局部提取的 Ubuntu NSIS 3.10。Tauri CLI 2.12.4 的 [NSIS bundler 源码](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.4/crates/tauri-bundler/src/bundle/windows/nsis/mod.rs) 会在启动 makensis 前移除 NSISDIR/NSISCONFDIR；局部工具因此最初找不到系统默认路径下的 Stubs。依据 [NSIS 官方环境变量文档](https://nsis.sourceforge.io/Docs/Chapter3.html)，在用户缓存创建 makensis 包装器，于真正编译器启动前设置本地资源目录。未改动系统 /usr/share、未修改 Tauri 源码或项目运行权限。额外 nsis_tauri_utils 插件由 Tauri 自带下载及 hash 校验逻辑获取。

测试素材均自行生成：64×32 纯色 PNG 和 64×36、约 1.2 秒的无音轨 H.264 MP4；使用局部 ffmpeg 生成 MP4，并在实际 Windows WebView2 的受限 range 协议中解码验证。ffmpeg 仅是开发工具，不作为应用依赖分发。不含用户 QQ 数据或第三方媒体素材。

## SQLite 并发连接与版本补充

最终 Windows 并发回归暴露了短连接反复创建/关闭时，WAL/SHM 在元数据检查后消失或变成删除中句柄的竞态。后端改为保留一个无活动事务的连接，其他命令仍用独立短连接；临时文件在检查后消失按正常关闭处理，硬链接和不安全路径仍拒绝。同一应用数据目录持有实例锁，避免另一实例把正在运行的任务错误标记为中断。

[SQLite 官方 WAL 文档](https://sqlite.org/wal.html#the_wal_file) 说明最后一个连接关闭会清理 WAL/SHM；同页记录 WAL-reset 并发问题于 3.51.3 起修复。核对本地旧 bundled 源码为 3.51.1，因此将 rusqlite 更新到 0.41.1，锁定 libsqlite3-sys 0.39.1，实际内置源头版本 3.54.1；增添运行时版本下限断言和并发扫描/查询回归。新连接不保持读事务，保留默认自动 checkpoint。上游依赖依据为 [rusqlite 官方仓库](https://github.com/rusqlite/rusqlite) 与项目 Cargo.lock。
