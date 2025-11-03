# Test GitHub backend with docker-compose on Windows

$ErrorActionPreference = "Stop"

# Test: Install docker-compose via GitHub backend
@"
[tools]
"github:docker/compose" = "2.29.1"
"@ | Set-Content -Path "mise.toml"

mise install -f github:docker/compose
if ($LASTEXITCODE -ne 0) { throw "Failed to install github:docker/compose" }

# Verify it's executable
mise exec github:docker/compose -- docker-compose version
if ($LASTEXITCODE -ne 0) { throw "Failed to execute docker-compose" }

Write-Host "GitHub backend docker-compose test passed!" -ForegroundColor Green