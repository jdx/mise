Describe 'backend_http' {
    BeforeAll {
        $env:MISE_EXPERIMENTAL = "1"
        @"
[tools]
"http:docker-compose" = { version = "2.29.1", url = "https://github.com/docker/compose/releases/download/v{version}/docker-compose-windows-x86_64.exe" }
"@ | Set-Content -Path "mise.toml"
    }

    AfterAll {
        $env:MISE_EXPERIMENTAL = ""
        Remove-Item "mise.toml" -ErrorAction SilentlyContinue
    }

    It 'installs and executes docker-compose via http backend with binary cleaning' {
        mise install -f http:docker-compose
        $LASTEXITCODE | Should -Be 0
        mise exec http:docker-compose -- docker-compose version | Should -BeLike "Docker Compose version *"
    }
}
