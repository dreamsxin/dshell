$ErrorActionPreference = "Stop"

$image = "dshell-ssh-test:ubuntu-24.04"
$dockerContext = Join-Path $PSScriptRoot "docker-ssh"
$dockerfile = Join-Path $dockerContext "Dockerfile"
$containerName = "dshell-ssh-manual"
$bindAddress = "127.0.0.1"
$port = 2222
$user = "dshelltest"

$existing = docker ps -a --filter "name=^/$containerName$" --format "{{.Names}}"
if ($LASTEXITCODE -ne 0) {
    throw "Could not query Docker containers."
}
if ($existing) {
    throw "Container $containerName already exists. Stop it with: docker rm --force $containerName"
}

$portInUse = Get-NetTCPConnection -LocalAddress $bindAddress -LocalPort $port -State Listen -ErrorAction SilentlyContinue
if ($portInUse) {
    throw "TCP port $port is already in use on $host. Stop that service or choose another port."
}

$passwordBytes = New-Object byte[] 24
$random = [Security.Cryptography.RandomNumberGenerator]::Create()
$random.GetBytes($passwordBytes)
$password = [BitConverter]::ToString($passwordBytes).Replace("-", "")
$random.Dispose()
[Array]::Clear($passwordBytes, 0, $passwordBytes.Length)
$hostKeyPath = Join-Path $env:TEMP "$containerName-host-key.pub"

try {
    docker build --tag $image --file $dockerfile $dockerContext
    if ($LASTEXITCODE -ne 0) {
        throw "Could not build the Ubuntu SSH image."
    }

    docker run --detach --name $containerName --publish "${bindAddress}:${port}:22" --env "DSHELL_TEST_PASSWORD=$password" $image | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "Could not start the SSH container."
    }

    $hostKey = $null
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        $running = docker inspect --format '{{.State.Running}}' $containerName 2>$null
        if ($LASTEXITCODE -ne 0 -or $running -ne "true") {
            $logs = docker logs --tail 40 $containerName 2>&1 | Out-String
            throw "The SSH container exited before becoming ready. $logs"
        }
        $hostKey = docker exec $containerName cat /etc/ssh/ssh_host_ed25519_key.pub 2>$null
        if ($LASTEXITCODE -eq 0 -and $hostKey) {
            break
        }
        Start-Sleep -Milliseconds 500
    }
    if (-not $hostKey) {
        throw "Could not read the temporary SSH host key."
    }

    $ready = $false
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        $client = [System.Net.Sockets.TcpClient]::new()
        try {
            $connect = $client.ConnectAsync($bindAddress, $port)
            if ($connect.Wait(500) -and $client.Connected) {
                $ready = $true
                break
            }
        }
        catch {
        }
        finally {
            $client.Dispose()
        }
        Start-Sleep -Milliseconds 500
    }
    if (-not $ready) {
        $logs = docker logs --tail 40 $containerName 2>&1 | Out-String
        throw "The SSH service did not become ready. $logs"
    }

    [System.IO.File]::WriteAllText($hostKeyPath, $hostKey.Trim() + "`n", [System.Text.Encoding]::ASCII)
    $fingerprint = (ssh-keygen -lf $hostKeyPath -E sha256 | Out-String).Trim()

    Write-Host "SSH service is ready for manual dshell testing."
    Write-Host "Host:     $bindAddress"
    Write-Host "Port:     $port"
    Write-Host "Username: $user"
    Write-Host "Password: $password"
    Write-Host ""
    Write-Host "Verify this fingerprint before adding the key to OpenSSH known_hosts:"
    Write-Host $fingerprint
    Write-Host "Known-hosts line (review it, then add it manually if the fingerprint matches):"
    Write-Host "[$bindAddress]:$port $($hostKey.Trim())"
    Write-Host ""
    Write-Host "Stop the service with: docker rm --force $containerName"
}
catch {
    docker rm --force $containerName *> $null
    throw
}
finally {
    Remove-Item -LiteralPath $hostKeyPath -Force -ErrorAction SilentlyContinue
    [Array]::Clear($passwordBytes, 0, $passwordBytes.Length)
}
