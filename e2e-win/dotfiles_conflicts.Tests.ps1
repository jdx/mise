Describe 'dotfiles-conflicts' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null

        # Saved here, restored in AfterAll: Pester runs every suite in one process, so removing
        # an inherited value would leave the next suite without it.
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot

        New-Item -ItemType Directory -Path (Join-Path $script:TestRoot "dotfiles") | Out-Null
        New-Item -ItemType Directory -Path (Join-Path $script:TestRoot "applied") | Out-Null
        "managed source" | Out-File -FilePath (Join-Path $script:TestRoot "dotfiles\gitconfig") -Encoding ascii -NoNewline

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
        if ($null -eq $script:OriginalTrusted) {
            Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted, 'Process')
        }
    }

    BeforeEach {
        # Each test starts from an unmanaged file the user wrote.
        "USER FILE" | Out-File -FilePath $script:Target -Encoding ascii -NoNewline
    }

    It 'refuses to overwrite an unmanaged file without --force' {
        $out = mise bootstrap dotfiles apply 2>&1 | Out-String
        $LASTEXITCODE | Should -Not -Be 0
        $out | Should -BeLike '*refusing to overwrite*'

        # Asserting the content, not just the exit code: the failure only means anything if the
        # file the user wrote is still there. Windows used to apply this entry and destroy it.
        (Get-Content $script:Target -Raw) | Should -Be "USER FILE"
    }

    It 'applies over it with --force' {
        # The control: the protection above is a prompt for --force, not a refusal to ever
        # manage the path.
        mise bootstrap dotfiles apply --force 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0
        (Get-Content $script:Target -Raw) | Should -Be "managed source"
    }
}
