$ErrorActionPreference = "Stop"

function assert {
  param($condition, $message)
  if ($condition) {
    Write-Host "Test passed: $message"
  } else {
    Write-Host "Test failed: $message"
    exit 1
  }
}
