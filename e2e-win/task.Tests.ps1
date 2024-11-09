
Describe 'task' {
    It 'executes a task' {
        mise run filetask.bat | Should -Be 'mytask'
    }
}
