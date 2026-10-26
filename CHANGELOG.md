# Changelog

All notable changes to dshell are documented here.

## [0.1.31] - 2026-10-26

Adds independent duplicate sessions and keeps reusable credentials in memory only.

### Added

- Add `Duplicate session` to saved session tab actions. Each duplicate opens an
  independent SSH connection and receives a unique session title.
- Keep passwords and key passphrases in the zeroizing runtime credential cache
  until dshell exits; do not write them to host profiles.
- Upgrade legacy host profile data while discarding any credential fields.
- Add Simplified Chinese labels and tests for duplicate titles and credential
  cache behavior.

### Verification

- Verification date: 2026-10-26 (Windows/MSVC).
- `cargo fmt --all` and hard-constraint verification passed (152 tests passed,
  9 Docker-only tests skipped by that script).
- Docker SSH integration passed 9/9.
- Isolated Windows GUI screenshots and manual DPI, IME and glyph checks remain
  outstanding.

## [0.1.30] - 2026-10-25

Adds logged-in session inspection to the server inspector.

### Added

- Add a Logged-in sessions group that reads up to 32 rows from the remote
  `who` command through a bounded SSH exec channel.
- Display user, terminal, login time and source address with loading, empty
  and failure states; keep results in the current session.
- Add Simplified Chinese labels and bounded parser/failure-path tests.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, `cargo check --locked --all-targets` and
  `cargo test --locked` (150 passed, 9 Docker-only ignored).
- Hard-constraint verification passed; Docker SSH integration passed 9/9.

## [0.1.29] - 2026-10-25

Adds remote listening-port inspection to the server inspector.

### Added

- Add a Listening ports group that reads up to 64 TCP/UDP listeners through
  `ss -H -lntup`, with a `netstat` fallback.
- Display protocol, local address and process information with loading,
  empty and failure states; keep results in the current session.
- Add Simplified Chinese labels and bounded parser/failure-path tests.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, `cargo check --locked --all-targets` and
  `cargo test --locked` (148 passed, 9 Docker-only ignored).
- Hard-constraint verification passed; Docker SSH integration passed 9/9.

## [0.1.28] - 2026-10-25

Adds remote storage mount inspection to the server inspector.

### Added

- Add a Storage group that reads up to 32 mounted filesystems through a
  bounded SSH exec channel using POSIX `df -P -k`.
- Display mount point, filesystem, used/total capacity, available capacity
  and usage severity with loading, empty and failure states.
- Add Simplified Chinese labels and parser, limit and formatting tests.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, `cargo check --locked --all-targets` and
  `cargo test --locked` (146 passed, 9 Docker-only ignored).
- Hard-constraint verification passed; Docker SSH integration passed 9/9.

## [0.1.27] - 2026-10-25

Adds remote network interface inspection to the server inspector.

### Added

- Add a Network group that reads up to 32 remote interfaces through a bounded SSH exec channel, using `ip -o -brief addr show` with a `/sys/class/net` fallback.
- Display interface name, operational state and reported addresses with loading, empty and failure states; keep results in the current session.
- Add Simplified Chinese labels and parser/failure-path tests.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, `cargo check --locked --all-targets` and `cargo test --locked` (143 passed, 9 Docker-only ignored).
- Hard-constraint verification passed; Docker SSH integration passed 9/9.

## [0.1.26] - 2026-10-25

Adds current-session server alert history.

### Added

- Record new or changed CPU, memory and root-disk threshold alerts after
  metrics or process refreshes.
- Show the latest bounded alert history in the Server health inspector with
  observation time and threshold values.
- Add a clear-history icon; history is session-only and is cleared on
  disconnect, failure or explicit clear.
- Add Simplified Chinese labels and pure threshold/history formatting tests.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, `cargo check --locked --all-targets --offline` and
  `cargo test --locked --offline` (141 passed, 9 Docker-only ignored).
- Hard-constraint verification and Docker SSH integration passed.

## [0.1.25] - 2026-10-25

Adds bounded remote service log viewing to the server inspector.

### Added

- Add a per-service log icon that reads the latest bounded entries through a
  separate SSH exec channel, using `journalctl` with a `service status` fallback.
- Keep service log output in the current session inspector only; cap requests at
  80 lines, 64 KiB and three seconds, with explicit failure feedback.
