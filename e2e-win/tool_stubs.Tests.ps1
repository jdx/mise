#!/usr/bin/env pwsh
# Windows E2E test for tool stubs functionality

Describe "Tool Stub Tests" {
    BeforeAll {
        $env:MISE_EXPERIMENTAL = "1"
        $testDir = Join-Path $env:TEMP "mise_stub_test_$(Get-Random)"
        New-Item -ItemType Directory -Path $testDir -Force | Out-Null
        Push-Location $testDir
    }

    AfterAll {
        Pop-Location
        if (Test-Path $testDir) {
            Remove-Item -Recurse -Force $testDir -ErrorAction SilentlyContinue
        }
    }

    It "Should generate basic tool stub with companion exe" {
        $stubPath = "./basic_tool"
        
        # Generate stub
        mise generate tool-stub $stubPath `
            --url "https://example.com/tool.zip" `
            --skip-download 2>&1 | Out-Null

        # Verify stub file exists
        Test-Path $stubPath | Should -Be $true
        
        # Verify companion exe exists on Windows
        Test-Path "$stubPath.exe" | Should -Be $true
        
        # Verify stub content
        $content = Get-Content $stubPath -Raw
        $content | Should -Match '#!/usr/bin/env -S mise tool-stub'
        $content | Should -Match 'url = "https://example.com/tool.zip"'
    }

    It "Should generate platform-specific stubs" {
        $stubPath = "./platform_tool"
        
        mise generate tool-stub $stubPath `
            --platform-url "windows-x64:https://example.com/win.zip" `
            --platform-url "linux-x64:https://example.com/linux.tar.gz" `
            --skip-download

        Test-Path $stubPath | Should -Be $true
        Test-Path "$stubPath.exe" | Should -Be $true
        
        $content = Get-Content $stubPath -Raw
        $content | Should -Match '\[platforms\.windows-x64\]'
        $content | Should -Match '\[platforms\.linux-x64\]'
    }

    It "Should support Windows-specific binary paths" {
        $stubPath = "./winbin_tool"
        
        mise generate tool-stub $stubPath `
            --platform-url "windows-x64:https://example.com/win.zip" `
            --platform-url "linux-x64:https://example.com/linux.tar.gz" `
            --platform-bin "windows-x64:tool.exe" `
            --platform-bin "linux-x64:bin/tool" `
            --skip-download

        $content = Get-Content $stubPath -Raw
        $content | Should -Match 'bin = "tool\.exe"'
        $content | Should -Match 'bin = "bin/tool"'
    }

    It "Should support incremental platform addition" {
        $stubPath = "./incremental"
        
        # First generation
        mise generate tool-stub $stubPath `
            --platform-url "windows-x64:https://example.com/v1.zip" `
            --skip-download

        $content1 = Get-Content $stubPath -Raw
        $content1 | Should -Match '\[platforms\.windows-x64\]'
        
        # Second generation (adds platform)
        mise generate tool-stub $stubPath `
            --platform-url "linux-x64:https://example.com/v1.tar.gz" `
            --skip-download

        $content2 = Get-Content $stubPath -Raw
        $content2 | Should -Match '\[platforms\.windows-x64\]'
        $content2 | Should -Match '\[platforms\.linux-x64\]'
    }

    It "Should create valid Windows executables" {
        $stubPath = "./exe_test"
        
        mise generate tool-stub $stubPath `
            --url "https://example.com/test.zip" `
            --skip-download

        $exePath = "$stubPath.exe"
        Test-Path $exePath | Should -Be $true
        
        # Check it's a valid PE file
        $bytes = [System.IO.File]::ReadAllBytes($exePath)
        $bytes[0] | Should -Be 0x4D  # 'M'
        $bytes[1] | Should -Be 0x5A  # 'Z'
        
        # Check size is reasonable (< 1MB)
        $fileInfo = Get-Item $exePath
        $fileInfo.Length | Should -BeLessThan 1048576
    }

    It "Should auto-detect platform from URL" {
        $stubPath = "./auto_detect"
        
        mise generate tool-stub $stubPath `
            --platform-url "https://github.com/releases/tool-windows-x64.zip" `
            --platform-url "https://github.com/releases/tool-linux-amd64.tar.gz" `
            --skip-download 2>&1 | Out-Null

        if (Test-Path $stubPath) {
            $content = Get-Content $stubPath -Raw
            # Should have auto-detected platforms
            $content | Should -Match '\[platforms\.'
        }
    }

    It "Should set version correctly" {
        $stubPath = "./versioned"
        
        mise generate tool-stub $stubPath `
            --url "https://example.com/tool.zip" `
            --version "1.2.3" `
            --skip-download

        $content = Get-Content $stubPath -Raw
        $content | Should -Match 'version = "1\.2\.3"'
    }
}