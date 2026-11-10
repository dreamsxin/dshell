# Changelog

All notable changes to dshell are documented here.

## Unreleased

No unreleased changes yet.

## [0.1.62] - 2026-11-09

Adds password authentication for HTTP CONNECT and SOCKS5 outbound proxies.

### Added

- Add optional proxy usernames to saved GUI host profiles. Prompt for the
  password at connection time; profile files and exports never contain it.
- Add `--proxy-user` and `--proxy-password-stdin` to CLI `exec` and SFTP
  operations. When proxy and SSH secrets are both read from stdin, the proxy
  password is first and the SSH password/passphrase is second.
- Send HTTP CONNECT Basic credentials and SOCKS5 username/password credentials
  from zeroizing runtime buffers. SOCKS5 authentication negotiation does not
  offer anonymous fallback when credentials are configured.
- Keep SSH target host-key checks and jump-host routing unchanged. Document
  that HTTP CONNECT Basic and SOCKS5 credentials are not encrypted by those
  proxy protocols and should only be used on trusted networks.

### Verification

- Verified on 2026-11-09 with `cargo fmt --all`,
  `cargo check --locked --all-targets`, `cargo test --locked` (127 library +
  141 GUI + 14 CLI passed; 14 Docker tests ignored by the default run), and
  `scripts\\verify-hard-constraints.ps1`.
- `scripts\\test-docker-ssh.ps1` passed 14/14. GUI connection tests and CLI
  SFTP completed real SSH/SFTP sessions through authenticated SOCKS5 and HTTP
  CONNECT as well as SOCKS4 and SOCKS4A.
- `scripts\\build-installer.ps1` built both Release binaries and compiled the
  installer with Inno Setup 6.7.3. `scripts\\test-installer.ps1` passed silent
  per-user installation and uninstall.
- Installer `dshell-setup-v0.1.62-x64.exe` is 11,254,791 bytes. SHA-256:
  `DC9E5875CF42D7BB463BF0DAB7528DCA994C5FECFD25A373DF43733C104A77BC`.
- Installer and executables are unsigned. GUI/installer visuals, DPI, IME and
  glyph checks remain manual work for an isolated Windows desktop.

## [0.1.61] - 2026-11-09

Adds SOCKS4 and SOCKS4A outbound proxy protocols.

### Added

- Add SOCKS4 and SOCKS4A choices to CLI and saved GUI host proxy settings.
- Resolve IPv4 destinations locally for SOCKS4 and send domain names through
  the proxy for SOCKS4A.
- Keep SSH target host-key verification and jump-host routing unchanged.

### Verification

- Verified on 2026-11-09 with `cargo fmt --all`,
  `cargo check --locked --all-targets`, `cargo test --locked` (280 passed), and
  `scripts\\verify-hard-constraints.ps1`.
- `scripts\\test-docker-ssh.ps1` passed 14/14, covering GUI and CLI sessions
  through SOCKS4, SOCKS4A, SOCKS5 and HTTP CONNECT.
- `cargo build --release --locked --bin dshell --bin dshell-cli` and Inno
  Setup 6.7.3 compilation passed; `scripts\\test-installer.ps1` passed silent
  per-user installation and uninstall.
- Installer `dshell-setup-v0.1.61-x64.exe` is 11,223,900 bytes. SHA-256:
  `5344EB55DCEF3AFD3CF8195113B0BB1A3F9E773EFCBBB80CD83981DAB52D6AD1`.
- Installer and executables are unsigned. GUI/installer visuals, DPI, IME and
  glyph checks remain manual work for an isolated Windows desktop.

## [0.1.60] - 2026-11-09

Adds per-host outbound proxy settings to the GUI.

### Added

- Configure direct, SOCKS5 or HTTP CONNECT transport in a saved host profile.
- Apply the proxy to the initial outbound TCP connection; with a jump chain,
  the proxy connects to the first jump host and later hops use SSH channels.
- Retain strict target and jump-host `known_hosts` verification and default
  older host profiles to direct connections.

### Verification

- Verified on 2026-11-09 with `cargo fmt --all`,
  `cargo check --locked --all-targets`, `cargo test --locked` (280 passed), and
  `scripts\\verify-hard-constraints.ps1`.
- `scripts\\test-docker-ssh.ps1` passed 14/14, including CLI SOCKS5/HTTP
  CONNECT tunnels and GUI connection-runner tunnels through SOCKS5 and HTTP
  CONNECT.
- `cargo build --release --locked --bin dshell --bin dshell-cli` and Inno
  Setup 6.7.3 compilation passed; `scripts\\test-installer.ps1` passed silent
  per-user installation and uninstall.
- Installer `dshell-setup-v0.1.60-x64.exe` is 11,216,984 bytes. SHA-256:
  `B8DD171B0115B20630EB2369D10388433E7100C8D093A15AC709DCF336C11D98`.
- Installer and executables are unsigned. GUI/installer visuals, DPI, IME and
  glyph checks remain manual work for an isolated Windows desktop.

## [0.1.59] - 2026-11-09

Adds outbound SOCKS5 and HTTP CONNECT proxies to CLI SSH operations.

### Added

- Add `--proxy-type socks5|http-connect`, `--proxy-host` and optional
  `--proxy-port` to CLI `exec` and SFTP commands.
- Tunnel SSH through the proxy while keeping DNS resolution for the target at
  the proxy and strict `known_hosts` verification bound to the SSH target.
- Fail closed when HTTP CONNECT is denied or SOCKS5 requests an unsupported
  authentication method.

### Verification

- Verified on 2026-11-09 with `cargo fmt --all`,
  `cargo check --locked --all-targets`, `cargo test --locked` (279 passed), and
  `scripts\\verify-hard-constraints.ps1`.
- `scripts\\test-docker-ssh.ps1` passed 13/13, including real SFTP sessions
  through HTTP CONNECT and SOCKS5 and explicit proxy failure-path tests.
