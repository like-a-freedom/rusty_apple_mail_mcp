# Release Notes — v1.11.0

## Changes

- **Migrated to the rmcp 3.1 SDK (MCP 2026-07-28 spec-sensitive).** The server now runs on `rmcp 3.1.0` (upgraded from `2.2.0`), moving to the new MCP Result framework response types:
  - `ServerHandler::call_tool` returns `CallToolResponse` (the `Complete` variant is wire-identical to the previous direct tool result for clients negotiating protocol `2024-11-05`).
  - `tools/list` results are built with `ListToolsResult::with_all_items`, which stays wire-compatible with legacy clients while remaining spec-valid for future 2026-07-28 peers.
  - Tool names, input/output schemas, and error codes are unchanged — existing MCP clients and the CLI work without modification.

## Internal

- Local clippy gate aligned with CI (`--all-targets --all-features`).
- `.worktrees/` directories added to `.gitignore`.

Full quality gate green on this release: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` (411 tests), `cargo doc --no-deps`. Stdio smoke test (`initialize` + `tools/list`) returns 2 results, 0 errors.