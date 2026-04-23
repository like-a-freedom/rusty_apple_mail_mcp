# Rusty Apple Mail MCP Server

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

In practice, it empowers AI workflows to search and read your mail archive safely and quickly, without relying on Mail.app automation like AppleScript (which damn slow and throws timeouts regularly) or network protocols.

## What the server can do

The current tool set is intentionally compact:

| Tool | What it does |
|---|---|
| `search_messages` | Search by subject, dates, sender, participant, account, or mailbox |
| `list_accounts` | Discover account identifiers; set `include_mailboxes=true` for combined account+mailbox discovery |
| `get_message` | Read one message in full; recipients omitted by default (`include_recipients=true` to include) |
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

The server supports two operating modes:

### MCP Mode (default)

In MCP mode, the server communicates via stdin/stdout using the Model Context Protocol. This is the primary mode for integration with AI agents, Claude Code, VS Code, and other MCP-compatible clients.

To start from source:

```bash
cargo run --release
```

Or with the installed binary:

```bash
rusty_apple_mail_mcp
```

For interactive experimentation, use the MCP Inspector:

```bash
npx -y @modelcontextprotocol/inspector ./target/release/rusty_apple_mail_mcp
```

### CLI Mode

In CLI mode, you can run individual commands directly from the terminal. This is useful for:

- **Scripting** — automation of mail search tasks in shell scripts
- **Debugging** — quick testing without setting up an MCP client
- **Integration** — piping results to other command-line tools
- **One-off queries** — when you need a quick answer without starting a persistent server

#### Usage

```bash
# List all accounts
rusty_apple_mail_mcp list-accounts
rusty_apple_mail_mcp list-accounts --include-mailboxes

# List all mailboxes
rusty_apple_mail_mcp list-mailboxes

# Search messages
rusty_apple_mail_mcp search --subject-query "invoice"
rusty_apple_mail_mcp search --sender "john@example.com" --limit 10
rusty_apple_mail_mcp search --date-from "2024-01-01" --date-to "2024-12-31"
rusty_apple_mail_mcp search --mailbox "INBOX" --include-body-preview

# Get a specific message
rusty_apple_mail_mcp get-message --message-id "12345"
rusty_apple_mail_mcp get-message --message-id "12345" --body-format html

# Get attachment content
rusty_apple_mail_mcp get-attachment --message-id "12345" --attachment-id "12345:0"
```

#### CLI Configuration

CLI mode supports the same configuration options as MCP mode:

| Option | Env Variable | Description |
|---|---|---|
| `--mail-directory` | `APPLE_MAIL_DIR` | Mail data directory (default: `~/Library/Mail`) |
| `--mail-version` | `APPLE_MAIL_VERSION` | Envelope Index version (default: `V10`) |
| `--account` | `APPLE_MAIL_ACCOUNT` | Account selector(s) |

Example:

```bash
rusty_apple_mail_mcp --mail-directory ~/Library/Mail --mail-version V10 search --subject-query "meeting"
```

Or with environment variables:

```bash
export APPLE_MAIL_DIR="$HOME/Library/Mail"
export APPLE_MAIL_VERSION="V10"
rusty_apple_mail_mcp list-accounts
```

#### CLI vs MCP: Key Differences

| Feature | MCP Mode | CLI Mode |
|---|---|---|
| **Protocol** | stdin/stdout (MCP) | Direct command execution |
| **Use case** | AI agents, IDE integration | Scripting, debugging, one-off queries |
| **Persistent process** | Yes | No (per-command spawn) |
| **Output format** | JSON-RPC messages | JSON (human-readable) |
| **Real-time streaming** | Yes | No (batch output) |
| **Error handling** | MCP error codes | Exit codes + stderr |

#### When to Use Each Mode

**Use MCP mode when:**
- Integrating with Claude Code, VS Code, or other MCP clients
- Building AI-powered workflows that need to make multiple queries
- You need a persistent server process
- Your client already speaks MCP

**Use CLI mode when:**
- Writing shell scripts or automation
- Quick debugging and testing
- Piping results to other tools (`jq`, `grep`, etc.)
- Making single queries without overhead of starting a server
- Running from cron jobs or CI/CD pipelines

Example CLI pipeline:

