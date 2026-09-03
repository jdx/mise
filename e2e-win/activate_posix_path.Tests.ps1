Describe 'POSIX activation PATH on Windows' {
    It 'keeps Git Bash usable while native mise updates PATH' {
        function Test-PosixRuntimeBash([string] $Path) {
            if (-not $Path -or -not (Test-Path $Path)) {
                return $false
            }
            $ancestor = (Get-Item $Path).Directory
            for ($depth = 0; $depth -lt 5 -and $ancestor; $depth++) {
                if ((Test-Path (Join-Path $ancestor.FullName 'usr/bin/msys-2.0.dll')) -or
                    (Test-Path (Join-Path $ancestor.FullName 'bin/cygwin1.dll'))) {
                    return $true
                }
                $ancestor = $ancestor.Parent
            }
            return $false
        }

        $bashCandidates = @(
            'C:\Program Files\Git\bin\bash.exe',
            'C:\Program Files (x86)\Git\bin\bash.exe',
            "$env:LOCALAPPDATA\Programs\Git\bin\bash.exe",
            'C:\msys64\usr\bin\bash.exe',
            'C:\msys32\usr\bin\bash.exe'
        ) + @(Get-Command bash.exe -All -ErrorAction SilentlyContinue | ForEach-Object { $_.Source })
        $wslLauncher = Join-Path $env:SystemRoot 'System32\bash.exe'
        $bash = $bashCandidates |
            Where-Object { $_ -and ($_ -ne $wslLauncher) -and (Test-PosixRuntimeBash $_) } |
            Select-Object -First 1
        if (-not $bash) {
            Set-ItResult -Skipped -Because 'Git Bash/MSYS2 is not installed'
            return
        }

        $root = Join-Path $TestDrive 'project root'
        $nested = Join-Path $root 'nested'
        $originalA = Join-Path $TestDrive 'original a bin'
        $originalB = Join-Path $TestDrive 'original b bin'
        $prepended = Join-Path $TestDrive 'prepended bin'
        $interleaved = Join-Path $TestDrive 'interleaved bin'
        $appended = Join-Path $TestDrive 'appended bin'
        $rootBin = Join-Path $root 'bin'
        $nestedBin = Join-Path $nested 'bin'
        foreach ($dir in $root, $nested, $originalA, $originalB, $prepended, $interleaved, $appended,
            $rootBin, $nestedBin) {
            New-Item -ItemType Directory -Path $dir -Force | Out-Null
        }

        # Real native executables make these functional command-resolution checks, not output-shape checks.
        $where = Join-Path $env:SystemRoot 'System32\where.exe'
        Copy-Item $where (Join-Path $originalA 'original-marker.exe')
        Copy-Item $where (Join-Path $rootBin 'root-marker.exe')
        Copy-Item $where (Join-Path $nestedBin 'nested-marker.exe')
        @'
[env]
_.path = ["bin"]
'@ | Out-File (Join-Path $root 'mise.toml') -Encoding utf8NoBOM
        @'
[env]
_.path = ["bin"]
'@ | Out-File (Join-Path $nested 'mise.toml') -Encoding utf8NoBOM
        @'
Write-Output $env:PATH
'@ | Out-File (Join-Path $TestDrive 'native-path.ps1') -Encoding utf8NoBOM

        $saved = @{}
        foreach ($name in 'MISE_E2E_EXE', 'MISE_E2E_ROOT', 'MISE_E2E_NESTED', 'MISE_E2E_ORIGINAL_A',
            'MISE_E2E_ORIGINAL_B', 'MISE_E2E_PREPENDED', 'MISE_E2E_INTERLEAVED', 'MISE_E2E_APPENDED',
            'MISE_E2E_NATIVE_PROBE', 'MISE_TRUSTED_CONFIG_PATHS') {
            $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        }
        $env:MISE_E2E_EXE = (Get-Command mise.exe).Source
        $env:MISE_E2E_ROOT = $root
        $env:MISE_E2E_NESTED = $nested
        $env:MISE_E2E_ORIGINAL_A = $originalA
        $env:MISE_E2E_ORIGINAL_B = $originalB
        $env:MISE_E2E_PREPENDED = $prepended
        $env:MISE_E2E_INTERLEAVED = $interleaved
        $env:MISE_E2E_APPENDED = $appended
        $env:MISE_E2E_NATIVE_PROBE = Join-Path $TestDrive 'native-path.ps1'
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive

        $probe = @'
set -e
original_a=$(cygpath -u "$MISE_E2E_ORIGINAL_A")
original_b=$(cygpath -u "$MISE_E2E_ORIGINAL_B")
prepended=$(cygpath -u "$MISE_E2E_PREPENDED")
interleaved=$(cygpath -u "$MISE_E2E_INTERLEAVED")
appended=$(cygpath -u "$MISE_E2E_APPENDED")
root=$(cygpath -u "$MISE_E2E_ROOT")
nested=$(cygpath -u "$MISE_E2E_NESTED")
export PATH="$original_a:$original_b:$PATH"

eval "$("$MISE_E2E_EXE" activate bash)"
original-marker.exe cmd.exe >/dev/null
rest=
IFS=: read -ra path_entries <<< "$PATH"
for entry in "${path_entries[@]}"; do
  if [[ $entry != "$original_a" && $entry != "$original_b" ]]; then
    rest=${rest:+$rest:}$entry
  fi
done
# Reorder the two pristine entries, interleave a user entry, and duplicate one pristine entry.
export PATH="$prepended:$original_b:$interleaved:$original_a:$rest:$original_a:$appended"

cd "$root"
eval "$("$MISE_E2E_EXE" hook-env -s bash)"
root-marker.exe cmd.exe >/dev/null
eval "$("$MISE_E2E_EXE" env -s bash)"
root-marker.exe cmd.exe >/dev/null
printf 'ROOT_PATH=%s\n' "$PATH"

cd "$nested"
eval "$("$MISE_E2E_EXE" hook-env -s bash)"
nested-marker.exe cmd.exe >/dev/null
printf 'NESTED_PATH=%s\n' "$PATH"

cd "$root"
eval "$("$MISE_E2E_EXE" hook-env -s bash)"
root-marker.exe cmd.exe >/dev/null
printf 'RETURN_PATH=%s\n' "$PATH"

powershell.exe -NoProfile -File "$MISE_E2E_NATIVE_PROBE" | sed 's/^/NATIVE_PATH=/'
eval "$("$MISE_E2E_EXE" deactivate)"
command -v rm >/dev/null
command -v cygpath >/dev/null
original-marker.exe cmd.exe >/dev/null
printf 'DEACTIVATED_PATH=%s\n' "$PATH"

eval "$("$MISE_E2E_EXE" activate bash --shims)"
printf 'SHIMS_PATH=%s\n' "$PATH"
'@

        try {
            $output = & $bash --noprofile --norc -c $probe 2>&1 | Out-String
            $LASTEXITCODE | Should -Be 0 -Because $output
            foreach ($label in 'ROOT_PATH', 'NESTED_PATH', 'RETURN_PATH', 'DEACTIVATED_PATH', 'SHIMS_PATH') {
                $line = ($output -split "`r?`n" | Where-Object { $_ -like "$label=*" } | Select-Object -Last 1)
                $line | Should -Not -BeNullOrEmpty
                $line | Should -Not -Match ';'
            }
            $rootPath = ($output -split "`r?`n" | Where-Object { $_ -like 'ROOT_PATH=*' } | Select-Object -Last 1)
            $rootPath.IndexOf((Split-Path $prepended -Leaf)) | Should -BeLessThan $rootPath.IndexOf((Split-Path $originalB -Leaf))
            $rootPath.IndexOf((Split-Path $originalB -Leaf)) | Should -BeLessThan $rootPath.IndexOf((Split-Path $originalA -Leaf))
            $rootPath | Should -Match ([regex]::Escape((Split-Path $interleaved -Leaf)))
            $rootPath.IndexOf((Split-Path $interleaved -Leaf)) | Should -BeLessThan $rootPath.IndexOf((Split-Path $appended -Leaf))
            ([regex]::Matches($rootPath, [regex]::Escape((Split-Path $originalA -Leaf))).Count) | Should -Be 2

            $nativePath = ($output -split "`r?`n" | Where-Object { $_ -like 'NATIVE_PATH=*' } | Select-Object -Last 1)
            $nativePath | Should -Not -BeNullOrEmpty
            $nativePath | Should -Match ';'
            $nativePath | Should -Match ([regex]::Escape($originalA))
            $nativePath | Should -Match ([regex]::Escape($originalB))
        }
        finally {
            foreach ($name in $saved.Keys) {
                if ($null -eq $saved[$name]) {
                    Remove-Item "Env:\$name" -ErrorAction SilentlyContinue
                } else {
                    [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process')
                }
            }
        }
    }
}
