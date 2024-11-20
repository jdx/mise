
Describe 'node' {
    It 'executes python 3.12.0' {
        mise x python@3.12.0 -- where python
        mise x python@3.12.0 -- python --version | Should -Be "Python 3.12.0"
    }
}
