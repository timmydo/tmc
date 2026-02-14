# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**tmc** (Timmy's Mail Console) is a TUI MUA (Mail User Agent) written in Rust with minimal (ideally zero) dependencies. It reads email from a JMAP server (RFC 8620/8621) and follows a unix philosophy: composing emails invokes `$EDITOR` rather than embedding an editor.

## Build

```bash
export CC=gcc   # required if using `ring` or other crates with C code
cargo build
cargo run
```

## Test

```bash
cargo test
cargo test <test_name>           # run a single test
cargo test -- --nocapture        # show stdout/stderr
```

## Lint

```bash
cargo clippy
cargo fmt -- --check
```

## Architecture

- **TUI**: Terminal UI using a notmuch/emacs-inspired keybinding scheme
- **Threading model**: The UI runs on its own logical thread; background tasks (JMAP fetches, sync) run on separate threads so the UI is never blocked
- **JMAP-only**: No IMAP/POP/mbox support. Connects to a JMAP server for all mail operations
- **$EDITOR integration**: Composing/replying launches `$EDITOR` in a subprocess; the TUI suspends and resumes after the editor exits
- **No async runtime**: Uses blocking I/O with threads, not tokio/async-std

## Key Design Principles

- Minimal dependencies — prefer std library and hand-rolled solutions over pulling in crates
- Unix philosophy — small, composable; delegate to external tools where possible
- notmuch emacs keybindings — `q` quit, `n`/`p` next/prev, `RET` open, `r` reply, `s` search, `g` refresh, `d` delete/flag
- Plain text first — prefer text/plain rendering; HTML is secondary or omitted

## Reference Material

- `~/src/rust-jmap-webmail/` — bootstrap JMAP client code with working session management, JMAP types, and API calls
- `~/src/rust-jmap-webmail/rfc/` — RFC 8620 (JMAP core), RFC 8621 (JMAP mail), RFC 8887 (JMAP WebSocket), RFC 9661 (JMAP Sieve)
- The JMAP server is at `https://mx.timmydouglas.com/.well-known/jmap`

## JMAP Implementation Notes

JMAP types and client patterns from the reference project can be adapted. Key operations:
- **Discovery**: `GET /.well-known/jmap` with Basic auth returns session with `apiUrl` and `primaryAccounts`
- **Mailbox/get**: List all mailboxes with counts
- **Email/query**: Search/filter/sort emails in a mailbox
- **Email/get**: Fetch full email with headers and body values
- All requests use `urn:ietf:params:jmap:core` and `urn:ietf:params:jmap:mail` capabilities
