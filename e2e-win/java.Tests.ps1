
Describe 'java' {
    It 'executes java@temurin-21' {
        mise x java@temurin-21 -- java --version | Select -Last 1 | Should -BeLike '*Temurin-21.*'
    }
}