- `cargo build --release --locked --bin dshell --bin dshell-cli` and Inno
  Setup 6.7.3 compilation passed; `scripts\\test-installer.ps1` passed silent
  per-user installation and uninstall.
- Installer `dshell-setup-v0.1.59-x64.exe` is 11,216,552 bytes. SHA-256:
  `F4575E62C077EBE01A39E925F11570CD7AB14E15099FED72D733B90CAA2A87F9`.
- Installer and executables are unsigned. GUI/installer visuals, DPI, IME and
  glyph checks remain manual work for an isolated Windows desktop.

## [0.1.58] - 2026-11-09

Extends OpenSSH user certificate authentication to saved GUI identities.

### Added

- Automatically discover the OpenSSH user certificate `<identity>-cert.pub`
  beside a saved GUI identity file.
- Keep ordinary public-key authentication when no adjacent certificate exists;
  fail closed when a discovered certificate cannot be loaded or is rejected.
- Retain explicit `--certificate PATH` support in CLI `exec` and SFTP commands.

### Verification

- Verified on 2026-11-09 with `cargo fmt --all`,
  `cargo check --locked --all-targets`, `cargo test --locked` (277 passed), and
  `scripts\\verify-hard-constraints.ps1`.
- `scripts\\test-docker-ssh.ps1` passed 13/13, including explicit CLI
  certificate auth, adjacent certificate discovery, and wrong-principal
  rejection.
- `cargo build --release --locked --bin dshell --bin dshell-cli` and Inno
  Setup 6.7.3 compilation passed; `scripts\\test-installer.ps1` passed silent
  per-user installation and uninstall.
- Installer `dshell-setup-v0.1.58-x64.exe` is 11,207,480 bytes. SHA-256:
  `30265C2F9F201CA54BF2A5A60CEDB4C1E14B23C5DC46590BCB4C9D8F014861C6`.
- Installer and executables are unsigned. GUI/installer visuals, DPI, IME and
  glyph checks remain manual work for an isolated Windows desktop.

## [0.1.57] - 2026-11-09

Adds OpenSSH user certificate authentication to the CLI.

### Added

- Accept `--certificate PATH` alongside `--identity-file PATH` for `exec` and
  SFTP commands.
- Load and authenticate OpenSSH user certificates using the matching private
  key; encrypted private keys continue to read passphrases from stdin.
- Keep certificate selection explicit and preserve strict `known_hosts`
  verification.

### Verification

- Verified on 2026-11-09 with `cargo fmt --all`,
  `cargo check --locked --all-targets`, `cargo test --locked` (277 passed), and
  `scripts\\verify-hard-constraints.ps1`.
- `scripts\\test-docker-ssh.ps1` passed 13/13, including a valid CA-signed
  user certificate and rejection of a certificate with the wrong principal.
- `cargo build --release --locked --bin dshell --bin dshell-cli` and Inno
  Setup 6.7.3 compilation passed; `scripts\\test-installer.ps1` passed silent
  per-user installation and uninstall.
- Installer `dshell-setup-v0.1.57-x64.exe` is 11,216,065 bytes. SHA-256:
  `DC3B97F236B5C839A60DB1398F085BE23427EAF291B957958A231E20FAC804CE`.
- Installer and executables are unsigned. GUI/installer visuals, DPI, IME and
  glyph checks remain manual work for an isolated Windows desktop.

## [0.1.56] - 2026-11-09

Adds explicit text-mode SFTP transfers with streaming line-ending conversion.

### Added

- Add `put-text` and `get-text` to the CLI and version 1 JSON batch files.
- Normalize CR, LF and CRLF to LF during text upload, and emit CRLF during text
  download while preserving all non-newline bytes.
- Keep ordinary `put` and `get` byte-preserving and text transfers non-resumable.

### Verification

- Verified on 2026-11-09 with `cargo fmt --all`,
  `cargo check --locked --all-targets`, `cargo test --locked` (276 passed), and
  `scripts\\verify-hard-constraints.ps1`.
- `scripts\\test-docker-ssh.ps1` passed 13/13, including text transfer
  conversions and JSON batch operation reuse.
- `cargo build --release --locked --bin dshell --bin dshell-cli` and Inno
  Setup 6.7.3 compilation passed; `scripts\\test-installer.ps1` passed silent
  per-user installation and uninstall.
- Installer `dshell-setup-v0.1.56-x64.exe` is 11,184,006 bytes. SHA-256:
  `4F55FF618ABCCA6BA36FF00613ED6A258953C02B806770740FCF511A57A39A42`.
- Installer and executables are unsigned. GUI/installer visuals, DPI, IME and
  glyph checks remain manual work for an isolated Windows desktop.

## [0.1.55] - 2026-11-09

Adds remote SFTP file-management operations to the CLI.

### Added

- Add `rename`, `chmod`, `readlink` and `symlink` SFTP commands.
- Support the same operations in version 1 JSON SFTP batch files.
- Parse chmod modes as 3- or 4-digit octal permissions and escape terminal
  control characters in operation and transfer output.

### Verification

- Verified on 2026-11-09 with `cargo fmt --all`,
  `cargo check --locked --all-targets`, `cargo test --locked` (273 passed), and
  `scripts\\verify-hard-constraints.ps1`.
- `scripts\\test-docker-ssh.ps1` passed 13/13, including permission changes,
  remote renames and symbolic-link create/read/remove.
- `cargo build --release --locked --bin dshell --bin dshell-cli` and Inno
  Setup 6.7.3 compilation passed; `scripts\\test-installer.ps1` passed silent
  per-user installation and uninstall.
- Installer `dshell-setup-v0.1.55-x64.exe` is 11,152,185 bytes. SHA-256:
  `2AA316C8FF1926786ED3510BD9571FC3D8E47DE15E4F538781D4DBFB37396820`.
