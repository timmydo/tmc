# AGENTS.md

This file provides guidance for coding agents working in this repository.

## Project Overview

**tmc** (Timmy's Mail Console) is a Rust TUI MUA that reads mail via JMAP.
It follows a Unix model: compose/reply opens `$EDITOR`; tmc does not submit mail.

## Build / Run / Test

```bash
cargo build
cargo run
cargo test
cargo clippy
cargo fmt -- --check
```

Runtime options:

```bash
tmc --help
tmc --log
```

## Configuration

Default path: `$XDG_CONFIG_HOME/tmc/config.toml` (or `~/.config/tmc/config.toml`).

Supported config styles:

```toml
[ui]
editor = "nvim"
page_size = 100
mouse = true
sync_interval_secs = 60

[account.personal]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "me@example.com"
password_command = "pass show email/example.com"

[account.work]
well_known_url = "https://mx.work.com/.well-known/jmap"
username = "me@work.com"
password_command = "pass show email/work.com"
```

Legacy fallback is still supported via `[jmap]` with the same three fields.

Credentials are fetched by running `password_command`; there is no interactive password prompt.

## Architecture

- `src/main.rs`: CLI flags (`--help`, `--log`), config loading, first-account connect, TUI bootstrap.
- `src/config.rs`: lightweight TOML-like parser for `[ui]`, `[jmap]`, and `[account.NAME]`.
- `src/backend.rs`: single backend worker thread + `mpsc` command/response channels.
- `src/jmap/client.rs`: blocking JMAP client (`ureq`), discovery + mail operations.
- `src/jmap/types.rs`: serde-backed JMAP models.
- `src/tui/`: raw terminal setup, input parsing, view stack, mailbox/email/help views.
- `src/compose.rs`: compose/reply draft generation and secure temp draft files.
- `src/log.rs`: file logging and `--log` support.

### Threading model

- UI loop runs on the main thread.
- JMAP operations run on one backend thread.
- UI communicates with backend over `std::sync::mpsc`.
- UI applies optimistic updates for some actions (read/unread, flag, move) before backend confirmation.

## Implemented User Flows

- Mailbox list (`Mailbox/get`) with role-aware sorting and unread counts.
- Email list (`Email/query` + `Email/get`) with per-mailbox search.
- Email view (`Email/get`) with plain text body rendering.
- Compose / reply / reply-all via `$EDITOR` on temp draft files.
- Mark read/unread, flag/unflag, move to mailbox (`Email/set` variants).
- Multi-account switching (`a`) from mailbox view.
- Mouse support (click select/open, wheel scrolling) for list/help views.

## Keybindings (implemented)

- Global: `?` help, `c` compose.
- Mailbox list: `q`, `n/p`, `j/k`, arrows, `RET`, `g`, `a`, mouse click/wheel.
- Email list: `q`, `n/p`, `j/k`, arrows, `RET`, `g`, `f`, `u`, `m`, `s`, `Esc` (clear search), mouse click/wheel.
- Email view: `q`, `n/p`, `j/k`, arrows, `PgUp/PgDn/Space/Home/End`, `r`, `R`, `f`, `u`, `c`.
- Help view: `q`/`?`/`Esc` close + navigation keys.

## Constraints and Non-Goals

- No IMAP/POP/mbox/Maildir support.
- No built-in editor.
- No HTML rendering beyond preview/plain-text fallback.
- No send path in tmc (submission is external).

## Commit Policy

- Agent-created commits must include a `Co-Authored-By:` trailer.
- Run `cargo fmt` and wait for it to complete before committing changes.

## Current Gaps (as of code in this repo)

- No attachment UI/download workflow despite `downloadUrl` discovery.
- No thread/conversation UI.
- Some UI actions are optimistic and do not show explicit failure state on backend errors.
