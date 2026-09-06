Describe 'pipx' {
    # Regression test for https://github.com/jdx/mise/discussions/5333.
    #
    # pipx is commonly on PATH only as pipx.cmd -- that is what scoop's shims and
    # `pip install pipx` leave behind. mise's dependency check finds it, because
    # executable_names expands windows_executable_extensions, but Command::new does
    # not: std only ever appends .exe to a bare name. So the install used to clear
    # the check and then die with "program not found".

    BeforeAll {
        $script:originalPath = Get-Location
        $script:originalEnvPath = $env:PATH
        # Pester runs every file in one runspace, so restore rather than remove: a value
        # set outside this file has to survive it. Same idiom as github_token.Tests.ps1.
        $script:originalPipxUvx = [Environment]::GetEnvironmentVariable('MISE_PIPX_UVX', 'Process')
        $script:miseDir = Split-Path (
            Get-Command -Type Application mise -All | Select-Object -First 1
        ).Source

        Set-Location TestDrive:

        $script:toolDir = Join-Path $TestDrive "pipxbin"
        New-Item -ItemType Directory -Path $script:toolDir -Force | Out-Null
        $script:markerFile = Join-Path $script:toolDir "pipx-ran.txt"

        # The stub records that it ran into a file beside itself rather than relying on
        # its stdout: piping a batch-file child through the PowerShell pipeline captures
        # nothing on these runners (see the note in python.Tests.ps1). A marker file is
        # a filesystem fact and is immune to that. No -NoNewline here, unlike
        # shim_recursion.Tests.ps1, because the last line must be terminated.
        @'
@echo off
> "%~dp0pipx-ran.txt" echo FAKE_PIPX_RAN %*
echo FAKE_PIPX_RAN
exit /b 42
'@ | Out-File -FilePath (Join-Path $script:toolDir "pipx.cmd") -Encoding ascii

        # uv would win the branch in install_version_ and the pipx spawn would never
        # be exercised at all.
        $env:MISE_PIPX_UVX = '0'

        # Trim PATH to the stub, mise and the system dirs so nothing else can answer.
        # Resolution is directory-major, so the stub dir being first is already decisive
        # here -- but the later Its deliberately drop the stub dir, and trimming keeps a
        # runner-installed pipx from answering there.
        $env:PATH = "$($script:toolDir);$($script:miseDir);$env:SystemRoot\System32;$env:SystemRoot"
    }

    AfterAll {
        $env:PATH = $script:originalEnvPath
        # Branch on purpose: passing $null to SetEnvironmentVariable does not restore the
        # unset state, because PowerShell binds it to the string parameter as '' instead.
        if ($null -eq $script:originalPipxUvx) {
            Remove-Item -Path Env:\MISE_PIPX_UVX -ErrorAction SilentlyContinue
        }
        else {
            $env:MISE_PIPX_UVX = $script:originalPipxUvx
        }
        Set-Location $script:originalPath
        Remove-Item -Path $script:toolDir -Recurse -ErrorAction SilentlyContinue
    }

    It 'spawns a pipx that exists only as a .cmd shim' {
        Remove-Item -Path $script:markerFile -ErrorAction SilentlyContinue

        # black 24.3.0 is exact semver, so resolve_exact_version answers locally and no
        # PyPI request is made. --force so a cached install cannot skip the spawn.
        $result = mise install --force pipx:black@24.3.0 2>&1
        # The stub always exits 42, so the install is expected to fail.
        $LASTEXITCODE | Should -Not -Be 0

        # The regression: before the fix this file never appears, because the spawn
        # failed before cmd.exe ever ran the stub.
        $script:markerFile | Should -Exist
        $marker = Get-Content -Path $script:markerFile -Raw
        $marker | Should -Match 'FAKE_PIPX_RAN install'
        # ...and the arguments survived std's cmd.exe routing.
        $marker | Should -Match 'black==24\.3\.0'

        $output = ($result | Out-String)
        $output | Should -Not -Match 'program not found'
    }

    It 'reports a missing pipx when the only candidate is a .ps1' {
        # The general fix, stated as a test. A .ps1 needs `pwsh -File`, so CreateProcess
        # cannot launch it -- but `.ps1` is in the default windows_executable_extensions
        # list, so mise's plain dependency lookup accepted it as evidence that pipx
        # existed. The install then reached the spawn and died with "program not found".
        # The gate and the spawn now ask the same question, and the resolver walks past a
        # candidate it cannot launch instead of stopping there, so a .ps1-only pipx is
        # simply not found -- and "not found" carries the actionable instructions.
        $ps1Dir = Join-Path $TestDrive "pipxps1"
        New-Item -ItemType Directory -Path $ps1Dir -Force | Out-Null
        'exit 42' | Out-File -FilePath (Join-Path $ps1Dir "pipx.ps1") -Encoding ascii
        $previousPath = $env:PATH
        try {
            $env:PATH = "$ps1Dir;$($script:miseDir);$env:SystemRoot\System32;$env:SystemRoot"
            $result = mise install --force pipx:black@24.3.0 2>&1
            $LASTEXITCODE | Should -Not -Be 0
            $output = ($result | Out-String)
            $output | Should -Match 'mise use pipx@latest'
            $output | Should -Not -Match 'program not found'
        }
        finally {
            $env:PATH = $previousPath
            Remove-Item -Path $ps1Dir -Recurse -ErrorAction SilentlyContinue
        }
    }

    It 'does not commit to the uv branch when uv is only a .ps1' {
        # The same question on the other side of the branch. `uv` decides whether the
        # install goes through `uv tool install` or through pipx, and the resolved path is
        # also the program handed to CmdLineRunner. A uv.ps1 satisfies the plain lookup, so
        # mise would pick the uv branch and then fail at process creation; treating it as
        # absent falls through to pipx, and with no pipx here that surfaces as the
        # instructions rather than a spawn error. uvx is left enabled on purpose -- this is
        # the branch the other Its suppress with MISE_PIPX_UVX=0.
        $uvDir = Join-Path $TestDrive "uvps1"
        New-Item -ItemType Directory -Path $uvDir -Force | Out-Null
        'exit 42' | Out-File -FilePath (Join-Path $uvDir "uv.ps1") -Encoding ascii
        $previousPath = $env:PATH
        $previousUvx = $env:MISE_PIPX_UVX
        try {
            Remove-Item -Path Env:\MISE_PIPX_UVX -ErrorAction SilentlyContinue
            $env:PATH = "$uvDir;$($script:miseDir);$env:SystemRoot\System32;$env:SystemRoot"
            $result = mise install --force pipx:black@24.3.0 2>&1
            $LASTEXITCODE | Should -Not -Be 0
            $output = ($result | Out-String)
            $output | Should -Match 'mise use pipx@latest'
            $output | Should -Not -Match 'program not found'
        }
        finally {
            $env:PATH = $previousPath
            # Same branch as AfterAll: assigning $null would leave an empty value behind
            # rather than restoring the unset state.
            if ($null -eq $previousUvx) {
                Remove-Item -Path Env:\MISE_PIPX_UVX -ErrorAction SilentlyContinue
            }
            else {
                $env:MISE_PIPX_UVX = $previousUvx
            }
            Remove-Item -Path $uvDir -Recurse -ErrorAction SilentlyContinue
        }
    }

    It 'still reports a missing pipx with install instructions' {
        # Guards the fallback arm of Backend::spawn_program, where nothing resolves at all,
        # together with the spawnable_dependency gate. The Windows counterpart of
        # e2e/backend/test_pipx_missing_dependency.
        $previousPath = $env:PATH
        try {
            $env:PATH = "$($script:miseDir);$env:SystemRoot\System32;$env:SystemRoot"
            $result = mise install --force pipx:black@24.3.0 2>&1
            $LASTEXITCODE | Should -Not -Be 0
            $output = ($result | Out-String)
            $output | Should -Match 'mise use pipx@latest'
            $output | Should -Not -Match 'program not found'
        }
        finally {
            $env:PATH = $previousPath
        }
    }
}
