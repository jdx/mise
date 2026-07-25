
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
        $python3Path = (mise x python@3.12.0 -- where python3 | Select-Object -First 1).Trim()
        $python3Path | Should -Be (Join-Path $installPath 'python3.exe')
        mise x python@3.12.0 -- python3 --version | Should -Be "Python 3.12.0"
    }

    # https://github.com/jdx/mise/discussions/3821 - python-build-standalone
    # ships no pip.exe on Windows, so mise synthesizes pip.cmd/pip3.cmd
    It 'synthesizes pip wrappers so pip works out of the box' {
        # no-op when the previous test already installed it; keeps this test
        # runnable on its own
        mise install python@3.12.0 | Out-Null
        $installPath = (mise where python@3.12.0).Trim()
        (Join-Path $installPath 'pip.cmd') | Should -Exist
        (Join-Path $installPath 'pip3.cmd') | Should -Exist

        mise x python@3.12.0 -- pip --version
        $LASTEXITCODE | Should -Be 0
        mise x python@3.12.0 -- pip3 --version
        $LASTEXITCODE | Should -Be 0

        # Check the actual output through an explicit file handle. Piping a
        # batch-file child into the PowerShell pipeline captures nothing on the
        # CI runners (a real .exe like python.exe captures fine), so redirect
        # to a file instead of asserting on pipeline output.
        $out = Join-Path $TestDrive 'pip-version.txt'
        $p = Start-Process -FilePath 'mise' -NoNewWindow -Wait -PassThru `
            -ArgumentList 'x', 'python@3.12.0', '--', 'pip', '--version' `
            -RedirectStandardOutput $out
        $p.ExitCode | Should -Be 0
        (Get-Content $out -Raw) | Should -Match '^pip \d'

        # normalize separators: `mise which` echoes the configured data dir
        # verbatim (it can contain forward slashes), while `mise where` returns
        # an absolutized path
        $whichPip = (mise which pip --tool python@3.12.0).Trim().Replace('/', '\')
        $whichPip | Should -Be (Join-Path $installPath 'pip.cmd').Replace('/', '\')
    }

    It 'puts Scripts on PATH so pip-installed console scripts run' {
        mise install python@3.12.0 | Out-Null
        mise x python@3.12.0 -- pip install pycowsay | Out-Null
        $LASTEXITCODE | Should -Be 0

        $installPath = (mise where python@3.12.0).Trim()
        (Join-Path $installPath 'Scripts\pycowsay.exe') | Should -Exist
        mise x python@3.12.0 -- pycowsay moo
        $LASTEXITCODE | Should -Be 0
    }

    # https://github.com/jdx/mise/discussions/8872 - default packages used to
    # spawn <install>\bin\python (a unix-only path) and silently warn
    It 'installs default packages on Windows' {
        $pkgFile = Join-Path $TestDrive 'default-python-packages'
        Set-Content -Path $pkgFile -Value 'pycowsay'
        $env:MISE_PYTHON_DEFAULT_PACKAGES_FILE = $pkgFile
        try {
            # --force wipes the install dir, so pycowsay from the previous
            # test only comes back if default packages actually installed it
            mise install python@3.12.0 --force | Out-Null
            $installPath = (mise where python@3.12.0).Trim()
            (Join-Path $installPath 'Scripts\pycowsay.exe') | Should -Exist
            mise x python@3.12.0 -- pycowsay moo
            $LASTEXITCODE | Should -Be 0
        }
        finally {
            Remove-Item Env:\MISE_PYTHON_DEFAULT_PACKAGES_FILE -ErrorAction SilentlyContinue
        }
    }
}
