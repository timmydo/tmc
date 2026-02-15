# tmc - Timmy's Mail Console

## Overview

A terminal-based Mail User Agent (MUA) written in Rust with a small dependency set.
Reads mail from a JMAP server and keeps composition in external tools (`$EDITOR`).

## Goals

- Keep architecture simple and dependency footprint small.
- Keep the UI responsive while network operations run off the UI thread.
- Provide a complete read path via JMAP (discovery, mailbox list, email list, email view).
- Support compose/reply/reply-all through `$EDITOR`.
- Support practical mailbox management (read/unread, flag, move, search).

## Non-Goals

- IMAP/POP3/mbox/Maildir support.
- Built-in text editor.
- Built-in SMTP/JMAP submission flow.
- Full HTML rendering.
- GUI or web interface.

## Current Architecture

### Execution model

- Main thread owns the TUI loop and rendering.
- One backend worker thread handles all JMAP operations.
- Communication is via `std::sync::mpsc` channels.
- The UI never performs network I/O directly.

### Module map (implemented)

```text
src/
|- main.rs                 # entrypoint, CLI flags, config load, initial connect
|- config.rs               # config parsing ([ui], [jmap], [account.NAME])
|- backend.rs              # BackendCommand/BackendResponse + worker thread
|- compose.rs              # compose/reply draft builders + temp draft files
|- log.rs                  # file logger + log path helper
|- jmap/
|  |- client.rs            # blocking JMAP client over ureq
|  |- types.rs             # serde models for session/mailbox/email objects
|  `- mod.rs
`- tui/
   |- mod.rs               # terminal lifecycle + view stack/event loop
   |- screen.rs            # raw mode, resize, ANSI drawing, mouse tracking
   |- input.rs             # key and SGR mouse parser
   `- views/
      |- mailbox_list.rs   # mailbox list + account switching
      |- email_list.rs     # mailbox messages, search, move/flag/read toggles
      |- email_view.rs     # full email render + reply/reply-all trigger
      |- help.rs
      `- mod.rs
```

## JMAP Operations (implemented)

- Session discovery: `GET /.well-known/jmap` (Basic auth, redirect handling).
- Mailbox list: `Mailbox/get`.
- Email list/search: `Email/query` (mailbox + optional text filter).
- Email fetch: `Email/get` (list detail and full body fetch).
- State changes via `Email/set` variants:
  - mark read / unread (`$seen`)
  - set / unset flagged (`$flagged`)
  - move message by mailbox IDs

## Compose/Reply Model

- Compose and reply drafts are generated in-app.
- Drafts are written to restrictive temp files (`0600`).
- `$EDITOR` (or configured editor) is spawned in a child process.
- tmc does not send mail; external tooling handles submission.

## UI and Keybindings (implemented)

### Mailbox list

- Navigate: `n/p`, `j/k`, arrows, page keys, home/end, wheel.
- Actions: `RET` open mailbox, `g` refresh, `c` compose, `a` switch account, `?` help, `q` quit.

### Email list

- Navigate: same movement keys + wheel/click.
- Actions: `RET` open, `g` refresh, `f` flag, `u` read/unread, `m` move, `s` search, `Esc` clear search, `c` compose, `?` help, `q` back.

### Email view

- Navigate/scroll: `n/p`, `j/k`, arrows, `PgUp/PgDn`, `Space`, `Home/End`.
- Actions: `r` reply, `R` reply-all, `f` flag, `u` read/unread, `c` compose, `?` help, `q` back.

## Configuration

Default config path:

- `$XDG_CONFIG_HOME/tmc/config.toml`, else `~/.config/tmc/config.toml`

Supported shape:

```toml
[ui]
editor = "nvim"
page_size = 100
mouse = true

[account.personal]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "me@example.com"
password_command = "pass show email/example.com"
```

Also supported for compatibility:

```toml
[jmap]
well_known_url = "https://mx.example.com/.well-known/jmap"
username = "me@example.com"
password_command = "pass show email/example.com"
```

## Known Gaps

- No periodic sync timer.
- No true pagination/infinite scrolling; query always starts at position 0.
- No thread/conversation grouping.
- No attachment list/download workflow.
- Optimistic UI updates do not currently surface backend write failures to users.
