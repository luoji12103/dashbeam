# DashBeam Local Android

本地定制基于 DashBeam 0.7.1，保留 AGPL-3.0 许可证。local.4 包名 `com.dashbeam.android.local`，应用名 DashBeam Local，versionCode 70105；与官方客户端并存，不覆盖官方安装或配对数据。首次使用需重新配对。更新已有定制版时保留原签名、直接覆盖安装，不卸载，即可保留原定制版的数据。此包不通过官方更新入口更新。

## 文字使用

- 发送页选择文字，可编辑/粘贴文字，或通过系统文档选择器导入单个 UTF-8 `.md` 正文（上限 1 MiB）。文件模式始终发送原文件。
- 其他应用的分享菜单可以分享纯文字到 DashBeam Local；已有草稿时确认是否替换，正在发送时保留待处理分享。
- 接收经过明确标记的文字后显示可选择的原文与复制按钮。普通 `.txt/.md` 文件不自动当作文字消息。
- 自动复制默认关闭。启用后也只允许应用前台复制；后台完成时通知不显示正文，回到应用后手动复制。通知需要系统权限，关闭权限时仍可回到应用查看。
- 待查看正文仅缓存于当前进程，保留最新结果；系统终止进程后不恢复文字弹窗。收到的 `.txt` 下载副本仍由原有文件接收流程保存，可从下载目录打开。
- 不承诺被系统终止或强制停止后仍能接收。后台传输仍受安卓电池策略和原有配对/常驻设置影响，无新增剪贴板轮询。

## 构建与本地签名

优先在 Debian DEV 的隔离源码目录构建，不覆盖已有工作区；本轮使用 `/home/luoji/code/dashbeam-latency-20261111.jVKZD9`。需要 Node/pnpm、Rust Android targets、Java 17、Android SDK 36 / build-tools 36.0.0 / NDK 29。

```sh
pnpm tauri android init --ci --skip-targets-install --config src-tauri/tauri.local-android.conf.json
# init 后恢复仓库所跟踪的 AndroidManifest.xml（系统分享过滤器等）。
CARGO_BUILD_JOBS=2 GRADLE_OPTS='-Dorg.gradle.workers.max=2' \
  pnpm tauri android build --apk --target aarch64 --split-per-abi --ci \
  --config src-tauri/tauri.local-android.conf.json
node scripts/package-local-android.mjs \
  src-tauri/gen/android/app/build/outputs/apk/arm64/release/app-arm64-release-unsigned.apk
```

隔离工程默认使用自己的 target。若另设 `CARGO_TARGET_DIR` 复用旧工程缓存，Tauri 2.10.1 的 Kotlin codegen 可能未在新工程重跑，导致 `MainActivity` 找不到 `TauriActivity`。这不是传输源码错误：改用独立 target 重新生成依赖代码，或仅在逐字节核对当前锁定版本的 `mobile/android-codegen/TauriActivity.kt` 模板（替换 package 后）一致时，复用对应的生成文件。本轮采用后者，不修改依赖、入口逻辑或签名身份，也不清理原工作区缓存。

签名脚本使用私有本地密钥，不冒用官方签名。密钥和随机密码存于构建用户的 `~/.local/share/dashbeam-local-signing/`（目录 0700、文件 0600），不进入 Git 或 APK。后续覆盖更新必须保留同一密钥。生成安装包会进行签名验证和 16 KiB ZIP 对齐验证。

产物：`dist/DashBeam-0.7.1-local-android-arm64.apk`，仅 ARM64。本地包最低 Android 10/API 29，避免上游 Android 7–9 默认接收落在应用私有目录的问题。用户指定的骁龙 8 系与天玑 9400+ 设备使用 ARM64；实际系统版本与 OEM 后台行为仍需实测。

不要用上游通用 `android:build:release` 命令制作此定制包；它未传入本地包配置，会使用官方包名。使用上述显式 local config 命令。

已知上游边界：导出失败/导出中进程被杀可能遗留应用私有 staging 文件，本轮未自动删除这些可能含未导出接收内容的文件。跨进程待导出恢复不在本轮实现范围。

## 验收

2026-11-11，local.4（产品提交 `2ef67ab`）：ARM64 APK 构建通过，versionCode 70105、minSdk 29、targetSdk 36；签名证书与之前的定制 APK 一致。APK 签名、16 KiB ZIP 对齐、ELF LOAD 对齐和压缩包完整性通过，包内保留 SEND/SEND_MULTIPLE 分享入口。APK SHA256：`89944ab430dad1cae60e9f99c0f9389a56785525da6ef0bad44ea0a1a645a28b`。前端 173 项、协议/native 22 项、app Rust 98 项通过，详见 [本轮验证记录](../reports/transport-audit-20261110/PRODUCT_UPDATE.md)；不代表安卓实机验收。

2026-09-22，Debian 13 DEV：163/163 前端库测试通过；lint 无错误（保留一个既有 tray labels 抑制提示）；Android 前端构建通过；ARM64 release APK 构建、签名、apksigner 验证和 16 KiB ZIP 对齐验证通过。APK SHA256：`d86e70073f2969fdc8b1ec309af9ca403067d114ab82ab78648da763b047ca51`。

当前没有连接安卓实机；不得把桌面端双向测试或安卓编译通过等同于安卓设备验收。首次实机需检查：粘贴/Markdown 导入、系统分享文字与文件、前台手动/自动复制、后台接收通知与点击恢复、通知拒绝后的手动查看、与 Mac 双向文字传输。
