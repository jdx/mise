param(
    [string]$TestName
)

$config = New-PesterConfiguration
$config.Run.Path = $PSScriptRoot
$config.Run.Exit = $true
$config.TestResult.Enabled = $true

if ($TestName) {
    $config.Filter.FullName = $TestName
}

$env:MISE_DEBUG = "1"

Invoke-Pester -Configuration $config
