# Rusty Apple Mail MCP Server

![Rust 2024](https://img.shields.io/badge/rust-2024-orange?style=flat-square&logo=rust)
![Protocol MCP](https://img.shields.io/badge/protocol-MCP-6f42c1?style=flat-square)
![Transport stdio](https://img.shields.io/badge/transport-stdio%20%7C%20CLI-0a7ea4?style=flat-square)
![Platform macOS](https://img.shields.io/badge/platform-macOS-111827?style=flat-square&logo=apple)
![Access read-only](https://img.shields.io/badge/access-read--only-15803d?style=flat-square)

## TL;DR

Read-only MCP server for Apple Mail on macOS. Also works as a CLI tool.

It gives an AI agent local access to Apple Mail metadata, message bodies, and attachment text. Searches run against the index, bodies and attachments load on demand from `.emlx` files, and nothing is ever written. No AppleScript, no IMAP/POP/EWS round trips.

## Why this matters

Apple Mail already contains the data an agent needs, but it is buried in a local SQLite index and scattered `.emlx` files.

This project exposes that storage through a small, intent-driven MCP interface so an agent can:

- locate messages by subject, date range, sender, participant, mailbox, or account;
- fetch a full message including metadata, recipients, body, and attachment summary;
- extract readable text from supported attachments;
- operate completely **locally** and **read-only**.

In practice, an agent searches and reads your mail archive without Mail.app automation. AppleScript is damn slow and throws timeouts regularly, and the network protocols add nothing you need on your own machine.

## What the server can do

The current tool set is intentionally compact:

| Tool | What it does |
|---|---|
| `search_messages` | Search by subject, dates, sender, participant, account, or mailbox |
| `list_accounts` | Discover account identifiers; set `include_mailboxes=true` for combined account+mailbox discovery |
| `get_message` | Read one message through a bounded content window; recipients omitted by default (`include_recipients=true` to include) |
| `get_attachment_content` | Extract readable attachment text through a bounded content window |

## Installation

### Prerequisites

- macOS
- Apple Mail installed and synced at least once
- Rust toolchain (`rustup`, `cargo`)

> **Note: macOS Full Disk Access required**
>
> Apple Mail's data directory (`~/Library/Mail`) is protected by macOS TCC (Transparency, Consent, and Control). The MCP server process **must** have Full Disk Access to read the Envelope Index database.
>
> Grant Full Disk Access to the application that runs the MCP server:
>
> 1. Open **System Settings → Privacy & Security → Full Disk Access**
> 2. Click **+** and add the application (e.g., your terminal emulator, VS Code, Zed, Cursor)
> 3. Ensure the toggle is enabled
>
> Without this, the server will fail with `SQLite error: unable to open database file` even though the file exists.

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

In CLI mode, you can run individual commands directly from the terminal: to script mail lookups, to debug without setting up an MCP client, to pipe results into other command-line tools, or for one-off queries that do not need a long-running server.

#### Usage

```bash
# Print the installed version
rusty_apple_mail_mcp --version

# List all accounts
rusty_apple_mail_mcp list-accounts
rusty_apple_mail_mcp list-accounts --include-mailboxes

# Search messages
rusty_apple_mail_mcp search --subject-query "invoice"
rusty_apple_mail_mcp search --sender "john@example.com" --limit 10
rusty_apple_mail_mcp search --date-from "2024-01-01" --date-to "2024-12-31"
rusty_apple_mail_mcp search --mailbox "INBOX" --include-body-preview

# Get a specific message
rusty_apple_mail_mcp get-message --message-id "12345"
rusty_apple_mail_mcp get-message --message-id "12345" --include-recipients
rusty_apple_mail_mcp get-message --message-id "12345" --offset 8192 --source-revision "..."

# Get attachment content
rusty_apple_mail_mcp get-attachment --message-id "12345" --attachment-id "12345:0"
rusty_apple_mail_mcp get-attachment --message-id "12345" --attachment-id "12345:0" --offset 8192 --source-revision "..."
```

#### Agent skills

The binary serves its own usage guidance, so an agent always reads instructions that match the
installed version. The command works without a configured mail directory.

```bash
# List the bundled skills
rusty_apple_mail_mcp skills list

# Read the skill
rusty_apple_mail_mcp skills get apple-mail-cli

# Read one reference only, or every reference at once
rusty_apple_mail_mcp skills get apple-mail-cli --ref cli-recipes
rusty_apple_mail_mcp skills get apple-mail-cli --full

# Machine-readable output
rusty_apple_mail_mcp skills list --json
```

`skills get` prints markdown, so the output can be piped straight into a context window. `--json`
wraps the same documents in an envelope with `name`, `description`, and `documents`. An unknown skill
name exits `2` with the structured error envelope on stderr.

#### CLI Configuration

CLI mode supports the same configuration options as MCP mode:

| Option | Env Variable | Description |
|---|---|---|
| `--mail-directory` | `APPLE_MAIL_DIR` | Mail data directory (default: `~/Library/Mail`) |
| `--mail-version` | `APPLE_MAIL_VERSION` | Envelope Index version (default: `V10`) |
| `--scope-account` | `APPLE_MAIL_ACCOUNT` | Startup Scope selector(s); comma-separated account names, emails, or IDs (see [Account scoping](#account-scoping)). The legacy top-level `--account` spelling is a compatibility alias; `search --account` remains a per-call Filter. |

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
| **Output format** | JSON-RPC messages | Compact JSON by default; `--pretty` opts into formatted JSON |
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

The server supports configuration via **environment variables**, **CLI flags**, and a **YAML config file**. Values are resolved with this priority chain:

```
CLI flags > environment variables > config.yaml > defaults
```

### Config file locations

The server looks for `config.yaml` in two locations (first found wins):

1. Next to the binary, in the same directory as the executable
2. The home config at `~/.config/rusty_apple_mail_mcp/config.yaml`

Copy `config.example.yaml` to one of these locations and fill in your values:

```bash
cp config.example.yaml ~/.config/rusty_apple_mail_mcp/config.yaml
```

### Configuration fields

| Config Key | Env Variable | CLI Flag | Default | Description |
|---|---|---|---|---|
| `apple_mail_dir` | `APPLE_MAIL_DIR` | `--mail-directory` | `~/Library/Mail` | Root folder of the Mail data |
| `apple_mail_version` | `APPLE_MAIL_VERSION` | `--mail-version` | `V10` | Envelope Index version subdirectory |
| `apple_mail_account` | `APPLE_MAIL_ACCOUNT` | `--scope-account` | unset | Startup Scope selector(s); legacy top-level `--account` remains a compatibility alias |
| `log_level` | `APPLE_MAIL_LOG_LEVEL` | none | `warn` | Log level; `RUST_LOG` takes precedence |

### Priority chain examples

```yaml
# ~/.config/rusty_apple_mail_mcp/config.yaml
apple_mail_dir: "~/Library/Mail"
apple_mail_version: "V10"
apple_mail_account: "Work Email"
log_level: "info"
```

```bash
# Environment variables override config.yaml
export APPLE_MAIL_VERSION="V9"

# CLI flags override everything
rusty_apple_mail_mcp --mail-version V8 search --subject-query "invoice"
```

### Security note

`config.yaml` may contain account selectors and other configuration. It is added to `.gitignore` to prevent accidental commits. Never commit real configuration to version control.

### `RUST_LOG` values

The server reads `RUST_LOG` through `tracing_subscriber::EnvFilter`, so it accepts the usual Rust tracing filter syntax.

Common values:

- `error`: only errors
- `warn`: warnings and errors
- `info`: startup and high-level operational logs
- `debug`: includes per-request debug logs
- `trace`: very verbose tracing
- `off`: disables logging

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

#### Discovering account selectors

Run `list_accounts` without scoping first. It shows every account with its name and email:

```bash
rusty_apple_mail_mcp list-accounts
```

Example output:

```json
{
  "accounts": [
    {
      "account_id": "ews://EXAMPLE-EXCHANGE-ID",
      "account_type": "ews",
      "account_name": "Exchange",
      "email": "work@example.com",
      "mailbox_count": 10,
      "message_count": 1000
    },
    {
      "account_id": "imap://EXAMPLE-IMAP-ID",
      "account_type": "imap",
      "email": "personal@example.com",
      "mailbox_count": 20,
      "message_count": 2000
    }
  ]
}
```

Use `account_name`, `email`, or `account_id` as selectors.

#### Selector format

| Selector type | Example | When to use |
|---|---|---|
| Account name | `Exchange` | Human-friendly, from macOS Accounts settings |
| Email address | `user@example.com` | Unique, works across protocols |
| Account ID | `ews://UUID` | Always available in `list_accounts` output |

Multiple selectors are comma-separated:

```bash
APPLE_MAIL_ACCOUNT="Exchange,personal@example.com"
```

#### How matching works

- Matching is case-insensitive and trims whitespace.
- Startup fails fast if a selector matches zero accounts (typo) or multiple accounts (ambiguous).
- Accounts that macOS Accounts settings does not register cannot be selected by name or email. Use their `account_id` instead.

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

- Routing hints live in `ServerInfo.instructions`, which loads once, instead of repeating them per tool on every request.
- HTML bodies are converted to plain text through DOM parsing (using `scraper`), typically ten to twenty times smaller than the raw HTML.
- Completed payloads carry an explicit `outcome` (`success`, `partial`, or `not_found`), while protocol errors stay native errors.
- `get_message` skips To/CC lists unless `include_recipients=true`.
- Dates are compact ISO 8601 without seconds (`2024-09-15T00:00Z`).
- `has_body` is gone: it was always true for indexed messages, so it only wasted tokens.

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
    "include_recipients": false,
    "offset": 0,
    "limit": 8192
}
```

`include_recipients` defaults to `false`. Set it to `true` when you need the To/CC lists, which saves tokens on corporate mail with 50+ recipients. If a response returns `outcome: "partial"`, continue with the returned `window.next_offset` and `window.source_revision`.

### Combined account + mailbox discovery

Use `include_mailboxes=true` to get accounts with their mailboxes grouped in a single call:

```json
{
    "include_mailboxes": true
}
```

This returns accounts with their mailboxes grouped, saving one round-trip.

## How it works

The server draws from two local sources:

- The **Envelope Index**, the SQLite database Apple Mail uses as a metadata index and relationship store
- The `.emlx` files, which hold the canonical bodies and attachments for individual messages

Search queries hit the lightweight index, keeping them fast; bodies and attachments are loaded on-demand from the `.emlx` files.

## Development

Handy commands while working on the codebase:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo doc --no-deps
```

## Limitations

- macOS only
- read-only access only
- stdio transport only
- requires Apple Mail storage to be present on disk
- some binary attachment formats may yield metadata instead of extracted text
