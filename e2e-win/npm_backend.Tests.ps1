Describe 'npm_backend' {
    It 'installs npm:prettier 3.6.2 with npm' {
        mise x node@24.4.1 npm:prettier@3.6.2 -- prettier --version | Should -be "3.6.2"
    }
    It 'installs npm:prettier 3.6.2 with bun' {
        $env:MISE_NPM_BUN = "true"
        $env:RUST_LOG = "trace"
        mise x node@24.4.1 bun@1.2.19 npm:prettier@3.6.2 -- prettier --version | Should -be "3.6.2"
        Remove-Item Env:MISE_NPM_BUN
        Remove-Item Env:RUST_LOG
    }
}
