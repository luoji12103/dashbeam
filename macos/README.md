# DashBeam macOS 本地定制

基于上游 v0.7.1 / f39184ed6bfe4d28a3a266b0ceccd37060af7902。AGPL-3.0，保留上游许可证。

## 行为

- 关闭主面板后切换为 Accessory，只保留菜单栏图标；打开主窗口恢复 Regular/Dock。无菜单栏图标时保留 Dock，避免无法找回窗口。
- macOS 文件打开事件、命令行文件参数、共享扩展都进入同一多文件队列。支持空格、中文及特殊字符，文件只进入待发送列表，不自动发送。
- 从设置页接收文件时跳转发送页；传输正在进行时提示用户稍后重试，不修改当前传输选择。
- 原生 Share Extension 使用 public.file-url，打开包含它的客户端。仅处理本地文件 URL，不执行 URL 中的命令。
- Finder 快速操作位于 macos/DashBeam.workflow，可通过系统设置 → 键盘 → 键盘快捷键 → 服务分配快捷键。
- 本地构建关闭应用内官方更新入口，避免更新覆盖定制。保留原用户偏好，不修改其已保存的自动更新选项。
- 菜单栏增加快捷设置、文件/文件夹选择、闪传开关及目标显示。
- 闪传默认关闭，必须明确选择已配对目标。开启后监听拖动事件，无空闲轮询；关闭销毁监听与浮窗。未发送选择和进行中的传输均受保护，不自动选择其他目标。接收端是否需要确认仍遵循原有配对规则。

## 构建

需要 Node 22.12+、pnpm、Rust（本机使用 stable 1.98.1）、macOS Command Line Tools。

```sh
pnpm install --frozen-lockfile
RUSTUP_TOOLCHAIN=stable CARGO_BUILD_JOBS=4 pnpm tauri build --config src-tauri/tauri.local-macos.conf.json --bundles app
bash macos/package-local.sh
```

产物：`dist/DashBeam-0.7.1-local-macos-arm64.zip`。本地 ad-hoc 签名，不冒用原作者 Developer ID，不宣称已公证。共享扩展需以 Finder 实际展示、选择文件并送达发送面板为验收标准。

## 备份与回滚

`rollback/` 被 Git 忽略，权限 0700。保存原官方应用 ZIP 和安装前应用数据；备份中含设备身份，只留本机，不上传。

回滚时先从菜单栏退出 DashBeam，解压原应用 ZIP，替换 `/Applications/DashBeam.app` 后重新启动即可。通常无需恢复应用数据；不要用旧数据覆盖之后新增的配对或历史。快速操作可单独从 `~/Library/Services/用 DashBeam 分享.workflow` 移除。

## 验证记录

2026-09-21 本机验证：

- release 构建、Swift 分享扩展打包和 `codesign --verify --deep --strict` 通过；最终包已安装至 `/Applications/DashBeam.app`，分享扩展启用。
- Rust 单测 82 项、前端 lib 单测 151 项通过；最终新增前端通过 TypeScript 编译，原生模块 arm64/x86_64 严格警告编译通过。
- Finder Share → DashBeam 实际导入两个含中文/空格/& 的测试文件成功；关闭窗口切换 UIElement 且进程继续运行。最终设置页显示闪传默认关闭。
- 替换后设备 identity.key 与原备份逐字节一致；未发送私人文件。
- Quick Action 工作流命令行执行与服务注册通过，但 Finder 快速操作启用列表尚未勾选。需在 Finder 右键 → Quick Actions → Customize 启用，再于键盘快捷键的服务页分配快捷键。
- 尚未验收：菜单栏点击完整操作、最终客户端右上角真实拖放、远端接收完成、开关跨重启保持。桌面验证期间 Finder 被切换到其他任务窗口，未继续操作其他任务文件。独立离线 harness 未完成有效拖动（event-count=0），不能作为闪传通过证据。
- 闪传代码按事件驱动设计，但未做同条件功耗 A/B；不宣称已经降低瓦数或整机耗电。

