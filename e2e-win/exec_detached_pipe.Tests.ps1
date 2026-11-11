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
        mise install node@22 | Out-Null
        $script:detach = "require('child_process').spawn(process.execPath, ['-e', 'setTimeout(() => {}, 20000)'], { stdio: 'ignore', detached: true }).unref()"
    }

    AfterAll {
        Pop-Location
        Remove-Item -Recurse -Force $script:workdir -ErrorAction Ignore
    }

    It 'returns without waiting for a detached process under mise exec' {
        $seconds = (Measure-Command { $out = & $script:miseExe exec node@22 -- node -e $script:detach }).TotalSeconds
        $seconds | Should -BeLessThan 10
    }

    It 'returns without waiting for a detached process under a shim' {
        mise use -g node@22 | Out-Null
        mise reshim
        $shim = Join-Path -Path $env:MISE_DATA_DIR -ChildPath 'shims\node.exe'
        $seconds = (Measure-Command { $out = & $shim -e $script:detach }).TotalSeconds
        $seconds | Should -BeLessThan 10
    }
}
