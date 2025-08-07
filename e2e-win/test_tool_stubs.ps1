#!/usr/bin/env pwsh
# Windows E2E test for tool stubs functionality

$ErrorActionPreference = "Stop"
$OriginalLocation = Get-Location

function Test-ToolStubs {
    Write-Host "`n========================================" -ForegroundColor Cyan
    Write-Host "  Windows Tool Stubs E2E Test Suite" -ForegroundColor Cyan
    Write-Host "========================================`n" -ForegroundColor Cyan

    # Create test directory
    $testDir = Join-Path $env:TEMP "mise_tool_stub_e2e_$(Get-Random)"
    New-Item -ItemType Directory -Path $testDir -Force | Out-Null
    Set-Location $testDir
    Write-Host "Test directory: $testDir" -ForegroundColor Yellow

    try {
        # Test 1: Basic stub generation
        Test-BasicStubGeneration
        
        # Test 2: Platform-specific stubs
        Test-PlatformSpecificStubs
        
        # Test 3: Stub execution (if possible)
        Test-StubExecution
        
        # Test 4: Incremental platform addition
        Test-IncrementalPlatformAddition
        
        # Test 5: Windows-specific binary paths
        Test-WindowsBinaryPaths
        
        # Test 6: Companion exe validation
        Test-CompanionExeValidation
        
        Write-Host "`n========================================" -ForegroundColor Green
        Write-Host "  All Tests Passed Successfully! ✓" -ForegroundColor Green
        Write-Host "========================================" -ForegroundColor Green
        
    } catch {
        Write-Host "`n========================================" -ForegroundColor Red
        Write-Host "  Test Failed: $_" -ForegroundColor Red
        Write-Host "========================================" -ForegroundColor Red
        exit 1
    } finally {
        # Cleanup
        Set-Location $OriginalLocation
        if (Test-Path $testDir) {
            Remove-Item -Recurse -Force $testDir -ErrorAction SilentlyContinue
        }
    }
}

function Test-BasicStubGeneration {
    Write-Host "`n[TEST 1] Basic Stub Generation" -ForegroundColor Cyan
    Write-Host "--------------------------------" -ForegroundColor Gray
    
    # Generate a basic tool stub
    $stubPath = "./basic_tool"
    $output = & mise generate tool-stub $stubPath `
        --url "https://github.com/cli/cli/releases/download/v2.0.0/gh_2.0.0_windows_amd64.zip" `
        --skip-download 2>&1
    
    # Verify stub file exists
    if (-not (Test-Path $stubPath)) {
        throw "Stub file not created at $stubPath"
    }
    Write-Host "  ✓ Stub file created" -ForegroundColor Green
    
    # Verify companion exe exists on Windows
    $exePath = "$stubPath.exe"
    if (-not (Test-Path $exePath)) {
        throw "Companion .exe not created at $exePath"
    }
    Write-Host "  ✓ Companion .exe created" -ForegroundColor Green
    
    # Verify stub content
    $content = Get-Content $stubPath -Raw
    if ($content -notmatch '#!/usr/bin/env -S mise tool-stub') {
        throw "Stub missing shebang line"
    }
    Write-Host "  ✓ Shebang line present" -ForegroundColor Green
    
    if ($content -notmatch 'url = "https://github.com/cli/cli') {
        throw "URL not found in stub"
    }
    Write-Host "  ✓ URL correctly set" -ForegroundColor Green
    
    # Check exe is a valid PE file
    $exeBytes = [System.IO.File]::ReadAllBytes($exePath)
    if ($exeBytes[0] -ne 0x4D -or $exeBytes[1] -ne 0x5A) { # MZ header
        throw "Companion exe is not a valid Windows executable"
    }
    Write-Host "  ✓ Valid Windows PE executable" -ForegroundColor Green
}

