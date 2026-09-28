param([string]$FixtureRoot, [string]$HostExe, [string]$ScenarioPath, [string]$Case)

function Invoke-LookupScenario {
    param([string]$Root, [scriptblock]$Scenario, [string]$Case)
    $Root = Join-Path $Root ([Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force -Path $Root | Out-Null
    $scenarioFile = Join-Path $Root 'scenario.ps1'
    $Scenario.ToString() | Set-Content -LiteralPath $scenarioFile
    $hostExe = (Resolve-Path target/debug/mise.exe).Path
    $output = & pwsh -NoProfile -NonInteractive -File $PSCommandPath -FixtureRoot $Root -HostExe $hostExe -ScenarioPath $scenarioFile -Case $Case 2>&1 | Out-String
    if ($LASTEXITCODE -ne 0) { throw $output }
}

function Assert-Lookup {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}

function Get-LookupResult {
    param([string]$Command, [string[]]$Arguments = @())
    $consoleError = [Console]::Error
    $capturedError = [IO.StringWriter]::new()
    try {
        [Console]::SetError($capturedError)
        $output = & $Command @Arguments 2>&1 | Out-String
    } finally {
        [Console]::SetError($consoleError)
    }
    $output += $capturedError.ToString()
    [pscustomobject]@{ Output = $output; ExitCode = $LASTEXITCODE }
}

function Assert-LookupOutput {
    param([string]$Command, [string]$Expected, [string[]]$Arguments = @())
    $result = Get-LookupResult $Command $Arguments
    Assert-Lookup ($result.ExitCode -eq 0 -and $result.Output -match $Expected) "Expected $Command to succeed with '$Expected': $($result.Output)"
}

function Assert-LookupFailure {
    param([string]$Command, [string]$Diagnostic, [string[]]$Arguments = @())
    $result = Get-LookupResult $Command $Arguments
    Assert-Lookup ($result.ExitCode -ne 0 -and $result.Output -match $Diagnostic -and $result.Output -notmatch 'GLOBAL_FALLBACK') "Expected $Command to fail with '$Diagnostic' without global fallback: $($result.Output)"
}

function New-LookupGlobals {
    param([string[]]$Names)
    $globalBin = Join-Path $root 'global-bin'
    New-Item -ItemType Directory -Force -Path $globalBin | Out-Null
    foreach ($name in $Names) {
        "@echo off`necho GLOBAL_FALLBACK" | Set-Content -LiteralPath (Join-Path $globalBin "$name.cmd")
    }
    $env:PATH = "$globalBin;$env:PATH"
}

function New-LookupBin {
    param([string]$Name = '', [switch]$Missing)
    $checkout = if ($Name) { Join-Path $root $Name } else { $root }
    $bin = Join-Path $checkout 'tools/bin'
    New-Item -ItemType Directory -Force -Path $bin | Out-Null
    if (-not $Missing) { Copy-Item -LiteralPath $hostExe -Destination (Join-Path $bin 'mise.exe') }
    $bin
}

function Set-LookupConfig {
    param([string]$Content, [string]$Name = '')
    $checkout = if ($Name) { Join-Path $root $Name } else { $root }
    $Content | Set-Content -LiteralPath (Join-Path $checkout 'mise.toml')
}

function New-LookupPathCheckout {
    param([string]$Name, [string]$Mark)
    $bin = New-LookupBin $Name
    Set-LookupConfig @"
[settings]
activate_mise_lookup = "env_path"
[tools]
node = { version = "system", lazy_bins = ["lookup-tool"] }
[env]
_.path = ["{{config_root}}/tools/bin"]
LOOKUP_MARK = "$Mark"
"@ $Name
    @'
@echo off
if "%~1"=="--exit-42" exit /b 42
pwsh -NoProfile -File "%~dp0path-probe.ps1"
'@ | Set-Content -LiteralPath (Join-Path $bin 'lookup-tool.cmd')
    @'
$entries = $env:PATH.Split(';') | Where-Object { $_ }
$bin = [IO.Path]::GetFullPath($PSScriptRoot)
$seen = $entries | Where-Object { [IO.Path]::GetFullPath($_) -eq $bin }
if ($entries.Count -le 1 -or -not $seen -or $env:PATH -match '(^|;)/[a-z]/' -or -not $env:LOOKUP_MARK) { throw 'The native grandchild did not receive the checkout environment and a valid Windows PATH.' }
"TOOL_$env:LOOKUP_MARK"
'@ | Set-Content -LiteralPath (Join-Path $bin 'path-probe.ps1')
}

function New-LookupArchive {
    param([string]$Name, [string[]]$Commands = @())
    $payload = Join-Path $root "$Name-payload"
    New-Item -ItemType Directory -Force -Path $payload | Out-Null
    foreach ($name in $Commands) { Copy-Item -LiteralPath $env:ComSpec -Destination (Join-Path $payload "$name.exe") }
    if (-not $Commands) { 'The declared executable is absent from this archive.' | Set-Content -LiteralPath (Join-Path $payload 'placeholder.txt') }
    $archive = Join-Path $root "$Name.zip"
    Compress-Archive -Path (Join-Path $payload '*') -DestinationPath $archive
    ([Uri]$archive).AbsoluteUri
}

function New-LookupLauncher {
    param([string]$Path, [string]$Source)
    $sourceFile = [IO.Path]::ChangeExtension($Path, 'cs')
    $Source | Set-Content -LiteralPath $sourceFile
    $compiler = Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
    & $compiler /nologo /target:exe "/out:$Path" $sourceFile | Out-Null
    Assert-Lookup ($LASTEXITCODE -eq 0) 'The selected mise launcher fixture could not be compiled.'
}

if ($ScenarioPath) {
    Get-ChildItem Env: | Where-Object { $_.Name -like 'MISE_*' -or $_.Name -like '__MISE_*' } | ForEach-Object { Remove-Item -LiteralPath ('Env:' + $_.Name) }
    $root = $FixtureRoot
    $env:MISE_TRUSTED_CONFIG_PATHS = $root
    $env:MISE_CEILING_PATHS = Split-Path $root -Parent
    $env:MISE_CONFIG_DIR = Join-Path $root 'config'
    $env:MISE_SYSTEM_CONFIG_DIR = Join-Path $root 'system-config'
    $env:MISE_DATA_DIR = Join-Path $root 'data'
    $env:MISE_STATE_DIR = Join-Path $root 'state'
    $env:MISE_CACHE_DIR = Join-Path $root 'cache'
    $env:MISE_TASK_CACHE_DIR = Join-Path $root 'task-cache'
    $env:MISE_AUTO_INSTALL = '1'
    $env:MISE_OFFLINE = '1'
    $PSNativeCommandUseErrorActionPreference = $false
    Set-Location -LiteralPath $root
    try {
        & $ScenarioPath
        exit 0
    } catch {
        Write-Error $_
        exit 1
    }
}
