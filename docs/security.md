# Security

mise provides controls for different parts of a development workflow. Choose the control
that addresses the operation you are running:

| Control                                | Applies to                                      | Purpose                                                          |
| -------------------------------------- | ----------------------------------------------- | ---------------------------------------------------------------- |
| Download verification                  | Supported tool installations                    | Check artifact integrity and available signatures or provenance. |
| [Configuration trust](/cli/trust.html) | Loading project configuration                   | Decide which configuration mise may execute or apply.            |
| [Safe mode](#safe-mode)                | Processing untrusted project configuration      | Disable project code execution and environment injection.        |
| [Paranoid mode](/paranoid.html)        | Trust and supported installation verification   | Require content-bound trust and recheck recorded provenance.     |
| [Sandboxing](/sandboxing.html)         | Commands launched by `mise exec` and `mise run` | Restrict child-process access on supported platforms.            |

These controls have different scopes. For example, trusting configuration does not verify
an upstream release, and sandboxing a task does not sandbox mise's configuration evaluation.
Report vulnerabilities through [SECURITY.md](https://github.com/jdx/mise/blob/main/SECURITY.md).

## Software verification

Verification depends on the backend and the upstream metadata. A checksum detects whether
bytes match an expected digest; a signature or provenance check also associates an artifact
with a signer or build identity. The expected checksum, key, or identity must come from a
source you trust.

For aqua tools with the corresponding registry metadata, mise has built-in support for
Cosign and Minisign signatures, SLSA provenance, and GitHub artifact attestations.
OpenPGP verification for Node.js and Swift is also built in; it does not need an external
`gpg` executable. [packslip](/dev-tools/backends/packslip.html) verifies signed manifests and
artifact digests.

Aqua's verification methods are enabled by default. See the
[aqua settings](/dev-tools/backends/aqua.html#settings) for individual controls. If verification
fails, check the selected artifact and the underlying error before changing those settings;
see [checksum errors](/errors.html).

A [lockfile](/dev-tools/mise-lock.html) can record checksums and provenance. By default, a
supported installation can reuse recorded provenance instead of repeating its verification.
Use [`locked_verify_provenance`](/configuration/settings.html#locked_verify_provenance) or
[paranoid mode](/paranoid.html#provenance-re-verification) when installation should recheck it.
This does not audit the contents of tools that are already installed and skipped.

## Safe mode

Set `MISE_SAFE=1` when automation must process configuration it does not control, such as
resolving tool versions from a pull request. For example:

```sh
MISE_SAFE=1 mise lock --bump --dry-run --json
```

Remove `--dry-run` when the command should update the lockfile. Safe mode does not itself
make a command read-only.

### Refused operations

Safe mode returns an error for operations that would execute project-selected code:

- Template `exec()` and `read_file()` calls.
- Task execution.
- Tool-level `postinstall` hooks and `install_env` during installation.
- asdf plugin scripts and plugin installation.

### Ignored configuration

Some behavior is suppressed so metadata queries can still run:

- Shell and installation hooks are skipped, like `--no-hooks`.
- Project `[env]`, environment directives, and `[shell_alias]` do not affect the environment.
- Project `[settings]` are ignored, preventing a repository from disabling verification or
  redirecting a backend through settings.
- `_.source` is ignored everywhere, including global configuration.

Operator-owned global and system configuration still applies, apart from these explicit
restrictions. Review that configuration and the environment of the process running mise.
The [`safe`](/configuration/settings.html#safe) setting is global-only, so a project cannot
turn it off for itself.

### Trust and backend support

Safe mode loads otherwise untrusted configuration without a trust prompt or untrusted-config
error, because the project execution and environment features above are disabled. This also
applies when paranoid mode is enabled. Syntax errors and unsupported operations still fail;
a successful load does not mean the configuration has been marked trusted for later normal use.

HTTP-based version resolution continues to work for supported backends. The Go backend uses
`GOTOOLCHAIN=local` to prevent a project `go.mod` from triggering a toolchain download during
metadata queries. Already-installed or embedded vfox plugin code can still run; a missing
plugin cannot be installed in safe mode.

Safe mode controls how mise handles project configuration. It is not an operating-system
sandbox for the mise process, its network requests, or operator-installed plugins. Use
[sandboxing](/sandboxing.html) for child-process restrictions, accounting for platform limits.

## Minimum release age

To limit supply-chain risk, you can restrict mise to only install versions released before a
certain date or duration. Newly published versions can be held back for a configurable period. This delay gives
time for problems to be discovered; it is not evidence that an older release is trustworthy.

```toml
# mise.toml
[settings]
minimum_release_age = "7d"  # only install versions released more than 7 days ago
```

The setting supports relative durations (`7d`, `6mo`, `1y`) and absolute dates (`2024-06-01`). For most
backends, it only affects fuzzy version resolution, such as `node@24` or `latest`.
Explicitly pinned versions like `node@24.0.0` bypass the filter.
During ordinary toolset resolution, already-installed fuzzy matches remain eligible:
`minimum_release_age` limits remote version selection and does not make an installed version
inactive. Lockfile generation may re-check fuzzy installed matches against release metadata.

Capability depends on the backend:

| Capability                                     | Backends                                                                                                                               |
| ---------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| Top-level version filtering                    | Backends that provide release timestamps, such as `aqua:`, `cargo:`, `github:`, `gitlab:`, `go:`, `npm:`, `pipx:`, and many core tools |
| Transitive dependency filtering during install | `npm:` and `pipx:`                                                                                                                     |

Versions without timestamps are included by default. Backends without transitive dependency support
may still select an older top-level tool version, but they do not constrain dependencies fetched by
the tool's installer/compiler.

For details on `npm:` and `pipx:` transitive dependency support, see the
[npm backend docs](/dev-tools/backends/npm.html) and
[pipx backend docs](/dev-tools/backends/pipx.html).

You can also set `minimum_release_age` per-tool to override the global setting:

```toml
# mise.toml
[settings]
minimum_release_age = "7d"  # default for all tools

[tools.trivy]
version = "latest"
minimum_release_age = "1d"  # trivy updates are time-sensitive, use a shorter window
```

Precedence: `--minimum-release-age` CLI flag > per-tool `minimum_release_age` > global
`minimum_release_age` setting.

Use `minimum_release_age_excludes` to exclude tools or backends from the global/default setting:

```toml
[settings]
minimum_release_age = "7d"
minimum_release_age_excludes = ["trivy", "npm:*"]
```

Exclusions can match backend wildcards like `npm:*`, tool shorthands like `trivy`, or full backend
IDs like `npm:prettier`. Matching tools skip the global setting and built-in default. Per-tool
`minimum_release_age` options and the CLI flag still apply even when a tool matches the exclusion
list. Exclusions from multiple config files are merged and deduplicated, so project
configuration can add exclusions without repeating those from the global configuration.

See [`minimum_release_age`](/configuration/settings.html#minimum_release_age) for the setting
reference.
