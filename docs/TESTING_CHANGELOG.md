# Testing changes / 测试版变更

## 0.1.0-test.4

- 恢复内部安装版正常使用时的系统代理开关；仅显式启用隔离模式的测试实例禁止写入系统代理。
- Restore system-proxy controls during normal use of the internal installer; only explicitly isolated test instances prohibit system-proxy writes.

For safe local acceptance, set `CLASH_VERGE_DEV_DISABLE_SYSTEM_PROXY_WRITES=1` before launching the isolated instance. Installing the internal package alone does not enable this restriction.

安全的本机隔离验收须在启动前设置 `CLASH_VERGE_DEV_DISABLE_SYSTEM_PROXY_WRITES=1`。仅安装内部测试包不再自动启用此限制。

## 0.1.0-test.3

- 修复进程搜索展示无关子进程，支持按名称、PID、路径和命令行过滤。
- 优化进程列表滚动与输入响应，仅渲染可见行。
- 精简应用规则卡片，完整路径和匹配信息保留在编辑窗口。
- 新增系统信息中的 Clash Verge Rev、Clew 和整合版本显示。
- 移除实验性透明主题，已有透明主题设置迁移为深色。
- 新增托盘状态标记，区分开启、关闭和无法读取的代理状态。
- 新增应用代理启停状态保存，正常退出后下次启动尝试恢复，可能需要 UAC 确认。
- 修复隔离测试版系统代理开关的虚假成功提示，改为只读显示实际 Windows 状态。
- 调整 helper 资源目录，避免更新包覆盖旧版本使用中的驱动路径；占用文件清理可延至重启。
- 新增测试版更新检查与后台下载界面；自动下载源尚未发布，安装仍由用户择时执行。

English:

- Fix process search retaining unrelated descendants; filter by name, PID, path and command line.
- Improve process-list scrolling and input response by rendering visible rows only.
- Simplify rule cards; retain full paths and match details in the editor.
- Show the Clash Verge Rev, Clew and integration versions in System Information.
- Remove the experimental glass theme and migrate existing glass settings to dark mode.
- Add tray state badges for enabled, disabled and unreadable proxy state.
- Save application-proxy intent and attempt restoration after a normal restart; UAC confirmation may be required.
- Replace misleading system-proxy toggle success in isolated builds with a read-only view of actual Windows state.
- Version helper resource directories to avoid overwriting an older loaded driver path; locked-file removal can wait until reboot.
- Add testing-update checks and background-download UI; no automatic download source has been published, and installation remains manual.

This candidate does not close the remaining Phase 3 failure-recovery and configuration-lifecycle acceptance items. The reported intermittent system-proxy shutdown and installer `.sys` error are not yet explained by a captured trace.

本候选版不代表阶段 3 剩余失败恢复及配置生命周期验收全部通过。用户报告的系统代理间歇性关闭与安装时 `.sys` 报错，尚未取得能确定原因的现场日志。
