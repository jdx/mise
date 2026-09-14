Describe 'npm_backend' {
    BeforeAll {
        $originalPath = $PWD
        $originalMiseConfigFile = [Environment]::GetEnvironmentVariable('MISE_CONFIG_FILE', 'Process')
        $originalMiseGlobalConfigFile = [Environment]::GetEnvironmentVariable('MISE_GLOBAL_CONFIG_FILE', 'Process')
        $originalMiseSystemConfigFile = [Environment]::GetEnvironmentVariable('MISE_SYSTEM_CONFIG_FILE', 'Process')
        $originalMiseTrustedConfigPaths = [Environment]::GetEnvironmentVariable('MISE_TRUSTED_CONFIG_PATHS', 'Process')
        $originalNpmPackageManager = [Environment]::GetEnvironmentVariable('MISE_NPM_PACKAGE_MANAGER', 'Process')
        $config = Join-Path $TestDrive 'mise.toml'
        $systemConfig = Join-Path $TestDrive 'system.toml'
        '' | Set-Content $systemConfig
        Set-Location TestDrive:
        $env:MISE_CONFIG_FILE = $config
        $env:MISE_GLOBAL_CONFIG_FILE = $config
        $env:MISE_SYSTEM_CONFIG_FILE = $systemConfig
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive
    }

    AfterAll {
        Set-Location $originalPath
        if ($null -eq $originalMiseConfigFile) {
            Remove-Item Env:MISE_CONFIG_FILE -ErrorAction SilentlyContinue
        }
        else {
            $env:MISE_CONFIG_FILE = $originalMiseConfigFile
        }
        if ($null -eq $originalMiseGlobalConfigFile) {
            Remove-Item Env:MISE_GLOBAL_CONFIG_FILE -ErrorAction SilentlyContinue
        }
        else {
            $env:MISE_GLOBAL_CONFIG_FILE = $originalMiseGlobalConfigFile
        }
        if ($null -eq $originalMiseSystemConfigFile) {
            Remove-Item Env:MISE_SYSTEM_CONFIG_FILE -ErrorAction SilentlyContinue
        }
        else {
            $env:MISE_SYSTEM_CONFIG_FILE = $originalMiseSystemConfigFile
        }
        if ($null -eq $originalMiseTrustedConfigPaths) {
            Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction SilentlyContinue
        }
        else {
            $env:MISE_TRUSTED_CONFIG_PATHS = $originalMiseTrustedConfigPaths
        }
        if ($null -eq $originalNpmPackageManager) {
            Remove-Item Env:MISE_NPM_PACKAGE_MANAGER -ErrorAction SilentlyContinue
        }
        else {
            $env:MISE_NPM_PACKAGE_MANAGER = $originalNpmPackageManager
        }
    }

    It 'installs npm:prettier 3.6.2 with aube' {
        @(
            '[tools]'
            'node = "24.4.1"'
            'aube = "1.1.0"'
            '"npm:prettier" = "3.6.2"'
        ) | Set-Content $config
        $env:MISE_NPM_PACKAGE_MANAGER = "auto"
        mise x node@24.4.1 aube@1.1.0 npm:prettier@3.6.2 -- prettier --version | Should -be "3.6.2"
    }
    It 'installs npm:cowsay 1.6.0 with bun' {
        @(
            '[tools]'
            'node = "24.4.1"'
            'bun = "1.2.19"'
            '"npm:cowsay" = "1.6.0"'
        ) | Set-Content $config
        $env:MISE_NPM_PACKAGE_MANAGER = "bun"
        mise x node@24.4.1 bun@1.2.19 npm:cowsay@1.6.0 -- cowsay --version | Should -be "1.6.0"
    }
    It 'installs npm:prettier 3.5.3 with the node-bundled npm, which ships no npm.exe' {
        @(
            '[tools]'
            'node = "24.4.1"'
            '"npm:prettier" = "3.5.3"'
        ) | Set-Content $config
        # Covers the spawn that `NPM_PROGRAM = "npm.cmd"` used to hardcode. mise's node
        # install ships `npm` (a `#!/usr/bin/env bash` script), `npm.cmd` and `npm.ps1` but
        # no `npm.exe`, and on Windows the node plugin puts the install root itself on PATH.
        # `executable_names` tries the bare name first, so mise's own lookup answers with
        # the bash script -- which CreateProcess cannot launch. The resolver has to walk
        # past it to npm.cmd.
        #
        # The default package manager is `auto` -> embedded aube, which never spawns npm,
        # so pinning it is what makes this reach the npm program at all. 3.5.3 rather than
        # the 3.6.2 the aube case above installs, so a cached install cannot let this skip
        # the spawn entirely.
        $env:MISE_NPM_PACKAGE_MANAGER = "npm"
        $out = mise install node@24.4.1 npm:prettier@3.5.3 2>&1 | Out-String
        $LASTEXITCODE | Should -Be 0
        # The pre-fix failure mode, from std only ever appending .exe to a bare name.
        $out | Should -Not -Match 'program not found'
        # Assert the result of that install, not just its exit code: the binary npm
        # placed has to be the requested version.
        mise x node@24.4.1 npm:prettier@3.5.3 -- prettier --version | Should -be "3.5.3"
    }
}