- Add Simplified Chinese labels, protocol parsing and failure-path tests.
- Harden the metrics marker command to use portable `printf` output.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, `cargo check --locked --all-targets --offline` and
  `cargo test --locked --offline` (139 passed, 9 Docker-only ignored).
- Hard-constraint verification passed; Docker SSH integration passed 9/9.
- The new Services log icon still needs an isolated Windows desktop screenshot;
  no GUI or DPI/IME claim is made here.

## [0.1.24] - 2026-10-25

Adds guarded remote server power controls to the Server health inspector.

### Added

- Add confirmed `Reboot server` and `Shut down server` actions.
- Route power actions through a separate SSH exec channel with fixed commands,
  a three-second timeout, a 64 KiB response limit and explicit result feedback.
- Add Simplified Chinese labels and parser coverage for power action responses.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, `cargo check --locked --all-targets --offline` and
  `cargo test --locked --offline` (146 passed, 9 Docker-only ignored).
- Hard-constraint verification passed; Docker SSH integration passed 9/9.
- Windows manual verification confirmed the Chinese confirmation dialog; the action was cancelled without changing the test server state.

## [0.1.23] - 2026-10-25

Adds confirmed remote process termination to the server inspector.

### Added

- Add a per-process Terminate action that validates the PID, sends `SIGTERM`
  through a separate SSH exec channel, reports failures and refreshes the list
  after success.
- Add confirmation UI, Simplified Chinese labels and parser coverage for
  process action responses.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, `cargo check --locked --all-targets --offline` and
  `cargo test --locked --offline` (136 passed, 9 Docker-only ignored).
- Hard-constraint and Docker SSH verification remain part of the release
  closeout.

## [0.1.22] - 2026-10-25

Adds reusable saved credentials and guarded service controls to the server
inspector.

### Added

- Save host passwords, key passphrases and jump-host credentials in the local
  host profile so saved-host connections can proceed without a repeated prompt.
- Persist credentials after a successful authentication, reopen the credential
  dialog after a rejection, and keep profile exports credential-free.
- Add Start, Stop and Restart service actions with service-name validation,
  confirmation UI, systemd/service fallback execution and automatic refresh.
- Add parser and validation coverage for service action responses and saved
  credential round trips.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, `cargo check --locked --all-targets --offline` and
  `cargo test --locked --offline` (135 passed, 9 Docker-only ignored).
- Hard-constraint and Docker SSH verification remain part of the release
  closeout.

## [0.1.21] - 2026-10-25

Adds service status and configurable threshold warnings to the server inspector.

### Added

- Add a Services inspector group with systemd parsing and a `service --status-all`
  fallback for hosts without systemd.
- Add persisted CPU, memory and root-disk alert thresholds with validation and
  warning states in the server inspector.
- Add bounded service parsing, threshold validation, Simplified Chinese labels
  and malformed-response coverage.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, `cargo check --locked --all-targets --offline` and
  `cargo test --locked --offline` (134 passed, 9 Docker-only ignored).
- `scripts\verify-hard-constraints.ps1` passed; `scripts\test-docker-ssh.ps1`
  passed all 9 Docker SSH tests. Services fallback was also exercised in the
  local `127.0.0.1:2222` container.

## [0.1.20] - 2026-10-25

Adds a bounded read-only process list to the connected-server inspector.

### Added

- Add a Processes inspector group with refresh, loading, empty and error states.
- Query PID, user, CPU percentage, memory percentage, state and command through
  a separate SSH exec channel without writing to the terminal or session log.
- Bound process responses to 64 KiB and three seconds, with parser coverage for
  valid, missing-marker and malformed rows in English and Simplified Chinese.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, `cargo check --locked --all-targets --offline` and
  `cargo test --locked --offline` (131 passed, 9 Docker-only ignored).
- Hard-constraint and Docker SSH verification remain part of the release closeout.

## [0.1.19] - 2026-10-25

Completes the saved-host authentication flow so connecting from the sidebar is
predictable and retryable.

### Fixed

- Clicking a saved host row now connects or opens the credential-only dialog;
  editing remains available from the settings icon.
- Cancelling after an authentication failure removes the failed session instead
  of leaving a disconnected entry in the sidebar.
