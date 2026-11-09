# 接收完成后的文件剪贴板操作

## 实现

`engine/native/src/receive.rs` 在完成事件中附加实际导出的 `exportedPaths`。前端只接受有效的绝对路径列表，并为每条 macOS 接收通知传递独立快照与本地化标签；开始新传输、失败或重置时清空完成页的路径。

`src-tauri/native/received_files.m` 使用 UserNotifications 公共 API 注册“复制文件 / 复制路径”操作；点击操作后从该通知的 `userInfo` 读取原路径，在主线程写入 NSPasteboard。文件复制提供多个 `NSURL` 的 `public.file-url`，并兼容旧 filenames 类型；路径复制使用纯文本和换行。全部文件先验证可读性，任一文件无效时不清空原剪贴板。

原有普通通知仍走既有通知库。新的原生接口不抢占旧 NSUserNotificationCenter 委托。通知中心仅在有效 `.app` 与支持的 macOS 上初始化；启动时即安装委托以处理旧通知操作，裸开发程序回退普通通知。应用退出后点击通知操作可能正常启动主窗口，不承诺后台无窗执行。

系统控制通知按钮布局、横幅持续时间、通知分组及是否展示；两个操作可能位于“选项”菜单。各聊天软件是否接受文件剪贴板取决于其自身支持。

## 已验证

- Debian DEV 独立源码目录：`/home/luoji/code/dashbeam-receive-copy-20261109.tsjpxb`。原有 `/home/luoji/code/dashbeam-macos` 的未提交源码未覆盖；复用其构建缓存与 Node 依赖。
- 前端 165 项测试通过，TypeScript 与 macOS 目标前端正式构建通过；保留既有动态导入与包体提示。
- Rust 两项路径选择测试通过（绝对路径、去重、重名后的实际名称及无效选择）；有一条既有 history 测试的 unused-import 提示。
- `macos/received-files-harness.m` 在独立私有 pasteboard 验证多文件 URL、legacy filenames、换行路径、中文/空格/特殊字符/重名后的名称；文件被删除后保留原剪贴板。未读取或改写用户剪贴板。
- 原生 Objective-C 以 macOS 10.13 deployment target、`-Wall -Wextra -Werror` 检查通过；裸 CLI 跳过 UserNotifications 初始化的路径通过。
- Mac 使用 Rust 1.91、单任务低优先级完成原生 release 编译；前端复用 DEV 产物。build 70104 已替换 `/Applications/DashBeam.app` 并以 `--hidden` 后台启动，进程存活；应用和 Finder 分享扩展深度严格签名检查通过，扩展保持启用，持久身份校验未变。替换前确认历史中无进行中的传输。
- 本地产物 `dist/DashBeam-0.7.1-local-macos-arm64.zip`，SHA-256：`cc19892790a5d64724cef44dce552f9db1d48b86eaa92b814be218d3b0d21d05`。尚未推送或发布新的 GitHub Release。

## 待完成

- 真实接收后系统通知操作、Finder 粘贴、旧通知快照与应用内按钮验收：进入该阶段时 Mac 已锁屏，等待用户手动解锁，不尝试解锁或修改锁屏策略。没有启动测试发送器，也没有向其他设备发送文件。

## 回滚

本轮备份目录：`rollback/receive-copy-20261109/`；旧应用为 `DashBeam-before-copy-70102.zip`，已验证压缩包完整性。不恢复或覆盖配对、身份和用户设置。回滚时退出 DashBeam，将 ZIP 内的旧应用恢复到 `/Applications/DashBeam.app` 后启动。
