# Release Notes — v1.9.0

## What's New

- **YAML config file support** — `config.yaml` is now loaded from next to the binary or `~/.config/rusty_apple_mail_mcp/config.yaml`. Priority chain: CLI flags > env vars > config.yaml > defaults.
- **`log_level` config option** — new `APPLE_MAIL_LOG_LEVEL` env var and `log_level` YAML key for server log verbosity.
- **`config.example.yaml`** — reference config shipped with the project.

## Fixes

- Account filter error now returns actionable guidance when account metadata is unavailable (ADR-0005).

## Dependencies

Bumped: tokio, serde, serde_json, thiserror, anyhow, clap to latest.

## Documentation

README configuration section rewritten to cover all three config sources.
