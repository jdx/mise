$config = New-PesterConfiguration
$config.Run.Path = $PSScriptRoot
$config.Run.Exit = $true
$config.TestResult.Enabled = $true

$env:MISE_DEBUG = "1"

Invoke-Pester -Configuration $config
