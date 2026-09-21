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