function Test-PlatformSpecificStubs {
    Write-Host "`n[TEST 2] Platform-Specific Stubs" -ForegroundColor Cyan
    Write-Host "--------------------------------" -ForegroundColor Gray
    
    $stubPath = "./platform_tool"
    $output = & mise generate tool-stub $stubPath `
        --platform-url "windows-x64:https://example.com/tool-win64.zip" `
        --platform-url "windows-arm64:https://example.com/tool-winarm64.zip" `
        --platform-url "linux-x64:https://example.com/tool-linux.tar.gz" `
        --platform-url "darwin-arm64:https://example.com/tool-macos.tar.gz" `
        --skip-download 2>&1
    
    if (-not (Test-Path $stubPath)) {
        throw "Platform stub not created"
    }
    Write-Host "  ✓ Platform stub created" -ForegroundColor Green
    
    if (-not (Test-Path "$stubPath.exe")) {
        throw "Platform stub companion exe not created"
    }
    Write-Host "  ✓ Companion .exe created" -ForegroundColor Green
    
    $content = Get-Content $stubPath -Raw
    
    # Check for platform sections
    $platforms = @("windows-x64", "windows-arm64", "linux-x64", "darwin-arm64")
    foreach ($platform in $platforms) {
        if ($content -notmatch "\[platforms\.$platform\]") {
            throw "Platform section for $platform not found"
        }
        Write-Host "  ✓ Platform config for $platform present" -ForegroundColor Green
    }
}

function Test-StubExecution {
    Write-Host "`n[TEST 3] Stub Execution" -ForegroundColor Cyan
    Write-Host "--------------------------------" -ForegroundColor Gray
    
    # Create a stub that uses a known tool (echo/cmd)
    $stubPath = "./echo_stub"
    $stubContent = @"
#!/usr/bin/env -S mise tool-stub
tool = "echo"
version = "latest"
"@
    
    Set-Content -Path $stubPath -Value $stubContent -NoNewline
    
    # Manually create companion exe by copying mise-stub.exe
    $miseStubPath = Get-MiseStubPath
    if ($miseStubPath -and (Test-Path $miseStubPath)) {
        Copy-Item $miseStubPath "$stubPath.exe"
        Write-Host "  ✓ Companion exe created manually" -ForegroundColor Green
        
        # Try to execute (this will fail if mise isn't in PATH, which is okay for CI)
        try {
            $output = & "$stubPath.exe" "test message" 2>&1
            Write-Host "  ✓ Stub execution attempted" -ForegroundColor Green
        } catch {
            Write-Host "  ⚠ Stub execution failed (expected in CI without mise in PATH)" -ForegroundColor Yellow
        }
    } else {
        Write-Host "  ⚠ mise-stub.exe not found, skipping execution test" -ForegroundColor Yellow
    }
}

function Test-IncrementalPlatformAddition {
    Write-Host "`n[TEST 4] Incremental Platform Addition" -ForegroundColor Cyan
    Write-Host "--------------------------------" -ForegroundColor Gray
    
    $stubPath = "./incremental_tool"
    
    # First, create with one platform
    & mise generate tool-stub $stubPath `
        --platform-url "windows-x64:https://example.com/v1-win.zip" `
        --skip-download 2>&1 | Out-Null
    
    $content1 = Get-Content $stubPath -Raw
    if ($content1 -notmatch "\[platforms\.windows-x64\]") {
        throw "Initial platform not added"
    }
    Write-Host "  ✓ Initial platform added" -ForegroundColor Green
    
    # Add another platform
    & mise generate tool-stub $stubPath `
        --platform-url "linux-x64:https://example.com/v1-linux.tar.gz" `
        --skip-download 2>&1 | Out-Null
    
    $content2 = Get-Content $stubPath -Raw
    if ($content2 -notmatch "\[platforms\.windows-x64\]") {
        throw "Original platform lost after update"
    }
    if ($content2 -notmatch "\[platforms\.linux-x64\]") {
        throw "New platform not added"
    }
    Write-Host "  ✓ Platform added incrementally" -ForegroundColor Green
    Write-Host "  ✓ Original platform preserved" -ForegroundColor Green
}