- Successful credentials remain reusable only in `Zeroizing` memory for the
  current run; rejected credentials are cleared before retrying.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, `cargo check --locked --all-targets --offline` and
  `cargo test --locked --offline` (129 passed, 9 Docker-only ignored).
- `scripts\verify-hard-constraints.ps1` passed; `scripts\test-docker-ssh.ps1`
  passed all 9 Docker SSH tests.
- Fresh Simplified Chinese authentication evidence:
  `artifacts/dshell-v0.1.19-auth-dialog-cn.png`.
- Password entry, retry and successful in-memory reuse remain manual Windows
  checks; DPI/IME and glyph-level checks remain manual as required.

## [0.1.18] - 2026-10-25

Adds an on-demand server health view for connected SSH sessions.

### Added

- Add a Server health inspector group with OS, kernel, load average, memory,
  root filesystem usage and uptime fields.
- Run the read-only probe through a short-lived SSH exec channel so metrics do
  not pollute the interactive terminal or session log.
- Add bounded parsing, timeout/error handling, Simplified Chinese labels and
  unit coverage for supported and malformed responses.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, `cargo check --locked --all-targets --offline` and
  `cargo test --locked --offline` (129 passed, 9 Docker-only ignored).
- Hard-constraint and Docker SSH verification are run for this milestone.

## [0.1.17] - 2026-10-25

Improves saved-host authentication so repeated connections do not require
entering the same secret during one dshell run.

### Added

- Add a credential-only modal for saved-host connections, including password,
  keyboard-interactive password, private-key passphrase and jump-host inputs.
- Remember successful passwords and passphrases only in Zeroizing memory for the
  current run; credentials remain absent from host profiles, settings and logs.
- Clear a rejected credential from the memory cache and reopen the prompt so the
  user can retry or cancel without returning to session properties.
- Add credential-cache unit coverage and a fresh Chinese workspace screenshot.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, `cargo check --locked --all-targets --offline`,
  `cargo test --locked --offline` (126 passed, 9 Docker-only ignored) and
  `scripts\verify-hard-constraints.ps1` passed.
- `scripts\test-docker-ssh.ps1` passed all 9 Docker SSH tests.
- Fresh UI evidence: `artifacts/dshell-v0.1.17-chinese-workspace.png`.
- Credential entry remains a manual Windows interaction; DPI/IME and glyph-level
  checks remain manual as required by the repository verification policy.

## [0.1.16] - 2026-10-25

Completes Simplified Chinese coverage for the newer workspace panels and
records a fresh Windows UI verification for the local manual SSH service.

### Added

- Translate tunnel/SOCKS5 controls, quick commands, keyword highlights,
  compose, keyboard-interactive authentication, synchronized input, tab menus,
  inspector labels and connection validation feedback.
- Keep user-provided host names, commands, remote paths and terminal output
  unchanged while translating application-owned labels and status text.
- Document the repeatable local SSH service at `127.0.0.1:2222` and include a
  fresh Chinese workspace screenshot from the current build.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, `cargo check --locked --all-targets --offline`,
  `cargo test --locked --offline` (121 passed, 9 Docker-only ignored) and
  `scripts\verify-hard-constraints.ps1` passed.
- Fresh UI evidence: `artifacts/dshell-v0.1.16-chinese-fresh.png`.
- Manual SSH container remains bound to `127.0.0.1:2222`; GUI DPI/IME and
  glyph-level interaction remain manual checks.

## [0.1.15] - 2026-10-25

Adds OpenSSH key-pair management to the Keys panel.

### Added

- Import an existing OpenSSH private key into `~/.ssh` without overwriting an
  existing key or relying on a sidecar `.pub` file.
- Export a validated private key and its matching public key to a selected
  destination.
- Delete a discovered key pair only after an explicit confirmation, with safe
  handling for missing or non-regular public-key companions.
- Add Simplified Chinese labels and feedback for key import, export and delete.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, `cargo test --locked --offline` and
  `scripts\verify-hard-constraints.ps1` passed (121 unit tests passed, 9
  Docker-only tests ignored by the unit-test run).
- `cargo check --locked --all-targets --offline` and
  `scripts\test-docker-ssh.ps1` passed (9 Docker integration tests).
- The new Windows Keys-panel import/export/delete clicks and confirmation
  remain a manual desktop check; the isolated Computer Use turn was stopped
  before those actions were completed.

## [0.1.14] - 2026-10-25

