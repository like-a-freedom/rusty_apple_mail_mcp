# Changelog

All notable changes to this project are documented in this file.

## [1.14.0] - 2026-11-12

### Added
- `skills` subcommand that serves the bundled agent skill, so an installed CLI can tell an agent how to use itself. `skills get apple-mail-cli` prints the guidance, `--ref <name>` loads one reference, and `--json` returns the same documents as an envelope.
- `--version` prints the package version, so an agent can check which version it is reading instructions for.

### Changed
- A validation failure shaped like a bad request now reports `error_kind: "usage_error"` instead of `"internal_error"`. Operational conditions keep a kind of their own: a missing body file reports `io_failure`, an excluded account reports `account_filter_invalid`, and a stale source revision or an out-of-range offset reports `usage_error` because the caller must correct the request.
- `--pretty` is accepted after the subcommand as well as before it.
- The help text and `--version` output use the installed binary name, `rusty_apple_mail_mcp`.

[1.14.0]: https://github.com/like-a-freedom/rusty_apple_mail_mcp/compare/v1.13.0...v1.14.0

## [1.13.0] - 2026-11-08

### Changed
- Updated all direct dependencies to their latest available releases.

### Fixed
- Validate MIME attachment name before searching for an external attachment file.

### Security
- Fail closed on unparseable mailbox URLs when an account allowlist is active, so out-of-scope mailboxes can no longer bypass the scope.
- Sanitize external attachment filenames to block path traversal (e.g. `../../secret.txt`) when looking up attachment files on disk.
- Cap `.emlx` file reads and external attachment reads at 256 MiB to prevent memory exhaustion from attacker-controlled messages.
- Cap decompressed ZIP entry size at 32 MiB for DOCX, XLSX, and PPTX extraction to mitigate decompression bombs.

[1.13.0]: https://github.com/like-a-freedom/rusty_apple_mail_mcp/compare/v1.12.0...v1.13.0

## [1.12.0] - 2026-10-14

### Changed
- Updated direct dependencies to their latest available releases and removed the fixed minimum Rust version.

### Fixed
- Preserve predefined and numeric XML character references in text extracted from XLSX, DOCX, and PPTX files.

[1.12.0]: https://github.com/like-a-freedom/rusty_apple_mail_mcp/compare/v1.11.0...v1.12.0
