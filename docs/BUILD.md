# Internal Windows build

This describes the isolated, unsigned development package. It does not install or launch the app on the build workstation.

## Inputs

- Windows x64, the repository-pinned Rust/MSVC toolchain, Node/pnpm dependencies, and the repository's existing Visual Studio/Windows SDK installation.
- Build the pinned Clew helper separately before packaging. The internal package reads `vendor/clew/build/Release/clew.exe` plus its adjacent WinDivert, Brotli and VC143 CRT files. Record their hashes in the build record; do not substitute a helper from a different build directory.
- The manual artifact workflow fixes Mihomo stable to `v1.19.31` and resolves the rolling Alpha once at build start. The precise Alpha version, helper and sidecar SHA-256, and source commit are recorded in `build-provenance.json`; Alpha assets may be removed upstream later. The package must not depend on `CLEW_DEV_HELPER_PATH`.

## Commands (PowerShell, repository root)

```powershell
pnpm install --frozen-lockfile
pnpm typecheck
pnpm web:build
cargo check --locked --manifest-path src-tauri/Cargo.toml --features internal-test
& .\node_modules\.bin\tauri.cmd build --debug --features internal-test --bundles nsis --config src-tauri/tauri.internal-test.conf.json --no-sign
Get-ChildItem 'target/debug/bundle/nsis/*-setup.exe' | Get-FileHash -Algorithm SHA256
```

The base configuration does not produce an installer. Only the internal overlay enables bundling; the NSIS template rejects other identifiers until inherited service and uninstall operations are isolated. This overlay uses `app.clashvergeclew.desktop.internal-test`, a new data directory that does not migrate the previous internal-test profile. It also disables the automatic frontend build step, so `pnpm web:build` must precede `tauri build`. Use `--no-sign` only for an explicitly unsigned internal artifact. Updater artifacts and endpoints are disabled.

Linux and macOS packaging are unsupported. Their Tauri overlays disable bundling and no longer include inherited package lifecycle scripts or upstream package identities. Do not force-enable bundling for those platforms until their service, data, and uninstall paths have been isolated and tested.

The Windows source-check CI `cargo check` uses temporary placeholders for Tauri's required sidecar, resource, and frontend paths. The separate, manually triggered [installer workflow](../.github/workflows/internal-test-artifact.yml) builds the real helper and unsigned NSIS package and records the exact inputs and SHA-256. Neither workflow validates installed behavior.

Before distributing a new installer, record its SHA-256, source commit **and any working-tree diff**, helper/driver/sidecar hashes, generated NSIS script resource entries, and Authenticode status in a local build record. Do not publish machine paths, credentials, or test logs with the source snapshot. Install and exercise it only in an isolated Windows test environment alongside a known-good original Clash. A successful build is not evidence of installed forwarding or coexistence.
