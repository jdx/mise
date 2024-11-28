#MISE alias=["format"]
#MISE wait_for=["build", "render:settings"]
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

cargo clippy --fix --allow-staged --allow-dirty -- -Dwarnings
prettier -w .
cargo fmt --all
