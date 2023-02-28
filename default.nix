{ pkgs, system, lib, rustPlatform, coreutils, bash, perl }:
let
in
rustPlatform.buildRustPackage {
  pname = "rtx";
  version = "1.18.2";

  src = lib.cleanSource ./.;

  checkPhase = ''
    substituteInPlace $PWD/.bin/rtx --replace '#!/bin/bash' '#!${bash}/bin/bash'
    PATH=$PWD/.bin:$PATH RUST_BACKTRACE=1 cargo test --features clap_mangen
  '';

  cargoHash = "sha256-2CxKiHiQU2PfGSo+hdFBMJt6D3xPbzB++qGm8D4UqHM=";

  # https://github.com/alexcrichton/openssl-src-rs/issues/45
  OPENSSL_SRC_PERL = "${perl}/bin/perl";

  meta = with lib; {
    description = "Polyglot runtime manager (asdf rust clone)";
    homepage = "https://github.com/jdxcode/rtx";
    license = licenses.mit;
  };
}
