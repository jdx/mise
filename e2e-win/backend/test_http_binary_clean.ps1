# Test HTTP backend binary name cleaning with docker-compose on Windows

$ErrorActionPreference = "Stop"
$env:MISE_EXPERIMENTAL = "1"

# Test: Install docker-compose via HTTP backend
# The binary name should be automatically cleaned from docker-compose-windows-x86_64.exe to docker-compose
@"
[tools]
"http:docker-compose" = { version = "2.29.1", url = "https://github.com/docker/compose/releases/download/v{version}/docker-compose-windows-x86_64.exe" }
"@ | Set-Content -Path "mise.toml"

mise install -f http:docker-compose
if ($LASTEXITCODE -ne 0) { throw "Failed to install http:docker-compose" }

# Verify it's executable
mise exec http:docker-compose -- docker-compose version
if ($LASTEXITCODE -ne 0) { throw "Failed to execute docker-compose" }

Write-Host "HTTP backend binary cleaning test passed!" -ForegroundColor Green