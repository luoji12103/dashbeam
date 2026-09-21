# DashBeam Local Android

本地定制基于 DashBeam 0.7.1，保留 AGPL-3.0 许可证。包名 `com.dashbeam.android.local`，应用名 DashBeam Local，versionCode 70103；与官方客户端并存，不覆盖官方安装或配对数据。首次使用需重新配对。此包不通过官方更新入口更新。

## 文字使用

- 发送页选择文字，可编辑/粘贴文字，或通过系统文档选择器导入单个 UTF-8 `.md` 正文（上限 1 MiB）。文件模式始终发送原文件。
- 其他应用的分享菜单可以分享纯文字到 DashBeam Local；已有草稿时确认是否替换，正在发送时保留待处理分享。
- 接收经过明确标记的文字后显示可选择的原文与复制按钮。普通 `.txt/.md` 文件不自动当作文字消息。
- 自动复制默认关闭。启用后也只允许应用前台复制；后台完成时通知不显示正文，回到应用后手动复制。通知需要系统权限，关闭权限时仍可回到应用查看。
- 待查看正文仅缓存于当前进程，保留最新结果；系统终止进程后不恢复文字弹窗。收到的 `.txt` 下载副本仍由原有文件接收流程保存，可从下载目录打开。
- 不承诺被系统终止或强制停止后仍能接收。后台传输仍受安卓电池策略和原有配对/常驻设置影响，无新增剪贴板轮询。

## 构建与本地签名

优先在 Debian DEV `/home/luoji/code/dashbeam-macos` 构建。需要 Node/pnpm、Rust Android targets、Java 17、Android SDK 36 / build-tools 36.0.0 / NDK 29。

```sh
pnpm tauri android init --ci --skip-targets-install --config src-tauri/tauri.local-android.conf.json
# init 后恢复仓库所跟踪的 AndroidManifest.xml（系统分享过滤器等）。
CARGO_BUILD_JOBS=2 GRADLE_OPTS='-Dorg.gradle.workers.max=2' \
  pnpm tauri android build --apk --target aarch64 --split-per-abi --ci \
  --config src-tauri/tauri.local-android.conf.json
node scripts/package-local-android.mjs \
  src-tauri/gen/android/app/build/outputs/apk/arm64/release/app-arm64-release-unsigned.apk
```

签名脚本使用私有本地密钥，不冒用官方签名。密钥和随机密码存于构建用户的 `~/.local/share/dashbeam-local-signing/`（目录 0700、文件 0600），不进入 Git 或 APK。后续覆盖更新必须保留同一密钥。生成安装包会进行签名验证和 16 KiB ZIP 对齐验证。

产物：`dist/DashBeam-0.7.1-local-android-arm64.apk`，仅 ARM64。本地包最低 Android 10/API 29，避免上游 Android 7–9 默认接收落在应用私有目录的问题。用户指定的骁龙 8 系与天玑 9400+ 设备使用 ARM64；实际系统版本与 OEM 后台行为仍需实测。

不要用上游通用 `android:build:release` 命令制作此定制包；它未传入本地包配置，会使用官方包名。使用上述显式 local config 命令。

已知上游边界：导出失败/导出中进程被杀可能遗留应用私有 staging 文件，本轮未自动删除这些可能含未导出接收内容的文件。跨进程待导出恢复不在本轮实现范围。

## 验收

构建与检查结果在完成后补充。当前没有连接安卓实机；不得把桌面端双向测试或安卓编译通过等同于安卓设备验收。首次实机需检查：粘贴/Markdown 导入、系统分享文字与文件、前台手动/自动复制、后台接收通知与点击恢复、通知拒绝后的手动查看、与 Mac 双向文字传输。
