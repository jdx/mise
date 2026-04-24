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
}
