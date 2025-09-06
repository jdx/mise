param(
    [string]$TestName
)

$config = New-PesterConfiguration
$config.Run.Path = $PSScriptRoot
$config.Run.Exit = $true
$config.TestResult.Enabled = $true
$config.Output.Verbosity = 'Detailed'

if ($TestName) {
    $config.Filter.FullName = $TestName
}

$env:MISE_DEBUG = "1"
$env:PATH = "$PSScriptRoot\..\target\debug;$env:PATH"

# Run tests and capture results
$startTime = Get-Date
$result = Invoke-Pester -Configuration $config -PassThru
$endTime = Get-Date
$totalDuration = $endTime - $startTime

# Generate summary for GitHub Actions
if ($env:GITHUB_STEP_SUMMARY) {
    $summary = @"
## Windows E2E Test Results

| Metric | Value |
|--------|-------|
| **Total Tests** | $($result.TotalCount) |
| **Passed** | ✅ $($result.PassedCount) |
| **Failed** | ❌ $($result.FailedCount) |
| **Skipped** | ⏭️ $($result.SkippedCount) |
| **Duration** | $("{0:N2}" -f $totalDuration.TotalSeconds)s |

"@

    if ($result.Tests.Count -gt 0) {
        $summary += @"

### Test Details

| Test | Duration | Status |
|------|----------|--------|
"@
        foreach ($test in $result.Tests) {
            $status = if ($test.Passed) { "✅ Pass" } 
                      elseif ($test.Skipped) { "⏭️ Skip" } 
                      else { "❌ Fail" }
            $duration = "{0:N3}" -f $test.Duration.TotalSeconds
            $testName = $test.ExpandedName -replace '^.*\\', ''
            $summary += "| ``$testName`` | ${duration}s | $status |`n"
        }
    }

    if ($result.FailedCount -gt 0) {
        $summary += @"

### Failed Tests

"@
        foreach ($test in $result.Tests | Where-Object { -not $_.Passed -and -not $_.Skipped }) {
            $summary += @"
<details>
<summary>❌ $($test.ExpandedName)</summary>

\`\`\`
$($test.ErrorRecord)
\`\`\`

</details>

"@
        }
    }

    # Write to GitHub Actions summary
    $summary | Out-File -Append -FilePath $env:GITHUB_STEP_SUMMARY -Encoding UTF8
}

# Exit with appropriate code
exit $result.FailedCount
