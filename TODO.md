# TODO

## Phase 0: Project Bootstrap
- [x] Initialize Cargo project with `cargo init`
- [x] Set up config.toml parsing (server URL)
- [x] Port JMAP types from ~/src/rust-jmap-webmail/ (serde types for Session, Mailbox, Email, etc.)
- [x] Port JMAP client (HTTP client with Basic auth, discovery, method calls)
- [x] Implement simple logging macros (no dependency)

## Phase 1: Minimal TUI
- [x] Raw terminal mode (termios via libc or hand-rolled)
- [x] Basic screen drawing: clear, cursor movement, text output
- [x] Input loop: read keystrokes, dispatch to handlers
- [x] View stack: push/pop views with `RET`/`q`
- [x] Resize handling (SIGWINCH)

## Phase 2: JMAP Read Path
- [x] Login prompt (username/password at startup)
- [x] JMAP session discovery (.well-known/jmap)
- [x] Mailbox list view (Mailbox/get)
- [x] Email list view (Email/query + Email/get for headers)
- [x] Email view (Email/get with body values, plain text rendering)

## Phase 3: Background Threading
- [x] Spawn backend thread for JMAP operations
- [x] mpsc channels: UI→Backend commands, Backend→UI results
- [x] UI refresh on incoming data (poll channel in event loop)
- [x] Loading indicators while fetching

## Phase 4: Compose & Reply
- [x] Compose new: create temp file with empty headers template
- [x] Reply/Reply-all: pre-fill headers and quoted body
- [x] Spawn $EDITOR (or configured editor) as background process with draft file
- [ ] NOTE: tmc does NOT send email — the editor/script is responsible for sending

## Phase 5: Email Management
- [x] Flag/unflag emails (Email/set keywords $flagged) — `f` key in email list and email view
- [x] Mark read/unread (Email/set keywords $seen) — auto-read on open; `u` key to toggle in email list and email view
- [x] Move to folder (Email/set mailboxIds) — `m` key in email list opens mailbox picker
- [x] Search (Email/query with text filter) — `s` key in email list; Escape to clear search
- NOTE: No delete feature — use move-to-folder (e.g. Trash/Archive) instead; retention policies on folders handle cleanup server-side

## Phase 6: Polish
- [ ] Periodic background sync (configurable interval)
- [ ] Scrolling and pagination in long lists
- [ ] Thread/conversation grouping (Email/query with collapseThreads)
- [ ] Attachment listing in email view
- [ ] Color scheme (colored headers, quoted text highlighting)
