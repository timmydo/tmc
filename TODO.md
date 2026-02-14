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
- [ ] Login prompt (username/password at startup)
- [ ] JMAP session discovery (.well-known/jmap)
- [ ] Mailbox list view (Mailbox/get)
- [ ] Email list view (Email/query + Email/get for headers)
- [ ] Email view (Email/get with body values, plain text rendering)

## Phase 3: Background Threading
- [ ] Spawn backend thread for JMAP operations
- [ ] mpsc channels: UI→Backend commands, Backend→UI results
- [ ] UI refresh on incoming data (poll channel in event loop)
- [ ] Loading indicators while fetching

## Phase 4: Compose & Reply
- [ ] $EDITOR integration: suspend TUI, exec editor, resume
- [ ] Compose new: create temp file with empty headers template
- [ ] Reply/Reply-all: pre-fill headers and quoted body
- [ ] Parse edited file back into JMAP Email/set + EmailSubmission/set
- [ ] Confirm send / abort prompt

## Phase 5: Email Management
- [ ] Flag/unflag emails (Email/set keywords $flagged)
- [ ] Delete emails (Email/set keywords $deleted or destroy)
- [ ] Mark read/unread (Email/set keywords $seen)
- [ ] Search (Email/query with filter text)

## Phase 6: Polish
- [ ] Periodic background sync (configurable interval)
- [ ] Scrolling and pagination in long lists
- [ ] Thread/conversation grouping (Email/query with collapseThreads)
- [ ] Attachment listing in email view
- [ ] Color scheme (colored headers, quoted text highlighting)
