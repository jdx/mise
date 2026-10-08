---
description: "Install conda packages and their dependencies from conda-forge or another channel, without conda."
---

# conda backend

The `conda` backend installs command-line packages and their dependencies from
[conda-forge](https://conda-forge.org/) or another conda channel. mise solves
and downloads the packages itself, so you do not need conda, mamba or
micromamba.

## Requirements

<span id="dependencies"></span>
<span id="platform-support"></span>

Nothing beyond mise. The package must be built for your platform, or be a
`noarch` package; mise picks the platform's conda subdirectory:

| Platform    | Conda subdir    |
| ----------- | --------------- |
| Linux x64   | `linux-64`      |
| Linux ARM64 | `linux-aarch64` |
| macOS x64   | `osx-64`        |
| macOS ARM64 | `osx-arm64`     |
| Windows x64 | `win-64`        |

The solver also considers `noarch` packages. A `noarch` package can still
depend on platform-specific packages, so it is not guaranteed to install on
every host. Native requirements such as a compatible libc or GPU driver still
come from the host.

## Usage

Install FFmpeg in the current project and run it:

```sh
mise use conda:ffmpeg
mise exec -- ffmpeg -version
```

This writes the following to `mise.toml`. Add `-g` for your global config.

```toml
[tools]
"conda:ffmpeg" = "latest"
```

<span id="specifying-a-version"></span>

Run `mise ls-remote conda:ffmpeg` to list versions, and pin one with
`mise use conda:ffmpeg@8.1.2`.

## How commands run

<span id="limitations"></span>

Each package is installed into its own conda prefix with its dependencies. Only
the requested package's commands reach your `PATH`; its dependencies' commands
do not.

If the prefix needs activating (the package ships `etc/conda/activate.d`
scripts, the prefix's `bin` directory holds executables from its dependencies,
or one of its commands is a script), mise starts its commands through a
launcher. The launcher sets `CONDA_PREFIX`, puts the prefix's executables ahead
of your `PATH`, and runs the activation scripts. Programs the command starts
inherit that `PATH`.

A single-binary package such as `conda:ripgrep` needs no activation, so on Unix
mise symlinks its commands directly. They start without an extra shell process
but do not see `CONDA_PREFIX`. On Windows every command goes through a launcher.

mise solves one package per tool in an isolated prefix. It does not read or
maintain an `environment.yml`.

## Mirrors

To fetch conda-forge or another channel through a mirror, map its URL with
[`url_replacements`](/url-replacements.html). mise applies the replacements to
channel metadata and package downloads, and refuses a replacement that would
send credentials from an `https://` URL to an `http://` one.

## Tool options

Set these on the tool's entry in `[tools]`, or inline, as in
`'conda:my-tool[channel=my-team]'`. Options every backend accepts are described
under [tool options](/dev-tools/#tool-options).

### `channel`

<span id="using-a-different-channel"></span>
<span id="common-channels"></span>

Install one package from a channel other than [`conda.channel`](/dev-tools/backends/conda.html#conda.channel),
which defaults to `conda-forge`:

```toml
[tools]
"conda:my-tool" = { version = "latest", channel = "my-team" }
```

The value can be a channel name on anaconda.org or a full channel URL, such as
`https://conda.example.com/my-team`, for a private or mirrored conda server.

mise solves against that one channel, so the package and all of its
dependencies must come from it. Channels that build on conda-forge, such as
bioconda, usually cannot be used on their own for this reason.

## Settings

<script setup>
import Settings from '/components/settings.vue';
</script>
<Settings child="conda" :level="3" />

## Troubleshooting

| Symptom                  | What to check                                                                                                   |
| ------------------------ | --------------------------------------------------------------------------------------------------------------- |
| Command not found        | Check that the package itself provides that command. Commands from its dependencies are not put on your `PATH`. |
| Solving fails            | Check that the package and every dependency exist in the selected channel for the platform named in the error.  |
| Package is not available | It may not be built for your platform; look for your subdir in the package's files on anaconda.org.             |

Implementation: [`src/backend/conda.rs`](https://github.com/jdx/mise/blob/main/src/backend/conda.rs).
