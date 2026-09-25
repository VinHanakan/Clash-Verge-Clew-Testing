# Windows 内部测试版构建

[English](BUILD.md) | 简体中文

本文说明隔离、未签名的开发测试包。构建步骤不会在构建机上安装或启动应用。

## 构建输入

- Windows x64、仓库指定的 Rust/MSVC 工具链、Node/pnpm 依赖，以及现有的 Visual Studio 与 Windows SDK。
- 打包前须从当前仓库源码构建 Clew helper。内部包读取 `vendor/clew/build/Release/clew.exe` 及相邻的 WinDivert、Brotli 和 VC143 CRT 文件。应记录它们的哈希，不得混用其他构建目录的 helper。
- 手动 Artifact 工作流将 Mihomo 稳定版固定为 `v1.19.31`，并在每次构建开始时确定当次滚动 Alpha 版本。实际 Alpha 版本、helper 与 sidecar 的 SHA-256、源码提交均写入 `build-provenance.json`；上游可能在以后移除该 Alpha 产物。安装包不得依赖 `CLEW_DEV_HELPER_PATH`。

## 命令（PowerShell，在仓库根目录执行）

```powershell
pnpm install --frozen-lockfile
pnpm typecheck
pnpm web:build
cargo check --locked --manifest-path src-tauri/Cargo.toml --features internal-test
& .\node_modules\.bin\tauri.cmd build --debug --features internal-test --bundles nsis --config src-tauri/tauri.internal-test.conf.json --no-sign
Get-ChildItem 'target/debug/bundle/nsis/*-setup.exe' | Get-FileHash -Algorithm SHA256
```

以上命令假定 helper、Mihomo sidecar 和资源已按前述要求准备好；可参考[手动安装包工作流](../.github/workflows/internal-test-artifact.yml)及其[原生输入准备脚本](../.github/scripts/prepare-internal-test.ps1)。

基础配置不会生成安装器。只有 internal-test 覆盖配置启用打包；NSIS 模板在继承的服务与卸载操作完成隔离前，会拒绝其他标识。该覆盖配置使用 `app.clashvergeclew.desktop.internal-test`，其新数据目录不会迁移旧 internal-test profile。它还关闭了自动前端构建步骤，因此必须在 `tauri build` 前执行 `pnpm web:build`。`--no-sign` 仅用于明确标为未签名的内部产物。Updater 产物和更新端点均已禁用。

Linux 与 macOS 打包不受支持。两者的 Tauri 覆盖配置均禁用 bundle，不再包含继承的包生命周期脚本或上游包标识。在服务、数据和卸载路径完成隔离与测试前，不要强制启用打包。

Windows 源码检查 CI 的 `cargo check` 使用临时占位文件来满足 Tauri 对 sidecar、资源及前端路径的要求。独立的[手动安装包工作流](../.github/workflows/internal-test-artifact.yml)则构建真实 helper 和未签名 NSIS 包，记录确切输入及 SHA-256。两者都不能证明安装后的行为。

分发新安装包前，应在本地构建记录中保存其 SHA-256、源码提交及**任何工作树差异**、helper／驱动／sidecar 哈希、生成的 NSIS 脚本资源项和 Authenticode 状态。不要把本机路径、凭据或测试日志提交进源码快照。只在隔离 Windows 测试环境中，与可用的原版 Clash 并存测试。构建成功不等于实际转发或共存已通过。