- Installer and executables are unsigned. GUI/installer visuals, DPI, IME and
  glyph checks remain manual work for an isolated Windows desktop.

## [0.1.54] - 2026-11-09

Adds remote SFTP metadata and canonical-path queries to the CLI.

### Added

- Add `dshell-cli sftp stat` to report remote file type, size, mode and
  modification time.
- Add `dshell-cli sftp realpath` to resolve a remote path through the SFTP
  subsystem.
- Support both read-only queries in version 1 JSON SFTP batch files.

### Verification

- Verified on 2026-11-09 with `cargo fmt --all`,
  `cargo check --locked --all-targets`, `cargo test --locked` (272 passed), and
  `scripts\\verify-hard-constraints.ps1`.
- `scripts\\test-docker-ssh.ps1` passed 13/13, including remote metadata,
  canonical path resolution and missing-path failure.
- `cargo build --release --locked --bin dshell --bin dshell-cli` and Inno
  Setup 6.7.3 compilation passed; `scripts\\test-installer.ps1` passed silent
  per-user installation and uninstall.
- Installer `dshell-setup-v0.1.54-x64.exe` is 11,131,536 bytes. SHA-256:
  `DA21994FAEDBCBDCE4F9F34677D64525A32E3684710D627E9CF1B21DA5619DA9`.
- Installer and executables are unsigned.
- GUI, installer visuals, DPI, IME and glyph checks remain manual work for an
  isolated Windows desktop.

## [0.1.53] - 2026-11-09

Adds opt-in SSH agent forwarding to GUI host profiles.

### Added

- Add a per-host SSH agent forwarding option, disabled by default and retained
  in profile JSON and exports.
- Request agent forwarding only on the final interactive shell session; jump
  hosts remain excluded.
- Show the permission scope in the host profile editor and refuse agent
  channels unless forwarding was enabled for the connection.

### Security Note

An enabled remote account can request signatures from identities available in
the local agent. Enable forwarding only for trusted hosts and accounts.

### Verification

- Verified on 2026-11-09 with `cargo fmt --all`,
  `cargo check --locked --all-targets`, `cargo test --locked` (284 tests), and
  `scripts\\verify-hard-constraints.ps1`.
- `scripts\\test-docker-ssh.ps1` passed 13/13; `scripts\\test-installer.ps1`
  passed silent per-user installation and uninstall.
- Installer `dshell-setup-v0.1.53-x64.exe` is 11,126,325 bytes. SHA-256:
  `A355F48D736E94603E90D6BD061275C1A7CC518B3DAF2D674FDE62F6878DFE5F`.
- Installer and executables are unsigned.
- GUI/profile editor visuals, DPI, IME and glyph checks remain manual work for
  an isolated Windows desktop.

## [0.1.52] - 2026-11-09

Adds opt-in SSH agent forwarding for command-line remote execution.

### Added

- Add `--forward-agent` to `dshell-cli exec`. Agent forwarding stays off unless
  explicitly enabled and is limited to the command's SSH session.
- Proxy agent protocol channels to the existing local OpenSSH agent or Pageant;
  private key material is not copied to the remote host.
- Refuse forwarded agent channels unless the CLI option was explicitly set.

### Security Note

An enabled remote account can request signatures from identities available in
the local agent. Use `--forward-agent` only with trusted remote hosts/accounts.

### Verification

- Verified on 2026-11-09 with `cargo fmt --all` and
  `scripts\\verify-hard-constraints.ps1`: 270 unit tests passed and 13
  Docker-only tests were skipped by that script.
- `scripts\\test-docker-ssh.ps1` passed 13/13, including agent forwarding
  disabled by default, available only when explicitly enabled, and protocol
  frames relayed unchanged.
- `cargo build --release --locked --bin dshell --bin dshell-cli` and Inno
  Setup 6.7.3 compilation passed; `scripts\\test-installer.ps1` passed silent
  per-user installation/uninstall and verified both executables are handled.
- Installer `dshell-setup-v0.1.52-x64.exe` is 11,123,186 bytes. SHA-256:
  `2099E274F13D0DCBEF7E85AB7DA2EACF62A702C720373E08C401D5416714E476`.
- Installer and executables are unsigned. Desktop UI, installer visuals,
  DPI, IME and glyph checks remain manual work for an isolated Windows desktop.

## [0.1.51] - 2026-11-09

Adds final-segment wildcard matching to CLI SFTP transfers.

### Added

- Add `put-glob` and `get-glob` CLI actions and `put_glob`/`get_glob` batch
  operations. Support `*`, `?`, and bracket character classes in the final
  path segment while requiring a literal parent path.
- Skip symlinks and special files, preserve deterministic match ordering,
  and require a literal destination directory.

### Verification

- Verified on 2026-11-09 with `cargo fmt --all` and
  `scripts\\verify-hard-constraints.ps1`: 268 unit tests passed and 13
  Docker-only tests were skipped by that script.
- `scripts\\test-docker-ssh.ps1` passed 13/13, including local and remote
  final-segment glob transfers and remote symlink exclusion.
- `cargo build --release --locked --bin dshell --bin dshell-cli` and Inno
  Setup 6.7.3 compilation passed; `scripts\\test-installer.ps1` passed silent
  per-user installation/uninstall and verified both executables are handled.
- Installer `dshell-setup-v0.1.51-x64.exe` is 11,078,600 bytes. SHA-256:
  `ADCA35B8E1229ED03D2013022AE315CFD3EF05D1A5E1F36481FEC785EE03F25C`.
- Installer and executables are unsigned. Desktop UI, installer visuals,
  DPI, IME and glyph checks remain manual work for an isolated Windows desktop.

## [0.1.50] - 2026-11-08

Adds versioned JSON SFTP batch execution to the command-line client.

### Added

- Execute an ordered batch of SFTP operations in one authenticated
  SSH/SFTP session from a version 1 JSON file.
