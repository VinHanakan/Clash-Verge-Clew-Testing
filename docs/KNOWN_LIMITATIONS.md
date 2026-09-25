# Known limitations

English | [简体中文](KNOWN_LIMITATIONS.zh-CN.md)

- Application interception is Windows IPv4 TCP only. IPv6 and UDP are **not intercepted by this feature**; they continue on the operating system/application's existing network path and can bypass the selected application outlet. This is not a leak-proof or strict blocking mode.
- An application that explicitly uses a local or remote proxy is not guaranteed to retain its original process identity in Mihomo. Do not assume its HTTP/SOCKS traffic follows the same policy as direct TCP without a separate end-to-end test.
- Loopback, LAN and DNS behavior follows the current Clew/Mihomo exclusion and resolution paths; this feature is not a whole-system DNS or TUN replacement. The app-proxy module does not change system DNS or system proxy settings.
- Profile/subscription switching is blocked while forwarding is active. Stop the application proxy first, switch, then validate group references and start manually. Saved rules do not imply automatic forwarding after restart, logon or app launch.
- Fixed-group names must exist in the currently active Mihomo configuration. A deleted or renamed group produces an error/unavailable state; it must not silently become `regular` or direct.
- Rule matching uses Clew's command-line/process behavior. Broad interpreter rules such as all `node.exe` or `python.exe` are rejected; use a specific script command line. Windows process telemetry may not expose a complete command line, so the process list alone cannot always create a safe script rule.
- Overlapping application rules bound to different outlets have no accepted priority contract yet. Use distinct executable paths or command-line patterns for each outlet; do not rely on list order to resolve overlap.
- The helper API currently has a single local instance on port `18080`. An existing owner causes a start error and must not be killed or treated as this instance's helper.
- The internal installer is unsigned. Application update checks, installation, artifacts and endpoints are disabled. No automatic update, driver auto-repair, crash-proof blocking, or complete hot configuration migration is claimed.
- The default tray icon uses the same artwork in each mode; the tray tooltip and menu still show proxy mode. Mode-specific icon variants have not been recreated.
- The internal-test build always starts Mihomo as a sidecar and rejects service installation, repair and removal commands. It does not manage the upstream Clash Verge service.
- The standalone Windows installer remains disabled until its inherited service and uninstall paths are independent of Clash Verge Rev. Only the isolated internal-test package may be built; its application data does not migrate automatically from the previous internal-test identifier.
- Rust unit tests currently cannot start on the development machine because the test executable returns `0xc0000139`; this is a runtime-dependency blocker, not a passed test.
