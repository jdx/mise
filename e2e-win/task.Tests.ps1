
Describe 'task' {
    It 'executes a task' {
        mise run filetask.bat | Select -Last 1 | Should -Be 'mytask'
    }
}
