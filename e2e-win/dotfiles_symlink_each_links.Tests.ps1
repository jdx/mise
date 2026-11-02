Describe 'dotfiles-symlink-each-links' {
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

        function script:Write-Config([string]$Root, [bool]$WindowsSymlinks) {
            $target = (Join-Path $Root 'target') -replace '\\', '/'
            $source = (Join-Path $Root 'source') -replace '\\', '/'
            $on = if ($WindowsSymlinks) { 'true' } else { 'false' }
            "[settings.dotfiles]`nwindows_symlinks = $on`n`n[dotfiles]`n`"$target`" = { source = `"$source`", mode = `"symlink-each`" }`n" |
                Out-File (Join-Path $Root 'mise.toml') -Encoding ascii
        }

        function script:New-Case([string]$Name) {
            $root = Join-Path $script:TestRoot $Name
            New-Item -ItemType Directory -Path (Join-Path $root 'source') -Force | Out-Null
            Write-Config $root $true
            $env:MISE_STATE_DIR = Join-Path $root 'state'
            Set-Location $root
            return $root
        }
    }

    BeforeEach {
        if (-not $script:CanSymlink) {
            Set-ItResult -Skipped -Because 'this runner cannot create file symbolic links'
        }
    }

    AfterAll {
        Set-Location $script:OriginalDir
        Remove-Item -Path $script:TestRoot -Recurse -Force -ErrorAction Ignore
        # a variable that was unset is removed, not left behind empty
        foreach ($pair in @(
                @('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted),
                @('MISE_STATE_DIR', $script:OriginalStateDir))) {
            if ($null -eq $pair[1]) {
                Remove-Item "Env:$($pair[0])" -ErrorAction Ignore
            } else {
                [Environment]::SetEnvironmentVariable($pair[0], $pair[1], 'Process')
            }
        }
    }

    It 'prunes a link whose source is gone after the state file is lost' {
        if (-not $script:CanSymlink) { return }
        $root = New-Case 'lost-state'
        Set-Content -LiteralPath (Join-Path $root 'source\file.txt') -Value 'file'
        $link = Join-Path $root 'target\file.txt'

        mise dot apply 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0
        (Get-Item -LiteralPath $link -Force).LinkType | Should -Be 'SymbolicLink'

        # the per-entry state files, not the per-target records under targets\
        Get-ChildItem -LiteralPath (Join-Path $env:MISE_STATE_DIR 'dotfiles') -Filter '*.toml' -File |
            Remove-Item -Force
        Remove-Item -LiteralPath (Join-Path $root 'source\file.txt')

        # Without the state file, apply used to see no stale link, leave this one dangling and
        # record an empty state, after which unapply did not know about it either.
        $out = mise dot apply 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0 -Because $out
        Test-Path -LiteralPath $link | Should -BeFalse
        [bool](Get-Item -LiteralPath $link -Force -ErrorAction Ignore) | Should -BeFalse
    }

    It 'counts a link left by the setting as a link, not a copy, once it is off' {
        if (-not $script:CanSymlink) { return }
        $root = New-Case 'toggle-off'
        Set-Content -LiteralPath (Join-Path $root 'source\old.txt') -Value 'old'

        mise dot apply 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0
        (Get-Item -LiteralPath (Join-Path $root 'target\old.txt') -Force).LinkType | Should -Be 'SymbolicLink'

        Write-Config $root $false
        Set-Content -LiteralPath (Join-Path $root 'source\new.txt') -Value 'new'
        $out = mise dot apply 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0 -Because $out
        # only new.txt was copied; old.txt stays the link it was
        $out | Should -BeLike '*copied 1 file(s) and linked 1*'
        (Get-Item -LiteralPath (Join-Path $root 'target\old.txt') -Force).LinkType | Should -Be 'SymbolicLink'
        (Get-Item -LiteralPath (Join-Path $root 'target\new.txt') -Force).LinkType | Should -BeNullOrEmpty
    }

    It 'keeps one state file when the target is spelled with other letter case' {
        if (-not $script:CanSymlink) { return }
        $root = New-Case 'casing'
        Set-Content -LiteralPath (Join-Path $root 'source\old.txt') -Value 'old'
        mise dot apply 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0

        # the same directory, which Windows does not tell apart; a new file makes apply save
        $source = (Join-Path $root 'source') -replace '\\', '/'
        $target = (Join-Path $root 'TARGET') -replace '\\', '/'
        "[settings.dotfiles]`nwindows_symlinks = true`n`n[dotfiles]`n`"$target`" = { source = `"$source`", mode = `"symlink-each`" }`n" |
            Out-File (Join-Path $root 'mise.toml') -Encoding ascii
        Set-Content -LiteralPath (Join-Path $root 'source\new.txt') -Value 'new'
        $out = mise dot apply 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0 -Because $out

        # This used to write a second state file beside the first.
        @(Get-ChildItem -LiteralPath (Join-Path $root 'state\dotfiles') -Filter '*.toml' -File).Count | Should -Be 1
        (Get-Item -LiteralPath (Join-Path $root 'target\old.txt') -Force).LinkType | Should -Be 'SymbolicLink'
        (Get-Item -LiteralPath (Join-Path $root 'target\new.txt') -Force).LinkType | Should -Be 'SymbolicLink'

        $out = mise dot unapply 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0 -Because $out
        Test-Path -LiteralPath (Join-Path $root 'target\old.txt') | Should -BeFalse
        Test-Path -LiteralPath (Join-Path $root 'target\new.txt') | Should -BeFalse
        @(Get-ChildItem -LiteralPath (Join-Path $root 'state\dotfiles') -Filter '*.toml' -File).Count | Should -Be 0
    }

    It 'keeps one target record when a single-file target is spelled with other letter case' {
        if (-not $script:CanSymlink) { return }
        $root = New-Case 'casing-single'
        $records = Join-Path $root 'state\dotfiles\targets'
        $write = {
            param([string]$Dir)
            $target = (Join-Path $root "$Dir\inline.conf") -replace '\\', '/'
            "[dotfiles]`n`"$target`" = { content = `"inline`" }`n" | Out-File (Join-Path $root 'mise.toml') -Encoding ascii
        }

        & $write 'made'
        $out = mise dot apply 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0 -Because $out
        @(Get-ChildItem -LiteralPath $records -Filter '*.toml' -File).Count | Should -Be 1

        # the same file to Windows: its record, with the content mise wrote, is the entry's
        & $write 'MADE'
        $out = mise dot apply 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0 -Because $out
        $out = mise dot unapply 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0 -Because $out
        Test-Path -LiteralPath (Join-Path $root 'made\inline.conf') | Should -BeFalse
        # This used to leave the first spelling's record behind.
        @(Get-ChildItem -LiteralPath $records -Filter '*.toml' -File -ErrorAction Ignore).Count | Should -Be 0
    }
}
