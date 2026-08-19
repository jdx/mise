Describe 'the pwsh command-not-found hook branches on the exit code' {
    It 'ships a script that reads $LASTEXITCODE rather than the command output' {
        $script = mise activate pwsh | Out-String

        # The hook only has meaning when auto-install is on, which is what emits this block.
        $script | Should -Match 'hook-not-found -s pwsh'
        $script | Should -Match 'LASTEXITCODE -eq 0'

        # Both halves of the contract: the output has to go to Out-Null for the exit code to be
        # the thing being read. Dropping the pipe would still satisfy the two lines above.
        $emitted = 'hook-not-found -s pwsh -- \$Name \| Out-Null'
        $script | Should -Match $emitted

        # The regression guard. `if (& mise hook-not-found ...)` is the tidier-looking form and the
        # one somebody will want back, so fail if it returns.
        $condition = 'if \(& .*hook-not-found'
        $script | Should -Not -Match $condition
    }

    It 'because `if (& native)` tests the output, not the code' {
        # The reason the line above matters, executed rather than asserted in a comment.
        # `mise hook-not-found` exits 0 when it installed something and writes nothing to stdout,
        # so the old form took the false branch exactly when it should have taken the true one.
        $cmd = Join-Path $env:SystemRoot 'System32\cmd.exe'

        $viaOutput = if (& $cmd /c 'exit 0') { 'true' } else { 'false' }
        $viaOutput | Should -Be 'false' -Because 'exit 0 with no stdout looks false to `if (& ...)`'

        & $cmd /c 'exit 0' | Out-Null
        $viaCode = if ($LASTEXITCODE -eq 0) { 'true' } else { 'false' }
        $viaCode | Should -Be 'true' -Because 'the exit code is the answer mise actually gives'

        # And the other direction: a failure that prints looked true to the old form.
        $viaOutput = if (& $cmd /c 'echo hi & exit 1') { 'true' } else { 'false' }
        $viaOutput | Should -Be 'true' -Because 'stdout, not the code, is what it was reading'

        & $cmd /c 'echo hi & exit 1' | Out-Null
        $viaCode = if ($LASTEXITCODE -eq 0) { 'true' } else { 'false' }
        $viaCode | Should -Be 'false' -Because 'the new form is right in both directions'
    }

    It 'refreshes the environment itself when --no-hook-env leaves _mise_hook undefined' {
        # Now that the branch can actually be taken, what it calls has to exist. `--no-hook-env`
        # suppresses the `_mise_hook` definition while still emitting this block.
        $script = mise activate pwsh --no-hook-env | Out-String

        $script | Should -Not -Match 'function Global:_mise_hook'
        $script | Should -Match 'hook-not-found -s pwsh'

        # A `-Not -Match` alone passes on any build that never had the branch, and a positive match
        # that merely wants `Test-Path` somewhere would accept a guard around nothing. Pin the whole
        # shape instead.
        $guardedCall = 'LASTEXITCODE -eq 0\)\{[^}]*if \(Test-Path -Path Function:\\_mise_hook\)\{\s*_mise_hook\s*\}'
        $script | Should -Match $guardedCall

        # Then require it to be the only call in the branch: a second one, added later and left
        # unguarded, would still satisfy the shape above.
        $branch = [regex]::Match($script, '(?s)hook-not-found -s pwsh.*?StopSearch = \$true').Value
        $branch | Should -Not -BeNullOrEmpty
        $calls = [regex]::Matches($branch, '(?m)^\s*_mise_hook\s*$').Count
        $calls | Should -Be 1 -Because 'the guarded one is the only call the branch may make'

        # Not throwing is only half of it: skipping the refresh leaves the tool that was just
        # installed off PATH, so the handoff finds nothing. The branch has to refresh on its own.
        $fallback = 'Function:\\_mise_hook\)\{\s*_mise_hook\s*\} else \{'
        $script | Should -Match $fallback
        # With the definition suppressed, the only `hook-env` left in the script is that fallback.
        $script | Should -Match 'hook-env.*-s pwsh'
    }

    It 'because an unresolved name inside the handler throws instead of continuing' {
        # A plain script block carries on past a command it cannot resolve, so the guard only looks
        # unnecessary until it is run where mise actually runs it: inside a CommandNotFoundAction,
        # where the same call aborts the handler and takes the $EventArgs handoff with it.
        function Invoke-MiseHandoffProbe {
            param([bool] $Guarded)

            $global:__mise_probe_entered = $false
            $global:__mise_probe_reached = $false
            $global:__mise_probe_guarded = $Guarded
            $global:__mise_probe_error = $null
            $previous = $ExecutionContext.SessionState.InvokeCommand.CommandNotFoundAction
            try {
                $ExecutionContext.SessionState.InvokeCommand.CommandNotFoundAction = [EventHandler[System.Management.Automation.CommandLookupEventArgs]] {
                    param([object] $Name, [System.Management.Automation.CommandLookupEventArgs] $eventArgs)
                    end {
                        if ($Name -ne 'mise-probe-missing-tool') { return }
                        $global:__mise_probe_entered = $true
                        if ($global:__mise_probe_guarded) {
                            if (Test-Path -Path Function:\_mise_probe_hook_absent) { _mise_probe_hook_absent }
                        } else {
                            _mise_probe_hook_absent
                        }
                        $global:__mise_probe_reached = $true
                        $eventArgs.Command = Get-Command Get-Date
                        $eventArgs.StopSearch = $true
                    }
                }
                try { mise-probe-missing-tool | Out-Null } catch { $global:__mise_probe_error = $_ }
                return [pscustomobject]@{
                    Entered = $global:__mise_probe_entered
                    Reached = $global:__mise_probe_reached
                    Error   = $global:__mise_probe_error
                }
            } finally {
                $ExecutionContext.SessionState.InvokeCommand.CommandNotFoundAction = $previous
                Remove-Variable -Name '__mise_probe_entered' -Scope Global -ErrorAction SilentlyContinue
                Remove-Variable -Name '__mise_probe_reached' -Scope Global -ErrorAction SilentlyContinue
                Remove-Variable -Name '__mise_probe_guarded' -Scope Global -ErrorAction SilentlyContinue
                Remove-Variable -Name '__mise_probe_error' -Scope Global -ErrorAction SilentlyContinue
            }
        }

        $unguarded = Invoke-MiseHandoffProbe -Guarded $false
        # Without this first check the rest proves nothing: a handler that never ran would leave
        # Reached false too, and the probe would look like it had demonstrated the abort.
        $unguarded.Entered | Should -BeTrue -Because 'the handler has to run for the rest to mean anything'
        $unguarded.Reached | Should -BeFalse -Because 'the handoff never runs when the hook is undefined'
        # The surfaced error names the command the user typed, not the hook that went missing —
        # so there is nothing in the message to match on, only the type.
        $unguarded.Error.Exception | Should -BeOfType ([System.Management.Automation.CommandNotFoundException])

        $guarded = Invoke-MiseHandoffProbe -Guarded $true
        $guarded.Entered | Should -BeTrue
        $guarded.Reached | Should -BeTrue -Because 'guarding the call lets the installed command be handed back'
        $guarded.Error | Should -BeNullOrEmpty -Because 'the handoff ran, so nothing propagated out'
    }
}

