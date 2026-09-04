# packslip Backend <Badge type="warning" text="experimental" />

The `packslip` backend installs a release from the vendor's own signed release manifest, a [packslip](https://packslip.dev). One file per release, `packslip.sigstore.json`, says what shipped and how to verify it: checksums, platforms, executables, and provenance links, in a standard sigstore bundle. mise verifies it against the identity the project name implies and installs exactly what it lists, so nothing is guessed from file names and no registry entry is needed.

The code for this is inside of the mise repository at [`./src/backend/packslip.rs`](https://github.com/jdx/mise/blob/main/src/backend/packslip.rs).

::: warning
packslip is a work-in-progress proposal and this backend is experimental: enable it with `mise settings experimental=true`. The format still changes between releases, and few projects publish one yet.
:::

## Why publish one

The backends most people use today work from the outside of a release:

- `github:owner/repo` picks an asset by scoring file names against the host (OS, arch, libc, format). That holds until a project renames its assets, ships two builds for one platform, or puts the executable somewhere unexpected; then every user needs `asset_pattern` or `bin` in their config. Nothing ties the file to the project unless the project also publishes checksums or provenance in a form mise recognises.
- `aqua:owner/repo` reads a registry entry that volunteers maintain for the project, with checksums and, for some packages, cosign or SLSA checks. When a project changes its asset names the entry is wrong until someone sends a fix ([pnpm v11](https://github.com/aquaproj/aqua-registry/pull/52822), [doggo v1.2.0](https://github.com/aquaproj/aqua-registry/pull/55890), [go-jsonnet v0.22.0](https://github.com/aquaproj/aqua-registry/pull/50942), [wrkflw v0.8.0](https://github.com/aquaproj/aqua-registry/pull/59777)), and the snapshot built into mise picks it up a release later.
- `packslip:owner/repo` reads what the project itself published with the release, signed by the project. Nothing to guess, nobody to wait for.

For a vendor:

- **A direct line to your users.** You say which file is for which platform, which executables it holds and what they are called on PATH, and what the host needs. Your users get that with the release itself, not after someone updates a registry, and when you change a layout or a naming scheme the next packslip says so and no install breaks.
- **You ship more than binaries.** Shell completions, man pages, a [usage](https://usage.jdx.dev) spec of the CLI, and [agent skills](https://packslip.dev/release/v1/#resources), each declared once and installed in the version the user actually has.
- **On GitHub, one step in the release job.** `uses: jdx/packslip@v1` with the artifact glob signs the manifest keylessly with the workflow's own identity, attests build provenance, and uploads it. There is no key to create, store, or rotate. On another forge, or a plain web server, `packslip create` signs with a key you hold and you publish a signed release list at a well-known URL on your domain; users pin the key in their config.
- **Nothing between you and the user can swap the bytes.** A mirror, a proxy, or a compromised download host cannot pass off other bytes as your release, because the signer is your workflow or your key.

For a user asking a vendor for one:

- **The name says who may sign, the way `~/.ssh/known_hosts` says which key a host must present.** `packslip:github.com/owner/repo` accepts only a packslip signed by that repository's release workflow through GitHub's issuer; the signature, its transparency log entry, the statement, and the artifact's digest and size are all checked before anything is unpacked. mise then remembers the signer it accepted, as [Pinned signers](#pinned-signers) describes, and refuses a later release from anyone else.
- **The right artifact, with no registry to update first.** musl and gnu, universal binaries, a `fips` or `baseline` variant, the executable's path inside the archive: all from the manifest the vendor wrote when it cut the release, so nobody plays catch-up after a rename.
- **Completions and skills that match the version in use**, from the vendor, with no per-tool setup in mise or in your shell config.
- **Upgrades are checked the way first installs are.** A checksum in `mise.lock` proves one download is the same file it was the first time. A packslip proves the next version came from the same signer as well, so an upgrade on a teammate's machine or in CI is checked against the project's identity rather than against a hash nobody could verify when it was first recorded. `mise.lock` records that signer next to the URL and digest, so a machine with the lockfile refuses a release signed by anyone else even on its first install.

The specification and the `packslip` CLI are at [packslip.dev](https://packslip.dev).

## Usage

A project is named the way Go names a module: a host, then a path. On github.com the name is also the pin: the packslip must be signed by a workflow of that repository, through GitHub's OIDC issuer.

```sh
$ mise use -g packslip:github.com/jdx/packslip
$ packslip version
packslip 0.2.0
```

`packslip:owner/repo` means the same as `packslip:github.com/owner/repo`. A tool in a monorepo adds its subpath, `packslip:github.com/oxc-project/oxc/oxlint`, and mise reads that tool's own `packslip.oxlint.sigstore.json` from the shared release.

A project on its own domain publishes a signed release list at `https://<host>/.well-known/packslip/<path>.json`. Nothing implies its signer, so pin it yourself:

```toml
[tools]
"packslip:tool.example.com" = { version = "latest", pubkey = "RWQ..." }
```

## What is verified

1. The bundle's signature, certificate chain, and transparency log entry, as sigstore defines them, against the pinned identity or key.
2. The statement's structure, and that it is for the project you asked for at the version you asked for.
3. The digest and size of the one artifact selected for this host, from the signed statement, before it is unpacked.

Only after that does mise unpack the artifact and put the executables the packslip names on PATH. The verified statement is kept in the install directory as `.mise-packslip.json`.

The artifact is chosen as the specification's consumer rules say. An artifact fits when each of its `os`, `arch`, and `libc` is absent or equal to the host's, so a universal macOS binary, a jar, or a script fits everywhere; among those that fit, the one naming the most of those three wins, so a build for the host beats a portable one; then mise's format preference decides, archives first (`tar.xz`, `tar.zst`, `tar.gz`, `tar.bz2`, `tar`, `zip`, `7z`), then a single compressed executable (`xz`, `zst`, `gz`, `bz2`), then a bare one. Installer formats such as `deb`, `dmg`, and `msi` are never chosen. When two artifacts still tie, mise refuses to guess; set `variant` to say which one you mean. A glibc host with no gnu build takes a musl build, which is static, and says so in the debug log.

## Pinned signers

mise remembers which signer it accepted a project's packslips from, the way SSH remembers hosts, in `packslip/pins.toml` under its state directory: the scheme, the workflow path (without its ref, since a new tag of the same workflow is the same signer) or key id, who attested, whether every artifact linked provenance, and the highest release-list sequence seen. A later release that comes from another signer, is a repackager's where the vendor's own was accepted before, or drops the provenance every artifact linked before is refused, as the specification's no-downgrade rule says; anything that got stronger is remembered as the new floor.

```sh
mise packslip pins                     # what is pinned, and from what
mise packslip forget github.com/o/r    # the vendor rotated a key: let the next release set the pin
```

A lockfile carries the project's commitment as well: each platform entry records the `signer` (as `scheme:signer`) and, for a repackager's document, `attested_by = "repackager"`, next to the URL and digest. A release signed by anyone else is refused on every machine that has the lockfile, whether or not that machine has seen the project before; removing the entry from `mise.lock` accepts the change for the project.

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `packslip` backend—these go in `[tools]` in `mise.toml`.

### `variant`

Selects one of several builds the vendor publishes for one platform, such as `fips` or `baseline`. Without it, only artifacts with no variant are considered.

```toml
[tools]
"packslip:github.com/example/tool" = { version = "latest", variant = "fips" }
```

### `pubkey`

For a project on its own domain: the vendor's public key, as the minisign-format line from its `.pub` file or the path of that file. The release list and every packslip must be signed with it.

### `identity`, `identity_prefix`, `issuer`

For a project on its own domain signed keylessly, or to override the identity a forge name implies: the exact certificate identity, or a prefix it must start with, and the OIDC issuer.

### `allow_unlogged`

Accept a key-signed bundle that carries no transparency log entry, which a vendor produces for an air-gapped release. Off by default; turn it on only for a vendor you have agreed to accept that from.

### `trust`

`trust = "vendor"` takes the vendor's own manifest under the vendor's pin, with no stamp from the hosts [`packslip.stampers`](/configuration/settings#packslip-stampers) names. See Stamps below. The choice is recorded in the lockfile, so an install from the lock does not quietly relax it later.

```toml
[tools]
"packslip:github.com/jdx/mise" = { version = "latest", trust = "vendor" }
```

## Stamps

A vendor's packslip proves what the vendor shipped. It does not say whether anyone else looked at it. A stamping host, a registry, a mirror, or a scanning service, publishes its own signed release list per vendor project at `https://<host>/.well-known/packslip/<project>.json`, naming the versions it checked, pinning the digest of each packslip it looked at, and saying what it checked. The [`packslip.stampers`](/configuration/settings#packslip-stampers) setting names the hosts you trust, each with its pin:

```toml
[settings.packslip]
stampers = ["registry.mise.jdx.dev=RWQ..."]
```

With hosts named, mise offers and installs a packslip tool only at versions one of them lists. A version no trusted host stamped is not shown by `mise ls-remote` and is refused by `mise install`, however valid the vendor's own manifest is; a host withdrawing its stamp does not veto another host’s non-yanked approval. Any one host’s non-yanked stamp suffices; vendor withdrawals still veto every stamp. The stamp points at the manifest it admitted, so mise fetches that exact file, checks it against the digest the host recorded, and then verifies it against the vendor's pin as always: the stamp never stands in for the vendor's signature. A stamp that records no digest is refused — the digest is what ties the host's review to a file, and without one the entry admits a URL rather than a manifest.

Each host's list carries an expiry and a sequence number. mise refuses a list that has expired, and remembers the highest sequence it accepted per host and project so a replayed older list is refused as a rollback.

A stamper may mirror the exact vendor-signed manifest. Mise checks both the stamper digest and any digest in the vendor’s list. Because the stamp already names the manifest, mise asks the vendor only for what the vendor decides — a withdrawal, and the digest they pinned — so a release asset deleted from GitHub does not veto a mirror of a release that was never withdrawn. Re-signed repackager manifests need a separate identity policy and are not supported by this backend yet.

Unset, no stamps are required and every version the vendor published is offered. mise's own registry host will become the default once it publishes stamps.

## Completions

A packslip may list what the release ships besides its executables: shell completions, a spec of the CLI, an agent skill. mise reads those `resources` for tools it installed this way.

```sh
mise completion zsh --tool rg           # print the script
mise completion zsh --tool rg --install # put a stub where zsh looks
```

The script comes from whichever version of the tool is active in the current directory. A vendor whose layouts differ by platform scopes an entry with `os`, `arch`, or `libc`; mise keeps the entries that apply to the artifact it installed and, of those, the most specific. It then takes the most verifiable source the vendor offered, in the order the specification gives: a file inside the artifact or a separate signed asset, a file from the source repository at the release's commit, a script derived from the tool's [usage](https://usage.jdx.dev) spec with the `usage` command, and only then a command of the tool's own. That last kind is how cobra, clap, and oclif tools ship completions, so mise runs it: the stub calls mise the first time your shell completes the command, which is when you were going to run the tool anyway, and mise caches the script beside the install so the command runs once per version rather than at every tab. No setting is needed. The [`packslip.exec`](/configuration/settings#packslip-exec) setting governs only exec resources mise would run at install time and write to disk, such as an agent skill.

`--install` writes a stub, not the script. Completions are global shell state while the active version depends on the directory, so the stub asks mise for the script when the shell completes the tool. In zsh and bash the vendor's script takes over for one completion and the stub is put back afterwards, so a version switch in another directory is followed on the next tab; fish and PowerShell load the script once per shell session. The file goes where the shell loads completions by name, the same place `mise completion zsh --install` puts mise's own, and the command prints any one-time line your shell still needs.

Files the vendor keeps outside the artifact (a separate release asset, or a path in the repository) are fetched at install time. An asset must match the digest the packslip signed; a repository file is pinned by the release's commit. Completions derived from a usage spec call `usage complete-word` at shell runtime, so install `usage` alongside such tools (`mise use -g usage`).

## Skills

A packslip may also declare an agent skill: a directory holding `SKILL.md` and whatever it references, in the Agent Skills format. mise fetches it at install time, from inside the artifact, from a separate signed asset, or from the source repository at the release's commit, so every installed version carries its own copy.

```sh
mise skills ls              # the skills of the tools active here
mise skills sync            # link them into .claude/skills
mise skills sync --prune    # and drop links for versions no longer active
```

Since a project pins its tool versions in `mise.toml`, mise knows exactly which version of each skill an agent working in that project should see. `mise skills sync` writes one symlink per skill into the project's `.claude/skills` (or `--dir` for another agent's location, `--global` for `~/.claude/skills`), pointing at the installed version's directory. Run it again after `mise use` changes a version and the links follow. Only links mise made are ever replaced or pruned; a directory or link of your own at a skill's name is left alone and reported.

Four settings shape this:

- [`skills.dir`](/configuration/settings#skills-dir) is where the links go, relative to the project root (or the home directory with `--global`): `.claude/skills` by default, `.agents/skills` for agents that look there.
- [`skills.auto_sync`](/configuration/settings#skills-auto-sync) runs the sync after every `mise install` and `mise use`, so the links never lag the versions in `mise.toml`.
- [`skills.prune`](/configuration/settings#skills-prune) makes removing links for versions no longer active the default, for the command and for auto sync.
- [`skills.fetch`](/configuration/settings#skills-fetch) turned off installs tools without their skills.

A skill the packslip offers only as a command of the tool's own (`tool skill`, printing `SKILL.md`) is generated at install time only when [`packslip.exec`](/configuration/settings#packslip-exec) is on, for the same reason as completions.

## Versions

A packslip version is semver, so mise ranks a project's versions by semver precedence whatever order GitHub lists its releases in, and treats a version with a prerelease part (`1.3.0-rc.1`, `1.4.0-nightly.20260904`) as a prerelease whatever the GitHub release's own flag says. Prereleases are skipped unless the `prerelease` tool option is set.

For a project on github.com, the versions are the repository's releases that carry a packslip, named by their tags as the specification reads them: the version itself, optionally after a `v`, and optionally after the tool's subpath, its last segment, or the repository name plus a separator (`v1.2.3`, `jq-1.7.1`, `oxlint_v1.0.0`), with loose spellings such as `v4.1` normalized to `4.1.0`. A tag that names no version is skipped. At install time mise checks that the packslip inside the release agrees with the version the tag named and refuses the release if it does not.

The repository may also keep a signed list at `.well-known/packslip.json` (or `.well-known/packslip/<tool>.json` for a monorepo tool) on its default branch, signed by the same identity as its packslips. When it does, a version the list marks withdrawn is dropped, a version it names has its packslip fetched from the URL the list gives and checked against the digest it signed, and a version it lists that the releases endpoint lacks is added.

For a project on its own domain, the versions are the entries of its signed release list, minus any the vendor has withdrawn. For either kind of list mise refuses one that has expired, and refuses one whose sequence is below the highest it has accepted for the project, which it remembers under its state directory.

Lockfiles record the artifact URL and its sha256 from the signed statement.

### Resource selection and command execution

Resources may name one exact `artifact` when layouts differ between archive formats or variants. Completions are selected for the command being completed; when a release has several executables, pass its command name to `--tool`. Generated scripts are cached separately for each installed version, executable, and shell.

Exec resources receive the manifest’s environment variables, with `{shell}` expanded for completions. They run in a temporary directory with the installed executables on PATH, no stdin, discarded stderr, and a five-second timeout. Failed or empty output is not cached. The same execution rules apply to skills when `packslip.exec` permits generating them.

### Release-list continuity and minimum age

Once mise has accepted a supplementary signed list, a missing list is an error: a 404 cannot silently restore releases the vendor withdrew. `mise packslip forget <project>` explicitly resets that remembered policy along with the signer pin.

The release API or list timestamp helps select candidates. Before downloading an artifact, mise checks the verified transparency-log timestamp against the effective `minimum_release_age` cutoff. Only a permitted unlogged bundle uses its signed publication timestamp instead.

## Latest

An unconstrained `latest` request uses the vendor’s signed release-list `latest` pointer first. On GitHub, absent a signed pointer, mise consults GitHub’s latest release. Without an eligible recommendation it falls back to highest eligible semver. Prefix and channel requests keep their existing matching rules and never use the default pointer.

Mise verifies each candidate manifest before accepting it, including its identity and digest pins. Minimum release age uses the verified log time (or signed publication time for explicitly allowed unlogged bundles), and stamping and host requirements still apply. A policy exclusion warns and tries the next candidate; a signature, digest, identity, or list freshness failure stops resolution. A signed pointer that is ineligible falls straight back to semver, not to GitHub’s pointer. Resolution and version listing need online policy checks and bypass mise’s remote-version cache so changed stamper trust, withdrawals, expired lists, or missing accepted lists take effect.

## Host requirements

After selecting the artifact, mise checks its declared minimum OS/glibc and host libraries before downloading it. Confirmed failures refuse installation; `ignore_requirements = true` overrides this for one tool. Missing or old commands warn, and unknown checks warn instead of refusing. Active mise command paths take precedence over ambient PATH. Requirements never break an artifact-selection tie or select a different build.

On Linux the OS version is the kernel release from `uname -r`, read up to the distribution's suffix: `6.8.0-31-generic` is compared as `6.8.0`. OS and glibc probes and command version probes have bounded execution and output. Linux library checks use `LD_LIBRARY_PATH`, standard directories, and `ldconfig -p`; Windows uses PATH and the system directory. macOS checks common library directories but reports unknown absence because the dyld shared cache can contain libraries with no filesystem entry.

Resource generators have a five-second deadline including inherited output pipes and a 4 MiB output limit. Their child processes are cleaned up on completion, failure, timeout, and cancellation.
