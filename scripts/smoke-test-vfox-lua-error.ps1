Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# Runs a packaged mise binary through a vfox hook that catches an error raised
# by a Rust-backed Lua function. Lua raises errors with longjmp, which MSVC
# implements as an SEH unwind across mise's Rust frames. A release profile with
# panic=abort turns that into "panic in a function that cannot unwind" (#12646),
# and the debug builds the Windows e2e suite uses cannot catch it.

$Mise = (Resolve-Path $args[0]).Path

$Root = Join-Path ([System.IO.Path]::GetTempPath()) "mise-smoke-$([guid]::NewGuid())"
$PluginDir = Join-Path $Root "plugin"
New-Item -ItemType Directory -Force -Path (Join-Path $PluginDir "hooks") | Out-Null

foreach ($dir in "DATA", "CACHE", "CONFIG", "STATE") {
    Set-Item "Env:MISE_${dir}_DIR" (Join-Path $Root $dir.ToLower())
}

# Run outside the checkout so the repository's untrusted mise.toml stays out.
Set-Location $Root

@'
PLUGIN = {}
PLUGIN.name = "lua-error-smoke"
PLUGIN.version = "0.1.0"
PLUGIN.description = "release smoke test for Lua errors raised through Rust callbacks"
'@ | Set-Content (Join-Path $PluginDir "metadata.lua")

# json.decode is implemented in Rust, so its error is raised from a Rust
# callback frame. pcall catches it, and the hook reports whether it did.
@'
function PLUGIN:Available(ctx)
    local ok = pcall(require("json").decode, "{")
    return {
        { version = "caught-" .. tostring(not ok) },
    }
end
'@ | Set-Content (Join-Path $PluginDir "hooks/available.lua")

& $Mise plugins link lua-error-smoke $PluginDir
if ($LASTEXITCODE -ne 0) { throw "mise plugins link exited with $LASTEXITCODE" }

$Output = & $Mise ls-remote lua-error-smoke
if ($LASTEXITCODE -ne 0) { throw "mise ls-remote exited with $LASTEXITCODE" }
if ($Output -notcontains "caught-true") {
    throw "expected the hook to catch the Lua error, got: $Output"
}
Write-Output "vfox Lua error smoke test passed"
