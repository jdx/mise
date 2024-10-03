
Describe 'java' {
    It 'executes java' {
        mise x java@21 -- java --version | Select -First 1 | Should -BeLike 'openjdk 21.*'
    }
}
