Describe 'conda' {
    It 'executes ripgrep via conda backend' {
        mise x conda:ripgrep@14.1.0 -- rg --version | Out-String | Should -Match "ripgrep 14"
    }

    # Entries in .mise-bins are copies on Windows, and a copied executable looks for its
    # imports beside itself. zstd links against DLLs that belong to dependency packages,
    # so before those were brought along too this exited with STATUS_DLL_NOT_FOUND
    # (0xC0000135) and printed nothing.
    It 'runs a binary that links DLLs from dependency packages' {
        mise x conda:zstd@1.5.7 -- zstd --version | Out-String | Should -Match "Zstandard CLI"
    }
}
