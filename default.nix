{ pkgs, lib, stdenv, fetchFromGitHub, rustPlatform, coreutils, bash, direnv, openssl }:

rustPlatform.buildRustPackage {
  pname = "mise";
  version = "2024.5.3";

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
    openssl
  ] ++ lib.optionals stdenv.isDarwin [ darwin.apple_sdk.frameworks.Security darwin.apple_sdk.frameworks.SystemConfiguration ];

  prePatch = ''
    substituteInPlace ./test/data/plugins/**/bin/* \
      --replace '#!/usr/bin/env bash' '#!${bash}/bin/bash'
    substituteInPlace ./src/fake_asdf.rs ./src/cli/reshim.rs \
      --replace '#!/bin/sh' '#!${bash}/bin/sh'
    substituteInPlace ./src/env_diff.rs \
      --replace '"bash"' '"${bash}/bin/bash"'
    substituteInPlace ./test/cwd/.mise/tasks/filetask \
      --replace '#!/usr/bin/env bash' '#!${bash}/bin/bash'
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

  meta = with lib; {
    description = "The front-end to your dev env";
    homepage = "https://github.com/jdx/mise";
    license = licenses.mit;
  };
}
