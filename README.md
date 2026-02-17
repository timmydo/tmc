# tmc (Timmy's Mail Console)

`tmc` is a Rust terminal mail client (MUA) for reading and triaging email over JMAP.

## Goals

- Fast, keyboard-first email workflow in a terminal UI.
- Unix-friendly composition flow: drafts open in `$EDITOR`.
- Clear separation of concerns: `tmc` reads/manages mail; message submission is external.
- Scriptable automation through a JSON-over-stdin/stdout CLI mode.

## What tmc Does

- Connects to one or more JMAP accounts.
- Lists mailboxes and emails, opens message view, and shows threads.
- Supports read/unread, flag/unflag, move, archive, delete, and mailbox-wide mark-read.
- Supports compose/reply/reply-all/forward draft generation.
- Supports optional mail rules and retention policies.
- Provides `--cli` NDJSON mode for integrations and automation.

## Requirements

- Rust toolchain (stable) with Cargo.
- A JMAP server/account.
- An editor available via `$EDITOR` (for compose/reply/forward flow).
- A non-interactive credential command for `password_command` (for example `pass`).

## Build

```bash
cargo build
```

Release build:

```bash
cargo build --release
```

## Install

From this repository:

```bash
cargo install --path .
```

Or use the compiled release binary directly:

```bash
./target/release/tmc
```

## Setup

Default config path:

- `$XDG_CONFIG_HOME/tmc/config.toml`
- Fallback: `~/.config/tmc/config.toml`

Example config:

```toml
[ui]
editor = "nvim"
page_size = 100
mouse = true
sync_interval_secs = 60

[mail]
archive_folder = "Archive"
deleted_folder = "Trash"
# Optional: override From used for draft generation
# reply_from = "Me <me@example.com>"

[account.personal]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "me@example.com"
password_command = "pass show email/example.com"

[account.work]
well_known_url = "https://mx.work.com/.well-known/jmap"
username = "me@work.com"
password_command = "pass show email/work.com"
```

Legacy fallback is supported via `[jmap]` with `well_known_url`, `username`, and `password_command`.

Optional rules file path defaults to `rules.toml` next to your config; override with `--rules=PATH`.

## Run

```bash
cargo run
```

For all command-line options, run:

```bash
tmc --help
```

## Development

```bash
cargo test
cargo clippy
cargo fmt -- --check
```
