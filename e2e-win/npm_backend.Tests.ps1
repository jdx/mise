Describe 'npm_backend' {
    It 'installs npm:prettier 3.6.2 with aube' {
        mise x node@24.4.1 aube@1.1.0 npm:prettier@3.6.2 -- prettier --version | Should -be "3.6.2"
    }
    It 'installs npm:cowsay 1.6.0 with bun' {
        $env:MISE_NPM_PACKAGE_MANAGER = "bun"
        try {
            mise x node@24.4.1 bun@1.2.19 npm:cowsay@1.6.0 -- cowsay --version | Should -be "1.6.0"
        }
        finally {
            Remove-Item Env:MISE_NPM_PACKAGE_MANAGER -ErrorAction SilentlyContinue
        }
    }
    It 'installs npm:prettier 3.5.3 with the node-bundled npm, which ships no npm.exe' {
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
        $previousPackageManager = [Environment]::GetEnvironmentVariable(
            'MISE_NPM_PACKAGE_MANAGER', 'Process'
        )
        $env:MISE_NPM_PACKAGE_MANAGER = "npm"
        try {
            $out = mise install node@24.4.1 npm:prettier@3.5.3 2>&1 | Out-String
            $LASTEXITCODE | Should -Be 0
            # The pre-fix failure mode, from std only ever appending .exe to a bare name.
            $out | Should -Not -Match 'program not found'
            # Assert the result of that install, not just its exit code: the binary npm
            # placed has to be the requested version.
            mise x node@24.4.1 npm:prettier@3.5.3 -- prettier --version | Should -be "3.5.3"
        }
        finally {
            # Restore rather than remove -- Pester shares one runspace, and $null does not
            # round-trip through SetEnvironmentVariable, so branch on it.
            if ($null -eq $previousPackageManager) {
                Remove-Item Env:\MISE_NPM_PACKAGE_MANAGER -ErrorAction SilentlyContinue
            }
            else {
                $env:MISE_NPM_PACKAGE_MANAGER = $previousPackageManager
            }
        }
    }
}
