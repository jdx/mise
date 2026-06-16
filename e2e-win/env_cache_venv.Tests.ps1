# Regression test for the Windows path of the env_cache / uv venv interaction.
# See e2e/env/test_env_cache_venv for the full rationale: the env cache key did
# not account for the uv.lock / .venv that python.uv_venv_auto discovers via
# find_up, so a venv leaked across directories sharing the same config files.

Describe 'env_cache uv venv' {
    BeforeAll {
        $script:OriginalDir = Get-Location

        $env:MISE_EXPERIMENTAL = "1"
        $env:MISE_ENV_CACHE = "1"
        # Fixed encryption key, as `mise activate` would set.
        $env:__MISE_ENV_CACHE_KEY = "dGVzdGtleXRlc3RrZXl0ZXN0a2V5dGVzdGtleXRlc3Q="

        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null
        $env:MISE_STATE_DIR = Join-Path $script:TestRoot "state"

        # venv auto-source lives in the parent config so both child dirs resolve
        # the same config files (and thus the same cache key); neither child has
        # its own config.
        @"
[settings]
python.uv_venv_auto = "source"
"@ | Out-File (Join-Path $script:TestRoot ".mise.toml")
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot

        # "source" mode only needs an existing .venv directory and a uv.lock, so
        # no python/uv install is required.
        $script:Proj = Join-Path $script:TestRoot "proj"
        $script:Other = Join-Path $script:TestRoot "other"
        New-Item -ItemType Directory -Force -Path (Join-Path $script:Proj ".venv\Scripts") | Out-Null
        New-Item -ItemType Directory -Force -Path $script:Other | Out-Null
        New-Item -ItemType File -Force -Path (Join-Path $script:Proj "uv.lock") | Out-Null
    }

    AfterAll {
        Set-Location $script:OriginalDir
        Remove-Item Env:MISE_EXPERIMENTAL, Env:MISE_ENV_CACHE, Env:__MISE_ENV_CACHE_KEY, `
            Env:MISE_STATE_DIR, Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction Ignore
    }

    It 'does not leak the venv into a sibling without a uv.lock' {
        Set-Location $script:Proj
        (mise env | Out-String) | Should -Match "VIRTUAL_ENV"

        Set-Location $script:Other
        (mise env | Out-String) | Should -Not -Match "VIRTUAL_ENV"
    }

    It 'does not let a cached non-venv sibling suppress the venv' {
        Remove-Item (Join-Path $env:MISE_STATE_DIR "env-cache") -Recurse -Force -ErrorAction Ignore

        Set-Location $script:Other
        (mise env | Out-String) | Should -Not -Match "VIRTUAL_ENV"

        Set-Location $script:Proj
        (mise env | Out-String) | Should -Match "VIRTUAL_ENV"
    }
}
