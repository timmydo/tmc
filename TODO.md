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

- [x] 'v' keybinding to show/hide all the mail headers for an email message
- [x] 'F' for forward email
- [x] --prompt=xyz command line parameter to generate prompts to AI agents so they will help you generate 'config', 'rules', etc.

## Phase 8: Integration testing

Also add command line options --config and --rules for providing a
custom config or rules file. This can be used to create a fully
integrated test environment (passing a config that points to a JMAP
server running on localhost). Do this also and create tests for
this. Implement a mock JMAP backend that can be used to test CLI mode.
Add a couple of integration tests for CLI mode.

## Phase 9: CLI Triage Automation

- [ ] Ensure `archive` and `delete_email` mailbox resolution works immediately after `connect`:
  fetch/cache mailboxes on-demand when folder resolution is attempted and cache is empty/stale.
- [ ] Add explicit mailbox-target configuration for CLI actions:
  support mailbox IDs in config (e.g., `archive_mailbox_id`, `deleted_mailbox_id`) with name/role fallback.
- [ ] Add date-range filtering to `query_emails` (e.g., `received_after`, `received_before`) to avoid client-side pagination scans.
- [ ] Add bulk mutation commands for triage workflows (e.g., `bulk_move`, `bulk_archive`, `bulk_delete_email`) with per-message status.
- [ ] Add dry-run triage suggestions command (e.g., `triage_suggest`) returning `archive`/`trash`/`keep` candidates with reasons.
- [ ] Add two-step plan/apply flow for safe automation:
  generate proposal first, then apply by plan ID or explicit approved IDs.
- [ ] Extend rules format to support triage actions and confidence scoring (`archive`, `trash`, `keep`) for reusable automation.
- [ ] Add integration tests for:
  post-`connect` archive/delete behavior, bulk operations, date filters, and plan/apply safety checks.
