# Android 最终构建与静态包验收：0.7.2 / 70201

核验日期：2026-11-15（Codex）。最终构建于 `2026-11-15T20:51:35Z` 成功，
`final-android-build.exit=0`。本报告仅覆盖冻结源码、DEV 构建、签名、包结构、
回传完整性与任务收尾；不代表手机安装、GUI、SAF、后台或真实传输验收。

## 冻结来源与产物

- 产品源码：`07d999b479e25657e6374e413210a0c086c0b853`。
- 产品版本：`0.7.2`；Android versionCode：`70201`。
- DEV 隔离任务：`/home/luoji/code/dashbeam-host-20261115.qxNURz`。
- 源码 manifest：`evidence/product-07d999b.sha256`，覆盖冻结提交的889个 tracked 文件。
- 最终构建前、构建后完整 manifest 均匹配；`final-android-source-check-after.exit=0`。
  没有把生成文件差异忽略后再称为全量匹配。

| 项目 | 最终结果 |
| --- | --- |
| 文件 | `DashBeam-0.7.2-local-android-arm64.apk` |
| 字节数 | `53,473,999` |
| SHA256 | `c38606cc903ac505811ac644e3c1f39a9b87fca26ca01179ad3f02c29a67954a` |
| DEV | `artifacts/final-0.7.2-70201/DashBeam-0.7.2-local-android-arm64.apk` |
| Mac | `/Users/luoji12103/Codex/dashbeam-macos/dist/DashBeam-0.7.2-local-android-arm64.apk` |
| 回传核验 | Mac 与 DEV 独立计算的 SHA256 完全一致 |

构建使用现有 Rust1.91.1、Tauri2.10.1、NDK`29.0.14206865`、JDK21，
两个 Cargo jobs、`CARGO_INCREMENTAL=0`，复用现有 shared target。
Android 保留原 release-profile 默认值；没有运行 Android init、生成新签名身份、
修改产品逻辑、提交/tag/release 或安装手机。

## 签名、版本与结构

| 检查 | 结果与证据 |
| --- | --- |
| 包名 / 版本 | `com.dashbeam.android.local` / `0.7.2` / `70201`；`final-android-badging.txt` |
| Android API | min29 / target36 / compile36 |
| ABI | 只有`arm64-v8a`；所有 native ELF 为 AArch64 |
| APK 签名 | `apksigner verify --verbose --print-certs`通过，v3=true，1 signer |
| 原签名身份 | 与既有已核验 local.4 公开证书记录一致；`final-android-certificate-match.txt`为true |
| 私有签名文件 | 仍为0600；前后 inode、mtime、大小、权限逐项一致，未读取到日志或复制密钥内容 |
| ZIP 完整性 | `unzip -t`通过 |
| 16KiB ZIP | `zipalign -c -P 16 4`通过 |
| 16KiB ELF | `libdashbeam_lib.so`全部4个LOAD段均为`0x4000` |
| 系统分享入口 | SEND、SEND_MULTIPLE与FileProvider均存在 |
| Android 初始化产物 | Gradle wrapper、wrapper jar、生成的TauriActivity前后SHA256均一致 |

本地定制签名证书的公开SHA256为
`fa481ae7c59700dfd66e558c6e7ac528359cbd4b7dfb7f5b35c90d4b491253dc`。
这仅证明本地定制签名沿用，不代表能够覆盖包名或签名不同的官方应用。

## 磁盘停止、归档与重试

首轮在`2026-11-15T20:37:59Z`由4.5GiB磁盘守护安全停止，退出码66，
停止时空闲`4,829,122,560`字节，仍高于4GiB底线。没有下调阈值或清理用户缓存。
该次日志与唯一生成schema差异保存在
`evidence/final-android-attempt1-disk-guard/`；重试前仅把该schema恢复至冻结提交，
完整manifest复验后才继续。

按主任务明确授权，先逐文件SHA256确认Mac归档完整，再仅用
`rsync --remove-source-files`移出三个旧候选APK精确副本、旧ELF解压目录内容和
旧临时`mac-ui-peer`文件，共290,097,902字节。源目录空壳保留，Mac归档仍可恢复：

`/Users/luoji12103/Codex/dashbeam-macos/rollback/source-stream-20261115/dev-archived/`

没有移出旧unsigned APK、共享Cargo/Gradle缓存或用户项目。归档核验记录为
`evidence/final-old-output-archive-check.json`。续编及最终签名仍保留4.5GiB守护。
最终静态包验收时DEV空闲约50.7GiB；额外空间增长未在本任务归因，不能当作上述
290MB移出的效果。

构建结束后确认无Cargo、rustc、Gradle wrapper/worker活动；
只对已按owner、进程class和最后任务cwd确认属于本任务的idle Gradle daemon作
精确TERM收尾，没有广杀用户daemon。共享target已直接交还验收任务，
APK核验和回传没有继续占用它。

## 原始证据与剩余边界

下列文件均位于DEV任务根`evidence/`：

- `final-android-build.txt`、`final-android-build.exit`。
- `final-android-source-commit.txt`、`final-android-source-manifest.sha256`、
  `final-android-source-check-before.txt`、`final-android-source-check-after.txt`及其`.exit`。
- `final-android-signature.txt`、`final-android-previous-signature.txt`、
  `final-android-certificate-match.txt`、`final-android-signing-metadata-{before,after}.txt`。
- `final-android-badging.txt`、`final-android-manifest.txt`、
  `final-android-alignment.txt`、`final-android-elf-alignment.txt`、
  `final-android-native-libs.txt`、`final-android-zip-integrity.txt`。
- `final-android-initialization-before.sha256`、`final-android-initialization-after.txt`、
  `final-android-SHA256SUMS`。

没有连接或安装Android实机。系统分享、通知授权、后台恢复、SAF源文件、
Android↔Mac传输及手机功耗仍未验收。Mac已安装版的独立实测应以本目录
`README.zh-CN.md`中的主任务记录为准，不能用Mac结果替代Android实机结论。
`v0.7.2-local.1`的最终发布与剩余跨端验收继续由主任务处理；本子任务未创建或上传Release。
