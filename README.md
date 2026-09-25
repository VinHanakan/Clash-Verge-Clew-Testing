# Clash Verge Clew Testing

English | [简体中文](README.zh-CN.md)

This is an independent experimental fork of [Clash Verge Rev](https://github.com/clash-verge-rev/clash-verge-rev), **not** an official release or support channel. Modification notice: this fork adds application-proxy integration, related UI changes, and independent branding through September 2026. The standalone Windows installer is disabled until service and uninstall isolation is verified. Only the restricted `internal-test` package is eligible for manually triggered Actions artifacts; there are no GitHub Releases or automatic updates.

## Testing build lineage

- Clash Verge Rev base: **2.5.4**.
- Clew source import baseline: **v0.10.0** (vendored and modified for headless integration; the built helper is identified by this repository's source commit and its binary hash, not by the unmodified upstream tag alone).
- Clash Verge Clew integration: **0.1.0-test.1**.
- Source revision: the exact commit shown on the [manual artifact workflow run](.github/workflows/internal-test-artifact.yml) and in its `build-provenance.json`; `main` is not a build identifier.
- Target: Windows 10/11 x64. The first artifact from this workflow requires installation and behavior testing; an Actions success alone does not prove installed forwarding or coexistence.

To obtain a limited test installer, open **Actions → Internal test installer (manual) → Run workflow** on the intended source commit. Download the resulting artifact, read `build-provenance.json`, and compare the installer against `SHA256SUMS.txt` before installing it in an isolated Windows test environment. The unsigned installer may trigger SmartScreen. It is an Actions artifact with limited retention, **not** a Release. Report problems through this repository's Issues with the source SHA, artifact SHA-256, Windows version, and redacted logs; never include subscriptions, credentials, or node configuration.

The Windows internal build adds per-application proxy rules using [Clew](https://github.com/ymonster/clew-proxy) and managed [Mihomo](https://github.com/MetaCubeX/mihomo) listeners. Multiple applications can use different existing Mihomo strategy groups or follow regular rules. The application-proxy feature currently intercepts **Windows IPv4 TCP only**. IPv6 and UDP use the existing system/application network path and may bypass the app proxy. Rules do not auto-start or auto-resume, and profile/subscription switching is blocked while application proxy runs.

The internal installer is unsigned. It is intended for isolated testing and has not completed the full lifecycle and failure-recovery matrix. The original Clash Verge installation, service, and proxy settings must remain untouched. See [architecture](docs/ARCHITECTURE.md), [build instructions](docs/BUILD.md), and [known limitations](docs/KNOWN_LIMITATIONS.md). Application updates and upstream deep-link registration are disabled.

## Attribution and licenses

- The application and modifications retain the [GPL-3.0 license](LICENSE). Provide corresponding source when distributing a binary; tag the source tree matching each release build.
- Integrated Clew code retains its [MIT license](vendor/clew/LICENSE) and [third-party notices](vendor/clew/THIRD_PARTY_NOTICES.md).
- Bundled WinDivert terms are in its [license file](vendor/clew/WinDivert-2.2.2-A/LICENSE). Preserve the applicable notices with source and installer distributions.
- Clash Verge Rev and its upstream [Clash Verge](https://github.com/zzzgydi/clash-verge) contributors remain credited. This fork does not imply their endorsement.

Before any public release, isolate the service and uninstall paths and audit repository history for credentials and private configuration. Automatic updates are disabled; use a fork-specific update channel and signing key only after a separate review. The [upstream README](https://github.com/clash-verge-rev/clash-verge-rev#readme) describes the original project's releases and support channels, not this fork's.

## Development

See [build instructions](docs/BUILD.md) and the [helper protocol](docs/HELPER_PROTOCOL.md). The internal build compiles the vendored Clew helper and prepares Mihomo sidecars; a generic `pnpm build` does not produce an app-proxy installer.
