$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$installerPath = Join-Path $repoRoot "dist\installer\dshell-setup-v0.1.56-x64.exe"
if (-not (Test-Path -LiteralPath $installerPath)) {
    throw "Build the installer before running this smoke test: $installerPath"
}

$tempRoot = [System.IO.Path]::GetFullPath($env:TEMP)
$smokeRoot = Join-Path $tempRoot ("dshell-installer-smoke-" + [guid]::NewGuid().ToString("N"))
$installRoot = Join-Path $smokeRoot "install"
New-Item -ItemType Directory -Path $smokeRoot | Out-Null

try {
    $installArguments = @(
        "/VERYSILENT",
        "/SUPPRESSMSGBOXES",
        "/NORESTART",
        "/NOICONS",
        ('/DIR="{0}"' -f $installRoot)
    )
    $setup = Start-Process -FilePath $installerPath -ArgumentList $installArguments -Wait -PassThru
    if ($setup.ExitCode -ne 0) {
        throw "Silent installer exited with code $($setup.ExitCode)."
    }

    $installedExe = Join-Path $installRoot "dshell.exe"
    if (-not (Test-Path -LiteralPath $installedExe)) {
        throw "The installer did not place dshell.exe at the selected destination."
    }
    $installedCli = Join-Path $installRoot "dshell-cli.exe"
    if (-not (Test-Path -LiteralPath $installedCli)) {
        throw "The installer did not place dshell-cli.exe at the selected destination."
    }

    $uninstaller = Join-Path $installRoot "unins000.exe"
    if (-not (Test-Path -LiteralPath $uninstaller)) {
        throw "The installation did not create its uninstaller."
    }
    $remove = Start-Process -FilePath $uninstaller -ArgumentList @(
        "/VERYSILENT",
        "/SUPPRESSMSGBOXES",
        "/NORESTART"
    ) -Wait -PassThru
    if ($remove.ExitCode -ne 0) {
        throw "Silent uninstaller exited with code $($remove.ExitCode)."
    }
    if (Test-Path -LiteralPath $installedExe) {
        throw "The uninstaller left dshell.exe in the selected destination."
    }
    if (Test-Path -LiteralPath $installedCli) {
        throw "The uninstaller left dshell-cli.exe in the selected destination."
    }

    Write-Output "Silent per-user install and uninstall passed."
} finally {
    if (Test-Path -LiteralPath $smokeRoot) {
        $resolvedSmokeRoot = (Resolve-Path -LiteralPath $smokeRoot).Path
        $tempPrefix = $tempRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
        if (-not $resolvedSmokeRoot.StartsWith($tempPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to remove a smoke-test directory outside TEMP: $resolvedSmokeRoot"
        }
        Remove-Item -LiteralPath $resolvedSmokeRoot -Recurse -Force
    }
}
