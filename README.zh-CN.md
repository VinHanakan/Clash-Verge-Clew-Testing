# Clash Verge Clew Testing

[English](README.md) | 简体中文

这是基于 [Clash Verge Rev](https://github.com/clash-verge-rev/clash-verge-rev) 的独立实验性修改版，**不是官方版本或官方支持渠道**。修改说明：截至 2026 年 9 月，本项目增加了应用代理集成、相关界面及独立品牌资源。正式独立 Windows 安装器仍被禁用，须先完成服务及卸载路径隔离。只有受限的 `internal-test` 包可通过手动触发的 Actions 生成 Artifact；目前没有 GitHub Release，也没有自动更新。

## 测试包版本溯源

- Clash Verge Rev 基线：**2.5.4**。
- Clew 源码导入基线：**v0.10.0**。仓库内的源码已为 headless 集成修改；实际 helper 应以本仓库的源码提交及二进制哈希识别，不能视为未经修改的上游版本。
- Clash Verge Clew 整合版本：**0.1.0-test.1**。
- 源码版本：以[手动构建工作流](.github/workflows/internal-test-artifact.yml)所用的确切提交及 Artifact 内的 `build-provenance.json` 为准；`main` 不是固定构建标识。
- 目标平台：Windows 10/11 x64。工作流成功只代表完成构建，不代表这份安装包已通过安装、实际转发或共存测试。

获取受限测试安装包：进入 **Actions → Internal test installer (manual) → Run workflow**，在目标源码提交上手动运行。下载生成的 Artifact，查看 `build-provenance.json`，并用 `SHA256SUMS.txt` 核对安装器哈希后，再在隔离的 Windows 测试环境中安装。安装器未签名，可能触发 SmartScreen；它是有保留期限的 Actions Artifact，**不是 Release**。反馈请使用本仓库 Issues，提供源码 SHA、安装包 SHA-256、Windows 版本和脱敏日志；不要上传订阅、凭据或节点配置。

Windows 内部测试版通过 [Clew](https://github.com/ymonster/clew-proxy) 和受管 [Mihomo](https://github.com/MetaCubeX/mihomo) listener 提供按应用代理规则。多个应用可选择不同的现有 Mihomo 策略组，或遵循常规规则。应用代理目前**只接管 Windows IPv4 TCP**；IPv6 和 UDP 仍走系统／应用原有网络路径，可能绕过所选出口。规则不会自动启动或恢复；应用代理运行期间禁止切换 profile 或订阅。

内部安装器未签名，仅供隔离环境测试，尚未通过完整生命周期和失败恢复矩阵。不得影响原版 Clash Verge 的安装、服务和系统代理设置。详见[架构](docs/ARCHITECTURE.zh-CN.md)、[构建说明](docs/BUILD.zh-CN.md)及[已知限制](docs/KNOWN_LIMITATIONS.zh-CN.md)。应用更新和上游 deep-link 注册均已禁用。

## 来源与许可

- 应用及其修改保留 [GPL-3.0 许可](LICENSE)。分发二进制时须提供对应源码；正式版本应以与构建对应的源码标签标识。
- 集成的 Clew 代码保留其 [MIT 许可](vendor/clew/LICENSE)及[第三方声明](vendor/clew/THIRD_PARTY_NOTICES.md)。
- 随包 WinDivert 的条款见其[许可文件](vendor/clew/WinDivert-2.2.2-A/LICENSE)。源码及安装包分发时应保留适用声明。
- 感谢 Clash Verge Rev 及其上游 [Clash Verge](https://github.com/zzzgydi/clash-verge) 的贡献者；本项目不代表他们的认可或背书。

公开发布正式安装包之前，仍须隔离服务与卸载路径，并审查仓库历史中的凭据或私人配置。自动更新已禁用；只有另行审查后才可启用本项目专属的更新渠道和签名密钥。[上游 README](https://github.com/clash-verge-rev/clash-verge-rev#readme)介绍的是原项目的发行与支持渠道，并非本项目。

## 开发

参见[构建说明](docs/BUILD.zh-CN.md)和 [helper 协议](docs/HELPER_PROTOCOL.zh-CN.md)。内部测试构建会编译仓库内的 Clew helper 并准备 Mihomo sidecar；普通的 `pnpm build` 不会生成应用代理测试安装包。
