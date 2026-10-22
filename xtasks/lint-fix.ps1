#MISE alias=["format"]
#MISE wait_for=["build", "render:schema"]
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

cargo clippy --fix --allow-staged --allow-dirty -- -Dwarnings
oxfmt --write .
cargo fmt --all
