# dshell

dshell is a Rust-native SSH terminal prototype built with GPUI, russh, and
alacritty_terminal. The terminal UI is drawn by GPUI; there is no WebView
runtime.

## Run

The server host key must already be present in the current user's OpenSSH
known_hosts file. Unknown or changed keys are rejected. Verify a key out of
band before adding it to known_hosts.

Run cargo run, enter the host, port, user, and password, then connect. The
password field is masked and cleared when connection starts. Passwords are
used only for the current SSH authentication attempt and are not persisted.
Commands are currently sent one line at a time.

## Verify

Run .\scripts\verify-hard-constraints.ps1 from PowerShell. It checks
formatting, Rust targets, tests, and the resolved dependency graph for
WebView/Electron packages.

See IMPLEMENTATION_STATUS.md for the current implementation boundary and the
next-session handoff.

## License

Licensed under the MIT License. See [LICENSE](LICENSE) for details.
