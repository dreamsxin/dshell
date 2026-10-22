# dshell

dshell is a Rust-native SSH terminal prototype built with GPUI, russh, and
alacritty_terminal. The terminal UI is drawn by GPUI; there is no WebView
runtime.

Drag with the left mouse button to select terminal text. Use `Ctrl+Shift+C` to
copy a non-empty selection and `Ctrl+Shift+V` to paste from the system clipboard;
the terminal's bracketed-paste mode is respected.

## Run

The server host key must already be present in the current user's OpenSSH
known_hosts file. Unknown or changed keys are rejected. Verify a key out of
band before adding it to known_hosts.

Run `cargo run --locked`, enter the host, port, user, and password, then
connect. The password field is masked and cleared when connection starts.
Passwords are used only for the current SSH authentication attempt and are not
persisted. Click the terminal area to focus it, then use the keyboard as you
would in a remote terminal. The terminal grid follows window resizing. When the
remote application enables mouse tracking, clicks, motion, and the wheel are
sent to it; otherwise, left-button dragging selects text and the wheel browses
terminal history.

Use `Cancel` while the SSH handshake is in progress and `Disconnect` to end an
active session. Remote exit and connection failure clear the terminal view so a
new connection starts with a clean session.

Saved hosts can be searched, loaded, updated, and deleted from the left panel.
The profile file stores only the display name, host, port, and username. On
Windows it is stored at `%APPDATA%\dshell\hosts.json`; passwords are requested
for each connection and are never written to the profile file.

## Test and Verify

Run the unit tests directly:

```powershell
cargo test --locked
```

Run the full repository checks from PowerShell:

```powershell
.\scripts\verify-hard-constraints.ps1
```

The verification script checks formatting, all-target compilation, unit tests,
and the resolved dependency graph for WebView/Electron packages.

Run the SSH/PTY integration test against a temporary Ubuntu 24.04 OpenSSH
container (Docker required):

```powershell
.\scripts\test-docker-ssh.ps1
```

The script builds a local `dshell-ssh-test:ubuntu-24.04` image with OpenSSH,
Vim, tmux, and htop. Docker reuses the package-install layer on later runs, so
`apt-get update` runs again only when that build layer is invalidated. Each test
uses a random temporary password and isolated `known_hosts` file, then removes
the container and temporary host-key file. The SSH/PTY round trip exercises
Vim's full-screen buffer and Unicode input/save/readback.

See [IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md) for the current
implementation boundary and next-session handoff. [ROADMAP.md](ROADMAP.md)
documents the product direction, SSH workflow priorities, and
staged implementation and acceptance plan.

## License

Licensed under the MIT License. See [LICENSE](LICENSE) for details.