原生浮窗独立验证：运行 `./macos/run-flash-drop-harness.sh` 后，在 30 秒内从 Finder 将测试文件拖到屏幕右上角。离线目标拒绝接收，结束输出 `panel-shown` 与事件数量；不发送文件，不请求新权限。

## 文字传输（2026-09-22，build 70102）

- 发送页切换“文字”，可自由输入/粘贴原始文本或 Markdown，也可“导入 Markdown”。文字模式的拖入只处理单个 `.md`；“文件”模式仍发送原文件。切换模式、分享失败/结束不会清除文字草稿，草稿只在当前进程内保存。
- 文字上限为 UTF-8 1 MiB，空串不能发送，不渲染或执行 Markdown/HTML。现有加密文件传输携带明确的 `content_kind=text` 标记；普通 `.txt/.md` 不被推断为剪贴板消息。
- 更新后的桌面接收端显示可选择文字的弹窗和复制按钮。设置中的“自动复制收到的文本”默认关闭；开启后成功接收会覆盖剪贴板，失败保留手动复制入口。是否自动接受传输仍由原有配对/信任规则决定。
- 旧客户端忽略 marker，收到 `DashBeam Text.txt`。更新后的客户端也保留这份下载文件，便于恢复；仅 UI 草稿/预览不另建文字历史。若独立 metadata 校验在 3 秒内失败，安全降级为普通 `.txt`，不自动复制。
- Windows/macOS/Linux 的原生剪贴板接口均已实现；Linux 使用常驻 owner thread 保持 X11 剪贴板内容，不轮询剪贴板。Windows 实机与 Wayland 桌面尚未验收，不等同于编译通过。

### 本轮已确认

- 通用验证在 `SER8-PVE-VM-102-Debian13-Dev`（实机 hostname `debian13-dev`，Debian 13.5，用户 `luoji`）的 `/home/luoji/code/dashbeam-macos` 完成：前端 159/159；Rust 86/86（另一个需显示器的测试默认 ignored）；隔离 `xvfb-run` 单独运行原生剪贴板测试通过；engine metadata/真实加密文字往返 3/3；Windows x86_64 GNU `cargo check` 通过。
- Mac 只做必要的原生 release 链接、Swift 分享扩展打包、ad-hoc 签名与 UI 验证；使用 DEV 的前端产物，`CARGO_BUILD_JOBS=2` + `nice -n 10`。本轮代理另跑过一次轻量本机前端单测，没有额外本机 Rust 测试或前端构建。
- build 70102 已安装。Mac UI 实测：粘贴中文 Markdown、切换模式保留草稿、从 `.md` 导入正文、DEV → Mac 弹窗接收、复制后实际粘贴原文、Mac → DEV `bytes=54 matches_fixture=true`、开启设置后的自动复制成功且不弹手动窗口。
- 原生签名深度严格校验通过，共享扩展保持启用；设备身份与 `rollback/app-data-before-text/identity.key` 逐字节一致。上个本地版本备份为 `rollback/DashBeam-before-text-70101.disabled`。没有向已有配对设备发送任何测试消息，没有把配对/用户数据同步到 DEV。
- 本轮不代表上一节的闪传拖放、菜单栏完整交互、Quick Action 或功耗 A/B 已完成；这些验收边界保持原样。
- 收尾已恢复自动复制为关闭并隐藏重启，持久设置为 `false`、进程为 `UIElement`、签名和设备身份再次核验通过。两份自有接收测试文件移入 `rollback/`；锁屏后未继续 UI 操作，因此最终设置页的重启后视觉复核未完成。

DEV 上构建 Mac 前端后同步 `frontend/dist/` 到 Mac，再执行：

```sh
RUSTUP_TOOLCHAIN=stable CARGO_BUILD_JOBS=2 nice -n 10 pnpm tauri build \
  --config src-tauri/tauri.local-macos.conf.json \
  --config '{"build":{"beforeBuildCommand":""}}' --bundles app
bash macos/package-local.sh
```

DEV 前端构建须设置 `VITE_LOCAL_MACOS_BUILD=true TAURI_ENV_PLATFORM=darwin`；只同步源码/构建产物，勿同步 `rollback/` 或应用数据。