Adds credential-free host profile backup and migration.

### Added

- Export saved host profiles to a versioned JSON file without recent history,
  passwords, passphrases or private key material.
- Import an exported host profile file from the flat icon menu and skip duplicate
  names without overwriting existing profiles.
- Add Simplified Chinese labels and feedback for both actions.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, `cargo test --locked --offline` and
  `scripts\verify-hard-constraints.ps1` passed (119 unit tests passed, 9
  Docker-only tests ignored by the unit-test run).
- The previous Docker SSH integration suite remains green; this milestone only
  changes profile serialization, menu actions and localized feedback.

## [0.1.13] - 2026-10-25

Adds persistent host-group management to the server sidebar.

### Added

- Create empty groups from the sidebar and persist their display names.
- Rename groups from the header actions; matching hosts and saved order update together.
- Delete groups with an explicit confirmation; their hosts move to `UNGROUPED`.
- Keep older `settings.json` files compatible through optional normalized group metadata.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, `cargo test --locked --offline` and
  `scripts\verify-hard-constraints.ps1` passed (116 unit tests passed, 9
  Docker-only tests ignored by the unit-test run).
- The previous Docker SSH integration suite remains green; this milestone only
  changes group metadata, sidebar UI and settings persistence.

## [0.1.12] - 2026-10-25

Completes the visible Simplified Chinese feedback for host profiles and the
main menu footer.

### Added

- Translate profile load, recent-connection, favorite, group-move, save/delete,
  and authentication-template feedback while preserving user-provided names.
- Translate the session editor's confirmation buttons and derive the footer
  version from `CARGO_PKG_VERSION` so it cannot lag behind the package version.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, `cargo test --locked --offline` and
  `scripts\verify-hard-constraints.ps1` passed (114 unit tests passed, 9
  Docker-only tests ignored by the unit-test run).
- The existing Docker SSH integration suite remains green from the `v0.1.11`
  release; this patch changes only UI localization and version display.
- Fresh-build screenshots are stored in `artifacts/` and remain untracked.

## [0.1.11] - 2026-10-25

Adds persistent ordering for host groups in the server sidebar.

### Added

- Drag a group header onto another group header to reorder saved groups.
- Persist normalized group order in versioned UI settings while remaining compatible with older settings files.
- Keep the local manual Docker SSH service available on `127.0.0.1:2222` for desktop smoke testing.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, `cargo test --locked --offline` and `scripts\verify-hard-constraints.ps1` passed (114 unit tests passed, 9 Docker-only tests ignored by the unit-test run).
- `scripts\test-docker-ssh.ps1`: 9 integration tests passed.
- Manual Windows screenshots captured from the fresh build: [Chinese workspace](artifacts/dshell-v0.1.11-chinese-workspace.png), [local SSH form](artifacts/dshell-v0.1.11-local-ssh-form.png), and [authentication form](artifacts/dshell-v0.1.11-auth-cn.png). Real DPI, IME and glyph alignment remain manual checks.

## [0.1.10] - 2026-10-25

Completes Simplified Chinese coverage for the workspace shell and input placeholders.

### Added

