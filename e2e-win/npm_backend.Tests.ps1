Describe 'npm_backend' {
    It 'installs npm:prettier 3.6.2 with npm' {
        mise x node npm:prettier@3.6.2 -- prettier --version | Should -be "3.6.2"
    }
    It 'installs npm:prettier 3.6.2 with bun' {
        $env:MISE_NPM_BUN = "true"
        mise settings npm.bun
        mise x npm:prettier@3.6.2 -- echo $env:PATH
        mise x npm:prettier@3.6.2 -- prettier --version
        mise x node bun npm:prettier@3.6.2 -- echo $env:PATH
        mise x node bun npm:prettier@3.6.2 -- prettier --version
        mise x npm:prettier@3.6.2 -- prettier --version | Should -be "3.6.2"
        mise x node bun npm:prettier@3.6.2 -- prettier --version | Should -be "3.6.2"
        Remove-Item Env:MISE_NPM_BUN
    }
}
