仓库保留测试结论和复现方式；历史日志、截图及安装包不入库。发布构建可通过 GitHub Actions 重新生成。

# 0.4.1 图标验证（2026-11-07）

- TypeScript / ESLint、Prettier、前端生产构建通过，Cargo 锁文件与版本元数据一致。
- PNG 原图及 32/128/256/512 像素导出均为 RGBA；实际解码核对透明区域和不透明主体，角像素 alpha 为 0 或抗锯齿产生的 1。ICO 包含 16/24/32/48/64/256 像素，32px 人工检查可辨识收纳盒。
- 4 项既有界面回归测试通过，实际截图核验页眉图标加载和布局。未为图标样式新增镜像实现的测试。
- Windows release EXE 与 NSIS 安装包构建通过；解析两者 PE 图标资源，6 个尺寸的图像数据均与新 ICO 完全匹配。安装器/卸载器均显式配置新图标。7-Zip 完整性核验通过，安装包内程序与独立 EXE 仅有预期的 Tauri 安装类型标记差异。
- Windows WebView2 原生自检退出 0，扫描、中文筛选、PNG/H.264、保护、增量复用、直接删除入口及不额外扫描断言通过。宿主 QQ 仍在运行，生产进程保护拦截删除，模拟文件完整。没有访问真实 QQ 数据或关闭 QQ。

