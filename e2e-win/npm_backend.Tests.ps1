Describe 'npm_backend' {
    It 'installs npm:prettier 3.6.2 with npm' {
        mise x node@24.4.1 npm:prettier@3.6.2 -- prettier --version | Should -be "3.6.2"
    }
    It 'installs npm:cowsay 1.6.0 with bun' {
        $env:MISE_NPM_BUN = "true"
        mise x node@24.4.1 bun@1.2.19 npm:cowsay@1.6.0 -- cowsay --version | Should -be "1.6.0"
        Remove-Item Env:MISE_NPM_BUN
    }
}
