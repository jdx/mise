Describe 'uninstall runtime symlink cleanup' {

    BeforeAll {
        $cfg = ".\mise.local.toml"

        $content = @"
[tools]
yq = "4.45.4"
"@
        $content | Out-File $cfg

        mise install yq@4.44.3 yq@4.45.4
        # derive the installs dir from a real install so any MISE_DATA_DIR works
        $script:yqDir = Split-Path -Parent (mise where yq@4.45.4)
    }

    AfterAll {
        Remove-Item $cfg -ErrorAction Ignore
    }

    # https://github.com/jdx/mise/discussions/5260 - on Windows, version
    # pointer files (regular files containing the target path) were left
    # behind on uninstall because they never look like missing symlinks
    It 'removes stale version pointers when a version is uninstalled' {
        Test-Path (Join-Path $yqDir "4.44") | Should -BeTrue

        mise uninstall yq@4.44.3

        Test-Path (Join-Path $yqDir "4.44.3") | Should -BeFalse
        Test-Path (Join-Path $yqDir "4.44") | Should -BeFalse
        # pointers for the remaining version are kept
        Test-Path (Join-Path $yqDir "4.45") | Should -BeTrue
        Test-Path (Join-Path $yqDir "latest") | Should -BeTrue
        mise x yq@4.45.4 -- yq --version | Should -Match "4.45.4"
    }

    It 'removes the tool directory when the last version is uninstalled' {
        # --all keeps this independent of the previous test's uninstall
        mise uninstall --all yq
        Test-Path $yqDir | Should -BeFalse
    }
}
