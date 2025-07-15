Describe '7z' {
    BeforeAll {
        $cfg = ".\mise.local.toml"
        $content = @"
[tools]
"github:ip7z/7zip" = { version = "25.00", asset_pattern = "*-extra.7z" }
"@
        $content | Out-File $cfg
        Get-Content $cfg
    }

    AfterAll {
        Remove-Item $cfg -ErrorAction Ignore
    }

    It 'executes 7za 25.00' {
        mise install
        mise x github:ip7z/7zip -- 7za | Out-String | Should -Match "7-Zip \(a\) 25\.00"
    }
}

Describe '7z-strip-components' {
    BeforeAll {
        $cfg = ".\mise.local.toml"
        $content = @"
[tools]
"http:ip7z/7zip" = { version = "25.00", url = "https://mise.jdx.dev/test-fixtures/7z2500-extra.7z" }
"@
        $content | Out-File $cfg
        Get-Content $cfg
    }

    AfterAll {
        Remove-Item $cfg -ErrorAction Ignore
    }

    It 'executes 7za 25.00' {
        mise install
        mise x http:ip7z/7zip -- 7za | Out-String | Should -Match "7-Zip \(a\) 25\.00"
    }
}
