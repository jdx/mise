{ pkgs, lib, rustPlatform, coreutils, bash, direnv, openssl, git }:

rustPlatform.buildRustPackage {
  pname = "mise";
  version = "2026.9.2";

  src = lib.cleanSource ./.;

  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  nativeBuildInputs = with pkgs; [
    cmakeMinimal
    clang
    llvmPackages.libclang
    pkg-config
    rustPlatform.bindgenHook
  ];
  nativeCheckInputs = with pkgs; [
    git
  ];
  buildInputs = with pkgs; [
    bash
    coreutils
    direnv
    gawk
    git
    gnused
    openssl
  ];

  LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";

  # tera-contrib's now() resolves its timezone by name (TimeZone::get("UTC")),
  # which requires a tzdb. The build sandbox provides none, so tests calling
  # now() fail with "Unknown timezone: UTC".
  TZDIR = "${pkgs.tzdata}/share/zoneinfo";

  prePatch = ''
    substituteInPlace ./src/test.rs ./test/data/plugins/**/bin/* \
      --replace '/usr/bin/env bash' '${bash}/bin/bash'
    substituteInPlace ./src/fake_asdf.rs ./src/cli/generate/git_pre_commit.rs ./src/cli/generate/snapshots/*.snap \
      --replace '/bin/sh' '${bash}/bin/sh'
    substituteInPlace ./src/env_diff.rs \
      --replace '"bash"' '"${bash}/bin/bash"'
    substituteInPlace ./src/cli/direnv/exec.rs \
      --replace '"env"' '"${coreutils}/bin/env"' \
      --replace 'cmd!("direnv"' 'cmd!("${direnv}/bin/direnv"'
    substituteInPlace ./src/git.rs ./src/test.rs \
      --replace '"git"' '"${git}/bin/git"'
  '';

  # Skip tests that require network, host tools unavailable in the sandbox,
  # or .git folder excluded by Nix.
  checkPhase = ''
    RUST_BACKTRACE=full cargo test --all-features -- \
      --skip cli::plugins::ls::tests::test_plugin_list_urls \
      --skip tera::tests::test_last_modified \
      --skip system::defaults::tests::test_status_missing_keys_are_unset \
      --skip plugins::core::ruby::tests::test_list_versions_matching \
      --skip cmd::tests::test_macos_sandbox_preserves_piped_stdin \
      --skip sandbox::macos::tests::test_allow_read_executes_shell_without_reading_siblings \
      --skip sandbox::macos::tests::test_deny_process_at_runtime \
      --skip system::packages::brew::cask::tests::ditto_into_stays_bound_after_directory_replacement \
      --skip system::packages::brew::cask::tests::installer_mutations_are_included_in_durable_symlink_sources \
      --skip system::packages::brew::cask::tests::staged_artifact_closure_merges_a_parent_after_its_child \
      --skip system::packages::brew::cask::tests::staged_symlink_source_accepts_canonical_stage_spelling \
      --skip system::packages::brew::cask::tests::staged_symlink_source_copies_reachable_internal_links \
      --skip system::packages::brew::cask::tests::staged_symlink_sources_become_caskroom_owned \
      --skip system::packages::brew::cask::tests::structured_copy_restores_external_target_without_status_tracking \
      --skip system::packages::brew::cask::tests::structured_copy_rollback_removes_target_with_created_parent
  '';

  meta = with lib; {
    description = "Dev tools, env vars, and tasks in one CLI";
    homepage = "https://github.com/jdx/mise";
    license = licenses.mit;
    mainProgram = "mise";
  };
}
