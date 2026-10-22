# Repository Instructions

- Keep the application native: use Rust and GPUI; do not add WebView, Electron, Tauri, or Wry dependencies.
- Preserve strict SSH host-key verification. Accept only keys already present in the user's OpenSSH `known_hosts`; reject unknown or changed keys without an automatic trust path.
- Never persist or log passwords. Keep credential handling scoped to the connection flow and preserve zeroization where the application controls the value.
- Keep russh work on its dedicated Tokio runtime thread. Use bounded channels to communicate with GPUI, and batch terminal output updates.
- Keep the terminal layers separated: `alacritty_terminal` owns VT state, the SSH loop owns remote I/O and PTY size changes, and GPUI renders the resulting frame.
- When `.codegraph/` exists at the repository root, use CodeGraph before searching for or reading source code.
- Run `./scripts/verify-hard-constraints.ps1` on Windows after Rust changes. It covers formatting, all-target type checking, dependency boundaries, and tests.
- Update `IMPLEMENTATION_STATUS.md` when a milestone changes. Record completed behavior, validation, and remaining limitations so another short session can continue without relying on chat history.
