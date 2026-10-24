# Repository Instructions

## Runtime and dependencies

- Keep the application native: Rust 2024 with GPUI and gpui-component. Never add WebView, Electron, Tauri or Wry dependencies.
- Prefer existing dependencies. Commit `Cargo.lock` changes and keep verification working with `--locked`; unit tests must not require network access.
- When `.codegraph/` exists at the repository root, use CodeGraph before searching or reading source code. Never stage `.codegraph/` or generated `target/` files.

## Security and credentials

- Preserve strict SSH host-key verification. Accept only keys already present in the user's OpenSSH `known_hosts`; unknown or changed keys fail closed. Never add an automatic trust path, an interactive trust prompt or code that rewrites `known_hosts`.
- Never persist or log passwords, passphrases, key material or keyboard-interactive responses. Keep application-owned copies in `Zeroizing` and clear credential inputs when connecting, cancelling or closing the editor.
- Host-key algorithms and fingerprints are display-only. Session logs may contain sensitive terminal output; recording stays explicitly opt-in and must never be described as redacted.
- SFTP remote names must be a single safe path segment. Validate every downloaded relative path so it cannot escape the selected local root, and skip symlinks and special files.

## Concurrency and terminal layers

- Keep russh on its dedicated Tokio runtime thread. Communicate with GPUI through bounded channels.
- Batch terminal output on the 16 ms render tick; do not emit a GPUI update per SSH packet.
- Keep the terminal layers separated: `alacritty_terminal` owns VT state, the SSH loop owns remote I/O and PTY size changes, and GPUI only renders the resulting frame.
- Keep `Ctrl/Cmd+Shift+C/V` for terminal copy/paste and `Alt+1..9` for tab selection. Workspace shortcuts must not shadow them.
- Recalculate PTY rows and columns once when a layout change settles, not on every drag or animation frame.

## UI conventions

- User-facing UI strings are English. All colors, spacing, radii and font sizes come from `src/ui/tokens.rs` and `ThemeTokens`; render code must not add raw colors or pixel literals.
- Reuse `src/ui/components.rs` (`Field`, `PanelHeader`, `Button`, `EmptyState`, `modal_*`, `toast`) instead of duplicating styles. Keep the sidebar within 200–360 px and the inspector within 300–520 px.
- Keep reusable visual primitives in `src/ui/`; `main.rs` may keep state and render helpers, but new shared widgets belong in the UI module. Source comments and doc comments are English.
- Overlays use the shared modal chrome, close on `Esc`, and return focus to the terminal. Destructive confirmations put the safe action first and never autofocus the destructive button.
- Keep terminal palettes independent from the UI light/dark theme. Use `InputState`/`TextInput` for all text entry so Windows IME composition keeps working.

## Persistence and formats

- Settings, host profiles and snippets are versioned JSON in the user configuration directory. Missing fields use documented defaults, older versions must load without losing data, and credentials are never written.
- New session logs use the `DSHELL-LOG` header and `DSL2` timestamp/length records. Keep legacy raw logs readable, and keep appending raw bytes when the destination is a legacy log.
- `docs/UI_DESIGN.md` is the visual source of truth; keep behavior, README, ROADMAP and status notes consistent with the implemented UI.

## Verification

- After Rust changes run `cargo fmt --all`, then `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\verify-hard-constraints.ps1`. The script covers formatting, all-target type checking, dependency boundaries, `known_hosts` usage and unit tests.
- Also run `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\test-docker-ssh.ps1` when touching SSH, authentication, SFTP, forwarding, logging or PTY paths. Docker tests run serially against temporary containers, keys and an isolated `known_hosts`.
- Add unit tests for new pure logic and failure paths. Never delete, ignore or weaken an existing test just to make a change pass.
- The Windows shell is PowerShell 5.1: `&&` is invalid; use separate commands or `;`.
- Do not launch the GUI on the user's active desktop for automated smoke tests. DPI, IME and glyph-alignment checks require a genuinely isolated interactive environment; report them as manual work instead of claiming automated coverage.

## Git and documentation

- Use conventional commit prefixes (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`) and one commit per milestone.
- Do not amend or rebase commits that were already pushed and never force push. Do not push unless the user authorized it; pull with rebase first if the remote moved.
- Update `IMPLEMENTATION_STATUS.md` for every milestone with completed behavior, verification counts, Docker result, remaining limitations and the next handoff. Update `README.md` for user-visible behavior and `ROADMAP.md` for phase status.

## Release baseline

- The first public release is `v0.1.0`, matching the package version in `Cargo.toml`.
- A release must include `CHANGELOG.md`, updated `README.md`, `ROADMAP.md` and `IMPLEMENTATION_STATUS.md`, plus a clean `cargo fmt --all` and `scripts\verify-hard-constraints.ps1` run.
- Before publishing, verify `cargo check --locked --all-targets`, `cargo test --locked`, the dependency boundary checks, and the strict `known_hosts` checks. Run `scripts\test-docker-ssh.ps1` when the release includes SSH, authentication, SFTP, forwarding, logging or PTY changes.
- Publish releases from a conventional commit, create an annotated `vX.Y.Z` tag, and push the branch and tag only after checking that `origin/main` has not moved. Never force-push release history.
- Release notes must state the verification date, test counts, Docker result, known limitations and any manual Windows desktop checks that remain outstanding. Do not claim GUI, DPI, IME or glyph validation unless it ran in a genuinely isolated interactive environment.
