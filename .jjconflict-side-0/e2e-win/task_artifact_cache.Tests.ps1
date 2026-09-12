Describe 'task artifact cache' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null
        Set-Location $script:TestRoot

        $script:OriginalEnv = @{
            MISE_CACHE_DIR = $env:MISE_CACHE_DIR
            MISE_STATE_DIR = $env:MISE_STATE_DIR
            MISE_TRUSTED_CONFIG_PATHS = $env:MISE_TRUSTED_CONFIG_PATHS
        }
        $env:MISE_CACHE_DIR = Join-Path $script:TestRoot 'cache'
        $env:MISE_STATE_DIR = Join-Path $script:TestRoot 'state'
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot

        @'
[settings]
experimental = true

[task_config.cache]
enabled = true

[tasks.build]
shell = "pwsh -NoProfile -Command"
run = '''
New-Item -ItemType Directory -Force -Path dist | Out-Null
Get-Content input.txt | Set-Content dist/result.txt
Add-Content runs.txt ran
Write-Output cache-stdout
[Console]::Error.WriteLine('cache-stderr')
'''
sources = ["input.txt"]
outputs = ["dist"]
'@ | Out-File -FilePath 'mise.toml' -Encoding utf8NoBOM
        Set-Content -Path 'input.txt' -Value 'windows-cache-input'
    }

    AfterAll {
        Set-Location $script:OriginalDir
        foreach ($name in $script:OriginalEnv.Keys) {
            if ($null -eq $script:OriginalEnv[$name]) {
                Remove-Item -Path "Env:$name" -ErrorAction Ignore
            } else {
                Set-Item -Path "Env:$name" -Value $script:OriginalEnv[$name]
            }
        }
    }

    It 'restores files and captured output from an archive' {
        $first = mise run build 2>&1 | Out-String
        $first | Should -Match 'cache-stdout'
        $first | Should -Match 'cache-stderr'
        (Get-Content 'dist/result.txt').Trim() | Should -Be 'windows-cache-input'
        @(Get-Content 'runs.txt').Count | Should -Be 1

        $cacheDir = Join-Path $env:MISE_CACHE_DIR 'task-artifacts/v2'
        @(Get-ChildItem $cacheDir -Filter '*.json').Count | Should -Be 1
        @(Get-ChildItem $cacheDir -Filter '*.tar.zst').Count | Should -Be 1

        Remove-Item 'dist' -Recurse -Force
        $second = mise run build 2>&1 | Out-String

        $second | Should -Match 'restored outputs from cache'
        $second | Should -Match 'cache-stdout'
        $second | Should -Match 'cache-stderr'
        (Get-Content 'dist/result.txt').Trim() | Should -Be 'windows-cache-input'
        @(Get-Content 'runs.txt').Count | Should -Be 1
    }
}
