$Version = (Get-Content -Path Cargo.toml | Select-String -Pattern '^version = "(.*)"' | ForEach-Object { $_.Matches.Groups[1].Value })
Write-Output $Version
