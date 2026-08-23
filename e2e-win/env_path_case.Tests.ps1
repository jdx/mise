Describe 'env names that differ only in case' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        # Saved here, restored in AfterAll: Pester runs every suite in one process, so removing an
        # inherited value would leave the next suite without it.
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot

        # `Path` is how Windows itself writes it, so it is the spelling a config is likely to use --
        # and it names the same variable as `PATH`, which mise owns. `Temp` is the control: mise
        # emits nothing of its own under that name, so it has never collided with anything.
        @(
            '[env]'
            'Path = "C:/custom-from-mise"'
            'Temp = "C:/mytemp"'
        ) | Out-File -Encoding ascii (Join-Path $script:TestRoot "mise.toml")

        Set-Location $script:TestRoot
    }

    AfterAll {
        Set-Location $script:OriginalDir
        if ($null -eq $script:OriginalTrusted) {
            Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted, 'Process')
        }
    }

    It 'emits PATH once' {
        # Two entries is the whole defect: the shell applies both, and the later one wins.
        $count = mise env --json | jq -r '[to_entries[] | select(.key | ascii_upcase == "PATH")] | length'
        $count | Should -Be 1
    }

    It 'does not let the declaration replace PATH' {
        # Asserted on the value rather than an exit code -- this failed while exiting 0.
        $path = mise env --json | jq -r 'to_entries[] | select(.key | ascii_upcase == "PATH") | .value'
        $path | Should -Match 'system32'
    }

    It 'leaves a child process with a usable PATH' {
        $path = mise exec -- pwsh -NoProfile -Command '$env:PATH' 2>&1 | Out-String
        $path | Should -Match 'system32'
    }

    It 'still applies a declaration that collides with nothing' {
        # Control: only a name mise emits itself can collide, so `Temp` has to keep working
        # exactly as before. Without this, folding too much would look like a fix.
        mise env --json | jq -r '.Temp' | Should -Be 'C:/mytemp'
    }
}

Describe 'PATH spelled another way in required and unset' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot
        Set-Location $script:TestRoot

        # Each case needs its own config, and `required` may not be combined with a value.
        function Write-Config {
            param([string[]]$Lines)
            $Lines | Out-File -Encoding ascii (Join-Path $script:TestRoot "mise.toml")
        }
    }

    AfterAll {
        Set-Location $script:OriginalDir
        if ($null -eq $script:OriginalTrusted) {
            Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted, 'Process')
        }
    }

    It 'sees a required PATH that is spelled differently' {
        # PATH is set -- it is always set -- but the lookup was on the literal name, so this
        # reported it missing. The message is the assertion because that is what the user gets.
        Write-Config @('[env]', 'Path = { required = true }')
        $out = mise env 2>&1 | Out-String
        $out | Should -Not -Match "Required environment variable 'Path' is not defined"
    }

    It 'leaves PATH usable after unsetting it by another spelling' {
        # A guard rather than a reproduction: PATH survives either way, because mise writes its
        # own after the directives are applied. It pins that folding `unset` did not turn into a
        # way to lose PATH.
        Write-Config @('[env]', 'Path = false')
        $path = mise env --json | jq -r 'to_entries[] | select(.key | ascii_upcase == "PATH") | .value'
        $path | Should -Match 'system32'
    }
}
