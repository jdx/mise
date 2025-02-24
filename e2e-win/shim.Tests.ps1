Describe 'shim_mode' {

    function changeShimMode {
        param (
            [string]$mode,
        )

        mise settings windows_shim_mode $mode
        mise reshim --force

    }

    BeforeAll {
         $shimPath = Join-Path -Path $env:MISE_DATA_DIR -ChildPath "shims"
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

        (Get-Item -Path  (Join-Path -Path $shimPath -ChildPath go.cmd)) | Should -Be ""
    }

    It 'run on hardlink' {
        changeShimMode "hardlink"

        mise x go@1.23.3 -- where go
        mise x go@1.23.3 -- go version | Should -BeLike "go version go1.23.3 windows/*"

        (Get-Item -Path (Join-Path -Path $shimPath -ChildPath go.exe)).LinkType | Should -Be "HardLink"
    }
}
