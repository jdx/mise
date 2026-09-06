Describe 'github token' {
    BeforeAll {
        $script:GitHubTokenEnvVars = @(
            "GITHUB_TOKEN",
            "GITHUB_API_TOKEN",
            "MISE_GITHUB_TOKEN",
            "MISE_GITHUB_ENTERPRISE_TOKEN",
            "MISE_GITHUB_CREDENTIAL_COMMAND",
            "MISE_WINDOWS_DEFAULT_INLINE_SHELL_ARGS"
        )
    }

    BeforeEach {
        $script:GitHubTokenEnv = @{}
        foreach ($name in $script:GitHubTokenEnvVars) {
            $script:GitHubTokenEnv[$name] = [Environment]::GetEnvironmentVariable($name, "Process")
            [Environment]::SetEnvironmentVariable($name, $null, "Process")
        }
    }

    AfterEach {
        foreach ($name in $script:GitHubTokenEnvVars) {
            [Environment]::SetEnvironmentVariable($name, $script:GitHubTokenEnv[$name], "Process")
        }
        $script:GitHubTokenEnv = $null
    }

    It 'runs credential_command with the Windows default inline shell' {
        $env:MISE_WINDOWS_DEFAULT_INLINE_SHELL_ARGS = "cmd /c"
        $env:MISE_GITHUB_CREDENTIAL_COMMAND = "echo %MISE_CREDENTIAL_PROVIDER%-%MISE_CREDENTIAL_HOST%-win-token"

        $output = mise token github --unmask

        $output | Should -Match "github-github.com-win-token"
        $output | Should -Match "credential_command"
        $output | Should -Not -Match "mise-credential-helper"
    }
}
