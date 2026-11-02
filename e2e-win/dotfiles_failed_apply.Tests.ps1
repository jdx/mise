Describe 'dotfiles-failed-apply' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null

        # Saved here, restored in AfterAll: Pester runs every suite in one process, so removing
        # an inherited value would leave the next suite without it.
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot
        $script:OriginalStateDir = [Environment]::GetEnvironmentVariable('MISE_STATE_DIR', 'Process')

        # Whether this runner may create file symbolic links (Developer Mode or elevation).
        $probeSource = Join-Path $script:TestRoot 'probe-source'
        $probeLink = Join-Path $script:TestRoot 'probe-link'
        Set-Content -LiteralPath $probeSource -Value 'probe'
        try {
            New-Item -ItemType SymbolicLink -Path $probeLink -Target $probeSource -ErrorAction Stop | Out-Null
            $script:CanSymlink = $true
        } catch {
            $script:CanSymlink = $false
        }
        Remove-Item -LiteralPath $probeLink, $probeSource -Force -ErrorAction Ignore
        $script:Sid = ([Security.Principal.WindowsIdentity]::GetCurrent()).User.Value
    }

    AfterAll {
        Set-Location $script:OriginalDir
        Remove-Item -Path $script:TestRoot -Recurse -Force -ErrorAction Ignore
        foreach ($pair in @(
                @('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted),
                @('MISE_STATE_DIR', $script:OriginalStateDir))) {
            # a variable that was unset is removed, not left behind empty
            if ($null -eq $pair[1]) {
                Remove-Item "Env:$($pair[0])" -ErrorAction Ignore
            } else {
                [Environment]::SetEnvironmentVariable($pair[0], $pair[1], 'Process')
            }
        }
    }

    It 'lets the next apply carry on after a symlink-each apply fails partway' {
        if (-not $script:CanSymlink) {
            Set-ItResult -Skipped -Because 'this runner cannot create file symbolic links'
            return
        }
        $root = Join-Path $script:TestRoot 'partial'
        New-Item -ItemType Directory -Path (Join-Path $root 'source\sub'), (Join-Path $root 'target\sub') -Force | Out-Null
        Set-Content -LiteralPath (Join-Path $root 'source\a.txt') -Value 'a'
        Set-Content -LiteralPath (Join-Path $root 'source\sub\b.txt') -Value 'b'
        $target = (Join-Path $root 'target') -replace '\\', '/'
        $source = (Join-Path $root 'source') -replace '\\', '/'
        "[settings.dotfiles]`nwindows_symlinks = true`n`n[dotfiles]`n`"$target`" = { source = `"$source`", mode = `"symlink-each`" }`n" |
            Out-File (Join-Path $root 'mise.toml') -Encoding utf8NoBOM
        $env:MISE_STATE_DIR = Join-Path $root 'state'
        Set-Location $root

        # a.txt links, then sub\b.txt cannot
        $sub = Join-Path $root 'target\sub'
        icacls $sub /deny "*$($script:Sid):(W)" | Out-Null
        try {
            $out = mise dot apply 2>&1 | Out-String
            $LASTEXITCODE | Should -Not -Be 0 -Because $out
        } finally {
            icacls $sub /remove:d "*$($script:Sid)" | Out-Null
        }
        (Get-Item -LiteralPath (Join-Path $root 'target\a.txt') -Force).LinkType | Should -Be 'SymbolicLink'
        # This used to leave an unfinished history record that only `mise dot recover` cleared.
        $out | Should -Not -Match 'recovery needs attention'
        $pending = mise bootstrap dotfiles history --pending --json
        $LASTEXITCODE | Should -Be 0
        @($pending | ConvertFrom-Json).Count | Should -Be 0

        $out = mise dot apply 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0 -Because $out
        (Get-Item -LiteralPath (Join-Path $root 'target\sub\b.txt') -Force).LinkType | Should -Be 'SymbolicLink'
    }
}
