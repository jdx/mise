Describe 'dotfiles-symlink-fallback' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null

        # Saved here, restored in AfterAll: Pester runs every suite in one process, so removing
        # an inherited value would leave the next suite without it.
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot
        $script:OriginalStateDir = [Environment]::GetEnvironmentVariable('MISE_STATE_DIR', 'Process')
        $script:OriginalWindowsSymlinks = [Environment]::GetEnvironmentVariable('MISE_DOTFILES_WINDOWS_SYMLINKS', 'Process')
        # the fallback only happens with the setting off, whatever a global config says
        $env:MISE_DOTFILES_WINDOWS_SYMLINKS = 'false'

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
    }

    AfterAll {
        Set-Location $script:OriginalDir
        Remove-Item -Path $script:TestRoot -Recurse -Force -ErrorAction Ignore
        foreach ($pair in @(
                @('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted),
                @('MISE_STATE_DIR', $script:OriginalStateDir),
                @('MISE_DOTFILES_WINDOWS_SYMLINKS', $script:OriginalWindowsSymlinks))) {
            # a variable that was unset is removed, not left behind empty
            if ($null -eq $pair[1]) {
                Remove-Item "Env:$($pair[0])" -ErrorAction Ignore
            } else {
                [Environment]::SetEnvironmentVariable($pair[0], $pair[1], 'Process')
            }
        }
    }

    It 'reports a symlink entry that Windows made a copy as copied' {
        # Only a runner without Developer Mode or elevation gets the copy fallback.
        if ($script:CanSymlink) {
            Set-ItResult -Skipped -Because 'this runner can create file symbolic links, so nothing falls back to a copy'
            return
        }
        $root = Join-Path $script:TestRoot 'fallback'
        New-Item -ItemType Directory -Path $root | Out-Null
        $source = Join-Path $root 'source.txt'
        Set-Content -LiteralPath $source -Value 'source-A'
        $target = Join-Path $root 'target.txt'
        "[dotfiles]`n`"$($target -replace '\\', '/')`" = { source = `"$($source -replace '\\', '/')`", mode = `"symlink`" }`n" |
            Out-File (Join-Path $root 'mise.toml') -Encoding ascii
        $env:MISE_STATE_DIR = Join-Path $root 'state'
        Set-Location $root

        # This used to say "created symlink" over a regular file.
        $out = mise dot apply 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0 -Because $out
        (Get-Item -LiteralPath $target -Force).LinkType | Should -BeNullOrEmpty
        $out | Should -Match 'copied .*Windows refused a symlink'
        $out | Should -Not -Match 'created symlink'

        $entry = (mise dot status --json | ConvertFrom-Json).files[0]
        $entry.state | Should -Be 'applied'
        $entry.reason | Should -Be 'a regular file, not a symlink'

        # the copy does not follow the source, and status says why it differs
        Set-Content -LiteralPath $source -Value 'source-B'
        $entry = (mise dot status --json | ConvertFrom-Json).files[0]
        $entry.state | Should -Be 'differs'
        $entry.reason | Should -Be 'content differs; a regular file, not a symlink'

        # the copy may hold edits, so a plain apply refuses to overwrite it
        $out = mise dot apply 2>&1 | Out-String
        $LASTEXITCODE | Should -Not -Be 0 -Because $out
        Get-Content -LiteralPath $target | Should -Be 'source-A'

        $out = mise dot apply --force 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0 -Because $out
        $out | Should -Match 'copied .*Windows refused a symlink'
        Get-Content -LiteralPath $target | Should -Be 'source-B'
    }
}
