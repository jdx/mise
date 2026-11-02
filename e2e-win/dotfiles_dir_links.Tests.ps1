Describe 'dotfiles-dir-links' {
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

        function script:New-Case([string]$Name, [string]$Mode, [bool]$WindowsSymlinks) {
            $root = Join-Path $script:TestRoot $Name
            New-Item -ItemType Directory -Path (Join-Path $root 'source\real-dir') -Force | Out-Null
            Set-Content -LiteralPath (Join-Path $root 'source\file.txt') -Value 'file'
            Set-Content -LiteralPath (Join-Path $root 'source\real-dir\inside.txt') -Value 'inside'
            $target = Join-Path $root 'target'
            $toml = $target -replace '\\', '/'
            $source = (Join-Path $root 'source') -replace '\\', '/'
            $settings = if ($WindowsSymlinks) { "[settings.dotfiles]`nwindows_symlinks = true`n`n" } else { '' }
            "$settings[dotfiles]`n`"$toml`" = { source = `"$source`", mode = `"$Mode`" }`n" |
                Out-File (Join-Path $root 'mise.toml') -Encoding ascii
            $env:MISE_STATE_DIR = Join-Path $root 'state'
            Set-Location $root
            return $root
        }
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

    It 'unapplies a symlink entry whose directory target is a junction' {
        # A directory source is linked as a junction whatever dotfiles.windows_symlinks says, and
        # `remove_file` is refused for one: unapply used to fail with "access denied" and leave
        # the junction in place.
        $root = New-Case 'symlink-dir' 'symlink' $false
        $target = Join-Path $root 'target'

        mise dot apply 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0
        (Get-Item -LiteralPath $target -Force).LinkType | Should -Be 'Junction'

        $out = mise dot unapply 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0 -Because $out
        Test-Path -LiteralPath $target | Should -BeFalse
        # removing the link must not reach through it
        Get-Content -LiteralPath (Join-Path $root 'source\file.txt') | Should -Be 'file'
    }

    It 'unapplies a symlink entry whose directory target is a native directory symlink' {
        # mise links a local directory as a junction, but a directory symlink already reaching
        # the source is applied as it is. `remove_file` is refused for one as for a junction.
        if (-not $script:CanSymlink) {
            Set-ItResult -Skipped -Because 'this runner cannot create symbolic links'
            return
        }
        $root = New-Case 'symlink-native-dir' 'symlink' $true
        $target = Join-Path $root 'target'
        New-Item -ItemType SymbolicLink -Path $target -Target (Join-Path $root 'source') | Out-Null
        (Get-Item -LiteralPath $target -Force).LinkType | Should -Be 'SymbolicLink'

        $out = mise dot apply 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0 -Because $out
        (Get-Item -LiteralPath $target -Force).LinkType | Should -Be 'SymbolicLink'

        $out = mise dot unapply 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0 -Because $out
        Test-Path -LiteralPath $target | Should -BeFalse
        Get-Content -LiteralPath (Join-Path $root 'source\file.txt') | Should -Be 'file'
    }

    It 'unapplies a symlink-each entry whose source holds a directory link' {
        if (-not $script:CanSymlink) {
            Set-ItResult -Skipped -Because 'this runner cannot create file symbolic links'
            return
        }
        $root = New-Case 'symlink-each-junction' 'symlink-each' $true
        New-Item -ItemType Junction -Path (Join-Path $root 'source\linkdir') -Target (Join-Path $root 'source\real-dir') | Out-Null
        $target = Join-Path $root 'target'

        $out = mise dot apply 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0 -Because $out
        (Get-Item -LiteralPath (Join-Path $target 'file.txt') -Force).LinkType | Should -Be 'SymbolicLink'
        (Get-Item -LiteralPath (Join-Path $target 'linkdir') -Force).LinkType | Should -Be 'Junction'

        # The junction used to stop unapply partway, after file.txt was already gone.
        $out = mise dot unapply 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0 -Because $out
        Test-Path -LiteralPath (Join-Path $target 'file.txt') | Should -BeFalse
        Test-Path -LiteralPath (Join-Path $target 'linkdir') | Should -BeFalse
        Test-Path -LiteralPath (Join-Path $target 'real-dir\inside.txt') | Should -BeFalse
        Get-Content -LiteralPath (Join-Path $root 'source\real-dir\inside.txt') | Should -Be 'inside'
    }

    It 'prunes a stale directory link once its source is deleted' {
        if (-not $script:CanSymlink) {
            Set-ItResult -Skipped -Because 'this runner cannot create file symbolic links'
            return
        }
        $root = New-Case 'symlink-each-stale-junction' 'symlink-each' $true
        $sourceLink = Join-Path $root 'source\linkdir'
        New-Item -ItemType Junction -Path $sourceLink -Target (Join-Path $root 'source\real-dir') | Out-Null
        $target = Join-Path $root 'target'

        mise dot apply 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0
        (Get-Item -LiteralPath (Join-Path $target 'linkdir') -Force).LinkType | Should -Be 'Junction'

        # deleting the source's junction makes the target's a stale link, which apply removes
        [System.IO.Directory]::Delete($sourceLink)
        $out = mise dot apply 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0 -Because $out
        Test-Path -LiteralPath (Join-Path $target 'linkdir') | Should -BeFalse
        Get-Content -LiteralPath (Join-Path $root 'source\real-dir\inside.txt') | Should -Be 'inside'
    }
}
