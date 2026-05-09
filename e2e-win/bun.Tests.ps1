
Describe 'bun' {
    It 'installs bun and produces a working bunx alongside bun.exe' {
        # The upstream bun-windows-*.zip ships only bun.exe; the bunx
        # entry is created post-install (see oven-sh/bun:src/cli/
        # install_completions_command.zig `installBunxSymlinkWindows`).
        # Mise mirrors that step so `bunx` works out of the box on Windows.
        mise install bun@1.3.13 --force | Out-Null

        $installPath = (mise where bun@1.3.13).Trim()
        $binDir = Join-Path $installPath 'bin'

        # bun.exe should always be present.
        (Join-Path $binDir 'bun.exe') | Should -Exist

        # A bunx entry — either the hardlinked bunx.exe or the cmd-shim
        # fallback — must sit next to bun.exe.
        $bunxExe = Join-Path $binDir 'bunx.exe'
        $bunxCmd = Join-Path $binDir 'bunx.cmd'
        ((Test-Path $bunxExe) -or (Test-Path $bunxCmd)) | Should -BeTrue

        mise x bun@1.3.13 -- bunx --version | Should -BeLike "1.3.13*"
    }
}
