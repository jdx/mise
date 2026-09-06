Describe 'backend_github' {
    BeforeAll {
        @"
[tools]
"github:docker/compose" = "2.29.1"
"@ | Set-Content -Path "mise.toml"
    }

    AfterAll {
        Remove-Item "mise.toml" -ErrorAction SilentlyContinue
    }

    It 'installs and executes docker-compose via github backend' {
        mise install -f github:docker/compose
        mise exec github:docker/compose -- docker-compose version | Should -BeLike "Docker Compose version *"
    }
}
