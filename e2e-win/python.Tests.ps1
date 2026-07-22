
Describe 'python' {
    BeforeAll {
        $env:MISE_PYTHON_GITHUB_ATTESTATIONS = "0"
    }

    It 'executes python and python3 for 3.12.0' {
        mise install python@3.12.0 --force | Out-Null

        $installPath = (mise where python@3.12.0).Trim()
        (Join-Path $installPath 'python3.exe') | Should -Exist

        mise x python@3.12.0 -- where python
        mise x python@3.12.0 -- python --version | Should -Be "Python 3.12.0"
        mise x python@3.12.0 -- where python3
        mise x python@3.12.0 -- python3 --version | Should -Be "Python 3.12.0"
    }
}
