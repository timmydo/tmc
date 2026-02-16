# TODO

## Phase 0: Project Bootstrap
- [x] Initialize Cargo project with `cargo init`
- [x] Config parsing for server/auth settings
- [x] JMAP types and client bootstrap
- [x] Logging support

## Phase 1: Minimal TUI
- [x] Raw terminal mode
- [x] Screen drawing primitives
- [x] Input loop and key parsing
- [x] View stack push/pop
- [x] Resize handling (SIGWINCH)
- [x] Mouse input support (SGR)

## Phase 2: JMAP Read Path
- [x] JMAP discovery from `.well-known/jmap`
- [x] Mailbox list view (`Mailbox/get`)
- [x] Email list view (`Email/query` + `Email/get`)
- [x] Email detail view with plain text extraction

## Phase 3: Background Threading
- [x] Backend worker thread for JMAP operations
- [x] `mpsc` command/response channels
- [x] UI refresh on incoming backend data
- [x] Loading states in views

## Phase 4: Compose and Reply
- [x] Compose draft template
- [x] Reply and reply-all draft generation
- [x] Spawn external editor for draft temp files
- [x] tmc does not send email; external tooling handles submission

## Phase 5: Email Management
- [x] Flag/unflag emails (`Email/set` `$flagged`)
- [x] Mark read/unread (`Email/set` `$seen`)
- [x] Move emails between mailboxes
- [x] Search within mailbox (`Email/query` text filter)
- [x] Multi-account switching from mailbox list

## Phase 6: Next Phase (Recommended Priority Order)
- [x] P0: Add explicit write-failure UX for optimistic actions (flag/read/move)
- [x] P1: Add true pagination/load-more for mailbox email lists
- [x] P1: Add periodic background sync (configurable interval)
- [x] P2: Add attachment list + open/download flow using JMAP `downloadUrl`
- [x] P2: Add conversation/thread grouping
- [x] P2: Improve config parser robustness (quoted strings, escapes, strict errors)
- [x] P2: Add integration tests for config parsing and JMAP response edge cases

## Phase 7: Misc

- [ ] 'v' keybinding to show/hide all the mail headers for an email message
- [ ] design and build mail sorting language
