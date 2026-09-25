# Clew headless helper protocol

English | [简体中文](HELPER_PROTOCOL.zh-CN.md)

Protocol version: 1.

Production launch is `--headless --config <path> --token-file <path>`. Verge creates a new token file for each start; the token is never placed on the process command line or logged. `--api-token` is rejected in headless mode. Transport is loopback HTTP; ACL-bound named-pipe/service transport is not implemented.

Headless mode does not mount static UI files, register shell/autostart routes, create WebView/tray UI, or apply/recover/stop system DNS. A headless config with `dns.enabled=true` is rejected, and config changes cannot re-enable DNS through the headless observer path.

Except for the low-sensitivity handshake, every `/api/` request—including diagnostic GETs and all HTTP methods—requires `Authorization: Bearer <token>`. Credentials are compared in memory and never logged.

## Version 1 surface

- `GET /api/helper/v1/handshake` — protocol identification.
- `GET /api/helper/v1/capabilities` — authenticated capability declaration.
- `GET /api/helper/v1/status` — authenticated helper readiness and interception state.
- `PUT /api/helper/v1/rules` — authenticated atomic replacement of forwarding rules; returns `effective_version`.
- `POST /api/helper/v1/stop` — authenticated repeat-safe stop request.
- `POST /api/helper/v1/exit` — authenticated repeat-safe helper exit request.

`helper_ready` means the helper's process tree, SOCKET and NETWORK layers are ready. `proxy_ready` is intentionally false with owner `verge`; Verge derives full proxy readiness from Mihomo, listener, helper and effective rule version. There is no single readiness boolean.

Rules are parsed and validated before replacement. Invalid payloads leave the previous configuration intact. A stale non-zero `expected_version` returns conflict. Every accepted replacement is a new revision; the response contains only the actual `effective_version`, with no false idempotency claim. The controller serializes updates and retains that version for subsequent requests.

For per-application routing, the owner sends `proxy_groups` in the same authenticated rule-replacement request as `rules`. Each rule's `proxy_group_id` refers to a backend-owned group ID. The owner creates and checks the corresponding loopback Mihomo listeners before publication. The persisted `strategy_group` field is a Mihomo policy group name or `regular`; it is never interpreted as a user-supplied host, port, or executable path.

Bad authentication, missing/invalid token-file startup, plaintext CLI token rejection, invalid rules, component startup failure, and stop lifecycle are handled by this protocol. Repeated stop/exit requests return the same stopping state without starting another cleanup operation. Port conflicts fail startup before helper readiness.
