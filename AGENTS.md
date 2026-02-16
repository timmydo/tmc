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
tmc --cli             # JSON-over-stdin/stdout CLI mode (NDJSON)
tmc --help-cli        # print CLI protocol documentation
tmc --prompt=config   # print AI-friendly prompt for generating config
tmc --prompt=rules    # print AI-friendly prompt for generating rules
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

- `src/main.rs`: CLI flags (`--help`, `--log`, `--cli`, `--help-cli`, `--prompt=TOPIC`), config loading, first-account connect, TUI bootstrap.
- `src/config.rs`: lightweight TOML-like parser for `[ui]`, `[jmap]`, and `[account.NAME]`.
- `src/backend.rs`: single backend worker thread + `mpsc` command/response channels.
- `src/jmap/client.rs`: blocking JMAP client (`ureq`), discovery + mail operations.
- `src/jmap/types.rs`: serde-backed JMAP models.
- `src/tui/`: raw terminal setup, input parsing, view stack, mailbox/email/help views.
- `src/cli.rs`: JSON-over-stdin/stdout CLI mode (NDJSON protocol), alternative UI reusing the same backend thread.
- `src/keybindings.rs`: centralized keybinding dictionary (`KeyBinding` struct + `all_keybindings()`), used by CLI export and `--help-cli`.
- `src/compose.rs`: compose/reply/forward draft generation and secure temp draft files.
- `src/log.rs`: file logging and `--log` support.

### Threading model

- UI loop runs on the main thread (TUI) or reads stdin line-by-line (CLI).
- JMAP operations run on one backend thread.
- Both TUI and CLI communicate with backend over `std::sync::mpsc` using the same `BackendCommand`/`BackendResponse` enums.
- TUI applies optimistic updates for some actions (read/unread, flag, move) before backend confirmation.
- CLI blocks synchronously on `resp_rx.recv()` for each command.

### CLI mode (`--cli`)

An alternative UI that speaks NDJSON (one JSON object per line) over stdin/stdout. It reuses the same backend thread and `BackendCommand`/`BackendResponse` protocol as the TUI, making it suitable for programmatic interaction and integration testing.

Supported commands: `list_accounts`, `connect`, `status`, `list_mailboxes`, `create_mailbox`, `delete_mailbox`, `query_emails`, `get_email`, `get_thread`, `mark_read`, `mark_unread`, `flag`, `unflag`, `move_email`, `archive`, `delete_email`, `destroy`, `mark_mailbox_read`, `get_raw_headers`, `download_attachment`, `compose_draft`, `reply_draft`, `forward_draft`, `keybindings`.

Response envelope: `{"ok": true, ...data}` or `{"ok": false, "error": "message"}`.

Context control for email viewing: `max_body_chars` (truncate body), `headers_only` (omit body/preview).

## Implemented User Flows

- Mailbox list (`Mailbox/get`) with role-aware sorting and unread counts.
- Email list (`Email/query` + `Email/get`) with per-mailbox search.
- Email view (`Email/get`) with plain text body rendering.
- Compose / reply / reply-all / forward via `$EDITOR` on temp draft files.
- Mark read/unread, flag/unflag, move to mailbox (`Email/set` variants).
- Multi-account switching (`a`) from mailbox view.
- Mouse support (click select/open, wheel scrolling) for list/help views.
- CLI mode (`--cli`): all of the above operations available via JSON commands over stdin/stdout.

## Keybindings (implemented)

- Global: `?` help, `c` compose.
- Mailbox list: `q`, `n/p`, `j/k`, arrows, `RET`, `g`, `a`, mouse click/wheel.
- Email list: `q`, `n/p`, `j/k`, arrows, `RET`, `g`, `f`, `u`, `m`, `s`, `Esc` (clear search), mouse click/wheel.
- Email view: `q`, `n/p`, `j/k`, arrows, `PgUp/PgDn/Space/Home/End`, `r`, `R`, `F`, `v`, `f`, `u`, `c`.
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

- Some UI actions are optimistic and do not show explicit failure state on backend errors.
