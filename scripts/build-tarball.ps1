Set-StrictMode -Version Latest
#Set-PSDebug -Trace 1

$Target = $args[0]
$Version = ./scripts/get-version.ps1
$BaseName = "mise-v$Version-$Env:OS-$Env:ARCH"

# Keep this list in sync with scripts/build-tarball.sh. `--no-default-features`
# matters: the default set turns on `native-tls`, and without disabling it the
# binary carries a second TLS stack (native-tls + schannel) next to the rustls
# one we actually use.
#
# No `openssl/vendored` here. It was needed when mise linked git2/libgit2
# (removed in 3463ef185, which moved to the pure-Rust gix), and it survived a
# revert/re-apply of the MITM-firewall fix (93b0d136e, f336df457) that left it
# stacked alongside rustls. On Windows native-tls is schannel, so vendored
# OpenSSL was compiled into every release and never called.
$Features = "rustls-native-roots,self_update,vfox/vendored-lua"

# The serious profile sets `panic = "abort"`, which Windows cannot use. Lua
# raises errors with longjmp, and mlua deliberately longjmps across its
# `extern "C-unwind"` callback trampolines. MSVC implements longjmp as an SEH
# unwind, so under panic=abort those frames abort the process with "panic in a
# function that cannot unwind" whenever a vfox plugin hits a Lua error.
# On Unix, longjmp does not unwind, so the other targets keep panic=abort.
$Env:CARGO_PROFILE_SERIOUS_PANIC = "unwind"

# Fat LTO optimizes the whole program as one LLVM module on one thread, and on
# the 4-vCPU/16 GB windows-latest runner that link ran from ~65 minutes to past
# the 150-minute job timeout depending on memory pressure. Thin LTO keeps most
# of the cross-crate inlining while optimizing in parallel with far less
# memory. Machines with plenty of RAM (the self-hosted Windows runner has 64 GB)
# build with the serious profile's default fat LTO like the other platforms. An
# explicit CARGO_PROFILE_SERIOUS_LTO always wins.
$MemoryGB = (Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB
if (-not $Env:CARGO_PROFILE_SERIOUS_LTO -and $MemoryGB -lt 48) {
    $Env:CARGO_PROFILE_SERIOUS_LTO = "thin"
}
Write-Host ("LTO: {0} ({1:N0} GB RAM)" -f $(if ($Env:CARGO_PROFILE_SERIOUS_LTO) { $Env:CARGO_PROFILE_SERIOUS_LTO } else { "fat (profile default)" }), $MemoryGB)

# Only the release PR's dry run may use the shared mbx build cache (release.yml
# leaves MBX_DISABLE=0 there). A real release sets MBX_DISABLE=1 and always
# compiles from scratch with plain cargo. Hosted runners have no mbx.
$Cargo = "cargo"
if ($Env:MBX_DISABLE -eq "0" -and (Get-Command mbx -ErrorAction SilentlyContinue)) { $Cargo = "mbx" }

# PowerShell keeps going after a failed native command, and packaging below would
# then zip whatever an earlier build left in target/.
& $Cargo build --profile=serious --ignore-rust-version --no-default-features --features "$Features" --target "$Target"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& $Cargo build --profile=serious -p mise-shim --target "$Target"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
mkdir -p dist/mise/bin
cp "target/$Target/serious/mise.exe" dist/mise/bin/mise.exe
cp "target/$Target/serious/mise-shim.exe" dist/mise/bin/mise-shim.exe
cp README.md dist/mise/README.md
cp LICENSE dist/mise/LICENSE
Set-Location dist
7z a -tzip "$BaseName.zip" mise
Set-Location ..
7z l "dist/$BaseName.zip"
