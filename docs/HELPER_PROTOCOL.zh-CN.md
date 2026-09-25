# Clew headless helper 协议

[English](HELPER_PROTOCOL.md) | 简体中文

协议版本：1。

生产启动参数为 `--headless --config <path> --token-file <path>`。Verge 每次启动均创建新 token 文件；token 不会放在进程命令行或日志中。headless 模式拒绝 `--api-token`。传输方式是回环 HTTP；尚未实现以 ACL 约束的命名管道或服务传输。

headless 模式不挂载静态 UI 文件，不注册 shell／自启动路由，不创建 WebView／托盘 UI，也不应用、恢复或停止系统 DNS。若 headless 配置中 `dns.enabled=true`，会拒绝该配置；headless 配置观察路径也不能通过后续变更重新启用 DNS。

除敏感度较低的握手接口外，**所有** `/api/` 请求——包括诊断 GET 和各种 HTTP 方法——都要求 `Authorization: Bearer <token>`。凭据仅在内存中比较，不记录到日志。

## 版本 1 接口

- `GET /api/helper/v1/handshake`：协议标识。
- `GET /api/helper/v1/capabilities`：认证后的能力声明。
- `GET /api/helper/v1/status`：认证后的 helper 就绪及拦截状态。
- `PUT /api/helper/v1/rules`：认证后的转发规则原子替换，返回 `effective_version`。
- `POST /api/helper/v1/stop`：认证后的可重复停止请求。
- `POST /api/helper/v1/exit`：认证后的可重复 helper 退出请求。

`helper_ready` 只表示 helper 的进程树、SOCKET 和 NETWORK 层已就绪。由 Verge 负责计算完整代理就绪，helper 自身的 `proxy_ready` 固定为 false、owner 为 `verge`；Verge 须综合 Mihomo、listener、helper 和实际规则版本。两者不是同一个布尔状态。

替换前先解析并校验规则。无效请求保留先前配置。过期的非零 `expected_version` 返回冲突。每次被接受的替换都会产生新 revision；响应只包含实际生效的 `effective_version`，不声称虚假的幂等性。控制器串行化更新，并保存版本供后续请求使用。

按应用路由时，控制端在同一次认证规则替换请求中发送 `proxy_groups` 与 `rules`。各规则的 `proxy_group_id` 引用后端持有的组 ID。控制端先创建并检查对应的回环 Mihomo listener，再发布规则。持久化的 `strategy_group` 是 Mihomo 策略组名称或 `regular`，不会被解释成用户提供的主机、端口或可执行文件路径。

协议处理错误认证、token 文件缺失或无效、命令行明文 token 被拒、规则无效、组件启动失败及停止生命周期。重复 stop／exit 请求返回相同的停止中状态，不会再启动一次清理。端口冲突在 helper 就绪前使启动失败。