图标说明与提示词见 icon-design.md；验证证据以 artifacts/*0.4.1* 命名。业务逻辑测试记录见下方 0.4.0，安装/卸载交互及真实 QQNT 兼容仍未扩大验收范围。

# 0.4.0 批量选择与免重扫验证（2026-11-06）

- TypeScript / ESLint、Prettier、Rust fmt / clippy、前端与 Windows release / NSIS 构建通过。安装包完整性及提取程序与独立 EXE 的预期安装类型标记差异均已核对。
- Linux 29 项、Windows 原生 37 项常规测试通过。新增覆盖索引 v4 迁移、匹配完整指纹的移除视图、同名替换文件、统计与保护叠加、6001 项查询范围与排除、分批处理中不跳项，以及应用索引父目录授权拒绝。
- Windows 私有测试在专用模拟临时目录实际批量删除和停止：130 张图片及数据库/未知文件中排除一张后，129 张图片成功删除，两个非媒体候选保留；接着删除此前排除的一张，始终只有原扫描会话。257 张图片的停止测试保留未处理项，统计与实际结果一致。测试的 QQ 退出检查替身只在 cfg(test, windows) 中存在，生产进程检查未改为可注入。
- 4 项浏览器测试通过：简洁界面、直接删除不新增扫描、450 项跨页手动选择、全选 450 项后跨页排除 2 项并删除其余 448 项。核验分页回退、头部统计与后端权威计数；明确 MOCK 的 IPC 只出现在测试初始化脚本中。
- 显式索引压力测试通过：100,001 条索引的全选捕获与完整 keyset 分批取出耗时 403.157632ms，每批不超过 64 项。此数字只衡量当前 Linux 环境的索引选择，不是实际文件删除耗时或 Windows 性能保证。
- Windows WebView2 原生自检通过：扫描、增量复用、图片/H.264、中文筛选、保护与直接删除入口均正常；删除入口不新增扫描会话。宿主 QQ 正在运行，真实进程保护拦截了删除，没有关闭用户 QQ。正向删除证据来自前述临时目录单元测试。结果见 artifacts/windows-0.4-native-smoke.json。

删除后只更新本程序已知的变更，外部变化由手动或定时扫描发现。安装/卸载交互、真实 QQNT 版本兼容及插件内加载仍未扩大验证范围。下方保留历史记录。

# 0.3.0 简化界面验证（2026-11-06）

- TypeScript / ESLint、Prettier、Rust fmt / clippy 与前端构建通过。
- Linux 26 项、Windows 原生 32 项测试通过。新增用例核验选定媒体真正永久删除、数据库/未知/保护项保留、恢复不覆盖，以及直接操作不启用后台自动清理。正向文件操作只用专用临时模拟目录；QQ 退出检查仅在私有单元测试中注入。
- 2 项浏览器测试通过：四个横向类型标签、收起的筛选/设置、主题保存；选择后点击删除只调用一次 delete_files，不打开对话框，不调用旧审核或启用开关 API。后一项为明确 MOCK 的测试 IPC，未加入产品代码。
- Windows WebView2 原生自检退出 0：真实扫描、统计、中文筛选、图片/H.264、保护和增量扫描通过；直接点击删除按钮无确认流程。由于宿主 QQ 正在运行，真实进程保护正确拦截，模拟源文件保持完整。没有关闭用户 QQ 或绕过生产检查。结果见 artifacts/windows-0.3-native-smoke.json。
- Windows release EXE / NSIS 构建成功，安装包已作完整性检查；提取程序与独立 EXE 的唯一区别为 Tauri 的预期安装类型标记。构建时复用已完成的前端产物，beforeBuildCommand 临时设为空；默认源码构建命令仍自动构建前端。

当前界面见 artifacts/browser-simple.png。安装/卸载交互、真实 QQNT 版本兼容、插件内加载的验证范围没有扩大。以下保留 0.2.0 的完整历史记录。

# 验证结果（2026-11-06，0.2.0）

## 实际环境与方法

WSL2 Ubuntu 26.04，Node 22.22.1/npm 9.2，Linux Rust stable 1.99.1。宿主 Windows 已有 WebView2，Windows Rust 1.93.1，但没有 Visual Studio link.exe。因此使用官方 cargo-xwin 0.23.1 + LLVM/Clang 21 + Microsoft SDK/CRT 缓存交叉链接 Windows x64 MSVC EXE，再通过 WSL interoperability **在宿主 Windows 实际执行**测试二进制。不是仅做 cfg 静态检查。

工具和 SDK 位于用户缓存，未提权或执行系统 apt 安装；从 Ubuntu 包局部提取 NSIS/运行库。cargo-xwin 发布文件校验了官方 SHA256。SDK/LLVM 不打入项目源码包。此前 Linux GTK/WebKit 开发检查属于 Phase 1 环境，WSL 重启后 /tmp 开发库已失效；本轮以真正 Windows 目标为桌面验证对象。

## 已通过检查

| 检查 | 实际结果 |
|---|---|
| npm strict TypeScript + ESLint | 通过，零警告 |
| Prettier + cargo fmt | 通过 |
| Vite 生产构建 | 通过，图表独立 chunk |
| npm audit --audit-level=moderate | 0 vulnerabilities |
| cargo clippy --workspace --all-targets --no-default-features -- -D warnings | 通过 |
| Linux workspace 常规测试 | 26 通过，2 个显式压力测试默认 ignored |
| Windows 原生 workspace 测试 | 14 个二进制实际执行，31 通过，2 个压力测试默认 ignored |
| Linux scanner 压力 | 100,001 个真实临时文件，700,007 bytes；28 次进度报告，批次 ≤256 |
| Linux scanner→SQLite→分页压力 | 100,001 个文件，约 18.94 秒，1834 次状态观察，60 项分页和总数一致 |
| Windows 原生 WebView2 自检 | 连续 3 次 exit 0；真实嵌入 React、Tauri IPC、扫描、中文筛选、统计、PNG 解码、H.264 视频、保护、Dry Run、默认关闭、增量复用全部通过 |
| Playwright 浏览器界面 | 1 项通过，八页导航/诚实空状态/禁用本地操作/主题持久化/无 pageerror |
| Node 可选插件边界 | 3 项通过；过滤敏感摘要、无效/链接/超限拒绝、固定 URI、仅两个 preload 方法 |
| 演示夹具 | 新建 MOCK 目录，7 文件 / 2163 bytes；有效 PNG/MP4 和损坏/保护输入 |

压力测量发生在 Linux 临时文件系统，scanner 单次约 2.77 秒，完整管线约 18.94 秒，不能当作 Windows SSD 完成时间保证或 UI 帧率。状态可持续查询是后台管线证据。

## Windows 原生覆盖

- 中文、多账号候选、空/未知布局、长路径、同级 nt_db 不越权、保护优先、硬链接只读统计、junction 跳过及占用继续扫描。
- 真实图片解码、损坏输入降级、授权视频 token、受限 range、改变文件后拒绝旧预览、磁盘缓存默认关闭和明确启用后的身份/摘要回收。
- Dry Run 不写原件、数据库清理提交被拒绝、计划执行前新保护生效、变化文件失败、同一计划不能重放、错误确认文字拒绝。
- 实际同盘隔离→恢复→再隔离→永久删除；恢复不覆盖新接收同名文件。操作均在专用 Windows 临时目录。
- QQ 运行检查拒绝、提交前 QQ 再启动拒绝、独占占用拒绝、只读属性、硬链接和变化文件拒绝；一项失败后另一项仍能隔离。
- 恢复路径替换为 junction 后拒绝，不写目标外部目录；清单持久化后，另建空应用索引可核对导入隔离记录。
- SQLite 3.54.1 运行时下限检查、单实例索引所有权、30 次连续扫描期间的并发查询（额外重复 5 轮）、SQLite v1→v3、重开、去重、分页、参数化筛选、未来版本拒绝及异常运行计划/操作中断标记。

原生自检使用 3 个模拟文件共 1883 bytes，页面渲染 `[1.84 KiB, 134 B, 1.69 KiB, 18 B]`，重复扫描复用 3 项。视频是 64×36 H.264 MP4，约 1.2 秒；图片 64×32 PNG。结果文件为 artifacts/windows-native-smoke.json，逐个原生测试结果为 artifacts/windows-unit-tests.json。WebView2 退出时出现过 Chrome_WidgetWin_0 注销错误 1412，进程 exit 0，功能断言全部通过；没有将该 stderr 隐藏为完全无日志运行。

## 桌面发布构建

最终 `npm run desktop:build -- --runner cargo-xwin --target x86_64-pc-windows-msvc` 退出码 0。生成优化的 Windows x64 GUI EXE（15,713,792 bytes）及当前用户 NSIS 安装包（4,261,125 bytes，约 4.06 MiB），已复制到 artifacts。未签名；未在用户系统执行安装/卸载。原生自检是单独测试二进制，复用生产命令和真实嵌入前端；不是把自检入口打入发布版。

7-Zip 对最终安装包的校验通过；提取的程序与独立 EXE 逐字节核对，唯一差异是 Tauri 预期的三字节 bundle type 标记（安装包内 NSS，独立版 UNK）。结果保存于 artifacts/installer-payload.json。源包和可选插件 ZIP 也通过 CRC、内容及必要文件检查。

最终并发回归复现并修复 SQLite 短连接关闭后的 WAL/SHM 消失/删除中句柄误报：后端保留无活动事务的连接，保留不安全链接拒绝；加入单实例索引锁。上游 WAL-reset 问题通过升级 bundled SQLite 3.54.1 避免，运行时版本下限测试通过。并发回归在 Windows 额外连续重复 5 轮，真实 WebView2 整体自检连续 3 轮通过。

Windows debug 原生自检已成功链接和运行。交叉链接器报告 Microsoft 静态 CRT PDB 缺失 LNK4099；这些是缺少 SDK 调试符号的警告，不影响已完成的可执行程序生成。

## 明确未验收项

- **未读取真实 QQ 数据，未完成任何具体 QQNT 版本的兼容性验收，也没有证明 QQ 媒体可重新下载。** 所有破坏性测试仅使用模拟临时目录。
- 安装/卸载交互、Explorer 定位交互、长期后台运行、各类真实杀毒/云同步/备份驱动、FAT/exFAT/ReFS、真实十万文件窗口帧率尚未实测。Windows NTFS 文件系统和原生 WebView2 链路已经实际运行。
- 内置浏览器和 Windows computer-use 工具因宿主 sandboxCwd URI 无法初始化，未用它们进行人工式操作；采用独立原生集成测试和本地 Playwright。测试字体仅引用本机 Windows 字体，不分发字体。
- LiteLoader 上游已归档。插件已编写/打包/边界测试，**未在 QQ 内实际加载**。CI 配置已更新但未推送远端执行。
- 没有 QQ SafeCandidate 规则，保守模式 QQ 候选为零；自动清理只覆盖应用生成的衍生缩略图。未知/受保护数据和未支持媒体不会为了“完整”而放宽检查。

源码、锁文件、测试、构建脚本、架构/安全/兼容文档均随源码包交付；原始 prompt.md 未修改。