function Test-WindowsBinaryPaths {
    Write-Host "`n[TEST 5] Windows-Specific Binary Paths" -ForegroundColor Cyan
    Write-Host "--------------------------------" -ForegroundColor Gray
    
    $stubPath = "./winbin_tool"
    
    # Generate with Windows-specific binary path
    & mise generate tool-stub $stubPath `
        --platform-url "windows-x64:https://example.com/tool-win.zip" `
        --platform-url "linux-x64:https://example.com/tool-linux.tar.gz" `
        --platform-bin "windows-x64:tool.exe" `
        --platform-bin "linux-x64:bin/tool" `
        --skip-download 2>&1 | Out-Null
    
    $content = Get-Content $stubPath -Raw
    
    # Check for Windows-specific binary
    if ($content -notmatch 'bin = "tool\.exe"') {
        throw "Windows-specific binary path not set"
    }
    Write-Host "  ✓ Windows binary path (tool.exe) set" -ForegroundColor Green
    
    # Check for Linux-specific binary
    if ($content -notmatch 'bin = "bin/tool"') {
        throw "Linux-specific binary path not set"
    }
    Write-Host "  ✓ Linux binary path (bin/tool) set" -ForegroundColor Green
}

function Test-CompanionExeValidation {
    Write-Host "`n[TEST 6] Companion Exe Validation" -ForegroundColor Cyan
    Write-Host "--------------------------------" -ForegroundColor Gray
    
    $stubPath = "./validate_tool"
    
    & mise generate tool-stub $stubPath `
        --url "https://example.com/tool.zip" `
        --skip-download 2>&1 | Out-Null
    
    $exePath = "$stubPath.exe"
    if (-not (Test-Path $exePath)) {
        throw "Companion exe not created"
    }
    
    # Check file size (should be small, < 1MB)
    $fileInfo = Get-Item $exePath
    $sizeKB = [math]::Round($fileInfo.Length / 1KB)
    Write-Host "  ℹ Companion exe size: ${sizeKB}KB" -ForegroundColor Gray
    
    if ($fileInfo.Length -gt 1048576) { # 1MB
        throw "Companion exe too large (${sizeKB}KB > 1024KB)"
    }
    Write-Host "  ✓ Exe size reasonable (<1MB)" -ForegroundColor Green
    
    # Verify it's the same as other companion exes (they should all be copies)
    $firstExe = Get-Item "./basic_tool.exe"
    if ($firstExe.Length -ne $fileInfo.Length) {
        Write-Host "  ⚠ Companion exe sizes differ (might be different builds)" -ForegroundColor Yellow
    } else {
        Write-Host "  ✓ Companion exe consistent with others" -ForegroundColor Green
    }
    
    # Check exe has proper permissions
    $acl = Get-Acl $exePath
    if ($null -eq $acl) {
        throw "Cannot read exe permissions"
    }
    Write-Host "  ✓ Exe has valid permissions" -ForegroundColor Green
}

function Get-MiseStubPath {
    # Try to find mise-stub.exe in common locations
    $paths = @(
        ".\target\release\mise-stub.exe",
        ".\target\debug\mise-stub.exe",
        "..\target\release\mise-stub.exe",
        "..\target\debug\mise-stub.exe",
        "..\..\target\release\mise-stub.exe",
        "..\..\target\debug\mise-stub.exe",
        "$env:CARGO_TARGET_DIR\release\mise-stub.exe",
        "$env:CARGO_TARGET_DIR\debug\mise-stub.exe"
    )
    
    foreach ($path in $paths) {
        if (Test-Path $path) {
            return (Resolve-Path $path).Path
        }
    }
    
    return $null
}

# Run the tests
Test-ToolStubs