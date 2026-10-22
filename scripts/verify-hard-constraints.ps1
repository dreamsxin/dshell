$ErrorActionPreference = "Stop"

$manifest = Get-Content -LiteralPath "Cargo.toml" -Raw
$forbiddenManifestNames = @("tauri", "wry", "webview", "web-view", "electron")
foreach ($name in $forbiddenManifestNames) {
    if ($manifest -match "(?im)^\s*$([regex]::Escape($name))\s*=") {
        throw "Forbidden desktop web runtime dependency in Cargo.toml: $name"
    }
}

$sshSource = Get-Content -LiteralPath "src\ssh.rs" -Raw
if ($sshSource -notmatch "check_server_key") {
    throw "SSH must implement an explicit server-key verification callback."
}
if ($sshSource -notmatch "check_known_hosts_path") {
    throw "SSH must check the server key against OpenSSH known_hosts."
}

$dependencyTree = cargo tree --locked --format "{p}" | Out-String
if ($LASTEXITCODE -ne 0) {
    throw "Could not inspect the locked Cargo dependency tree."
}
$forbiddenTreePattern = "(?im)^\s*(tauri|wry|webview|web-view|electron)(?:\s|@|$)"
if ($dependencyTree -match $forbiddenTreePattern) {
    throw "A WebView or Electron package is present in the resolved dependency tree."
}

cargo fmt --all -- --check
if ($LASTEXITCODE -ne 0) {
    throw "Rust formatting check failed."
}

cargo check --locked --all-targets
if ($LASTEXITCODE -ne 0) {
    throw "Rust type check failed."
}

cargo test --locked
if ($LASTEXITCODE -ne 0) {
    throw "Rust test suite failed."
}
