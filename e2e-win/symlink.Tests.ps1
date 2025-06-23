Describe 'windows runtime symlink' {

    BeforeAll {
        $cfg = ".\mise.local.toml"
        $tool = "yq"

        $content = @"
[tools]
$tool = "latest"
"@
        $content | Out-File $cfg
        Get-Content $cfg
    }

    AfterAll {
        Remove-Item $cfg -ErrorAction Ignore
    }

    It 'version is correct, not latest' {
        mise install yq@4.45.4

        # correct output
        # mise/2025.5.15/bin/mise ls yq
        # Tool  Version  Source                                        Requested
        # yq    4.45.4   D:\Users\qianlongzt\.config\mise\config.toml  latest

        # wrong output
        # mise/2025.5.16/bin/mise ls yq
        # Tool  Version  Source                                        Requested
        # yq    latest   D:\Users\qianlongzt\.config\mise\config.toml  latest
        # yq    4.45.4

        # https://github.com/jdx/mise/discussions/5254

        $output = mise ls --json yq
        $output | jq '.[] | select(.source ) | .version' | Should -Be '"4.45.4"'
        $output | jq '.[] | select(.version == "latest" ) | .version' | Should -Be $null
    }
}
