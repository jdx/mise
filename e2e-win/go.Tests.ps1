
Describe 'node' {
    It 'executes go 1.23.3' {
        mise x go@1.23.3 -- where go
        mise x go@1.23.3 -- go version | Should -BeLike "go version go1.23.3 windows/*"
    }
}