- Limit batch files to 1 MiB and 256 operations; reject unknown fields and
  prevent batch files from supplying credentials.
- Keep existing destination refusal, explicit `--overwrite`/`--resume`,
  recursive path checks, and terminal-control escaping from `v0.1.49`.
- Stop a batch on its first failed operation and report its one-based index;
  earlier completed operations are not rolled back.

### Verification

- Verified on 2026-11-08 with `cargo fmt --all` and
  `scripts\\verify-hard-constraints.ps1`: 267 unit tests passed and 13
  Docker-only tests were skipped by that script.
- `scripts\\test-docker-ssh.ps1` passed 13/13, including ordered batch
  operations over one SFTP session and a failure test that identifies the
  operation index without rolling back earlier completed operations.
- `cargo build --release --locked --bin dshell --bin dshell-cli` and Inno
  Setup 6.7.3 compilation passed; silent install/uninstall passed and verified
  that both executables are installed and removed.
- Installer `dshell-setup-v0.1.50-x64.exe` is 11,057,222 bytes. SHA-256:
  `7DB77B99A0ACFEEF377098E72C4FB4364BA7F142F9A83A47D8992C8663465FDE`.
- Installer and executables are unsigned. Desktop UI, installer visuals,
  DPI, IME and glyph checks remain manual work for an isolated Windows desktop.

## [0.1.49] - 2026-11-08

Adds batch SFTP actions to the command-line client.

### Added

- Add `dshell-cli sftp` actions for listing, single-file and recursive
  directory transfers, directory creation, file removal, and directory removal.
- Reuse the desktop SFTP transfer worker for resume and recursive path safety.
  Existing destinations require an explicit `--overwrite` or `--resume`.
- Escape terminal control characters in remote file names printed by `ls`.

### Verification

- `cargo fmt --all`, the hard-constraint checks, Docker SSH 13/13, dual-binary
  release build, CLI help, Inno Setup 6.7.3 compilation, and silent installer
  install/uninstall passed.
- Installer `dshell-setup-v0.1.49-x64.exe` is 11,006,630 bytes. SHA-256:
  `173729EAF75D02898873EB6A78D018C231FA48AA59351E3E93095768323C074F`.
- Installer and executables are unsigned. GUI, installer visuals, DPI, IME and
  glyph checks remain manual work for an isolated Windows desktop.

## [0.1.48] - 2026-11-08

Adds a native command-line SSH client for non-interactive remote commands.

### Added

- Add `dshell-cli exec` with SSH agent, private-key, and stdin password
  authentication; forward stdout/stderr and preserve the remote exit status.
- Reuse the desktop client's strict `known_hosts` verification. Bound
  connection time, command time, and combined output size.
- Install `dshell-cli.exe` beside the Windows desktop application.

### Verification

- Verified on 2026-11-08 with `cargo fmt --all` and
  `scripts\\verify-hard-constraints.ps1`: 262 unit tests passed and 12
  Docker-only tests were skipped by that script.
- `scripts\\test-docker-ssh.ps1` passed 12/12, including remote command
  stdout/stderr separation, exit status 7, and rejection with an empty
  `known_hosts` file.
- `cargo build --release --locked --bin dshell --bin dshell-cli` and Inno
  Setup 6.7.3 compilation passed; silent per-user install/uninstall passed
  and verified that both executables are installed.
- Installer `dshell-setup-v0.1.48-x64.exe` is 10,777,950 bytes. SHA-256:
  `23F87A41ABC2D000B1B51DE759190CD95DC8E0C9E4AA3152A71052C8398AC867`.
- Installer and executables are unsigned. Desktop UI, installer visuals,
  DPI, IME and glyph checks remain manual work for an isolated Windows desktop.

## [0.1.47] - 2026-11-08

Adds server-side Unix domain socket forwarding over SSH.

### Added

- Add `Local Unix Socket`: accept a local TCP connection and open an SSH
  `direct-streamlocal` channel to a Unix socket on the SSH server.
- Add `Remote Unix Socket`: request a server-side socket listener and forward
  accepted `forwarded-streamlocal` channels to a TCP target on this machine.
- Save both rule types with host profiles. Older profile/export documents
  default the new socket path field to empty and continue to load.

### Verification

- Verified on 2026-11-08 with `cargo fmt --all` and
  `scripts\\verify-hard-constraints.ps1`: 260 unit tests passed and 11
  Docker-only tests were skipped by that script.
- `scripts\\test-docker-ssh.ps1` passed 11/11, including local TCP to server
  Unix socket echo, server Unix socket to local TCP request/reply, and rule
  removal.
- `cargo build --release --locked --bin dshell` and Inno Setup 6.7.3
  compilation passed; `scripts\\test-installer.ps1` passed silent per-user
  installation and uninstall.
- Installer `dshell-setup-v0.1.47-x64.exe` is 9,137,289 bytes. SHA-256:
  `09B16BCE02E34D1DD7EE6397AB33A081E4CAADEACC36BE529DE86874D56A2DC7`.
- Installer and executable are unsigned. Tunnel form and rule detail layout,
  installer visuals, DPI, IME and glyph checks remain manual work for an
  isolated Windows desktop.

## [0.1.46] - 2026-11-08

Adds per-connection visibility to active SSH forwarding rules.

### Added

- Show each active tunnel's connected peer and destination in both the Tunnels
  panel and the session inspector. Connection entries are removed by unique
  channel ID when the forwarded channel closes, including when peers reuse the
  same local address.

### Verification

- Verified on 2026-11-08 with `cargo fmt --all` and
  `scripts\\verify-hard-constraints.ps1`: 258 unit tests passed and 11
  Docker-only tests were skipped by that script.
- `scripts\\test-docker-ssh.ps1` passed 11/11, including a real SSH tunnel
  echo test that verifies matching connection IDs on channel open and close.
