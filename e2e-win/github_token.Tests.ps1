Describe 'github token' {
    BeforeEach {
        Remove-Item -Path Env:\GITHUB_TOKEN -ErrorAction SilentlyContinue
        Remove-Item -Path Env:\GITHUB_API_TOKEN -ErrorAction SilentlyContinue
        Remove-Item -Path Env:\MISE_GITHUB_TOKEN -ErrorAction SilentlyContinue
        Remove-Item -Path Env:\MISE_GITHUB_ENTERPRISE_TOKEN -ErrorAction SilentlyContinue
        Remove-Item -Path Env:\MISE_GITHUB_CREDENTIAL_COMMAND -ErrorAction SilentlyContinue
        Remove-Item -Path Env:\MISE_WINDOWS_DEFAULT_INLINE_SHELL_ARGS -ErrorAction SilentlyContinue
    }

    AfterEach {
        Remove-Item -Path Env:\MISE_GITHUB_CREDENTIAL_COMMAND -ErrorAction SilentlyContinue
        Remove-Item -Path Env:\MISE_WINDOWS_DEFAULT_INLINE_SHELL_ARGS -ErrorAction SilentlyContinue
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