Describe 'the pwsh command-not-found hook leaves mise its own names' {
    It 'skips them before spending a hook-not-found call' {
        $script = mise activate pwsh | Out-String

        $guard = $script.IndexOf("if (`$Name -eq 'mise' -or `$Name -like 'mise-*') { return }")
        ($guard -ge 0) | Should -BeTrue -Because 'bash, zsh and fish all skip mise''s own names'

        # Position matters, not just presence: past the guard the handler pays a full mise
        # startup only to be told that `mise-foo` is not a tool.
        $call = $script.IndexOf('hook-not-found -s pwsh')
        ($guard -lt $call) | Should -BeTrue -Because 'the guard exists to avoid that call'
    }

    It 'skips exactly mise and mise-*, and nothing that merely contains it' {
        # Evaluate the condition the script actually ships rather than a copy of it, so a
        # rewrite to `-like ''mise*''` or `-match ''mise''` fails here instead of silently
        # swallowing somebody else''s tool.
        $script = mise activate pwsh | Out-String
        $pattern = '(?m)^\s*if \((\$Name -eq .+?)\) \{ return \}'
        $condition = [regex]::Match($script, $pattern).Groups[1].Value
        $condition | Should -Not -BeNullOrEmpty

        $skips = [scriptblock]::Create("param(`$Name) $condition")

        # mise's own names, in the casing Windows lets you type them in
        foreach ($name in 'mise', 'MISE', 'mise-foo', 'MISE-FOO') {
            (& $skips $name) | Should -BeTrue -Because "$name is mise, not a tool"
        }
        # and names that only look like it
        foreach ($name in 'premise', 'mise2', 'misexyz', 'my-mise-tool') {
            (& $skips $name) | Should -BeFalse -Because "$name is somebody else's tool"
        }
    }
}
