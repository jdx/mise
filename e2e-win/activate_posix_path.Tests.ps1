Describe 'POSIX activation PATH on Windows' {
    It 'keeps <Runtime> usable while native mise updates PATH' -TestCases @(
        @{ Runtime = 'Git Bash'; Candidate = 'C:\Program Files\Git\bin\bash.exe' },
        @{ Runtime = 'raw Git Bash'; Candidate = 'C:\Program Files\Git\usr\bin\bash.exe' },
        @{ Runtime = 'MSYS2'; Candidate = "$(if ($env:MISE_TEST_MSYS_ROOT) { $env:MISE_TEST_MSYS_ROOT } else { 'C:\msys64' })\usr\bin\bash.exe" },
        @{ Runtime = 'Cygwin'; Candidate = "$(if ($env:MISE_TEST_CYGWIN_ROOT) { $env:MISE_TEST_CYGWIN_ROOT } else { 'C:\cygwin64' })\bin\bash.exe" }
    ) {
        param($Runtime, $Candidate)
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

        $bash = $Candidate
        if (-not (Test-PosixRuntimeBash $bash)) {
            Set-ItResult -Skipped -Because "$Runtime is not installed at $Candidate"
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
            'MISE_E2E_NATIVE_PROBE', 'MISE_TRUSTED_CONFIG_PATHS', 'PATH', 'SHELL', 'MSYSTEM') {
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
        $env:PATH = "$(Split-Path $bash);$env:PATH"
        Remove-Item Env:SHELL, Env:MSYSTEM -ErrorAction Ignore

        $probe = @'
set -eu
mise_exe=$(cygpath -u "$MISE_E2E_EXE")
apply_mise() {
  local code
  code=$("$mise_exe" "$@") || return $?
  eval "$code"
}
original_a=$(cygpath -u "$MISE_E2E_ORIGINAL_A")
original_b=$(cygpath -u "$MISE_E2E_ORIGINAL_B")
prepended=$(cygpath -u "$MISE_E2E_PREPENDED")
interleaved=$(cygpath -u "$MISE_E2E_INTERLEAVED")
appended=$(cygpath -u "$MISE_E2E_APPENDED")
root=$(cygpath -u "$MISE_E2E_ROOT")
nested=$(cygpath -u "$MISE_E2E_NESTED")
export PATH="$original_a:$original_b:$PATH"

apply_mise activate bash
mise --version >/dev/null
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
# Exercise the generated cd hook before a direct invocation could conceal failure.
root-marker.exe cmd.exe >/dev/null
apply_mise hook-env -s bash
root-marker.exe cmd.exe >/dev/null
printf 'ROOT_PATH=%s\n' "$PATH"
apply_mise env -s bash
root-marker.exe cmd.exe >/dev/null
printf 'ENV_PATH=%s\n' "$PATH"

cd "$nested"
nested-marker.exe cmd.exe >/dev/null
apply_mise hook-env -s bash
printf 'NESTED_PATH=%s\n' "$PATH"

cd "$root"
root-marker.exe cmd.exe >/dev/null
apply_mise hook-env -s bash
printf 'RETURN_PATH=%s\n' "$PATH"

powershell.exe -NoProfile -Command '(Get-Command root-marker.exe).Source' | sed 's/^/NATIVE_LOOKUP=/'
powershell.exe -NoProfile -File "$MISE_E2E_NATIVE_PROBE" | sed 's/^/NATIVE_PATH=/'
apply_mise deactivate
command -v rm >/dev/null
command -v cygpath >/dev/null
original-marker.exe cmd.exe >/dev/null
printf 'DEACTIVATED_PATH=%s\n' "$PATH"

apply_mise activate bash --shims
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
            ([regex]::Matches($rootPath, [regex]::Escape((Split-Path $originalA -Leaf))).Count) | Should -Be 2 -Because $rootPath
            # `mise env` computes a deduplicated environment; hook-env preserves the
            # live shell's user-owned duplicates. Pin these contracts separately.
            $envPath = ($output -split "`r?`n" | Where-Object { $_ -like 'ENV_PATH=*' } | Select-Object -Last 1)
            ([regex]::Matches($envPath, [regex]::Escape((Split-Path $originalA -Leaf))).Count) | Should -Be 1
            foreach ($line in ($output -split "`r?`n" | Where-Object { $_ -match '^(ROOT|ENV|NESTED|RETURN|DEACTIVATED|SHIMS)_PATH=' })) {
                $line | Should -Not -Match '(?:^|=|:)[A-Za-z]:[\\/]'
                $line | Should -Not -Match '::'
            }
            $output | Should -Match ('NATIVE_LOOKUP=' + [regex]::Escape((Join-Path $rootBin 'root-marker.exe')))
            $output | Should -Match 'SHIMS_PATH=[^\r\n]*/shims:'

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
