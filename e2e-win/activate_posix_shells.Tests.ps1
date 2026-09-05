Describe 'POSIX generated hooks on Windows' {
    It 'runs the generated <Language> hooks in <Runtime>' -TestCases @(
        @{ Runtime = 'MSYS2'; Language = 'zsh'; Root = "$(if ($env:MISE_TEST_MSYS_ROOT) { $env:MISE_TEST_MSYS_ROOT } else { 'C:\msys64' })"; Bin = 'usr\bin' },
        @{ Runtime = 'MSYS2'; Language = 'fish'; Root = "$(if ($env:MISE_TEST_MSYS_ROOT) { $env:MISE_TEST_MSYS_ROOT } else { 'C:\msys64' })"; Bin = 'usr\bin' },
        @{ Runtime = 'Cygwin'; Language = 'zsh'; Root = "$(if ($env:MISE_TEST_CYGWIN_ROOT) { $env:MISE_TEST_CYGWIN_ROOT } else { 'C:\cygwin64' })"; Bin = 'bin' },
        @{ Runtime = 'Cygwin'; Language = 'fish'; Root = "$(if ($env:MISE_TEST_CYGWIN_ROOT) { $env:MISE_TEST_CYGWIN_ROOT } else { 'C:\cygwin64' })"; Bin = 'bin' }
    ) {
        param($Runtime, $Language, $Root, $Bin)
        $shell = Join-Path $Root "$Bin\$Language.exe"
        if (-not (Test-Path $shell)) {
            Set-ItResult -Skipped -Because "$Runtime $Language is not installed"
            return
        }
        $fixture = Join-Path $TestDrive "$Runtime-$Language shell's project"
        foreach ($dir in 'outside', 'project\bin', 'original bin') {
            New-Item -ItemType Directory -Force (Join-Path $fixture $dir) | Out-Null
        }
        Copy-Item "$env:SystemRoot\System32\where.exe" (Join-Path $fixture 'project\bin\root-marker.exe')
        Copy-Item "$env:SystemRoot\System32\where.exe" (Join-Path $fixture 'original bin\original-marker.exe')
        "[env]`n_.path = ['bin']" | Set-Content (Join-Path $fixture 'project\mise.toml')
        $saved = @{}
        foreach ($name in 'MISE_TEST_EXE', 'MISE_TEST_ROOT', 'MISE_TRUSTED_CONFIG_PATHS', 'PATH', 'SHELL', 'MSYSTEM') {
            $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        }
        $env:MISE_TEST_EXE = Join-Path $fixture 'mise.exe'
        Copy-Item (Get-Command mise.exe).Source $env:MISE_TEST_EXE
        $env:MISE_TEST_ROOT = $fixture
        $env:MISE_TRUSTED_CONFIG_PATHS = $TestDrive
        $env:PATH = "$(Join-Path $Root $Bin);$env:PATH"
        # Deliberately omit login-shell hints; the actual caller establishes the runtime.
        Remove-Item Env:SHELL, Env:MSYSTEM -ErrorAction Ignore
        $zsh = @'
set -eu
exe=$(cygpath -u "$MISE_TEST_EXE")
root=$(cygpath -u "$MISE_TEST_ROOT")
export PATH="$root/original bin:$PATH"
cd "$root/outside"
code=$("$exe" activate zsh)
eval "$code"
mise --version >/dev/null
original-marker.exe cmd.exe >/dev/null
cd "$root/project"
root-marker.exe cmd.exe >/dev/null
printf 'PROJECT_PATH=%s\n' "$PATH"
powershell.exe -NoProfile -Command '(Get-Command root-marker.exe).Source'
mise deactivate
original-marker.exe cmd.exe >/dev/null
if command -v root-marker.exe >/dev/null; then exit 1; fi
code=$("$exe" activate zsh --shims)
eval "$code"
printf 'SHIMS_PATH=%s\n' "$PATH"
printf 'COMPLETE=ok\n'
'@
        $fish = @'
set exe (cygpath -u "$MISE_TEST_EXE")
set root (cygpath -u "$MISE_TEST_ROOT")
set -gx PATH "$root/original bin" $PATH
cd "$root/outside"
set code (command $exe activate fish)
test $status -eq 0; or exit 1
printf '%s\n' $code | source
mise --version >/dev/null; or exit 1
original-marker.exe cmd.exe >/dev/null; or exit 1
cd "$root/project"
root-marker.exe cmd.exe >/dev/null; or exit 1
printf 'PROJECT_PATH=%s\n' (string join : $PATH)
powershell.exe -NoProfile -Command '(Get-Command root-marker.exe).Source'
mise deactivate
original-marker.exe cmd.exe >/dev/null; or exit 1
if command -q root-marker.exe; exit 1; end
set code (command $exe activate fish --shims)
test $status -eq 0; or exit 1
printf '%s\n' $code | source
printf 'SHIMS_PATH=%s\n' (string join : $PATH)
printf 'COMPLETE=ok\n'
'@
        try {
            [string[]]$shellArgs = if ($Language -eq 'fish') { @('--no-config', '-c', $fish) } else { @('-f', '-c', $zsh) }
            $output = & $shell @shellArgs 2>&1 | Out-String
            $LASTEXITCODE | Should -Be 0 -Because $output
            $output | Should -Match 'COMPLETE=ok'
            $output | Should -Not -Match 'command not found|Unknown command'
            $output | Should -Match ([regex]::Escape((Join-Path $fixture 'project\bin\root-marker.exe')))
            $output | Should -Match 'SHIMS_PATH=[^\r\n]*/shims:'
            foreach ($line in ($output -split "`r?`n" | Where-Object { $_ -match '^(PROJECT|SHIMS)_PATH=' })) {
                $line | Should -Not -Match '(?:^|=|:)[A-Za-z]:[\\/]|;|::'
            }
        }
        finally {
            foreach ($name in $saved.Keys) {
                [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process')
            }
        }
    }

    It 'leaves native callers unchanged despite inherited POSIX hints' {
        $saved = @{}
        foreach ($name in 'SHELL', 'MSYSTEM', 'WSL_DISTRO_NAME') {
            $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        }
        try {
            $env:SHELL = 'C:\Program Files\Git\usr\bin\bash.exe'
            $env:MSYSTEM = 'MINGW64'
            $env:WSL_DISTRO_NAME = 'inherited-marker'
            $output = mise activate bash --shims | Out-String
            $LASTEXITCODE | Should -Be 0
            $output | Should -Match '[A-Za-z]:[\\/]'
        }
        finally {
            foreach ($name in $saved.Keys) {
                [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process')
            }
        }
    }
}
