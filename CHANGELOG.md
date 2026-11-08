# Changelog

All notable changes to this project are documented in this file.

## [1.13.0] - 2026-11-08

### Security
- Fail closed on unparseable mailbox URLs when an account allowlist is active, so out-of-scope mailboxes can no longer bypass the scope.
- Sanitize external attachment filenames to block path traversal (e.g. `../../secret.txt`) when looking up attachment files on disk.
- Cap `.emlx` file reads and external attachment reads at 256 MiB to prevent memory exhaustion from attacker-controlled messages.
- Cap decompressed ZIP entry size at 32 MiB for DOCX, XLSX, and PPTX extraction to mitigate decompression bombs.

### Fixed
- Validate MIME attachment name before searching for an external attachment file.

### Changed
- Updated all direct dependencies to their latest available releases.

[1.13.0]: https://github.com/like-a-freedom/rusty_apple_mail_mcp/compare/v1.12.0...v1.13.0

## [1.12.0] - 2026-10-14

### Changed
- Updated direct dependencies to their latest available releases and removed the fixed minimum Rust version.

### Fixed
- Preserve predefined and numeric XML character references in text extracted from XLSX, DOCX, and PPTX files.

[1.12.0]: https://github.com/like-a-freedom/rusty_apple_mail_mcp/compare/v1.11.0...v1.12.0
