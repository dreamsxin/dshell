# Changelog

All notable changes to dshell are documented here.

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

