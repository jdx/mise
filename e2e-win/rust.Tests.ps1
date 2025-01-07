
Describe 'rust' {
    It 'installs rust 1.83.0' {
        $env:MISE_CARGO_HOME = "%TEMP%\.cargo"
        $env:MISE_RUSTUP_HOME = "%TEMP%\.rustup"
        mise x rust@1.83.0 -- rustc -V | Should -BeLike "rustc 1.83.0*"
        Remove-Item Env:MISE_CARGO_HOME
        Remove-Item Env:MISE_RUSTUP_HOME
    }

    It 'executes rust 1.82.0' {
        mise x rust@1.82.0 -- rustc -V | Should -BeLike "rustc 1.82.0*"
    }
}
