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

cargo build --profile=serious --ignore-rust-version --no-default-features --features "$Features" --target "$Target"
cargo build --profile=serious -p mise-shim --target "$Target"
mkdir -p dist/mise/bin
cp "target/$Target/serious/mise.exe" dist/mise/bin/mise.exe
cp "target/$Target/serious/mise-shim.exe" dist/mise/bin/mise-shim.exe
cp README.md dist/mise/README.md
cp LICENSE dist/mise/LICENSE
Set-Location dist
7z a -tzip "$BaseName.zip" mise
Set-Location ..
7z l "dist/$BaseName.zip"