```bash
# Find all messages from sender, extract subjects, save to file
rusty_apple_mail_mcp search --sender "boss@company.com" | \
  jq -r '.messages[].subject' > ~/meeting-subjects.txt

# Count messages from last month
rusty_apple_mail_mcp search --date-from "2024-12-01" --date-to "2024-12-31" | \
  jq '.messages | length'
```

## Configuration

The server is configured **only** through environment variables:

| Variable | Required | Default | Description |
|---|---|---|---|
| `APPLE_MAIL_DIR` | no | `~/Library/Mail` | Root folder of the Mail data |
| `APPLE_MAIL_VERSION` | no | `V10` | Envelope Index version subdirectory |
| `APPLE_MAIL_ACCOUNT` | no | unset | Comma-separated account selectors such as `Work Email` or `user@example.com`; when set, the whole server is restricted to the resolved account(s) |
| `RUST_LOG` | no | unset | Standard Rust tracing filter used by `tracing_subscriber`; controls server logs written to stderr |

Example setup:

```bash
export APPLE_MAIL_DIR="$HOME/Library/Mail"
export APPLE_MAIL_VERSION="V10"
export APPLE_MAIL_ACCOUNT="Work Email"
export RUST_LOG="warn"
```

### `RUST_LOG` values

The server reads `RUST_LOG` through `tracing_subscriber::EnvFilter`, so it accepts the usual Rust tracing filter syntax.

Common values:

- `error` — only errors
- `warn` — warnings and errors
- `info` — startup and high-level operational logs
- `debug` — includes per-request debug logs
- `trace` — very verbose tracing
- `off` — disables logging

You can also scope logs per module/crate:

- `rusty_apple_mail_mcp=debug`
- `rusty_apple_mail_mcp=trace,rusqlite=warn`
- `info,rmcp=warn`

When `RUST_LOG` enables `debug` for this crate, `search_messages` logs timing breakdowns to stderr, including:

- total matched rows
- SQL query time
- metadata hydration time from SQLite
- body preview fallback time
- total request time

### Account scoping

If `APPLE_MAIL_ACCOUNT` is set, the server resolves each selector through macOS `~/Library/Accounts/Accounts4.sqlite` and then restricts **all** tools to the matched Mail account IDs.

- Matching is case-insensitive and trims whitespace.
- Supported selectors include human-friendly account names and email addresses.
- Startup fails fast if a selector matches zero accounts or multiple accounts.
- `list_accounts` returns `account_name` and `email` when available, so valid selector values are discoverable from the MCP interface itself.

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
                "APPLE_MAIL_ACCOUNT": "Work Email",
                "RUST_LOG": "warn"
            }
        }
    }
}
```

## Usage

Typical usage pattern:

1. Call `list_accounts` (optionally with `include_mailboxes=true`) to discover accounts and mailboxes.
2. Use `search_messages` to build a shortlist of candidates.
3. Call `get_message` to fetch the full message you care about.
4. Use `get_attachment_content` when you need the text of a particular attachment.

### Token efficiency

The server is optimized to minimize token consumption:

- **Compact tool descriptions** — routing hints live in `ServerInfo.instructions` (loaded once), not repeated per-tool on every request.
- **HTML → plain text** — HTML email bodies are converted to clean text via DOM parsing (using `scraper`), typically 10–20× smaller than raw HTML.
- **`status: "success"` omitted** — the status field only appears on error/not_found/partial responses.
- **Recipients omitted by default** — `get_message` skips To/CC lists unless `include_recipients=true`.
- **Compact dates** — ISO 8601 without seconds (`2024-09-15T00:00Z`).
- **`has_body` removed** — always true for indexed messages; no longer wasting tokens.

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
    "body_format": "text",
    "include_recipients": false
}
```

`include_recipients` defaults to `false` — set it to `true` when you need the To/CC lists (saves tokens on corporate mail with 50+ recipients).

### Combined account + mailbox discovery

Instead of calling `list_accounts` then `list_mailboxes` separately, use `include_mailboxes=true`:

```json
{
    "include_mailboxes": true
}
```

This returns accounts with their mailboxes grouped, saving one round-trip.

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

## Limitations

- macOS only
- read-only access only
- stdio transport only
- requires Apple Mail storage to be present on disk
- some binary attachment formats may yield metadata instead of extracted text

## TL;DR

Need an AI agent to safely search and read Apple Mail on your Mac? This project provides a clean, read-only MCP layer over Apple Mail’s native storage — fast index-based searches, on‑demand body hydration from `.emlx`, and zero write access.
