Describe 'path-env' {
    It 'mise x produces the same path environment for successive runs' {
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
}
