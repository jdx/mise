Describe 'a tool the registry does not list for this platform' {
    # `registry/docker-slim.toml` carries `os = ["linux", "macos"]`, so on Windows the tool is
    # dropped from the request set before any version resolves -- silently, because it is neither
    # unknown nor user-disabled. Nothing is installed, `mint` is simply absent, and
    # `mise x` reported only `cannot find binary path` while `mise install` and `mise use` said
    # exactly what was wrong.
    #
    # The tool also provides a bin under a different name than its own, which is the branch that
    # has to name both.
    #
    # Checked here as well as on the bash side because Windows has its own `exec_program`
    # (`#[cfg(all(windows, not(test)))]`) with its own call to `err_cannot_find_binary_path`;
    # the unix path passing says nothing about this one.

    BeforeAll {
        $script:OriginalDir = Get-Location
        $script:TestRoot = Join-Path $TestDrive 'os-unsupported'
        New-Item -ItemType Directory -Path $script:TestRoot -Force | Out-Null
        Set-Location $script:TestRoot

        # Whether the bin is absent from the runner decides whether this file tests anything at
        # all: if it resolves, `mise x` succeeds and never reaches the message. Recorded rather
        # than assumed -- an earlier revision used `aws-cli`, and the Windows image ships `aws`.
        $script:SubjectOnPath = [bool](Get-Command mint -ErrorAction SilentlyContinue)
        $script:ControlOnPath = [bool](Get-Command not-a-tool-9f3a -ErrorAction SilentlyContinue)

        # No install is attempted for an excluded tool, so nothing here reaches the network.
        $script:Out = mise x docker-slim -- mint --version 2>&1 | Out-String
        $script:Exit = $LASTEXITCODE
        $script:Control = mise x -- not-a-tool-9f3a 2>&1 | Out-String
        # Captured before anything else can overwrite it. A control that only checks for absent
        # text would be satisfied by a command that unexpectedly succeeded and printed nothing.
        $script:ControlExit = $LASTEXITCODE
    }

    AfterAll {
        Set-Location $script:OriginalDir
    }

    It 'is asking about a binary this machine does not have' {
        # Checked on its own so a runner that starts shipping these fails here, saying why,
        # instead of failing the assertions below as though the message had regressed.
        $script:SubjectOnPath | Should -BeFalse
        $script:ControlOnPath | Should -BeFalse
    }

    It 'still fails, because the tool genuinely cannot run here' {
        $script:Exit | Should -Not -Be 0
    }

    It 'names the tool and this platform instead of only the missing binary' {
        $script:Out | Should -Match 'docker-slim'
        $script:Out | Should -Match 'not available on windows'
        # The bin is named differently from the tool, so the message has to carry both or the
        # user cannot connect the two.
        $script:Out | Should -Match 'mint'
    }

    It 'says where the tool does run' {
        # Without this the message tells the user something is impossible and nothing else.
        $script:Out | Should -Match 'linux'
        $script:Out | Should -Match 'macos'
    }

    It 'leaves an unrelated missing binary alone' {
        # The control: a hint appended to every resolution failure would pass the assertions above.
        # It has to have failed for the same reason as the subject, or "the text is absent" says
        # nothing about the hint.
        $script:ControlExit | Should -Not -Be 0
        $script:Control | Should -Not -Match 'registry lists it for'
        $script:Control | Should -Not -Match 'not available on'
    }
}
