Set-StrictMode -Version Latest
#Set-PSDebug -Trace 1

$Target = $args[0]
$Version = ./scripts/get-version.ps1
$BaseName = "mise-v$Version-$Env:OS-$Env:ARCH"

cargo build --release --features openssl/vendored,git2/vendored-libgit2,git2/vendored-openssl --target "$Target"
mkdir -p dist/mise/bin
cp "target/$Target/release/mise.exe" dist/mise/bin/mise.exe
cp README.md dist/mise/README.md
cp LICENSE dist/mise/LICENSE
Compress-Archive -Path dist/mise -DestinationPath "dist/$BaseName.zip"
