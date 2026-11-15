# Linux 首次 CI 失败定位

记录日期：2026-11-15。此文件只保留脱敏诊断要点；不包含 token、私有配置或完整环境转储。

- Run：`43759620277`
- Workflow dispatcher：`9a2bba2af5a3a9cf0809b2974383b94d9fe365f1`
- 实际产品 checkout：`07d999b479e25657e6374e413210a0c086c0b853`
- 结果：`failure`，完成于 `2026-11-15T20:46:03Z`
- 失败步骤：`Rebuild and check WASM bridge`
- 后续 app tests、node integration、device_probe 和 deb 构建均 skipped，不能当作失败或通过的产品测试。

## 直接证据

`Set up Rust` 安装了 `1.91.1` 工具链及它的 WASM target。随后 rustup 明确提示：

```text
toolchain '1.91-x86_64-unknown-linux-gnu' is currently in use
(overridden by '.../rust-toolchain.toml')
the missing active toolchain `1.91-x86_64-unknown-linux-gnu` has been auto-installed
```

两者的编译器版本字符串相同，但 rustup 安装名称不同。`wasm-bindgen-cli 0.2.126` 的安装已完成；真正失败发生在随后产品 WASM 构建：

```text
2026-11-15T20:45:59Z
error[E0463]: can't find crate for `core`
note: the `wasm32-unknown-unknown` target may not be installed
error: could not compile `pin-project-lite` (lib) due to 1 previous error
```

这不是 wasm-bindgen CLI 的 MSRV 错误，也没有产品源码编译诊断。

## 最小 CI 修复

仅修改 `.github/workflows/linux-local-release.yml`：

- 设置 job 环境 `RUSTUP_TOOLCHAIN: "1.91.1"`，使 Cargo 与已安装 WASM target 使用同一安装。
- 记录 `rustup show active-toolchain`、`rustup target list --installed`。
- 在构建前确认当前 sysroot 中存在 `wasm32-unknown-unknown/lib`。

未修改产品源码、依赖或 lockfile。主任务提交 CI 修复为 `93797cd`，
并启动替代 run `43760222998`；其产品输入仍固定为 `07d999b`。
此记录不预先宣称替代 run 成功。

原始构建日志 artifact 下载在
`dist/final-0.7.2-linux/ci-43759620277-logs/`，仅供本地诊断；
这里没有复制完整日志进 Git 或发布目录。