- Localize compose, replay, split, disconnect, authentication explanations and remaining input placeholder labels.
- Refresh host name, SFTP metadata and keyboard-interactive response placeholders when the language changes.
- Keep the manual Docker SSH service available on `127.0.0.1:2222` for desktop smoke testing.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all` and `scripts\verify-hard-constraints.ps1` passed (113 unit tests passed, 9 Docker-only tests ignored by the unit-test run).
- `scripts\test-docker-ssh.ps1`: 9 integration tests passed.
- Manual Windows screenshot captured from the running Chinese shell; real DPI, IME and glyph alignment remain manual checks.

## [0.1.9] - 2026-10-25

Adds persistent collapsing for host groups in the server sidebar.

### Added

- Collapse or expand each host group from its chevron header.
- Persist collapsed group keys in versioned UI settings without storing credentials.
- Load older settings files with all groups expanded and normalize group keys case-insensitively.
- Add Simplified Chinese documentation for the new sidebar behavior.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, offline `cargo check --locked`, and `cargo test --locked --offline` (113 passed, 9 ignored) passed.
- `scripts\test-docker-ssh.ps1`: 9 integration tests passed.
- Real Windows DPI, Chinese IME, and sidebar interaction remain manual checks.

## [0.1.8] - 2026-10-25

Adds remote filesystem capacity visibility to the SFTP workflow.

### Added

- Show total, used, available and free remote filesystem space in the SFTP panel.
- Show filesystem block size and maximum filename length when `statvfs@openssh.com` is available.
- Add an icon refresh action and Simplified Chinese labels for disk usage details.
- Cover the SFTP filesystem information path in the Docker OpenSSH integration test.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, offline `cargo check --locked`, and `cargo test --locked --offline` (112 passed, 9 ignored) passed.
- `scripts\test-docker-ssh.ps1`: 9 integration tests passed.
- Real Windows DPI, Chinese IME, and SFTP panel interaction remain manual checks.

## [0.1.7] - 2026-10-25

Adds remote SFTP metadata management.

### Added

- Show each SFTP entry's permission mode and modification timestamp.
- Add an icon action for editing octal permissions and Unix access/modification timestamps.
- Apply metadata through SFTP `SETSTAT` while preserving the remote file type bits.
- Localize the metadata editor and validation messages in Simplified Chinese.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, offline `cargo check --locked`, and `cargo test --locked --offline` (111 passed, 9 ignored) passed.
- Docker SSH/SFTP integration includes metadata read, update, and missing-target failure checks.
- Real Windows DPI, Chinese IME, and metadata editor interaction remain manual checks.

## [0.1.6] - 2026-10-25

Adds local SSH identity passphrase management.

### Added

- Add a localized Change passphrase action for every discovered OpenSSH private key.
- Verify the old passphrase before writing and keep the original file unchanged on failure.
- Allow an empty new passphrase to remove encryption while preserving the key comment.
- Stage updates beside the original key and atomically replace it, including Windows `MoveFileExW` replacement semantics.

### Verification

- Verification date: 2026-10-25 (Windows/MSVC).
- `cargo fmt --all`, offline `cargo check --locked`, `cargo test --locked --offline` (109 passed, 9 ignored), and `scripts\\verify-hard-constraints.ps1` passed.
- `scripts\\test-docker-ssh.ps1` passed all 9 Docker SSH tests.
- Existing Windows screenshots confirm the Simplified Chinese shell and Keys modal; the new passphrase modal and real DPI/IME behavior remain manual checks.

## [0.1.5] - 2026-10-25

Adds a bounded trigger workflow for common terminal reactions.

### Added

- Add a Bell icon entry in the command bar and More menu for Triggers.
- Keep the trigger engine and every rule disabled until explicitly enabled.
- Support literal, case-insensitive rules with Highlight, Notify and preset-text actions.
- Allow preset text to be sent with an explicit Enter choice through the existing terminal queue.
- Merge enabled trigger highlights into live terminal and log replay rendering.
- Localize the trigger panel, validation messages and input placeholders in Simplified Chinese.
- Clear trigger latches when a session disconnects and keep side panels mutually exclusive.

### Verification

- `cargo fmt --all`, offline `cargo check --locked`, `cargo test --locked --offline` (107 passed, 9 ignored), `scripts\\verify-hard-constraints.ps1` and `scripts\\test-docker-ssh.ps1` passed.

## [0.1.4] - 2026-10-25

Improves local SSH identity management for repeatable server connections.

### Added

- Discover likely OpenSSH private keys in the user's `.ssh` directory without reading private-key contents.
- Show existing identity paths and public-key algorithms in the Keys panel.
- Fill the host authentication form with a discovered identity using one action.
- Copy a discovered public key when its `.pub` file is available.
- Refresh the discovered key list after generating a new key pair.
- Localize the complete Keys modal, including discovery, generation and copy actions, in Simplified Chinese.

### Verification

- `cargo fmt --all`, offline `cargo check --locked`, `cargo test --locked --offline` (104 passed, 9 ignored), hard-constraint verification and Docker SSH integration (9 passed) passed.

## [0.1.3] - 2026-10-24

Improves the SFTP workflow for repeated server administration tasks.

### Added

- Allow selecting multiple local files for upload.
- Add a per-session FIFO transfer queue shared by file and directory transfers.
- Keep overwrite confirmation tied to the active transfer so queued jobs wait safely.
- Automatically start the next queued transfer after success, failure or cancellation.
- Add a visible queued-transfer count and a clear-queue action in the SFTP panel.
- Localize the new queue and SFTP panel labels in Simplified Chinese.

### Verification

- `cargo fmt --all`, offline `cargo check --locked` and `scripts\verify-hard-constraints.ps1` passed.
- Unit tests: 101 passed, 9 Docker-only tests skipped by the unit-test run.
- `scripts\test-docker-ssh.ps1`: 9 integration tests passed.

## [0.1.2] - 2026-10-24

Adds a Simplified Chinese application shell and a repeatable local SSH service
for manual desktop testing.

### Added

- Add English / Simplified Chinese language selection persisted in the user settings file.
- Translate the workspace shell, session editor, settings, sidebar and common tool panels while preserving terminal and remote command output.
- Add `scripts\start-docker-ssh-manual.ps1`, which starts an SSH test container bound only to `127.0.0.1:2222` and prints its temporary credentials, fingerprint and reviewed `known_hosts` line.

### Verification

- `cargo fmt --all`, offline `cargo check --locked`, `scripts\verify-hard-constraints.ps1` and `git diff --check` passed.
- Unit tests: 99 passed, 9 Docker-only tests skipped by the unit-test run.
- `scripts\test-docker-ssh.ps1`: 9 integration tests passed.
- Windows manual UI capture confirmed the Simplified Chinese shell and SSH session form; the local service is reachable on `127.0.0.1:2222`.

## [0.1.1] - 2026-10-24

Maintenance release fixing missing command-bar and menu icons in fresh builds.

### Fixed

- Provide the SVG asset source required by gpui-component icons, including the toolbar, More menu and workspace layout presets.
- Remove duplicate hover styling from layout preset buttons that could panic when a preset was activated.

### Verification

- `scripts\verify-hard-constraints.ps1` passed: all-target type checking and 106 tests (97 passed, 9 Docker-only tests skipped by the unit-test run).
- Docker integration tests were not rerun because this release changes only UI assets and layout-menu styling; the previous SSH integration run is documented under `v0.1.0`.
- UI evidence: `artifacts/dshell-v0.1.1-layout-menu.png` and `artifacts/dshell-v0.1.1-more-menu.png`.

## [0.1.0] - 2026-10-24

First public release of the native Rust + GPUI SSH desktop client.

### Added

- Strict OpenSSH `known_hosts` verification that rejects unknown and changed server keys.
- Independent SSH session tabs with horizontal and stacked split layouts, tab reordering, reconnect and disconnect handling.
- Saved hosts with groups, favorites, recent connections, search, OpenSSH config import and one-level jump hosts.
- Password, OpenSSH private-key, keyboard-interactive and system SSH-agent authentication without persisting credentials.
- SFTP browsing and recursive transfer with resume, overwrite confirmation, cancellation, rename, directory creation and deletion.
- Local, remote and dynamic SOCKS5 forwarding with persisted rules and per-rule controls.
- Timestamped session logs, legacy raw-log replay, replay timeline navigation and terminal search.
- Ed25519, RSA and ECDSA key generation, reusable authentication templates and persisted quick commands.
- Synchronized input with explicit connected-session targets and a compose pane with draft history.
- Flat icon command bar, grouped More menu, workspace layout presets, tab context actions and flat Quick commands cards.
- Keyword highlighting for up to 32 in-memory literal, case-insensitive rules in live terminals and log replay.
- Semantic light/dark UI themes, terminal themes, adjustable sidebar and inspector widths, and keyboard-first overlays.

### Verification

- Windows/MSVC verification completed on October 24, 2026.
- `scripts\verify-hard-constraints.ps1` passed: 105 tests ran, 96 passed and 9 Docker-only tests were skipped by the unit-test run.
- SSH, authentication, SFTP, forwarding, logging and PTY integration coverage was previously verified by `scripts\test-docker-ssh.ps1`; the final release-only UI/menu changes did not alter those paths.
- `Cargo.lock` remains the dependency source of truth and no WebView, Electron, Tauri or Wry runtime is included.

### Known limitations

- Trigger automation, arbitrary scripts, local shell sessions and non-SSH protocols are not included.
- Keyword rules are in-memory only and do not persist across restarts.
- Real Windows DPI, IME composition, glyph alignment and GUI interaction still require a genuinely isolated desktop smoke test.

