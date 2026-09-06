# Conda Backend

The `conda` backend installs command-line packages and their transitive
dependencies from [conda-forge](https://conda-forge.org/) or another Anaconda
channel. It solves dependencies and downloads packages directly, so conda,
mamba, and micromamba do not need to be installed.

Commands from the selected package run inside that package's isolated conda prefix. mise sets
`CONDA_PREFIX`, makes the prefix's executable directories available to the command process, and applies
`etc/conda/activate.d` scripts before starting it. This lets a command use its packaged runtime
dependencies without adding dependency commands to your interactive shell's `PATH`.

The code for this is inside the mise repository at [`./src/backend/conda.rs`](https://github.com/jdx/mise/blob/main/src/backend/conda.rs).

## Dependencies

No separate conda package manager is required. The selected packages must still
support your operating system, architecture, and native runtime environment.

## Usage

Install ruff in the current project and verify its executable:

```sh
mise use conda:ruff
mise exec -- ruff --version
```

This writes the following to `mise.toml`. Add `-g` for global configuration.

```toml
[tools]
"conda:ruff" = "latest"
```

### Specifying a Version

List versions with `mise ls-remote conda:ruff`, then select one with
`mise use conda:ruff@VERSION`. Replace `VERSION` with a listed release.

### Using a Different Channel

The default channel is `conda-forge`. For a package published in your team's
channel, replace these placeholders with its package and channel names:

```toml
[tools]
"conda:my-tool" = { version = "latest", channel = "my-team" }
```

The solver uses the selected channel for the package and its dependencies. The
complete dependency set must be available there; this is not a multi-channel
conda environment specification.

## Platform Support

The conda backend automatically selects the appropriate package for your platform:

| Platform    | Conda Subdir  |
| ----------- | ------------- |
| Linux x64   | linux-64      |
| Linux ARM64 | linux-aarch64 |
| macOS x64   | osx-64        |
| macOS ARM64 | osx-arm64     |
| Windows x64 | win-64        |

The solver considers both the platform subdirectory and `noarch`. A `noarch`
package may still depend on platform-specific packages, so it does not guarantee
that an installation works on every host.

## Settings

Set these with `mise settings set [VARIABLE]=[VALUE]` or by setting the environment variable listed.

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="conda" :level="3" />

## Tool Options

The following [tool-options](/dev-tools/#tool-options) are available for the `conda` backend—these
go in `[tools]` in `mise.toml`.

### `channel`

Override the conda channel for a specific package:

```toml
[tools]
"conda:my-tool" = { version = "latest", channel = "my-team" }
```

## Common Channels

- `conda-forge` - Community-maintained packages (default)
- `bioconda` - Bioinformatics packages
- `nvidia` - NVIDIA CUDA packages

## Limitations

- mise solves and installs transitive dependencies in an isolated prefix for each tool. It does not import or maintain a general-purpose `environment.yml`.
- Only commands belonging to the requested package are exposed to your shell. Dependency executables remain available inside that tool's launcher environment.
- The solver uses one channel per tool. Packages from channels such as bioconda may require dependencies from another channel that this configuration cannot supply.
- Native requirements such as a compatible libc or GPU driver still belong to the host.

If a command cannot be found, check whether the requested package actually
provides a CLI. If solving fails, check package availability, the selected
channel, and the platform reported in the error before changing versions.
