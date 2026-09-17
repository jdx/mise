# https://github.com/jdx/mise/discussions/13196 - PowerShell's parameter binder claims
# the first bare `--` on a line as its own end-of-parameters token and drops it before
# $args is populated, so `mise exec -- pnpm --version` reached the activation wrapper as
# `exec pnpm --version` and mise read `--version` as its own flag. bash, zsh and fish
# forward "$@" untouched, which is why only pwsh needs the separator put back.
#
# Every case runs in a child process: mise_hook.Tests.ps1 activates in the shared Pester
# runspace, so an in-process activation here would exercise whichever wrapper happened to
# be installed already. -NoProfile keeps a developer profile from pre-activating for the
# same reason.
Describe 'mise activate pwsh double-dash separator' {
    BeforeAll {
        # Defined here rather than at file scope: Pester runs an It block in a scope built
        # during the run phase, which a function defined during discovery never reaches.
        function Invoke-Activated {
            param([string]$Body, [string]$ConfigFile)

            $prelude = @(
                "`$ErrorActionPreference = 'Continue'"
                "`$env:MISE_CONFIG_FILE = '$ConfigFile'"
                'mise activate pwsh | Out-String | Invoke-Expression'
                # Without this the calls below would fall through to the native executable
                # and pass even if Invoke-Expression never defined the wrapper.
                "if (-not (Get-Command mise -CommandType Function -ErrorAction SilentlyContinue)) { Write-Output 'ACTIVATION-ERROR'; exit 1 }"
                $Body
            ) -join "`n"

            pwsh -NoProfile -NonInteractive -Command $prelude 2>&1 | Out-String
        }

        # Prints its own arguments, so a swallowed separator shows up as mise refusing the
        # flag rather than as this script silently never running.
        $script:echoArgs = Join-Path $TestDrive 'echo-args.ps1'
        "'ARGS=' + (`$args -join '|')" | Set-Content $script:echoArgs
        # An empty config keeps `mise exec` from resolving whatever tools happen to be
        # configured for the checkout the suite runs from.
        $script:config = Join-Path $TestDrive 'mise.toml'
        '' | Set-Content $script:config
    }

    It 'forwards the separator to mise exec' {
        $out = Invoke-Activated "mise exec -- pwsh -NoProfile -File '$script:echoArgs' --version" $script:config

        $out | Should -Not -Match 'ACTIVATION-ERROR'
        $out | Should -Match 'ARGS=--version'
        $out | Should -Not -Match 'unexpected argument'
        $out | Should -Not -Match 'Usage: mise exec'
    }

    It 'forwards the separator to the short alias' {
        $out = Invoke-Activated "mise x -- pwsh -NoProfile -File '$script:echoArgs' --version --verbose" $script:config

        $out | Should -Match 'ARGS=--version\|--verbose'
        $out | Should -Not -Match 'unexpected argument'
    }

    # Quoting already defeated the binder, so the separator was never eaten here and the
    # repair must not add a second one.
    It 'leaves an already-quoted separator alone' {
        $out = Invoke-Activated "mise exec '--' pwsh -NoProfile -File '$script:echoArgs' --version" $script:config

        $out | Should -Match 'ARGS=--version'
        $out | Should -Not -Match "couldn't exec process"
    }

    # The binder takes one separator per command, not one per line, so the second
    # invocation needs its own repair - which means finding the right command on the line.
    It 'repairs each invocation on a shared line' {
        $body = "mise ls --offline | Out-Null; mise exec -- pwsh -NoProfile -File '$script:echoArgs' --version"
        $out = Invoke-Activated $body $script:config

        $out | Should -Match 'ARGS=--version'
        $out | Should -Not -Match 'unexpected argument'
    }

    # Only what stands before the separator fixes its position, so a splat after it expands
    # freely and must not stop the repair. Counting the whole element list against $args did.
    It 'repairs a separator followed by a splatted array' {
        $body = @(
            "`$childArgs = @('--version')"
            "mise exec -- pwsh -NoProfile -File '$script:echoArgs' @childArgs"
        ) -join "`n"
        $out = Invoke-Activated $body $script:config

        $out | Should -Match 'ARGS=--version'
        $out | Should -Not -Match 'unexpected argument'
    }

    # A splat *before* the separator hides how many arguments precede it, so the position
    # genuinely is not knowable and the repair has to stand down rather than guess.
    It 'leaves a separator preceded by a splatted array alone' {
        $body = @(
            "`$pre = @('-C', '.')"
            "mise exec @pre -- pwsh -NoProfile -File '$script:echoArgs' --version"
        ) -join "`n"
        $out = Invoke-Activated $body $script:config

        # Named rather than left as an absence: without the separator mise reads the whole
        # line as its own, and `-NoProfile` — the first word past where the `--` used to be —
        # is what it refuses. Asserting only that the script did not run would pass just as
        # well if activation had failed or mise had died for some unrelated reason.
        $out | Should -Not -Match 'ACTIVATION-ERROR'
        $out | Should -Match "unexpected argument '-NoProfile'"
        $out | Should -Not -Match 'ARGS='
    }

    # PowerShell unrolls an array on output, so a repair that hands the arguments back
    # without re-wrapping them returns a bare string for a single argument - and the
    # wrapper then indexes into that string a character at a time, running `mise -`.
    It 'passes a lone argument through whole' {
        $out = Invoke-Activated 'mise --version' $script:config

        $out | Should -Match '(?m)^\d+\.\d+\.\d+'
        $out | Should -Not -Match 'unexpected argument'
    }

    It 'leaves a command with no separator alone' {
        $out = Invoke-Activated 'mise ls --offline | Out-Null; Write-Output "LS-OK"' $script:config

        $out | Should -Match 'LS-OK'
        $out | Should -Not -Match 'unexpected argument'
    }
}
