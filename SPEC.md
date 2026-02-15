# tmc — Timmy's Mail Console

## Overview

A terminal-based Mail User Agent (MUA) written in Rust with minimal dependencies. Reads mail from a JMAP server. Follows unix philosophy: composing is done in `$EDITOR`, not in-app. The UI follows a notmuch emacs keybinding scheme.

## Goals

- Zero or near-zero external crate dependencies
- Responsive TUI that is never blocked by network operations
- Full read path via JMAP (RFC 8620 core, RFC 8621 mail)
- Compose/reply via `$EDITOR`
- notmuch emacs-style keybindings

## Non-Goals

- IMAP, POP3, mbox, or Maildir support
- Built-in text editor
- HTML rendering (plain text only, or very basic fallback)
- GUI or web interface

## Architecture

### Threading Model

```
┌─────────────┐       channels        ┌──────────────────┐
│   UI Thread  │◄─────────────────────►│  Backend Thread   │
│  (TUI loop)  │                       │  (JMAP fetch,     │
│              │   UI sends commands    │   sync, submit)   │
│              │   Backend sends data   │                   │
└─────────────┘                        └──────────────────┘
```

- **UI thread**: Owns the terminal. Renders views, handles keystrokes, dispatches commands to the backend via a channel.
- **Backend thread(s)**: Perform all JMAP network operations. Send results back to the UI thread via a channel. The UI never makes network calls directly.
- Communication uses `std::sync::mpsc` or similar lock-free channels.

### Module Structure

```
src/
├── main.rs            # Entry point, thread spawning, channel setup
├── config.rs          # Config file parsing (TOML)
├── tui/
│   ├── mod.rs         # TUI initialization and main loop
│   ├── views/
│   │   ├── mailbox_list.rs   # Left pane: list of mailboxes
│   │   ├── email_list.rs     # Email list within a mailbox
│   │   ├── email_view.rs     # Reading a single email
│   │   └── search.rs         # Search prompt and results
│   ├── keybindings.rs        # Key mapping definitions
│   └── render.rs             # Terminal drawing primitives
├── jmap/
│   ├── mod.rs
│   ├── client.rs      # HTTP client for JMAP (blocking)
│   ├── types.rs       # Serde types for JMAP objects
│   └── methods.rs     # High-level JMAP method wrappers
├── mail/
│   ├── mod.rs
│   ├── store.rs       # Local cache of fetched emails/mailboxes
│   └── compose.rs     # $EDITOR invocation for compose/reply
├── session.rs         # Auth credentials, JMAP session state
└── log.rs             # Simple logging macros
```

### TUI Views

The TUI operates as a stack of views. Pressing `RET` pushes a new view; pressing `q` pops back.

1. **Mailbox List** — shows all mailboxes with unread counts. Default landing view after login.
2. **Email List** — shows emails in the selected mailbox (subject, from, date, unread indicator).
3. **Email View** — shows a single email: headers (From, To, Cc, Date, Subject) and plain text body.
4. **Search** — prompted with `s`, results displayed like Email List.

### Keybindings (notmuch emacs scheme)

| Key   | Context        | Action                              |
|-------|----------------|-------------------------------------|
| `q`   | any            | Quit current view (pop)             |
| `n`   | list/view      | Next item / scroll down             |
| `p`   | list/view      | Previous item / scroll up           |
| `RET` | list           | Open selected item                  |
| `r`   | email view     | Reply (opens $EDITOR)               |
| `R`   | email view     | Reply-all (opens $EDITOR)           |
| `c`   | any            | Compose new (opens $EDITOR)         |
| `s`   | mailbox list   | Search                              |
| `g`   | any            | Refresh / sync                      |
| `d`   | email list     | Toggle deleted flag                 |
| `*`   | email list     | Toggle flagged/starred              |
| `tab` | email view     | Next MIME part / attachment          |
| `j/k` | list/view      | Alternative down/up (vim-style)     |

### $EDITOR Integration

When composing or replying:
1. Construct a draft in RFC 5322 message format (headers + body) in a temp file.
2. Spawn `$EDITOR <tempfile>` as a background process (falling back to `vi`).
3. The TUI remains interactive while the editor runs separately.
4. The editor/script is responsible for sending the email — tmc does NOT submit mail.
5. A background thread cleans up the temp file after the editor exits.

### JMAP Operations

All network access goes through a single `JmapClient` on the backend thread.

| Operation          | JMAP Method           | RFC Section      |
|--------------------|-----------------------|------------------|
| Discover session   | GET .well-known/jmap  | RFC 8620 §2      |
| List mailboxes     | Mailbox/get           | RFC 8621 §2      |
| List emails        | Email/query           | RFC 8621 §4.4    |
| Fetch emails       | Email/get             | RFC 8621 §4.2    |
| Search emails      | Email/query (filter)  | RFC 8621 §4.4    |
| ~~Submit email~~   | ~~EmailSubmission/set~~| *(not used — editor/script sends)* |
| Set flags          | Email/set (keywords)  | RFC 8621 §4.3    |
| Create draft       | Email/set             | RFC 8621 §4.3    |
| Delete email       | Email/set (destroy)   | RFC 8621 §4.3    |

### Configuration

File: `config.toml`

```toml
[jmap]
well_known_url = "https://mx.timmydouglas.com/.well-known/jmap"

[ui]
# editor = "$EDITOR"   # defaults to $EDITOR env var, then vi
# page_size = 50       # emails per page
```

Credentials are prompted at startup (username/password for JMAP Basic auth), never stored on disk.

### Local Cache

A lightweight in-memory store caches:
- Mailbox list with counts (refreshed on `g` or periodic sync)
- Email headers for list views (fetched per-mailbox on open)
- Full email bodies (fetched on demand when viewing)

No on-disk cache in v1. Future versions may add SQLite or flat-file caching.

## Security Considerations

- JMAP connections always over HTTPS (credentials in Authorization header)
- Credentials held in memory only, never persisted to disk
- Temp files for compose are created with restrictive permissions and cleaned up after send/abort
