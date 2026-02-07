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
