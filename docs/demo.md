---
description: "Watch mise run one-off commands with mise exec, install global tools, and switch Node.js versions between a global default and a project."
socialDescription: "Watch mise run one-off commands, install global tools, and switch Node.js versions per project."
---

# Demo

This recording runs one-off commands with `mise exec`, installs global tools
with `mise use --global`, and switches Node.js versions between a global default
and a project. It was recorded in March 2025 with Node.js 22 and 23; the
transcript below uses current versions.

<video style="max-width: 100%; height: auto;" controls="controls" src="./tapes/demo.mp4" />

## Guided transcript {#transcript}

The commands follow the recording with current versions, so the versions and
output you see differ from the video. To follow along,
[install mise](/installing-mise.html) and open a Bash shell. The demo writes
global defaults, so to keep them out of your normal global config, point mise
at a temporary one first:

```sh
export MISE_GLOBAL_CONFIG_FILE="$(mktemp -d)/config.toml"
```

### Run one command

```sh
mise exec node@26 -- node --version
mise exec terraform -- terraform version
```

`mise exec` installs a missing tool and makes it available to that one command.
It does not add the tool to the calling shell or save it in `mise.toml`, so a
plain `node --version` afterward runs whatever Node.js was already on `PATH`, if
any.

### Activate and choose global defaults

```sh
eval "$(mise activate bash)"
mise use --global node@lts
node --version
which node
```

Activation updates `PATH` each time the prompt appears, so the selected Node.js
is on `PATH` for the next command. `lts` is a version request that the Node.js
backend resolves to the current long-term support release, so the version it
selects changes over time. `which node` prints the installed Node.js binary
itself, not a shim.

Add other global tools and inspect their selection:

```sh
mise use --global terraform jq go
terraform version
jq --version
go version
mise ls --current
```

### Override defaults in a project

```sh
mkdir myproj
cd myproj
mise use node@26 pnpm@10
node --version
pnpm --version
cat mise.toml
```

The project config contains:

```toml
[tools]
node = "26"
pnpm = "10"
```

Within this project, Node.js 26 overrides the global `lts` request. Leave the
project, and mise restores the global selection; most shells update on `cd`
([directory changes](/dev-tools/shims.html#hook-on-cd)):

```sh
cd ..
node --version
mise ls --current
```

For a first project with tools, environment variables, and tasks, continue with
[Getting started](/getting-started.html). To bring mise into a project that
already exists, see [Use mise in an existing project](/walkthrough.html).
