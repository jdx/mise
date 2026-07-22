Describe 'shim_mode' {

    BeforeAll {
        function changeShimMode {
            param (
                [string]$mode
            )

            mise settings windows_shim_mode $mode
            mise reshim --force
        }

        $shimPath = Join-Path -Path $env:MISE_DATA_DIR -ChildPath "shims"
    }

    AfterAll {
        mise settings unset windows_shim_mode
    }

    It 'run on symlink' {
        changeShimMode "symlink"

        mise x go@1.23.3 -- where go
        mise x go@1.23.3 -- go version | Should -BeLike "go version go1.23.3 windows/*"
        
        (Get-Item -Path (Join-Path -Path $shimPath -ChildPath go.exe)).LinkType | Should -Be "SymbolicLink"
    }

    It 'run on file' {
        changeShimMode "file"

        mise x go@1.23.3 -- where go
        mise x go@1.23.3 -- go version | Should -BeLike "go version go1.23.3 windows/*"

        (Get-Item -Path  (Join-Path -Path $shimPath -ChildPath go.cmd)).LinkType | Should -Be $null

        # file mode needs the extension-less shim for Git Bash/Cygwin (no .exe,
        # and Cygwin does not auto-append .cmd)
        Test-Path -Path (Join-Path -Path $shimPath -ChildPath go) -PathType Leaf | Should -Be $true
    }

    It 'run on exe' {
        changeShimMode "exe"

        $wherePath = mise x go@1.23.3 -- where go
        $LASTEXITCODE | Should -Be 0
        $wherePath | Should -BeLike "*go.exe"
        mise x go@1.23.3 -- go version | Should -BeLike "go version go1.23.3 windows/*"

        $goShim = Get-Item -Path (Join-Path -Path $shimPath -ChildPath go.exe)
        $goShim.LinkType | Should -Be $null
        $goShim.Length | Should -BeGreaterThan 0

        # exe mode must NOT create an extension-less shim: it leaks into WSL via
        # /mnt/c PATH interop (#10299) and is unnecessary because Git Bash/Cygwin
        # resolve `go` to `go.exe` via their `.exe` magic.
        Test-Path -Path (Join-Path -Path $shimPath -ChildPath go) -PathType Leaf | Should -Be $false
    }

    It 'exe shim dispatch for an unresolvable bin gives an actionable error' {
        # Reproduces discussion #11183: an exe-mode shim dispatches through
        # `mise x -- <tool>` with __MISE_SHIM_PATH set. When the tool cannot be
        # resolved (e.g. a project-scoped tool invoked from outside the project),
        # the Windows arm used to surface the opaque `cannot find binary path`.
        # It should now surface the same which_shim-style guidance Unix gets.
        $fakeShim = Join-Path -Path $shimPath -ChildPath "mise-11183-not-real.exe"
        $env:__MISE_SHIM_PATH = $fakeShim
        try {
            $out = mise x -- mise-11183-not-real.exe 2>&1
            $LASTEXITCODE | Should -Not -Be 0
            $joined = ($out | Out-String)
            $joined | Should -Not -Match "cannot find binary path"
            $joined | Should -Match "not a valid shim|No version is set for shim"
            $joined | Should -Match "mise-11183-not-real"
            $joined | Should -Not -Match "mise-11183-not-real\.exe"
        }
        finally {
            Remove-Item Env:\__MISE_SHIM_PATH -ErrorAction SilentlyContinue
        }
    }

    It 'run on hardlink' {
        mise settings windows_shim_mode "hardlink"

        # make mise is on same filesystem
        $misePath = (Get-Command -Type Application mise -all | Select-Object -First 1).Source
        $binPath = (Join-Path -Path $env:MISE_DATA_DIR -ChildPath "bin")
        $newMisePath = (Join-Path -Path $binPath -ChildPath "mise.exe")
        New-Item -ItemType Directory -Path $binPath -Force
        Copy-Item  $misePath $newMisePath

        &$newMisePath reshim --force

        &$newMisePath x go@1.23.3 -- where go
        &$newMisePath x go@1.23.3 -- go version | Should -BeLike "go version go1.23.3 windows/*"

        (Get-Item -Path (Join-Path -Path $shimPath -ChildPath go.exe)).LinkType | Should -Be "HardLink"
    }
}
