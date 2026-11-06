# 可选 LiteLoaderQQNT 入口 0.2.0（实验性）

独立桌面程序不依赖此插件。插件使用官方 manifest v4 的 main / preload / renderer 和 `onSettingWindowCreated`，仅在设置页显示上次扫描总大小、文件数、时间与兼容性状态，并打开独立管理器。

上游 LiteLoaderQQNT 仓库于 2026-05-17 归档。这里检查了官方文档与实际源码，但**尚未在实际 QQNT / LiteLoader 组合上加载验收**，不承诺新版 QQ 可注入。不能正常加载时直接使用桌面程序，不需要修改 QQ 核心程序。

先安装 QQNT Storage Manager Windows 安装包，以当前用户注册固定的 `qqnt-storage-manager://open` 入口。源码直接运行或便携 EXE 不会注册协议，可手动启动。将本目录或发行 ZIP 解压为支持 manifest v4 的 LiteLoader 插件目录，按你的 LiteLoader 安装方式启用。插件自身无 npm 依赖或构建步骤。

摘要只读取 `%LOCALAPPDATA%\io.github.qqnt-storage-manager\public-summary.json`，不含账号、路径或媒体内容；首次完成扫描后产生。preload 只暴露读取摘要和打开固定 URI 两个方法。插件不扫描、保护、移动或删除 QQ 文件，不调用 QQ 私有缓存 API。通过 `node --test tests/plugin/*.test.cjs` 验证摘要过滤和固定入口边界；这些单元测试不等于 QQ 内实际加载测试。

打包：`python3 scripts/package-plugin.py`。上游研究与许可证记录见 `docs/research.md`；本插件为独立编写的 MIT 源码。
