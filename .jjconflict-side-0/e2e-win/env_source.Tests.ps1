Describe 'env _.source' {

    BeforeAll {
        $cfg = ".\mise.local.toml"
        $script = Join-Path $PWD "env_source_test.sh"
        # LF endings - a CRLF script would leave `r in sourced values
        [IO.File]::WriteAllText($script,
            "export SOURCED_VAR=`"hello from bash`"`nexport PATH=`"/c/fake/prepended:`$PATH`"`n")
        # https://github.com/jdx/mise/discussions/6513 - sourcing failed entirely
        # on Windows (WSL launcher routing / literal /bin/bash fallback)
        "[env]`n_.source = `"./env_source_test.sh`"" | Out-File $cfg
    }

    AfterAll {
        Remove-Item $cfg, ".\env_source_test.sh" -ErrorAction Ignore
    }

    It 'exports variables from the sourced script' {
        mise env --json | jq -r '.SOURCED_VAR' | Should -Be 'hello from bash'
    }

    It 'converts prepended PATH entries to windows form' {
        $path = mise env --json | jq -r 'to_entries[] | select(.key | ascii_upcase == "PATH") | .value'
        $path | Should -Match ([regex]::Escape('C:\fake\prepended'))
    }
}
