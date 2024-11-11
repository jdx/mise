
Describe 'node' {
    It 'executes python 3.12.0' {
        mise x python@3.12.0 -- where python
        mise x python@3.12.0 -- python --version | Should -Be "Python 3.12.0"
    }
    It 'executes vfox:python 3.13.0' {
        mise x vfox:python@3.13.0 -- where python
        mise x vfox:python@3.13.0 -- python --version | Should -Be "Python 3.13.0"
    }
}
