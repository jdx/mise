Set-StrictMode -Version Latest
#Set-PSDebug -Trace 1

$Target = $args[0]
$Version = ./scripts/get-version.ps1
$BaseName = "mise-v$Version-$Env:OS-$Env:ARCH"

# TODO: use "serious" feature
cargo build --release --features rustls-native-roots,openssl/vendored --target "$Target"
cargo build --release -p mise-shim --target "$Target"
mkdir -p dist/mise/bin
cp "target/$Target/release/mise.exe" dist/mise/bin/mise.exe
cp "target/$Target/release/mise-shim.exe" dist/mise/bin/mise-shim.exe
cp README.md dist/mise/README.md
cp LICENSE dist/mise/LICENSE
Set-Location dist
7z a -tzip "$BaseName.zip" mise
Set-Location ..
7z l "dist/$BaseName.zip"
