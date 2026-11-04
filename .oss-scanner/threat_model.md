# Threat model

## What this project does and where untrusted input enters

mise is a Rust CLI that installs developer tools, manages per-directory environment variables and runs tasks.
Treat these as attacker-controlled:

- Files in a cloned repository that mise reads before the user has trusted it. Config files are only
  executed or templated once trusted (`mise trust`); a way to run code, read secrets or write files from an
  untrusted config is a real bug.
- Network responses from tool sources: GitHub/GitLab/Forgejo releases, HTTP and S3 downloads, the aqua
  registry, packslip manifests, npm/cargo/pip/gem/go registries, and the mise-versions mirror.
- Downloaded archives (tar, zip, 7z) and the filenames inside them.
- Version strings, tool names and backend options from config or the network.
- Shell input to `mise activate`, shims and hook-env (directory names, env var values, PATH).

Out of scope as inputs: the user's own trusted config, their own shell, and anything that already requires local code execution as that user.

## Components that matter most / least

Most: download and extraction (path traversal, symlink and hardlink escapes), checksum, signature and
provenance verification (a bypass or downgrade is a bug), trust handling, template rendering, shell
activation output (quoting and injection), shims, lockfile handling, credential and token handling
(leaks into logs, env, URLs, or sending a token to the wrong host).
Least: docs/, e2e/, benchmarks/, scripts/, xtasks/, and CI workflow files. `crates/vfox` Lua plugins
run with the user's privileges once installed, so installing a plugin is trusted; report only sandbox or
trust-boundary escapes beyond that.

## How to exercise it

`target/debug/mise` is already built. `cargo test` runs unit tests; `e2e/` holds bash tests (run through `mise run test:e2e <file>`).
Use a throwaway `HOME`, `MISE_DATA_DIR`, `MISE_CONFIG_DIR` and `MISE_STATE_DIR` so a proof of concept does not touch real state.

## How you rate severity

- Critical: code execution or arbitrary file write from untrusted config without trust, from a malicious
  or tampered download that passes verification, or from a crafted archive escaping the install dir.
- High: bypass of trust or of checksum/signature verification; credential disclosure to a third party;
  injection into emitted shell code. Rate by demonstrated impact: a bypass that leads to code execution
  or arbitrary file write is Critical, and the highest applicable level wins.
- Medium: denial of service from untrusted input (crash, unbounded memory or disk), local-only
  information disclosure.
- Low: hardening gaps with no demonstrated exploit.

## Anything to leave alone

- Behavior that requires the user to run `mise trust`, install a plugin, or add a tool source they chose.
- A malicious tool being malicious after it is correctly installed.
- Reports that need a pre-compromised machine, an attacker-controlled `PATH` or `HOME`, or root.
- Panics reachable only through `unwrap` on trusted internal state that untrusted input cannot trigger or
  influence. A panic an attacker can trigger stays in scope (Medium, see above).

Reports should include a runnable reproducer; a minimal patch is preferred over a large refactor.
