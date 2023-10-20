{ pkgs, lib, stdenv, fetchFromGitHub, rustPlatform, coreutils, bash, direnv, perl }:
let
  libssl = with pkgs; symlinkJoin {
    name = "libssl-merged";
    paths = [ openssl.out openssl.dev ];
  };
in
rustPlatform.buildRustPackage {
  pname = "rtx";
  version = "2023.10.3";

  src = lib.cleanSource ./.;

  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  nativeBuildInputs = with pkgs; [ pkg-config ];
  buildInputs = with pkgs; [
    coreutils
    bash
    direnv
    gnused
    git
    gawk
  ] ++ lib.optional stdenv.isDarwin darwin.apple_sdk.frameworks.Security;

  prePatch = ''
    substituteInPlace ./test/data/plugins/**/bin/* \
      --replace '#!/usr/bin/env bash' '#!${bash}/bin/bash'
    substituteInPlace ./src/fake_asdf.rs ./src/cli/reshim.rs \
      --replace '#!/bin/sh' '#!${bash}/bin/sh'
    substituteInPlace ./src/env_diff.rs \
      --replace '"bash"' '"${bash}/bin/bash"'
    substituteInPlace ./src/cli/direnv/exec.rs \
      --replace '"env"' '"${coreutils}/bin/env"' \
      --replace 'cmd!("direnv"' 'cmd!("${direnv}/bin/direnv"'
  '';

  # Skip the test_plugin_list_urls as it uses the .git folder, which
  # is excluded by default from Nix.
  checkPhase = ''
    RUST_BACKTRACE=full cargo test --all-features -- \
      --skip cli::plugins::ls::tests::test_plugin_list_urls
  '';

  # Need this to ensure openssl-src's build uses an available version of `perl`
  # https://github.com/alexcrichton/openssl-src-rs/issues/45
  OPENSSL_SRC_PERL = "${perl}/bin/perl";
  OPENSSL_DIR = "${libssl}";

  meta = with lib; {
    description = "Polyglot runtime manager (asdf rust clone)";
    homepage = "https://github.com/jdx/rtx";
    license = licenses.mit;
  };
}
