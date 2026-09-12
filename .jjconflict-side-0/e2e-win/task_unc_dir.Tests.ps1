Describe 'a task whose working directory is a UNC path' {
    BeforeAll {
        # cmd.exe refuses a UNC working directory: it prints "UNC paths are not supported", starts
        # in C:\Windows and carries on, so the task used to run somewhere nobody asked for while
        # mise reported success. The guard fires on the shape of the path, before anything spawns,
        # so this share does not need to exist and no admin rights are involved.
        $script:UncDir = "\\example\share"
        $script:Proj = Join-Path $TestDrive "unc-task"
        New-Item -ItemType Directory -Path $script:Proj -Force | Out-Null
    }

    It 'refuses to run it under the default cmd shell' {
        $config = @"
[tasks.here]
run = "cd"
dir = '$($script:UncDir)'
"@
        Set-Content -LiteralPath (Join-Path $script:Proj "mise.toml") -Value $config

        Push-Location $script:Proj
        try {
            mise trust | Out-Null
            $output = mise run here 2>&1 | Out-String
            $code = $LASTEXITCODE
        } finally {
            Pop-Location
        }

        $code | Should -Not -Be 0
        $output | Should -Match 'UNC path as a working directory'
        # Bound first: in argument mode `Should -Match [regex]::Escape($x)` is parsed as the two
        # arguments `[regex]::Escape` and `$x`, so the pattern becomes the literal method name and
        # the path silently becomes the -Because text.
        $escapedDir = [regex]::Escape($script:UncDir)
        $output | Should -Match $escapedDir
        # The message is only useful if it says what to do instead.
        $output | Should -Match 'shell'
    }

    It 'refuses a cache command input before it can hash the wrong directory' {
        # These commands feed the artifact cache key, and they run through the same inline shell
        # from the same task directory. Measured on 2026.8.6 with a real UNC share: an input that
        # reads a file present in the project failed, because cmd ran it in C:\Windows.
        $config = @"
[settings]
experimental = true

[tasks.build]
run = "echo built"
dir = '$($script:UncDir)'
sources = ["input.txt"]
outputs = ["out.txt"]
cache = { enabled = true, command_inputs = ["type marker.txt"] }
"@
        Set-Content -LiteralPath (Join-Path $script:Proj "mise.toml") -Value $config

        Push-Location $script:Proj
        try {
            mise trust | Out-Null
            $output = mise run build 2>&1 | Out-String
            $code = $LASTEXITCODE
        } finally {
            Pop-Location
        }

        $code | Should -Not -Be 0
        $output | Should -Match 'UNC path as a working directory'
        # Without the guard this is what came out instead, from a command that had already run.
        $output | Should -Not -Match 'cache command input failed'
    }

    It 'leaves a shell that accepts UNC paths alone' {
        # Control: the guard keys off the shell, not off the path alone. pwsh still fails here
        # because the share does not exist, but it must fail on that rather than on this check.
        $config = @"
[tasks.here]
run = "Write-Output `$PWD.Path"
dir = '$($script:UncDir)'
shell = "pwsh -c"
"@
        Set-Content -LiteralPath (Join-Path $script:Proj "mise.toml") -Value $config

        Push-Location $script:Proj
        try {
            mise trust | Out-Null
            $output = mise run here 2>&1 | Out-String
        } finally {
            Pop-Location
        }

        $output | Should -Not -Match 'UNC path as a working directory'
    }
}