- `cargo build --release --locked --bin dshell` and Inno Setup 6.7.3
  compilation passed; `scripts\\test-installer.ps1` passed silent per-user
  installation and uninstall.
- Installer `dshell-setup-v0.1.46-x64.exe` is 9,119,811 bytes. SHA-256:
  `BE0FED80C26A39D20144D27D541BB3A2E9FDC907B8DB8509E2D32BFE0FB4205F`.
- Installer and executable are unsigned. Tunnel detail UI, installer visuals,
  DPI, IME and glyph checks remain manual work for an isolated Windows desktop.

## [0.1.45] - 2026-11-08

Adds Windows Remote Desktop over the active SSH tunnel, following the Bitvise
SSH Client workflow.

### Added

- Add a Remote Desktop action in the Tunnels panel. Enter an RDP target host
  and port reachable from the SSH server; the defaults are
  `127.0.0.1:3389` on that server.
- Create an ephemeral local forward on a random port bound only to
  `127.0.0.1`, then start `mstsc.exe` after the forward listener is ready.
  The temporary forward is not written to the host profile and is removed
  with the SSH session. Windows Remote Desktop continues to handle RDP
  certificate warnings.

### Verification

- `cargo fmt --all` and `scripts\\verify-hard-constraints.ps1` passed:
  257 unit tests passed, 11 Docker-only tests skipped in that run.
- `scripts\\test-docker-ssh.ps1` passed 11/11, including an echo test to
  container loopback port 3389 through a dynamically allocated local SSH
  forward and listener removal.
- `cargo build --release --locked --bin dshell` and Inno Setup 6.7.3
  installer compilation passed; silent per-user install/uninstall passed
  with `scripts\\test-installer.ps1`.
- Installer `dshell-setup-v0.1.45-x64.exe` SHA-256:
  `F5865A91EA1DE471FC2D09B8BCA87A716C01327A5B9CB52F1DCCF77FAD5BD1F7`.
- Installer and executable are unsigned. Remote Desktop GUI behavior,
  installer visuals, DPI, IME and glyph checks remain manual work for an
  isolated Windows desktop.

## [0.1.44] - 2026-11-08

### Changed

- Cleaner sidebar host interactions, following Xshell conventions: a
  single click now only selects a host row and a double click connects
  (or focuses the live session), so a stray click no longer opens a
  connection; recent entries activate on double click too. Row and
  group-header action icons fade in on hover instead of always taking
  space in every row, connected hosts show a green status dot, and the
  hover action of a connected host disconnects it instead of
  connecting again. The recent-connections arrow now connects to the
  target (focusing a live session, reusing a matching saved profile or
  asking for credentials) instead of only opening the connection
  form; loading the form stays available as the row's second action.
  Host and recent rows gain a right-click context menu (connect or
  show/disconnect the session, edit, favorite, two-step delete;
  connect, load into form, remove from recent) and the RECENT header
  gains a clear-all button.
- Make destructive confirmation dialogs keyboard-safe: pressing Enter no
  longer invokes process termination, service control or server power actions.
- Make Alt+1..9 follow the visible tab order when session tabs are filtered,
  and give the command palette its own search field so its query cannot leak
  into the session switcher.

### Added

- Add the Bitvise-parity batch 2: a host key manager in Settings lists
  the user's known_hosts entries with host pattern, key algorithm and
  SHA256 fingerprint and removes individual entries after an explicit
  two-step confirmation; per-host environment variables ("NAME=value"
  lines in the session editor) are requested on the session channel
  before the PTY and silently ignored by servers that do not accept
  them. The RDP-over-SSH shortcut turns out to be already implemented
  (Tunnels panel, Remote Desktop group); the gap audit document was
  corrected accordingly, and PuTTY .ppk import stays deferred with an
  evaluation note.
- Add Bitvise-style one-click public key installation: the credential
  prompt gains an "Install public key" action that logs in with the
  entered password and appends the selected identity file's public key
  to the account's authorized_keys (idempotent, validated command
  builder with unit tests).
- Enable SSH transport compression (zlib) by turning on russh's
  flate2 feature; sessions now negotiate compression with the server
  instead of being limited to "none".
- Fix the terminal rendering in a proportional font: the default font
  family "monospace" is a GDI generic alias that DirectWrite does not
  resolve, so the terminal silently fell back to a proportional font
  and no cell grid could ever align. The default is now the real
  installed "Consolas" family on Windows, saved settings using the
  broken alias migrate to the platform default on load, and the key
  panels' "Consolas, monospace" font stacks (also unresolvable) use
  the same default.
- Fix the terminal cursor drifting away from typed characters: cell
  metrics are now measured from the configured font's real advance
  width via the text system instead of a fixed ratio estimate, so the
  grid, cursor and glyph positions all agree at every font size and
  family.
- Add Xshell-style input conveniences: copy on select writes terminal
  selections to the clipboard when the mouse is released, right click
  pastes the clipboard while the remote application is not capturing
  the mouse, and the main window restores its last size and position
  on launch. Each behavior is on by default and can be turned off in
  Settings.
- Add ZMODEM send: pick a local file from the session More menu
  ("Send file via ZMODEM (rz)") and dshell starts `rz` on the remote
  host, offers the file and streams it with ZCRCG-continuation
  subpackets up to the 1 MiB ZMODEM limit. The receive parser now also
  handles multi-subpacket ZDATA frames (real `sz` streams) instead of
  single-subpacket frames only.
- Add multi-hop jump host chains: enter comma-separated saved host names
  in the jump host field (for example `bastion, relay`) and dshell dials
  them left to right through nested direct-tcpip channels before reaching
  the target. Each hop authenticates with its saved credentials, each hop
  and the target are host-key verified against known_hosts, and the whole
  chain is torn down when the session closes. Verified by a Docker
  integration test that connects through two chained hops.
- Add a system tray panel: tray icon with a context menu (Open dshell,
  New session, Quit) drawn programmatically in the app accent color.
  Tray clicks activate the main window; New session opens the session
  editor. GUI behavior requires manual verification.
