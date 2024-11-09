
Describe 'task' {
    It 'executes a task' {
        mise run filetask.bat | Should -BeLike '*mytask'
    }
}
