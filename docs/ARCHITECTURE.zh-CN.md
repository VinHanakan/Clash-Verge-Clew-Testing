# 应用代理架构

[English](ARCHITECTURE.md) | 简体中文

“应用流量”页面管理应用规则列表。每条启用的规则绑定到 `regular`（遵循 Mihomo 常规规则）或一个实际存在的 Mihomo 策略组。后端验证指定组存在于当前活动配置，为每种不同策略创建一个仅监听回环地址的受管 Mihomo listener，并根据 listener 映射生成各规则的 helper `proxy_group_id`。共用策略的应用共用 listener；端口和 helper ID 由后端生成，不由界面提供。helper 运行期间保留基础 `regular` listener，因此从一个指定组切换到另一组时，不依赖旧组继续存在。

Verge 的配置生成流程按名称将期望的受管 listener 合并到运行配置。控制器确认实际加载的 listener 名称、端口和指定组后，才报告 `proxy_ready`。它还要求 helper 认证及就绪、预期的 `rules_revision`、进程存活，以及配置更新 epoch 稳定。界面不能只凭端口可连接就推断就绪。

Clew 以提权的 headless helper 运行，Verge 界面进程保持普通权限。每次启动使用具有受限 Windows ACL 的独立凭据文件控制回环 helper；token 不经命令行传递，也不写入日志。helper 接收带版本号的规则与代理组原子更新。正常更新时，后端先准备新 listener，发布并确认 helper revision，持久化规则，最后回收不用的旧 listener。已确认的发布失败会恢复新准备的 listener；若无法确定发布或回滚结果，则停止本实例持有的 helper，报告失败／降级状态，而不再宣称就绪。启动、规则修改与停止共用控制器锁。

清理范围仅包括当前 Verge 实例的 helper、凭据和命名 listener。`internal-test` 构建使用独立应用标识及数据目录，禁止系统代理写入和上游更新。应用代理运行期间拒绝切换 profile／订阅；运行中热迁移尚不是已验收能力。退出后保存的规则仍在，但转发不会自动启动。

遥测有两种不同来源：helper 运行时，页面查询其截流的 TCP 数据；helper 停止时，使用只读的 Windows 进程／TCP 快照展示系统连接。helper 遥测失败会明确显示，而不是悄悄退回系统直连遥测。
