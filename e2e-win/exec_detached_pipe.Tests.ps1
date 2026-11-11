Describe 'exec_detached_pipe' {
    # Regression test for #15582: a process a tool starts detached with
    # `stdio: 'ignore'` must not keep the caller's stdout pipe open. PowerShell
    # reads `$out = ...` until every holder of the pipe's write end has closed
    # it, so a leaked handle made the capture wait for the detached process.

    BeforeAll {
        $script:miseExe = (Get-Command mise).Source
        $script:workdir = Join-Path -Path $env:TEMP -ChildPath "mise-15582-$PID"
        New-Item -ItemType Directory -Force -Path $script:workdir | Out-Null
        Push-Location $script:workdir
        # A local config, so the shim case does not change the global node.
        Set-Content -Path 'mise.toml' -Value "[tools]`nnode = '22'"
        mise trust | Out-Null
        mise install | Out-Null
        # The detached child lives long enough to show a leak and prints a marker so a failed
        # launch cannot pass as a fast return.
        $script:js = "require('child_process').spawn(process.execPath, ['-e', 'setTimeout(() => {}, 20000)'], { stdio: 'ignore', detached: true }).unref(); console.log('started')"
        $script:savedShimMode = $env:MISE_WINDOWS_SHIM_MODE
    }

    AfterAll {
        $env:MISE_WINDOWS_SHIM_MODE = $script:savedShimMode
        # The detached children run in the workdir and would lock it.
        Get-CimInstance Win32_Process -Filter "Name = 'node.exe'" |
            Where-Object { $_.CommandLine -like '*setTimeout(() => {}, 20000)*' } |
            ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction Ignore }
        Pop-Location
        Remove-Item -Recurse -Force $script:workdir -ErrorAction Ignore
    }

    It 'returns without waiting for a detached process under mise exec' {
        $out = $null
        $seconds = (Measure-Command { $out = & $script:miseExe exec -- node -e $script:js }).TotalSeconds
        $LASTEXITCODE | Should -Be 0
        "$out".Trim() | Should -Be 'started'
        $seconds | Should -BeLessThan 10
    }

    It 'returns without waiting for a detached process under a shim' {
        $env:MISE_WINDOWS_SHIM_MODE = 'exe'
        mise reshim
        $shim = Join-Path -Path $env:MISE_DATA_DIR -ChildPath 'shims\node.exe'
        $shim | Should -Exist
        $out = $null
        $seconds = (Measure-Command { $out = & $shim -e $script:js }).TotalSeconds
        $LASTEXITCODE | Should -Be 0
        "$out".Trim() | Should -Be 'started'
        $seconds | Should -BeLessThan 10
    }
}
