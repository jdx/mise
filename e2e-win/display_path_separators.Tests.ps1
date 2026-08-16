Describe 'displayed path separators' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        # Saved here, restored in AfterAll: Pester runs every suite in one process, so removing an
        # inherited value would leave the next suite without it.
        $script:OriginalTrusted = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot
        "[tools]" | Out-File -Encoding ascii (Join-Path $script:TestRoot "mise.toml")
        Set-Location $script:TestRoot
        git init -q 2>&1 | Out-Null
    }

    AfterAll {
        Set-Location $script:OriginalDir
        if ($null -eq $script:OriginalTrusted) {
            Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore
        } else {
            [Environment]::SetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', $script:OriginalTrusted, 'Process')
        }
    }

    # `Path::join` appends a multi-segment literal verbatim, so `root.join(".git/hooks")` keeps the
    # `/` it was given, and the root itself arrives `/`-separated from the git library. The path
    # reported here used to switch form three times.
    It 'reports a written path with a single separator' {
        $output = mise generate git-pre-commit --write 2>&1 | Out-String
        # Anchored so an unrelated line cannot join in, and the ANSI reset mise may append is
        # trimmed off the end.
        $line = (($output -split "`n" | Where-Object { $_ -match '^\s*Wrote to ' }) -join '').Trim()
        $line | Should -Not -BeNullOrEmpty
        # Matched as a literal: `-BeLike` would read `?` and `[` as wildcards. The line is carried
        # into the message so a failure says which spelling arrived.
        $line.Contains('/') | Should -BeFalse -Because "the reported path was: $line"
        # and the path is still reported -- settled, not dropped
        $line.Contains('pre-commit') | Should -BeTrue -Because "the reported path was: $line"
        $line.Contains($script:TestRoot) | Should -BeTrue -Because "the reported path was: $line"
    }

    It 'reports config paths with a single separator' {
        $output = mise config ls 2>&1 | Out-String
        $ours = ($output -split "`n" | Where-Object { $_ -match 'mise\.toml' }) -join "`n"
        $ours | Should -Not -BeNullOrEmpty
        $ours.Contains('/') | Should -BeFalse -Because "the reported lines were: $ours"
    }
}
