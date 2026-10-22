$ErrorActionPreference = "Stop"

$image = "dshell-ssh-test:ubuntu-24.04"
$dockerContext = Join-Path $PSScriptRoot "docker-ssh"
$dockerfile = Join-Path $dockerContext "Dockerfile"
$containerName = "dshell-ssh-test-$([Guid]::NewGuid().ToString('N'))"
$knownHostsPath = Join-Path $env:TEMP "$containerName-known_hosts"
$passwordBytes = New-Object byte[] 24
$random = [Security.Cryptography.RandomNumberGenerator]::Create()
$random.GetBytes($passwordBytes)
$password = [BitConverter]::ToString($passwordBytes).Replace("-", "")
$random.Dispose()
[Array]::Clear($passwordBytes, 0, $passwordBytes.Length)

try {
    docker build --tag $image --file $dockerfile $dockerContext
    if ($LASTEXITCODE -ne 0) {
        throw "Could not build the cached Ubuntu SSH test image."
    }

    docker run --detach --name $containerName --publish 127.0.0.1::22 --env "DSHELL_TEST_PASSWORD=$password" $image | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "Could not start the Ubuntu SSH container."
    }

    $portMapping = docker port $containerName 22/tcp
    if ($LASTEXITCODE -ne 0 -or -not $portMapping) {
        throw "Could not determine the Ubuntu SSH container port."
    }
    $port = [int]($portMapping.Trim().Split(":")[-1])

    $hostKey = $null
    for ($attempt = 0; $attempt -lt 480; $attempt++) {
        $containerState = docker inspect --format '{{.State.Running}}' $containerName 2>$null
        if ($LASTEXITCODE -ne 0 -or $containerState -ne "true") {
            $containerLogs = docker logs --tail 60 $containerName 2>&1 | Out-String
            throw "Ubuntu SSH container exited before providing its host key. Container output: $containerLogs"
        }

        $hostKey = docker exec $containerName cat /etc/ssh/ssh_host_ed25519_key.pub 2>$null
        if ($LASTEXITCODE -eq 0 -and $hostKey) {
            break
        }
        Start-Sleep -Seconds 1
    }
    if (-not $hostKey) {
        $containerLogs = docker logs --tail 60 $containerName 2>&1 | Out-String
        throw "Could not read the temporary container's SSH host key. Container output: $containerLogs"
    }
    $hostKeyFields = $hostKey.Trim() -split "\s+"
    if ($hostKeyFields.Count -lt 2 -or $hostKeyFields[0] -ne "ssh-ed25519") {
        throw "The temporary container did not provide an Ed25519 SSH host key."
    }

    $ready = $false
    for ($attempt = 0; $attempt -lt 240; $attempt++) {
        $client = [System.Net.Sockets.TcpClient]::new()
        try {
            $connect = $client.ConnectAsync("127.0.0.1", $port)
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
        $containerLogs = docker logs --tail 60 $containerName 2>&1 | Out-String
        throw "Ubuntu SSH did not become ready. Container output: $containerLogs"
    }

    [System.IO.File]::WriteAllText(
        $knownHostsPath,
        "[127.0.0.1]:$port $($hostKey.Trim())`n",
        [System.Text.Encoding]::ASCII
    )

    $env:DSHELL_DOCKER_SSH_HOST = "127.0.0.1"
    $env:DSHELL_DOCKER_SSH_PORT = "$port"
    $env:DSHELL_DOCKER_SSH_USER = "dshelltest"
    $env:DSHELL_DOCKER_SSH_PASSWORD = $password
    $env:DSHELL_DOCKER_SSH_KNOWN_HOSTS = $knownHostsPath

    cargo test --locked docker_ssh_ -- --ignored --nocapture
    if ($LASTEXITCODE -ne 0) {
        throw "The application SSH terminal integration test failed."
    }
}
finally {
    Remove-Item Env:\DSHELL_DOCKER_SSH_HOST -ErrorAction SilentlyContinue
    Remove-Item Env:\DSHELL_DOCKER_SSH_PORT -ErrorAction SilentlyContinue
    Remove-Item Env:\DSHELL_DOCKER_SSH_USER -ErrorAction SilentlyContinue
    Remove-Item Env:\DSHELL_DOCKER_SSH_PASSWORD -ErrorAction SilentlyContinue
    Remove-Item Env:\DSHELL_DOCKER_SSH_KNOWN_HOSTS -ErrorAction SilentlyContinue
    docker rm --force $containerName *> $null
    Remove-Item -LiteralPath $knownHostsPath -Force -ErrorAction SilentlyContinue
}