- Add explicit screen-lock password setup, update and clear controls. Locking
  requires a password; the password stays in zeroizing process memory and
  cannot be bypassed with Esc or the lock shortcut.
- Add a multi-size Windows application icon, embed it into the executable,
  and reuse its terminal mark in the system tray.

### Verification

- Verified 2026-11-08 on Windows/MSVC: `cargo fmt --all` and
  `scripts\verify-hard-constraints.ps1` passed; 256 unit tests passed and 11
  Docker-only tests were skipped by that run.
- `scripts\test-docker-ssh.ps1` passed all 11 integration tests.
- `cargo build --release --locked --bin dshell` passed. Inno Setup 6.7.3
  produced `dshell-setup-v0.1.44-x64.exe`; its SHA-256 is
  `17AE94AEBFD93EE901428A60BB7224157D6D991478492D893E27525708779F08`.
- `scripts\test-installer.ps1` passed silent per-user installation and
  uninstallation in a temporary directory.
- The executable and installer are unsigned because no Authenticode
  certificate is configured. Installer wizard visuals, taskbar icon display,
  DPI, IME and glyph alignment still need isolated Windows desktop checks.

## [0.1.43] - 2026-11-06

Adds the first wave of Netcatty/Xshell-parity features: quick-connect
address bar, SFTP file editor, host key verification dialog, command
palette, ZMODEM receive, session restore, profile sync, live server
monitor, Docker containers panel, iTerm2 inline image saving, screen
lock, script recording, session tracing, archive extraction shortcuts,
search match highlighting, host distribution icons, host notes, CSV
export, TOTP keyboard-interactive authentication, and OSC 0/2 title,
OSC 52 clipboard and OSC 9 notification handling.

### Added

- Add a live server monitor in the Server health inspector: toggle auto
  refresh at 1/2/5/10 second intervals with a cycle button; monitoring
  stops automatically when the session disconnects.
- Add archive extraction shortcuts: scan the visible terminal for archive
  filenames (.tar.gz, .zip, .7z, etc.) with absolute paths and fill the
  corresponding extraction command into the terminal.
- Add script recording: capture user input commands during a session and
  save them as a shell script with session metadata and the final visible
  screen as comments.
- Add session tracing: protocol events (connection status, host key info,
  reconnection attempts) are collected per session (max 128 entries) and
  can be exported to a diagnostic log file.
- Add a TOTP (time-based one-time password) module that generates RFC 6238 codes for keyboard-interactive authentication.
- Add a lock screen that covers the workspace with a password field.
  The configured password stays only in the running session, is never
  written to settings or logs, and connections keep running while locked.
- Add Ctrl+Shift+L to lock or unlock the screen.
- Add a command palette (Ctrl+K) that lists workspace actions and
  supports filtering by name.
- Add a hex view of the visible terminal screen in the More menu.
- Add a cursor row highlight in the terminal, visible in live sessions
  and log replay.
- Add a connection test button in the session editor that completes a
  SSH handshake without opening a terminal.
- Add CSV export of host profiles that omit all secret fields.
- Add search match highlighting in the terminal: matched text gets a distinct
  background color from the palette, visible in both live sessions and log
  replay.
- Add distribution icons in the sidebar host list, auto-detected from the
  hostname (Ubuntu, Debian, CentOS, Alpine, Arch, Fedora, Kali, openSUSE,
  Amazon).
- Add a Notes field to host profiles, editable in the session editor and
  saved with the profile.
- Add OSC sequence parsing for dynamic tab titles (OSC 0/2), remote
  clipboard writes (OSC 52), and remote desktop notifications (OSC 9).
- Add an address bar in the command bar (Alt+D to focus) for quick
  `user@host:port` connections.
- Add a "Save visible screen" option in the More menu to save the terminal's
  current visible text to a file.
- Add built-in keyword highlight rules (error, failed, fatal, denied,
  warning, deprecated) that highlight common error and warning output in
  the terminal and log replay without user configuration.
- Add a broadcast mode toggle (Bell icon) to send input to all connected
  sessions simultaneously.
- Add a multi-line paste confirmation dialog that previews clipboard content
  before pasting into the terminal.
- Add a host key verification dialog that prompts to trust and save unknown
  server keys to `~/.ssh/known_hosts` before connecting.
- Add a file picker button for the PublicKey identity file field.
- Add a built-in SFTP file editor: edit a remote file in a modal multi-line
  text area and save back to the host with Ctrl+S, with a 1 MiB size limit,
  unsaved-change confirmation, and Simplified Chinese translations.
- Add a searchable active-session switcher to locate sessions by title, user,
  host or port, show connection state, and switch with keyboard navigation.
- Add the `Ctrl+Shift+P` shortcut and Simplified Chinese translations.
- Add bulk synchronized-input target selection for connected sessions in a
  saved host group and its subgroups.
- Add a read-only comparison of visible terminal screens from two sessions,
  with aligned changes and refresh support.
- Let saved host profiles choose a terminal color scheme independently, while
  older profiles continue to follow the application theme by default.
- Let saved sessions use a custom RGB hex color for their tab accent, with
  versioned profile/export support and Simplified Chinese UI text.
- Filter active session tabs by saved host group, including nested groups and
  ungrouped sessions, with visible counts and selection-safe split behavior.
- Add a built-in SFTP file editor: edit a remote file in a modal multi-line
  text area and save back to the host with Ctrl+S, with a 1 MiB size limit,
  unsaved-change confirmation, and Simplified Chinese translations.

### Verification

- 2026-11-06: `cargo fmt --all`, `scriptserify-hard-constraints.ps1` passed
  (238 unit tests, 10 Docker-specific ignores),
  `scripts	est-docker-ssh.ps1` passed 10/10 against a temporary Ubuntu
  container.
