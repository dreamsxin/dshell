# Changelog

All notable changes to dshell are documented here.

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

