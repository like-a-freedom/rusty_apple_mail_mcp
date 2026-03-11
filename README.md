# rusty_apple_mail_mcp

![Rust 2024](https://img.shields.io/badge/rust-2024-orange?style=flat-square&logo=rust)
![Protocol MCP](https://img.shields.io/badge/protocol-MCP-6f42c1?style=flat-square)
![Transport stdio](https://img.shields.io/badge/transport-stdio-0a7ea4?style=flat-square)
![Platform macOS](https://img.shields.io/badge/platform-macOS-111827?style=flat-square&logo=apple)
![Access read-only](https://img.shields.io/badge/access-read--only-15803d?style=flat-square)

Read-only MCP server for Apple Mail on macOS.

It gives an LLM or AI agent fast local access to Apple Mail metadata, message bodies, and attachment text **without AppleScript and without IMAP/POP/EWS network calls**.

## Why this matters

Apple Mail already contains the data an agent needs, but it is buried in a local SQLite index and scattered `.emlx` files.

This project exposes that storage through a small, intent-driven MCP interface so an agent can:

- locate messages by subject, date range, sender, participant, mailbox, or account;
- fetch a full message including metadata, recipients, body, and attachment summary;
- extract readable text from supported attachments;
- operate completely **locally** and **read-only**.

In practice, it empowers AI workflows to search and read your mail archive safely and quickly, without relying on Mail.app automation or network protocols.

## Что умеет сервер

The current tool set is intentionally compact:

| Tool | What it does |
|---|---|
| `search_messages` | Search by subject, dates, sender, participant, account, or mailbox |
| `list_accounts` | Discover account identifiers like `imap://...` or `ews://...` |
| `get_message` | Read one message in full |
| `get_attachment_content` | Extract readable attachment content |
| `list_mailboxes` | List mailboxes/folders with message counts |

## Installation

### Prerequisites

- macOS
- Apple Mail installed and synced at least once
- Rust toolchain (`rustup`, `cargo`)

### Build from source

```bash
cargo build --release
```

### Install locally

```bash
cargo install --path .
```

This installs the binary under the name `rusty_apple_mail_mcp`.

## Running the server

To start from source:

```bash
APPLE_MAIL_PRIMARY_EMAIL="you@example.com" cargo run --release
```

Or with the installed binary:

```bash
APPLE_MAIL_PRIMARY_EMAIL="you@example.com" rusty_apple_mail_mcp
```

For interactive experimentation, use the MCP Inspector:

```bash
npx -y @modelcontextprotocol/inspector ./target/release/rusty_apple_mail_mcp
```

## Configuration

The server is configured **only** through environment variables:

| Variable | Required | Default | Description |
|---|---|---|---|
| `APPLE_MAIL_DIR` | no | `~/Library/Mail` | Root folder of the Mail data |
| `APPLE_MAIL_VERSION` | no | `V10` | Envelope Index version subdirectory |
| `APPLE_MAIL_PRIMARY_EMAIL` | yes | — | Primary account email for startup validation |

Example setup:

```bash
export APPLE_MAIL_DIR="$HOME/Library/Mail"
export APPLE_MAIL_VERSION="V10"
export APPLE_MAIL_PRIMARY_EMAIL="you@example.com"
```

## VS Code integration

Example minimum `.vscode/mcp.json` configuration:

```json
{
    "servers": {
        "mail_mcp": {
            "command": "rusty_apple_mail_mcp",
            "args": [],
            "env": {
                "APPLE_MAIL_DIR": "/Users/your-user/Library/Mail",
                "APPLE_MAIL_VERSION": "V10",
                "APPLE_MAIL_PRIMARY_EMAIL": "you@example.com"
            }
        }
    }
}
```

## Usage

Typical usage pattern:

1. Call `list_accounts` to see available account identifiers or scope your query.
2. Use `search_messages` to build a shortlist of candidates.
3. Call `get_message` to fetch the full message you care about.
4. Use `get_attachment_content` when you need the text of a particular attachment.

### Sample search request

```json
{
    "subject_query": "invoice",
    "account": "imap://ACCOUNT-ID",
    "limit": 10
}
```

### Sample message retrieval

```json
{
    "message_id": "12345",
    "include_body": true,
    "include_attachments_summary": true,
    "body_format": "text"
}
```

## How it works

The server draws from two local sources:

- **Envelope Index** – the SQLite database Apple Mail uses as a metadata index and relationship store
- **.emlx files** – the canonical bodies and attachments for individual messages

Search queries hit the lightweight index, keeping them fast; bodies and attachments are loaded on-demand from the `.emlx` files.

## Development

Handy commands while working on the codebase:

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo doc --no-deps
```

## Documentation

- `docs/APPLE_MAIL_MCP_SRS.md` — product and behavior requirements
- `docs/APPLE_MAIL_MCP_IMPL_PLAN.md` — implementation notes and architecture
- `docs/SMOKE_TEST.md` — real-data smoke testing on macOS
- `docs/INTENT_DRIVEN_MCP_DESIGN_GUIDE.md` — design guidance for focused MCP tools

## Limitations

- macOS only
- read-only access only
- stdio transport only
- requires Apple Mail storage to be present on disk
- some binary attachment formats may yield metadata instead of extracted text

## TL;DR

Need an AI agent to safely search and read Apple Mail on your Mac? This project provides a clean, read-only MCP layer over Apple Mail’s native storage—fast index-based searches, on‑demand body hydration from `.emlx`, and zero write access.