- Known limitations: DPI, IME and glyph alignment checks plus real remote
  `sz`/`docker ps` integration still require isolated Windows desktop
  manual verification; tray icon behavior is newly added and needs manual
  verification (see below).

## [0.1.42] - 2026-10-27

Adds optional commands after login and allows duplicating any active SSH session.

### Added

- Configure up to 16 single-line commands in session properties. When enabled,
  dshell submits them together through the visible interactive terminal after
  the first successful login; failed connections and automatic reconnects do
  not run them.
- Keep command text in version 8 host profiles and version 4 profile exports.
  Older local profiles migrate with the feature disabled, and imported profiles
  always have automatic execution disabled until the user opts in.
- Validate command count, per-command and total size, empty lines, and control
  characters. Added Simplified Chinese labels and validation messages.
- Duplicate a session directly from any active tab, including ad-hoc connections.
  Reuse non-secret launch settings and in-memory credentials when available;
  reopen the existing authentication flow when they are not.

### Verification

- Verified 2026-10-27 on Windows/MSVC: `cargo fmt --all`,
  `scripts\verify-hard-constraints.ps1`, `cargo check --locked --all-targets`
  and `cargo test --locked` passed (196 tests passed, 9 Docker SSH tests ignored
  by the standard suite). `scripts\test-docker-ssh.ps1` passed all 9 tests.
- Isolated Windows GUI interaction, screenshots, DPI/IME and glyph checks remain
  manual; the GUI was not launched on the user's active desktop.

## [0.1.41] - 2026-10-27

Organizes reusable quick commands into optional named groups.

### Added

- Assign a command to a group from the editor; group headings sort by name and
  ungrouped commands remain available under `Ungrouped`.
- Preserve groups in local storage and JSON command bundles. New documents use
  version 2; version 1 files and bundles remain readable, with commands without
  a group defaulting to ungrouped.
- Validate group names for length, surrounding whitespace and control
  characters. Added Simplified Chinese labels and validation text.

### Verification

- Verified 2026-10-27 on Windows/MSVC: `cargo fmt --all`,
  `scripts\verify-hard-constraints.ps1`, `cargo check --locked --all-targets` and
  `cargo test --locked` passed (189 tests passed, 9 Docker SSH tests ignored by
  the standard suite). Docker SSH tests were not rerun because this change does
  not touch SSH or SFTP paths.
- The GUI was not launched on the user's active desktop. Isolated screenshots,
  DPI/IME, glyph alignment and command-group interaction checks remain
  outstanding.
- Group headings are currently expanded and alphabetically ordered; collapse
  and custom ordering remain future work.

## [0.1.40] - 2026-10-27

Adds target-aware variables to reusable quick commands.

### Added

- Insert `{{host}}`, `{{port}}` and `{{user}}` placeholders from the command
  editor; each active or broadcast session expands them using its own connection
  metadata.
- Quote expanded values for POSIX shells and preserve unknown placeholders.
- Keep the versioned command bundle format unchanged; templates remain portable
  across teams and do not persist credentials.

### Verification

- Verified 2026-10-27 on Windows/MSVC: `cargo fmt --all` and
  `scripts\verify-hard-constraints.ps1` passed (187 tests passed, 9 Docker SSH
  tests ignored by the standard suite). `scripts\test-docker-ssh.ps1` passed
  all 9 integration tests.
- The GUI was not launched on the user's active desktop. Isolated screenshots,
  DPI/IME, glyph alignment and command-editor interaction checks remain
  outstanding.

## [0.1.39] - 2026-10-27

Adds a per-session terminal input lock.

### Added

- Lock or unlock terminal input from a session tab's actions menu; locked tabs
  show a lock icon with a tooltip.
- Block keyboard, terminal mouse, clipboard paste and secret-paste requests for
  locked sessions while leaving terminal output and non-input controls active.
- Prevent synchronized input from being sent elsewhere when the active session
  is locked, and keep unsent Compose drafts in the editor.

### Verification

- Verified 2026-10-27 on Windows/MSVC: `cargo fmt --all` and
  `scripts\verify-hard-constraints.ps1` passed (184 tests passed, 9 Docker SSH
  tests ignored by the standard suite). `scripts\test-docker-ssh.ps1` passed
  all 9 integration tests.
- The GUI was not launched on the user's active desktop. Isolated screenshots,
  DPI/IME, glyph alignment and tab interaction checks remain outstanding.

## [0.1.38] - 2026-10-27

Adds per-session tab titles and colors.

### Added

- Let saved host profiles set an optional tab title and choose a theme-aware
  accent color. Existing and duplicate sessions retain their identity; new
  duplicates receive unique titles based on the configured label.
- Validate tab titles to 80 Unicode characters and reject control characters.
- Migrate host profile documents to version 7 and profile exports to version 3;
  older documents default to the session name and theme accent color.
- Add English and Simplified Chinese labels for title, color, and palette options.

### Verification

- Verified 2026-10-27 on Windows/MSVC: `cargo fmt --all` and
  `scripts\verify-hard-constraints.ps1` passed (182 tests passed, 9 Docker SSH
  tests ignored by the standard suite). The Docker SSH script was not run because
  no SSH transport, authentication, SFTP, forwarding, logging, or PTY behavior
  changed.
- Isolated desktop screenshots, DPI/IME, and glyph alignment remain outstanding;
  the GUI was not launched on the user's active desktop.

## [0.1.37] - 2026-10-27

Adds one-click startup for saved host groups.

### Added

- Add a terminal icon to each populated group header to start saved profiles in
  that group and its nested groups; include an action for `UNGROUPED`.
- Skip active and duplicate host/port/user targets. Collect missing credentials
  one profile at a time, then attempt connections sequentially. Authentication
  failures pause the queue until retry or skip; users can stop remaining items.
- Keep staged credentials in zeroizing memory and cache them only after
  successful authentication.
