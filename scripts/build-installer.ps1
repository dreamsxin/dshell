$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$compilerCommand = Get-Command ISCC.exe -ErrorAction SilentlyContinue
if ($null -eq $compilerCommand) {
    $compilerCandidates = @(
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    )
    $compilerPath = $compilerCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if ($null -eq $compilerPath) {
        throw "Inno Setup 6 compiler ISCC.exe was not found. Install JRSoftware.InnoSetup first."
    }
} else {
    $compilerPath = $compilerCommand.Source
}

Push-Location $repoRoot
try {
    cargo build --release --locked --bin dshell --bin dshell-cli
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo release build failed."
    }

    & $compilerPath /Qp "installer\dshell.iss"
    if ($LASTEXITCODE -ne 0) {
        throw "Inno Setup compilation failed."
    }

    $installerPath = Join-Path $repoRoot "dist\installer\dshell-setup-v0.1.55-x64.exe"
    if (-not (Test-Path -LiteralPath $installerPath)) {
        throw "The expected installer output was not created: $installerPath"
    }

    $hash = Get-FileHash -LiteralPath $installerPath -Algorithm SHA256
    $checksumPath = "$installerPath.sha256"
    $checksumLine = "{0}  {1}{2}" -f $hash.Hash, (Split-Path -Leaf $installerPath), [Environment]::NewLine
    [System.IO.File]::WriteAllText($checksumPath, $checksumLine, [System.Text.Encoding]::ASCII)

    Write-Output "Installer: $installerPath"
    Write-Output "SHA-256: $($hash.Hash)"
} finally {
    Pop-Location
}
