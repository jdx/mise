
Describe 'task' {

    BeforeEach {
        Remove-Item -Path Env:\MISE_WINDOWS_EXECUTABLE_EXTENSIONS -ErrorAction SilentlyContinue
        Remove-Item -Path Env:\MISE_WINDOWS_DEFAULT_FILE_SHELL_ARGS -ErrorAction SilentlyContinue
        Remove-Item -Path Env:\MISE_USE_FILE_SHELL_FOR_EXECUTABLE_TASKS -ErrorAction SilentlyContinue
    }

    It 'executes a task' {
        mise run filetask.bat | Select -Last 1 | Should -Be 'mytask'
    }

    It 'executes a task without extension' {
        $env:MISE_WINDOWS_DEFAULT_FILE_SHELL_ARGS = "bat"
        mise run filetask | Select -Last 1 | Should -Be 'mytask'
    }

    It 'executes a task in pwsh' {
        $env:MISE_WINDOWS_EXECUTABLE_EXTENSIONS = "ps1"
        $env:MISE_WINDOWS_DEFAULT_FILE_SHELL_ARGS = "pwsh.exe"
        $env:MISE_USE_FILE_SHELL_FOR_EXECUTABLE_TASKS = "true"
        mise run testtask | Select -Last 1 | Should -Be 'windows'
    }
}