- Add English and Simplified Chinese labels, candidate-filter tests, and queue
  state coverage for skipping a profile during credential collection.

### Verification

- Verified 2026-10-27 on Windows/MSVC: `cargo fmt --all` and
  `scripts\verify-hard-constraints.ps1` passed (178 tests passed, 9 SSH tests
  ignored in the standard run); `scripts\test-docker-ssh.ps1` passed (9/9).
- Isolated desktop screenshots, DPI/IME, glyph alignment, and manual group
  launch interaction checks remain outstanding.

## [0.1.36] - 2026-10-26

Adds prompt-based responses to terminal triggers.

### Added

- Save trigger engine state and non-secret rules in versioned JSON; rules stay
  disabled by default.
- Add an ask-and-send action that opens a masked response prompt when terminal
  output matches. The response is sent to that SSH session followed by Enter,
  held in zeroizing buffers, and omitted from saved trigger settings.
- Queue trigger prompts alongside keyboard-interactive authentication prompts
  so one prompt cannot replace another; cancelling a trigger prompt keeps the
  SSH session connected.
- Keep preset-text trigger rules in memory for the current run and omit their
  text from disk.
- Add English and Simplified Chinese labels, bounds and storage tests.

### Verification

- Verification date: 2026-10-26 (Windows/MSVC).
- `cargo fmt --all`, `cargo check --locked --all-targets` and
  `cargo test --locked` passed; the tests reported 168 passed and 9
  Docker-only tests ignored.
- `scripts\verify-hard-constraints.ps1` passed (168 passed, 9 ignored), and
  `scripts\test-docker-ssh.ps1` passed all 9 SSH/SFTP integration tests.
- Isolated Windows GUI, masked-input, DPI, IME, glyph and screenshot checks
  remain outstanding; the GUI was not launched on the user's active desktop.

## [0.1.35] - 2026-10-26

Adds drag-and-drop uploads to the SFTP panel.

### Added

- Accept files and folders dragged from Windows Explorer onto the active SFTP
  panel and queue them using the existing per-session FIFO transfer queue.
- Reject unavailable paths, symbolic links and special files before queuing;
  file and folder picker uploads use the same validation.
- Add English and Simplified Chinese drop-zone guidance and validation messages.

### Verification

- Verification date: 2026-10-26 (Windows/MSVC).
- `cargo fmt --all` and `scripts\verify-hard-constraints.ps1` passed (166
  passed, 9 Docker-only tests ignored).
- `scripts\test-docker-ssh.ps1` passed all 9 SSH/SFTP integration tests.
- Isolated Windows drag-and-drop, DPI, IME, glyph and screenshot checks remain
  outstanding; the GUI was not launched on the user's active desktop.

## [0.1.34] - 2026-10-26

Adds per-host automatic reconnect for interrupted SSH sessions.

### Added

- Add disabled-by-default reconnect settings with a 1–300 second interval and
  up to 20 attempts, including migration for saved host profiles.
- Retry transport failures after an established connection, while rechecking
  the user's existing OpenSSH `known_hosts` file on every attempt. Authentication
  and host-key failures stop without retrying.
- Reject and discard terminal, SFTP and forwarding requests during reconnect;
  start a new remote shell, restore saved forwarding rules and refresh the
  remembered SFTP directory after reconnection.
- Add Simplified Chinese labels and reconnect status messages.

### Verification

- Verification date: 2026-10-26 (Windows/MSVC).
- `cargo fmt --all` and `scripts\verify-hard-constraints.ps1` passed (165
  passed, 9 Docker-only tests ignored).
- `cargo test --locked --offline` passed (165 passed, 9 ignored).
- `scripts\test-docker-ssh.ps1` passed all 9 SSH/SFTP integration tests.
- Isolated Windows GUI and manual disconnect/reconnect checks remain outstanding;
  the GUI was not launched on the user's active desktop.

## [0.1.33] - 2026-10-26

Remembers SFTP directories for recent connections.

### Added

- Restore the most recently browsed remote directory for the same host, port
  and user after reconnecting.
- Remember a local folder per recent connection, reuse it for later downloads,
  and add an icon action in the SFTP panel to change it.
- Upgrade recent-connection data to version 5 while loading older files with
  empty directory defaults; keep local paths out of shareable host-profile
  exports.
- Add Simplified Chinese labels and migration, persistence and path-validation
  tests.

### Verification

- Verification date: 2026-10-26 (Windows/MSVC).
- `cargo fmt --all` and `cargo test --locked --offline` passed (158 passed,
  9 Docker-only tests ignored).
- `scripts\verify-hard-constraints.ps1` passed (158 passed, 9 Docker-only tests
  ignored).
- `scripts\test-docker-ssh.ps1` passed all 9 SSH/SFTP integration tests.
- Isolated Windows GUI screenshots and manual DPI, IME and glyph checks remain
  outstanding; the GUI was not launched on the user's active desktop.

## [0.1.32] - 2026-10-26

Adds portable Quick Commands bundles for team sharing.

### Added

- Add native file-dialog actions to import and export versioned JSON command
  bundles from the Quick Commands panel.
- Merge imported commands by case-insensitive name, report skipped duplicates,
  and reject additions that would exceed the 200-command limit.
- Bound bundle files to 1 MiB and validate version, UTF-8, command fields and
  duplicate names before changing local snippets.
- Add Simplified Chinese labels and bundle round-trip, validation, merge and
  limit tests. Importing a bundle stores commands locally without executing
  them.

### Verification

- Verification date: 2026-10-26 (Windows/MSVC).
- `cargo fmt --all`, hard-constraint verification and locked build passed.
- `cargo test --locked`: 155 passed, 9 Docker-only tests ignored.
- Docker SSH integration was not run because this change does not modify SSH
  or other Docker-covered paths.
- Isolated Windows GUI screenshots and manual DPI, IME and glyph checks remain
  outstanding.

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

