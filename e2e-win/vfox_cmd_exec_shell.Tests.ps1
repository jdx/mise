Describe 'vfox-cmd-exec-shell' {
    BeforeAll {
        $script:OriginalDir = Get-Location
        $script:TestRoot = Join-Path $TestDrive ([System.Guid]::NewGuid().ToString())
        New-Item -ItemType Directory -Path $script:TestRoot | Out-Null

        # Saved and restored in AfterAll: Pester runs every suite in one process.
        $script:Saved = @{}
        foreach ($name in 'MISE_DATA_DIR', 'MISE_CONFIG_FILE', 'MISE_TRUSTED_CONFIG_PATHS', 'MISE_WINDOWS_DEFAULT_INLINE_SHELL_ARGS') {
            $script:Saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        }
        $env:MISE_DATA_DIR = Join-Path $script:TestRoot 'data'
        $env:MISE_CONFIG_FILE = Join-Path $script:TestRoot 'mise.toml'
        $env:MISE_TRUSTED_CONFIG_PATHS = $script:TestRoot
        Set-Location $script:TestRoot
    }

    AfterAll {
        Set-Location $script:OriginalDir
        foreach ($name in $script:Saved.Keys) {
            [Environment]::SetEnvironmentVariable($name, $script:Saved[$name], 'Process')
        }
    }

    # Plugins write cmd.exec commands for cmd on Windows and cannot adapt to the user's task shell.
    # A shell that cannot run, or one that expands `$`, must not wrap them. (#16033)
    It 'runs plugin commands with cmd, not windows_default_inline_shell_args' {
        $pluginDir = Join-Path $env:MISE_DATA_DIR 'plugins\test-cmd-shell'
        New-Item -ItemType Directory -Path (Join-Path $pluginDir 'hooks') -Force | Out-Null
        @'
PLUGIN = {}
PLUGIN.name = "test-cmd-shell"
PLUGIN.version = "1.0.0"
PLUGIN.homepage = "https://example.com"
PLUGIN.license = "MIT"
PLUGIN.description = "Test that cmd.exec ignores the default inline shell"
PLUGIN.minRuntimeVersion = "0.3.0"
'@ | Out-File (Join-Path $pluginDir 'metadata.lua') -Encoding ascii
        @'
local cmd = require("cmd")

function PLUGIN:MiseEnv(ctx)
    local output = cmd.exec(
        "powershell.exe -NoLogo -NoProfile -NonInteractive -Command "
            .. "$null = 1; Write-Output $env:TEST_CMD_VALUE",
        { env = { TEST_CMD_VALUE = "plugin-body" } }
    )
    output = output:gsub("%s+$", "")
    return {
        env = {{key = "TEST_CMD_SHELL_RESULT", value = output}},
        cacheable = false,
        watch_files = {}
    }
end
'@ | Out-File (Join-Path $pluginDir 'hooks\mise_env.lua') -Encoding ascii
        @'
[env]
_.test-cmd-shell = {}
'@ | Out-File $env:MISE_CONFIG_FILE -Encoding ascii

        $env:MISE_WINDOWS_DEFAULT_INLINE_SHELL_ARGS = 'definitely-not-a-shell -c'
        $out = mise env --json 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        $out | Should -BeLike '*"TEST_CMD_SHELL_RESULT": "plugin-body"*'
    }
}
