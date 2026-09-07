Describe 'dotfiles' {
    BeforeAll {
        $script:OriginalExperimental = [Environment]::GetEnvironmentVariable('MISE_EXPERIMENTAL', 'Process')
        $env:MISE_EXPERIMENTAL = '1'
        $script:OriginalDir = Get-Location
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null

        # Saved here, restored in AfterAll: Pester runs every suite in one process, so removing
        # an inherited value would leave the next suite without it.
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot
        # Applies record file checkpoints under the state dir; keep them out of the runner's.
        $script:OriginalStateDir = [Environment]::GetEnvironmentVariable('MISE_STATE_DIR', 'Process')
        $env:MISE_STATE_DIR = Join-Path $script:TestRoot "state"

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
        [Environment]::SetEnvironmentVariable('MISE_EXPERIMENTAL', $script:OriginalExperimental, 'Process')
        Set-Location $script:OriginalDir
        Remove-Item -Path $script:TestRoot -Recurse -Force -ErrorAction Ignore
        if ($null -eq $script:OriginalTrusted) {
            Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted, 'Process')
        }
        if ($null -eq $script:OriginalStateDir) {
            Remove-Item Env:MISE_STATE_DIR -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_STATE_DIR', $script:OriginalStateDir, 'Process')
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

    It 'keeps history private despite inherited parent permissions' {
        mise bootstrap dotfiles apply 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0
        $history = Join-Path $env:MISE_STATE_DIR 'history'
        $acl = Get-Acl -LiteralPath $history
        $acl.AreAccessRulesProtected | Should -BeTrue
        $acl.Sddl | Should -Match ';;;OW\)'
        @($acl.Access).Count | Should -Be 1
        $probe = Join-Path $history 'private-acl-probe'
        Set-Content -LiteralPath $probe -Value 'private'
        $inherited = Get-Acl -LiteralPath $probe
        @($inherited.Access).Count | Should -Be 1
        $inherited.Sddl | Should -Match ';;;OW\)'
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

    It 'records a history checkpoint pair for an apply' {
        # The previous test left the target unapplied, so this apply changes something and
        # must leave a pair of checkpoints behind; a no-op apply would record nothing.
        mise bootstrap dotfiles apply 2>&1 | Out-String | Out-Null
        $LASTEXITCODE | Should -Be 0

        $json = mise bootstrap dotfiles history --json 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $checkpoints = $json | ConvertFrom-Json
        @($checkpoints).Count | Should -BeGreaterOrEqual 1
        $latest = @($checkpoints)[0]
        $latest.operation.status | Should -Be 'completed'
        $latest.operation.command | Should -BeLike 'bootstrap dotfiles apply*'
        $latest.trigger | Should -Be 'bootstrap'
        $status = mise bootstrap dotfiles status --json 2>&1 | Out-String | ConvertFrom-Json
        $status.history.unavailable | Should -BeNullOrEmpty -Because ("the store should be usable: " + ($status.history | ConvertTo-Json -Depth 6 -Compress))
        $latest.tree.available | Should -BeTrue -Because ("the checkpoint should hold a snapshot: " + ($latest.tree | ConvertTo-Json -Depth 4 -Compress))
        Test-Path (Join-Path $env:MISE_STATE_DIR 'history\repo.git') | Should -BeTrue -Because ("the store lives under " + $env:MISE_STATE_DIR)

        $out = mise bootstrap dotfiles history show latest 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -BeLike '*Operation:*bootstrap (completed)*'

        $history = mise bootstrap dotfiles history --json 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $checkpoints = $history | ConvertFrom-Json
        @($checkpoints)[1].trigger | Should -Be 'bootstrap-before'
    }
}
