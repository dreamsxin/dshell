# Changelog

All notable changes to dshell are documented here.

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

