. "$PSScriptRoot/helpers/mise_lookup.ps1"

Describe 'checkout-selected mise activation' {
    It 'keeps ordinary default-mode commands available after an untrusted refresh' {
        Invoke-LookupScenario $TestDrive {
            $env:MISE_TRUSTED_CONFIG_PATHS = Join-Path $root 'trusted'
            $project = Join-Path $root 'untrusted'
            New-Item -ItemType Directory -Path $project | Out-Null
            Set-LookupConfig @'
[settings]
activate_mise_lookup = "self"
[env]
LOOKUP_MARK = "TRUSTED"
'@ 'untrusted'
            (& $hostExe activate pwsh) | Out-String | Invoke-Expression
            Set-Location $project
            _mise_hook --force 2>$null
            Assert-Lookup (-not $env:__MISE_LOOKUP_ERROR) 'Default activation must not create lookup failure state.'
            Assert-LookupOutput mise 'Mark a config file as trusted' @('trust', '--help')
            mise trust
            _mise_hook --force
            Assert-Lookup ($env:LOOKUP_MARK -eq 'TRUSTED') 'Default activation must recover after trusting the checkout.'
            mise deactivate
        }
    }

    It 'guards first-entry commands when the selected mise is <Case>' -TestCases @(
        @{ Case = 'missing' }
        @{ Case = 'unavailable' }
    ) {
        param($Case)
        Invoke-LookupScenario $TestDrive -Case $Case -Scenario {
            New-LookupGlobals @('node', 'npm', 'npx', 'probe-tool', 'array-tool', 'wrapped-tool', 'excluded-tool', 'unknown-tool')
            $bin = New-LookupBin 'checkout' -Missing
            Set-LookupConfig @'
[settings]
activate_mise_lookup = "env_path"
shims.exclude = ["excluded-tool"]
[tools]
node = "24"
"http:probe" = { version = "1", lazy = true, lazy_bins = ["probe-tool", "excluded-tool"] }
"http:multi" = [{ version = "1", lazy_bins = ["array-tool"] }, { version = "2" }]
[wrappers.wrapped-tool]
command = "unused-wrapper"
[env]
_.path = ["{{config_root}}/tools/bin"]
LOOKUP_MARK = "CHILD_ONLY"
'@ 'checkout'
            if ($Case -eq 'unavailable') {
                New-LookupLauncher (Join-Path $bin 'mise.exe') @'
using System;
class Launcher {
    static int Main() {
        Console.Error.WriteLine("Selected mise download failed");
        return 1;
    }
}
'@
            }
            Assert-LookupOutput probe-tool 'GLOBAL_FALLBACK'
            (& $hostExe activate pwsh) | Out-String | Invoke-Expression
            Set-Location (Join-Path $root 'checkout')
            _mise_hook
            $diagnostic = if ($Case -eq 'missing') { 'no executable mise' } else { 'Selected mise download failed' }
            Assert-Lookup ($env:__MISE_LOOKUP_ERROR -match $diagnostic -and -not $env:__MISE_LOOKUP_EXE -and -not $env:LOOKUP_MARK) 'First-entry failure must clear selection and keep project environment out of the parent.'
            foreach ($name in 'probe-tool', 'node', 'npm', 'npx', 'array-tool', 'wrapped-tool') {
                Assert-LookupFailure $name $diagnostic
            }
            Assert-LookupFailure $env:ComSpec $diagnostic @('/c', 'probe-tool')
            Assert-LookupOutput excluded-tool 'GLOBAL_FALLBACK'
            Assert-LookupOutput unknown-tool 'GLOBAL_FALLBACK'
            $shim = (Get-Command probe-tool).Source
            Remove-Item -LiteralPath $shim, ([IO.Path]::ChangeExtension($shim, 'lookup'))
            _mise_hook
            Assert-LookupFailure probe-tool $diagnostic
            _mise_hook
            Assert-LookupFailure probe-tool $diagnostic
            Assert-Lookup (-not (Test-Path (Join-Path $env:MISE_DATA_DIR 'installs'))) 'The host must not install tools to create first-entry guards.'
            Set-Location $root
            _mise_hook
            Assert-LookupOutput probe-tool 'GLOBAL_FALLBACK'
            Set-Location (Join-Path $root 'checkout')
            _mise_hook
            Copy-Item -LiteralPath $hostExe -Destination (Join-Path $bin 'mise.exe') -Force
            Set-LookupConfig @'
[settings]
activate_mise_lookup = "env_path"
[tools]
node = { version = "system", lazy_bins = ["probe-tool"] }
[env]
_.path = ["{{config_root}}/tools/bin"]
'@ 'checkout'
            '@echo off' + [Environment]::NewLine + 'echo SELECTED_TOOL' | Set-Content -LiteralPath (Join-Path $bin 'probe-tool.cmd')
            _mise_hook
            Assert-LookupOutput probe-tool 'SELECTED_TOOL'
            Assert-LookupOutput array-tool 'GLOBAL_FALLBACK'
            mise deactivate
            Assert-LookupOutput probe-tool 'GLOBAL_FALLBACK'
        }
    }

    It 'preserves native child PATH, checkout selection, exit status, and dispatcher repair' {
        Invoke-LookupScenario $TestDrive {
            New-LookupGlobals @('lookup-tool')
            New-LookupPathCheckout 'a' 'A'
            New-LookupPathCheckout 'b' 'B'
            (& $hostExe activate pwsh) | Out-String | Invoke-Expression
            Set-Location (Join-Path $root 'a')
            _mise_hook --force
            Assert-Lookup (-not $env:LOOKUP_MARK) 'The parent shell must receive only project PATH changes.'
            Assert-LookupOutput lookup-tool 'TOOL_A'
            $result = Get-LookupResult lookup-tool @('--exit-42')
            Assert-Lookup ($result.ExitCode -eq 42) 'A dispatcher must preserve the selected command exit status.'
            $shim = (Get-Command lookup-tool).Source
            Assert-Lookup ($shim -match 'lookup-shims') 'A bare tool must use a native lookup dispatcher.'
            Remove-Item -LiteralPath $shim
            _mise_hook
            Assert-LookupOutput lookup-tool 'TOOL_A'
            Remove-Item -LiteralPath ([IO.Path]::ChangeExtension($shim, 'lookup'))
            Assert-LookupFailure lookup-tool 'metadata is missing'
            _mise_hook
            Assert-LookupOutput lookup-tool 'TOOL_A'
            Assert-LookupOutput $env:ComSpec '^2026\.' @('/c', 'mise --version')
            Assert-LookupFailure $env:ComSpec 'requires activate_mise_lookup' @('/c', ('cd /d "' + $root + '" && mise --version'))
            Assert-LookupOutput $env:ComSpec 'TOOL_B' @('/c', ('cd /d "' + (Join-Path $root 'b') + '" && lookup-tool'))
            Set-Location (Join-Path $root 'b')
            _mise_hook
            Assert-LookupOutput lookup-tool 'TOOL_B'
            Remove-Item -LiteralPath (Join-Path $root 'b/tools/bin/mise.exe')
            Assert-LookupFailure $env:ComSpec 'no executable mise' @('/c', 'mise --version')
            _mise_hook
            Assert-LookupFailure lookup-tool 'no executable mise'
            Assert-Lookup ($env:PATH.Split(';') -notcontains (Join-Path $root 'b/tools/bin')) 'A failed refresh must remove the stale checkout path.'
            _mise_hook
            Assert-LookupFailure lookup-tool 'no executable mise'
            Assert-LookupOutput $env:ComSpec 'TOOL_A' @('/c', ('cd /d "' + (Join-Path $root 'a') + '" && lookup-tool'))
            Set-Location $root
            _mise_hook
            Assert-Lookup (-not $env:__MISE_LOOKUP_EXE -and $env:PATH -notmatch 'lookup-shims') 'Leaving a checkout must remove lookup state and dispatchers.'
            Assert-LookupOutput lookup-tool 'GLOBAL_FALLBACK'
            mise deactivate
        }
    }

    It 'cleans up mode changes, invalid settings, and untrusted lookup configuration' {
        Invoke-LookupScenario $TestDrive {
            New-LookupPathCheckout 'checkout' 'A'
            (& $hostExe activate pwsh) | Out-String | Invoke-Expression
            Set-Location (Join-Path $root 'checkout')
            _mise_hook --force
            $configPath = Join-Path $root 'checkout/mise.toml'
            $original = Get-Content -LiteralPath $configPath -Raw
            Set-LookupConfig ($original.Replace('"env_path"', '"self"')) 'checkout'
            _mise_hook
            Assert-Lookup ($env:LOOKUP_MARK -eq 'A' -and -not $env:__MISE_LOOKUP_EXE -and $env:PATH -notmatch 'lookup-shims') 'Self activation must restore the full project environment and remove dispatchers.'
            Set-LookupConfig $original 'checkout'
            _mise_hook
            Assert-Lookup (-not $env:LOOKUP_MARK -and $env:__MISE_LOOKUP_EXE) 'Returning to lookup activation must remove the parent project environment.'
            Set-LookupConfig ($original.Replace('"env_path"', '"invalid"')) 'checkout'
            _mise_hook
            Assert-LookupFailure lookup-tool 'activate_mise_lookup'
            Assert-Lookup (-not $env:__MISE_LOOKUP_EXE -and -not $env:LOOKUP_MARK) 'Invalid settings must remove the previous selection and project environment.'
            Set-LookupConfig $original 'checkout'
            _mise_hook
            $env:MISE_TRUSTED_CONFIG_PATHS = Join-Path $root 'elsewhere'
            & $hostExe trust --untrust $configPath
            Set-LookupConfig ($original.Replace('"env_path"', '"self"')) 'checkout'
            _mise_hook
            Assert-LookupFailure mise 'not trusted' @('--version')
            Assert-LookupFailure lookup-tool 'requires activate_mise_lookup'
            Assert-Lookup (-not $env:__MISE_LOOKUP_EXE) 'A failed transition to self activation must retain guards and clear the selected executable.'
            Set-LookupConfig $original 'checkout'
            _mise_hook
            Assert-LookupFailure lookup-tool 'not trusted'
            Assert-Lookup (-not $env:__MISE_LOOKUP_EXE) 'Untrusted lookup configuration must clear the selected executable.'
            mise deactivate
            Assert-Lookup (-not (Get-Command mise -CommandType Function -ErrorAction SilentlyContinue)) 'Deactivation must remove the shell function even after failed refreshes.'
        }
    }

    It 'rejects an incompatible selected mise and enforces its minimum version' {
        Invoke-LookupScenario $TestDrive {
            $bin = New-LookupBin -Missing
            Copy-Item -LiteralPath $env:ComSpec -Destination (Join-Path $bin 'mise.exe')
            $config = @'
[settings]
activate_mise_lookup = "env_path"
[env]
_.path = ["{{config_root}}/tools/bin"]
'@
            Set-LookupConfig $config
            (& $hostExe activate pwsh) | Out-String | Invoke-Expression
            Assert-LookupFailure mise 'update the pinned mise version' @('--version')
            Copy-Item -LiteralPath $hostExe -Destination (Join-Path $bin 'mise.exe') -Force
            Set-LookupConfig ('min_version = "9999.0.0"' + [Environment]::NewLine + $config)
            _mise_hook
            Assert-LookupFailure mise '9999.0.0' @('--version')
            mise deactivate
        }
    }

    It 'remembers configured executables across damage and cache clearing' {
        Invoke-LookupScenario $TestDrive {
            $bin = New-LookupBin
            New-LookupGlobals @('probe-tool', 'extra-tool', 'broken-tool', 'eager-extra')
            $url = New-LookupArchive 'probe' @('probe-tool', 'extra-tool')
            $missingUrl = New-LookupArchive 'missing'
            Set-LookupConfig @"
[settings]
activate_mise_lookup = "env_path"
[tools]
"http:probe-tool" = { version = "1", url = "$url", lazy = true, lazy_bins = ["probe-tool"] }
[env]
_.path = ["{{config_root}}/tools/bin"]
"@
            (& $hostExe activate pwsh) | Out-String | Invoke-Expression
            Assert-LookupOutput probe-tool 'SELECTED_PROVIDER' @('/d', '/c', 'echo SELECTED_PROVIDER')
            _mise_hook
            Assert-LookupOutput extra-tool 'DISCOVERED_PROVIDER' @('/d', '/c', 'echo DISCOVERED_PROVIDER')
            Assert-LookupOutput mise 'UNRELATED_SYSTEM' @('exec', '--', 'cmd', '/d', '/c', 'echo UNRELATED_SYSTEM')
            $installed = (& $hostExe where 'http:probe-tool').Trim()
            Remove-Item -LiteralPath (Join-Path $installed 'probe-tool.exe'), (Join-Path $installed 'extra-tool.exe')
            foreach ($name in 'probe-tool', 'extra-tool') {
                Assert-LookupFailure $name 'does not provide executable'
                Assert-LookupFailure (Join-Path $bin 'mise.exe') 'does not provide executable' @('exec', '--', $name)
            }
            mise cache clear
            Assert-LookupFailure (Join-Path $bin 'mise.exe') 'does not provide executable' @('exec', '--', 'extra-tool')
            _mise_hook
            Assert-LookupFailure extra-tool 'does not provide executable'
            Set-LookupConfig @"
[settings]
activate_mise_lookup = "env_path"
[tools]
"http:broken-tool" = { version = "1", url = "$missingUrl", lazy = true, lazy_bins = ["broken-tool"] }
[env]
_.path = ["{{config_root}}/tools/bin"]
"@
            _mise_hook
            Assert-LookupFailure broken-tool 'does not provide executable'
            Assert-LookupFailure (Join-Path $bin 'mise.exe') 'does not provide executable' @('exec', '--', 'broken-tool')
            $eagerUrl = New-LookupArchive 'eager' @('eager-extra')
            Set-LookupConfig @"
[settings]
activate_mise_lookup = "env_path"
[tools]
"http:eager-provider" = { version = "1", url = "$eagerUrl" }
[env]
_.path = ["{{config_root}}/tools/bin"]
"@
            & (Join-Path $bin 'mise.exe') install
            $installed = (& $hostExe where 'http:eager-provider').Trim()
            Remove-Item -LiteralPath (Join-Path $installed 'eager-extra.exe')
            Assert-LookupFailure (Join-Path $bin 'mise.exe') 'does not provide executable' @('exec', '--', 'eager-extra')
            mise deactivate
        }
    }

    It 'discovers new installed commands after reshim without changing configuration' {
        Invoke-LookupScenario $TestDrive {
            $null = New-LookupBin
            $url = New-LookupArchive 'probe' @('probe-tool')
            Set-LookupConfig @"
[settings]
activate_mise_lookup = "env_path"
[tools]
"http:probe-tool" = { version = "1", url = "$url", lazy = true, lazy_bins = ["probe-tool"] }
[env]
_.path = ["{{config_root}}/tools/bin"]
"@
            (& $hostExe activate pwsh) | Out-String | Invoke-Expression
            Assert-LookupOutput probe-tool 'INSTALLED' @('/d', '/c', 'echo INSTALLED')
            _mise_hook
            $installed = (& $hostExe where 'http:probe-tool').Trim()
            Copy-Item -LiteralPath $env:ComSpec -Destination (Join-Path $installed 'new-command.exe')
            Assert-Lookup (-not (Get-Command new-command -ErrorAction SilentlyContinue)) 'A newly added executable must be absent before shim discovery is refreshed.'
            mise reshim
            _mise_hook
            Assert-LookupOutput new-command 'NEW_COMMAND' @('/d', '/c', 'echo NEW_COMMAND')
            mise deactivate
        }
    }

    It 'rediscovers names and capabilities when only an adjacent Windows manifest changes' {
        Invoke-LookupScenario $TestDrive {
            $bin = New-LookupBin -Missing
            New-LookupLauncher (Join-Path $bin 'mise.exe') @'
using System;
using System.IO;
using System.Reflection;
class Launcher {
    static int Main(string[] args) {
        var name = File.ReadAllText(Path.ChangeExtension(Assembly.GetExecutingAssembly().Location, null)).Trim();
        if (name == "incompatible") { Console.Error.WriteLine("Unsupported lookup protocol"); return 1; }
        if (name == "legacy-protocol" && args.Length > 1 && args[0] == "__lookup-shim-names" && args[1] != "1") { Console.Error.WriteLine("Unsupported lookup protocol"); return 1; }
        if (args.Length > 0 && args[0] == "__lookup-shim-names") { Console.WriteLine("[\"" + name + "\"]"); return 0; }
        Console.WriteLine(name);
        return 0;
    }
}
'@
            Set-LookupConfig @'
[settings]
activate_mise_lookup = "env_path"
[env]
_.path = ["{{config_root}}/tools/bin"]
'@
            $manifest = Join-Path $bin 'mise'
            'first-tool' | Set-Content -LiteralPath $manifest
            (& $hostExe activate pwsh) | Out-String | Invoke-Expression
            Assert-LookupOutput first-tool 'first-tool'
            'second-tool' | Set-Content -LiteralPath $manifest
            _mise_hook
            Assert-LookupOutput second-tool 'second-tool'
            Assert-Lookup (-not (Get-Command first-tool -ErrorAction SilentlyContinue)) 'Manifest changes must remove obsolete command names from PATH.'
            'incompatible' | Set-Content -LiteralPath $manifest
            Assert-LookupFailure $env:ComSpec 'update the pinned mise version' @('/c', 'second-tool')
            _mise_hook
            Assert-LookupFailure mise 'update the pinned mise version' @('--version')
            'legacy-protocol' | Set-Content -LiteralPath $manifest
            _mise_hook
            Assert-LookupFailure mise 'update the pinned mise version' @('--version')
            'second-tool' | Set-Content -LiteralPath $manifest
            _mise_hook
            Assert-LookupOutput mise 'second-tool' @('--version')
            mise deactivate
        }
    }
}
