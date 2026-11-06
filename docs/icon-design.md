# 0.4.1 应用图标

使用 imagegen 技能的内置图像生成工具制作。深石墨圆角底配薄荷绿收纳盒、米白文件卡和暖金星芒；圆角外保留透明区域。原始生成图保存在 `apps/desktop/src-tauri/icons/app-icon.png`，Tauri CLI 导出多尺寸 ICO 与 PNG；同一设计用于 Windows 程序、安装包、窗口、界面页眉及浏览器 favicon。

再导出：`npm run tauri -w apps/desktop -- icon src-tauri/icons/app-icon.png -o src-tauri/icons`。命令会额外生成其他平台资源；当前发布只使用 Windows 与网页尺寸。

生成提示词：

> Use case: logo-brand. Asset type: final production Windows desktop application icon for a local storage and media organizer. Create ONE polished standalone square app icon, not a mockup or contact sheet. A broad dark graphite rounded-square tile containing a bold mint-green storage box / file drawer, with two neatly stacked cream and mint file cards peeking out, and one small warm amber four-point glint at the upper right suggesting a tidy organized space. Refined modern desktop utility aesthetic, mostly clean geometric shapes, gentle dimensional shading and crisp smooth edges, disciplined large shapes that remain legible at 16, 24, 32 and 48px. Front view, centered, balanced, compact silhouette occupying about 88 percent of the square canvas, square 1024x1024. Dark charcoal, soft mint, warm ivory and a tiny amber accent; restrained highlights, premium quality. Transparent background OUTSIDE the rounded-square tile, with genuine alpha at the corners. No text, letters, numbers, logos, QQ penguin, blue blank tile, watermarks, busy ornaments, photoreal background, perspective scene, extraneous elements or large floating drop shadows.
