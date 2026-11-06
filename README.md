# QQNT 存储管理器 0.4.1

Windows QQNT 本地文件管理工具，普通用户运行，无需 LiteLoader。

## 使用

1. 选择 QQNT 数据目录，程序自动扫描。
2. 用“全部 / 图片 / 视频 / 大文件”浏览并选择文件。
3. 点击“删除”直接永久删除，不需要审核页、模式选择或输入确认文字。

每页显示 200 项，翻页保留选择；“全选”包含当前筛选结果的所有页面，可取消勾选个别文件。批量删除显示进度并支持停止。

搜索、排序始终可用；日期和大小筛选默认收起。保护、预览与文件位置都在文件列表操作。设置中提供定时扫描、预览缓存、扫描历史和恢复区。

QQ 运行、文件占用、未知文件、数据库、重要附件或保护项仍会拦截。删除完成后直接更新列表与统计，不重新扫描目录。外部新增或变化的文件可通过刷新按钮或原有定时扫描更新。没有证明 QQ 媒体可以重新下载，请只选择确实不要的文件。扫描始终只读；直接删除只授权本次选择，不会开启自动后台清理。

删除前的身份/摘要检查、同盘暂存和最终删除由后端自动完成。若最终删除失败，文件会保留在“设置 → 恢复区”；这部分尚未释放空间，可在重新授权源目录后恢复。恢复不会覆盖已有文件。

## 下载

Windows x64 安装包和独立 EXE 可在 [GitHub Actions](https://github.com/Mellow-Sky/QQNT-Storage-Manager/actions/workflows/ci.yml) 的成功运行中下载 `qqnt-storage-manager-windows` 构建产物。仓库只包含源码、资源、测试与构建配置；编译缓存、依赖、日志和安装包不入库。

需要 WebView2；安装包在运行库缺失时可能从 Microsoft 下载它。程序未签名。

可选 LiteLoader 插件仍为实验性入口；实际 QQ 内加载未验收。实际 QQNT 版本兼容性与安装/卸载交互仍需实测，模拟数据测试不能替代版本验收。

## 构建

Windows：Node.js 22.12+、当前 Rust stable MSVC、Visual Studio C++ Build Tools / Windows SDK、WebView2。

```powershell
npm ci
npm run desktop
# 生成安装包
npm run desktop:build
```

```sh
npm run check
npm run format:check
npm run build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --no-default-features -- -D warnings
cargo test --workspace --no-default-features --locked
npm run test:ui
```

模拟数据：`python3 scripts/create-fixture.py`。仅在全新临时目录生成带 MOCK 标记的文件。

扫描索引保存在 `%LOCALAPPDATA%\io.github.qqnt-storage-manager`，包含未加密的本地路径、保护设置和审计。默认不保存媒体内容；预览缓存需主动启用。源目录内的 `.qqnt-storage-manager-quarantine` 保存暂存文件和恢复清单，不要单独删除。原始 `prompt.md` 未修改。

更多资料：[安全设计](docs/safety.md)、[架构](docs/architecture.md)、[兼容性](docs/qqnt-compatibility.md)、[测试记录](docs/test-results.md)。MIT 许可；[第三方声明](THIRD_PARTY_NOTICES.md)。
