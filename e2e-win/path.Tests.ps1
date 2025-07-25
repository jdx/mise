Describe 'path-basics' {

    It 'does not give empty path' {
        $output = "$(mise x -- cmd.exe /d /s /c "echo %path%")".Trim()
        $output | Should -Not -Match "ECHO is on."
        $output = "$(mise x -- cmd.exe /d /s /c set path)".Trim()
        $output | Should -Match "path="
    }

    It 'can resolve where' {
        $output = "$(mise x -- cmd.exe /d /s /c "where where")".Trim()
        $output | Should -Match "where\.exe"
        $output = "$(mise x -- where where | Out-String)".Trim()
        $output | Should -Match "where\.exe"
    }

    It 'uses semi-colon as path separator' {
        $paths = "$(mise x -- cmd.exe /d /s /c "echo %path%")".Trim() -split ";"
        $paths.Count | Should -BeGreaterThan 1

        foreach ($path in $paths) {
            # prove that every : is used in the context of a drive-letter
            if ($path -match ":") {
                $path | Should -Match ":[\\/]"
            }
        }
    }

    It 'contains expected path' {
        $expected = "$PSScriptRoot\..\target\debug"
        $paths = "$(mise x -- cmd.exe /d /s /c "echo %path%")".Trim() -split ";"
        $paths -split ";" | Should -Contain "$expected"
    }

    # It 'can resolve mise npx' {
    #     mise x node@24.4.1 -- npx -y prettier@3.6.2 --version | Should -be "3.6.2"
    # }

    It 'outputs PATH in uppercase' {
        $output = "$(mise x -- cmd.exe /d /s /c set path)".Trim()
        $output | Should -MatchExactly "PATH="
    }
}

Describe 'path-basics-stable' {

    It 'never produces empty path' {
        for ($i = 0; $i -lt 10; $i++) {
            $output = "$(mise x -- cmd.exe /d /s /c "echo %path%")".Trim()
            $output | Should -Not -Match "ECHO is on."
            $output = "$(mise x -- cmd.exe /d /s /c set path)".Trim()
            $output | Should -Match "path="
        }
    }

    It 'always resolves where' {
        for ($i = 0; $i -lt 10; $i++) {
            $output = "$(mise x -- cmd.exe /d /s /c "where where")".Trim()
            $output | Should -Match "where\.exe"
            $output = "$(mise x -- where where | Out-String)".Trim()
            $output | Should -Match "where\.exe"
        }
    }

    It 'always uses semi-colon as path separator' {
        for ($i = 0; $i -lt 10; $i++) {
            $paths = "$(mise x -- cmd.exe /d /s /c "echo %path%")".Trim() -split ";"
            $paths.Count | Should -BeGreaterThan 1
            foreach ($path in $paths) {
                # prove that every : is used in the context of a drive-letter
                if ($path -match ":") {
                    $path | Should -Match ":[\\/]"
                }
            }
        }
    }

    It 'always contains expected path' {
        $expected = "$PSScriptRoot\..\target\debug"
        for ($i = 0; $i -lt 10; $i++) {
            $paths = "$(mise x -- cmd.exe /d /s /c "echo %path%")".Trim() -split ";"
            $paths -split ";" | Should -Contain "$expected"
        }
    }

    # It 'always resolves mise npx' {
    #     for ($i = 0; $i -lt 10; $i++) {
    #         mise x node@24.4.1 -- npx -y prettier@3.6.2 --version | Should -be "3.6.2"
    #     }
    # }

    It 'always has consistent path' {
        $paths = 1..10 | ForEach-Object {
            $output = "$(mise x -- cmd.exe /d /s /c "echo %path%")".Trim()
            $output
        }
        $paths | Should -HaveCount 10
        $paths | ForEach-Object { 
            $_ | Should -Not -Match "ECHO is on."
            $_.Length | Should -BeExactly $paths[0].Length
            $_ | Should -BeExactly $paths[0]
        }
    }

    It 'always outputs PATH in uppercase' {
        $set1 = @()
        $set2 = @()
        for ($i = 0; $i -lt 10; $i++) {
            $output = "$(mise x -- cmd.exe /d /s /c set path)".Trim()
            if ($output -cmatch "PATH=") { $set1 += $output }
            if ($output -cmatch "Path=") { $set2 += $output }
        }
        $set1 | Should -HaveCount 10
        $set2 | Should -HaveCount 0
    }

}
