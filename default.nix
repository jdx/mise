{ pkgs, lib, fetchFromGitHub, rustPlatform, coreutils, bash, direnv, perl }:
rustPlatform.buildRustPackage {
  pname = "rtx";
  version = "1.18.2";

  src = lib.cleanSource ./.;

  prePatch = ''
    substituteInPlace ./test/data/plugins/**/bin/* \
      --replace '#!/usr/bin/env bash' '#!${bash}/bin/bash'
    substituteInPlace ./src/fake_asdf.rs ./src/cli/reshim.rs \
      --replace '#!/bin/sh' '#!${coreutils}/bin/sh'
    substituteInPlace ./src/env_diff.rs \
      --replace '"bash"' '"${bash}/bin/bash"'
    substituteInPlace ./src/cli/direnv/exec.rs \
      --replace '"env"' '"${coreutils}/bin/env"' \
      --replace 'cmd!("direnv"' 'cmd!("${direnv}/bin/direnv"'
  '';

  # Skip the test_plugin_list_urls as it uses the .git folder, which
  # is excluded by default from Nix.
  checkPhase = ''
    RUST_BACKTRACE=full cargo test --features clap_mangen -- \
      --skip cli::plugins::ls::tests::test_plugin_list_urls
  '';

  cargoHash = "sha256-2CxKiHiQU2PfGSo+hdFBMJt6D3xPbzB++qGm8D4UqHM=";

  # Need this to ensure openssl-src's build uses an available version of `perl`
  # https://github.com/alexcrichton/openssl-src-rs/issues/45
  OPENSSL_SRC_PERL = "${perl}/bin/perl";

  meta = with lib; {
    description = "Polyglot runtime manager (asdf rust clone)";
    homepage = "https://github.com/jdxcode/rtx";
    license = licenses.mit;
  };
}
