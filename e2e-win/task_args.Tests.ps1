Describe 'task argument quoting' {
    BeforeAll {
        $originalPath = Get-Location
        Set-Location TestDrive:

        @'
[tasks.type]
run = "type"
'@ | Out-File -FilePath "mise.toml" -Encoding utf8

        # Create directory and file with spaces
        New-Item -ItemType Directory -Path "test dir" -Force | Out-Null
        Set-Content -Path "test dir\file.txt" -Value "test content"
    }

    AfterAll {
        Remove-Item -Path "test dir" -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item -Path "mise.toml" -Force -ErrorAction SilentlyContinue
        Set-Location $originalPath
    }

    It 'handles file path with spaces' {
        $output = mise run type ".\test dir\file.txt"
        $output | Should -Match "test content"
    }
}
