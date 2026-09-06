Describe 'dotfiles' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null

        # Saved here, restored in AfterAll: Pester runs every suite in one process, so removing
        # an inherited value would leave the next suite without it.
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot

        New-Item -ItemType Directory -Path (Join-Path $script:TestRoot "dotfiles") | Out-Null
        $script:Source = Join-Path $script:TestRoot "dotfiles\gitconfig"
        "gitconfig content" | Out-File -FilePath $script:Source -Encoding ascii -NoNewline

        $script:Target = Join-Path $script:TestRoot "applied\.gitconfig"
        $targetToml = $script:Target -replace '\\', '/'
        @"
[dotfiles]
"$targetToml" = { source = "dotfiles/gitconfig", mode = "symlink" }
"@ | Out-File (Join-Path $script:TestRoot "mise.toml")

        Set-Location $script:TestRoot
    }

    AfterAll {
        Set-Location $script:OriginalDir
        Remove-Item -Path $script:TestRoot -Recurse -Force -ErrorAction Ignore
        if ($null -eq $script:OriginalTrusted) {
            Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted, 'Process')
        }
    }

    It 'reports the entry as missing before applying' {
        $out = mise bootstrap dotfiles status 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -BeLike '*missing*'
    }

    It 'applies a symlink entry and then reads it back as applied' {
        # `symlink` mode produces a real symlink when Windows allows one without elevation
        # (Developer Mode) and a copy otherwise. Which one lands is a property of the runner,
        # not of mise, so this asserts what has to hold either way: the entry applies, the
        # content is right, and `status` recognises whichever form is on disk.
        mise bootstrap dotfiles apply 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0

        Test-Path $script:Target | Should -BeTrue
        (Get-Content $script:Target -Raw) | Should -Be "gitconfig content"

        $out = mise bootstrap dotfiles status 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        # Asserting the positive state, not just the absence of "missing": a row reading
        # "differs" would satisfy the weaker check while meaning the entry did not converge.
        $out | Should -BeLike '*applied*'

        # Report which branch the runner took, so a failure elsewhere in this file is readable.
        $link = (Get-Item $script:Target).LinkType
        Write-Host "  applied as: $(if ($link) { $link } else { 'copy' })"
    }

    It 'unapplies without needing --force' {
        # The regression this guards: a real symlink used to be routed through the content
        # comparison, which rejects a symlink and demands --force.
        #
        # Applied here rather than relying on the previous test, so this one states its own
        # precondition and does not silently pass if that test stopped applying.
        mise bootstrap dotfiles apply 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0
        Test-Path $script:Target | Should -BeTrue

        mise bootstrap dotfiles unapply 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0
        Test-Path $script:Target | Should -BeFalse
    }
}
