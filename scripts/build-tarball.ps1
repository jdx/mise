Set-StrictMode -Version Latest
#Set-PSDebug -Trace 1

# BASENAME=$NAME-$VERSION-$(get_os)-$(get_arch)$(get_suffix)
$Target = $args[0]
$Version = ./scripts/get-version.ps1
$BaseName = "mise-$Version-$Target"

cargo build --release --features openssl/vendored --target "$Target"
mkdir -p dist/mise/bin
cp "target/$Target/release/mise.exe" dist/mise/bin/mise.exe
cp README.md dist/mise/README.md
cp LICENSE dist/mise/LICENSE
Compress-Archive -Path dist/mise -DestinationPath "dist/$BaseName.zip"
