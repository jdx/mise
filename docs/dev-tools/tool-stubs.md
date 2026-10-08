---
description: "Commit an executable file that names one tool and version, so ./bin/tool installs and runs it through mise."
---

# Tool stubs

A tool stub is an executable file that records how to get and run one tool.
Commit it to a repository so that a command such as `./bin/py` runs the
intended version and passes its arguments along. mise installs the tool the
first time the stub runs; `mise install` does not look for stub files.

Use a stub when the executable file itself should carry the tool definition,
for example a script directory that works without a project `mise.toml`. For
a set of tools that should install when first used, prefer
[`lazy = true` in `[tools]`](/dev-tools/shims.html#lazy-tools). For a one-line
wrapper without download metadata, see
[a `mise exec` wrapper](#alternative-creating-simple-stubs-with-mise-x). Tool
stubs are modeled on Meta's [dotslash](https://github.com/facebook/dotslash).

A stub needs `mise` on `PATH`, unless it was generated with
[`--bootstrap`](#bootstrap), which installs mise first.

## Write a stub {#tool-non-http-stubs}

A stub starts with a shebang that runs `mise tool-stub`, followed by TOML that
names the tool. Save this as `bin/py`:

```toml [bin/py]
#!/usr/bin/env -S mise tool-stub

tool = "python"
version = "3.14"
bin = "python"
```

Make it executable and run it:

```sh
chmod +x ./bin/py
./bin/py --version
./bin/py -c 'import sys; print(sys.executable)'
```

The stub is named `py` but runs the installed `python` executable, and
arguments after the stub path go to Python. A backend stub resolves `version`
the way `mise.toml` does; use an exact version or [lock the
stub](#locked-tool-stub) when everyone must get the same build.

`env -S` splits the rest of the shebang line into separate arguments, because
a Unix shebang passes at most one argument to its interpreter.

## Stub fields {#configuration-fields}

A stub holds one tool declaration, not a whole `mise.toml`. Put the fields at
the top level, without a `[tools]` table:

| Field                     | Default                                                      | Purpose                                                        |
| ------------------------- | ------------------------------------------------------------ | -------------------------------------------------------------- |
| `tool`                    | `http:<file name>` when a URL is present, else the file name | Tool name or backend, such as `python` or `github:cli/cli`     |
| `version`                 | `latest`                                                     | [Version request](/dev-tools/versions.html)                    |
| `bin`                     | The stub's file name                                         | Executable to run, relative to the installation                |
| `os`                      | All platforms                                                | Platforms the stub runs on, such as `["linux", "macos/arm64"]` |
| `install_env`             | None                                                         | Environment variables for the install                          |
| `url`, `checksum`, `size` | None                                                         | An HTTP download used on every platform                        |
| `[platforms.<os>-<arch>]` | None                                                         | Per-platform `url`, `checksum`, `size`, and `bin`              |

Any other key goes to the backend as a [tool option](/dev-tools/#tool-options).
For example, this stub installs the GitHub CLI from its releases:

```toml [bin/gh]
#!/usr/bin/env -S mise tool-stub

tool = "github:cli/cli"
version = "latest"
bin = "gh"
```

## Download from URLs {#http-stubs}

A stub with download URLs and no `tool` field uses the
[HTTP backend](/dev-tools/backends/http.html). A top-level `url` applies on
every platform, so use it only for an artifact that runs everywhere, such as a
JAR or a script. For platform-specific binaries, give each platform a table.
The URLs below are placeholders; [generate](#generating-tool-stubs-http) a stub
to record real download metadata.

```toml [bin/tool]
#!/usr/bin/env -S mise tool-stub

version = "1.0.0"

[platforms.linux-x64]
url = "https://releases.example.com/v{{version}}/tool-linux-x64.tar.gz"

[platforms.macos-arm64]
url = "https://releases.example.com/v{{version}}/tool-macos-arm64.tar.gz"
```

With `version` left at `latest`, an HTTP stub installs whatever its URLs point
to. To upgrade, change the URLs and regenerate the checksums.

### Executable paths {#platform-specific-binary-paths}

Set `bin` relative to the installed directory, after mise strips the archive's
top-level directory. Set it per platform when layouts or file names differ:

```toml [bin/tool]
#!/usr/bin/env -S mise tool-stub

bin = "bin/tool" # used by platforms without their own bin

[platforms.linux-x64]
url = "https://example.com/tool-linux.tar.gz"

[platforms.windows-x64]
url = "https://example.com/tool-windows.zip"
bin = "tool.exe"
```

The generator detects the executable in each archive and writes a single `bin`
when every platform agrees, or per-platform `bin` fields when they differ.

## Generate a stub {#generating-tool-stubs-http}

[`mise generate tool-stub`](/cli/generate/tool-stub.html) writes HTTP stubs.
It downloads the artifact, records its checksum and size, finds the
executable, and makes the file executable. Pass `--platform-url`, and mise
detects the platform from the file name:

```sh
mise generate tool-stub ./bin/rg \
  --platform-url https://github.com/BurntSushi/ripgrep/releases/download/15.2.0/ripgrep-15.2.0-x86_64-unknown-linux-musl.tar.gz
```

```toml [bin/rg]
#!/usr/bin/env -S mise tool-stub

[platforms.linux-x64]
url = "https://github.com/BurntSushi/ripgrep/releases/download/15.2.0/ripgrep-15.2.0-x86_64-unknown-linux-musl.tar.gz"
checksum = "blake3:85bc9b662e1868a010691ae699ac4fb7c87f79fa9075a0efa14478164aafd4e7"
size = 2265718 # 2.2 MiB
```

mise found `rg` in the archive, and because the stub is also named `rg`, it
needed no `bin` line. A stub with another name gets `bin = "rg"`. Pass `--bin`
when an archive holds several executables and mise picks the wrong one.

Run the command again with another platform's URL to add it to the same stub:

```sh
mise generate tool-stub ./bin/rg \
  --platform-url https://github.com/BurntSushi/ripgrep/releases/download/15.2.0/ripgrep-15.2.0-aarch64-apple-darwin.tar.gz
mise generate tool-stub ./bin/rg \
  --platform-url https://github.com/BurntSushi/ripgrep/releases/download/15.2.0/ripgrep-15.2.0-x86_64-pc-windows-msvc.zip
```

Re-running a platform replaces its entry, so review the diff before you commit.
Prefix a URL with the platform, as in `linux-x64:https://...`, when its file
name does not say which platform it is for.

Pass `--url` instead only for an artifact that runs on every platform. See
[`mise generate tool-stub`](/cli/generate/tool-stub.html) for every flag.

A checksum detects later changes to the artifact. It does not prove who
published the first download, so take the URL from a source you trust.

The generator reads `.tar.gz`, `.tgz`, `.tar.xz`, `.txz`, `.tar.bz2`, `.tbz2`,
`.tar.zst`, `.tzst`, `.zip`, and `.7z` archives.

### Checksums for other consumers {#checksums}

Checksums are `blake3` by default. For consumers such as Bazel that need
SHA-256, pass `--checksum-algorithm sha256`:

```sh
mise generate tool-stub ./bin/rg --checksum-algorithm sha256 \
  --platform-url https://github.com/BurntSushi/ripgrep/releases/download/15.2.0/ripgrep-15.2.0-x86_64-unknown-linux-musl.tar.gz
```

`--skip-download` writes a stub without downloading anything, so it has no
checksum, size, or detected `bin`. Check the `bin` field, then run
`mise generate tool-stub ./bin/rg --fetch` to fill in the missing checksums and
sizes. `--fetch` uses the algorithm you pass with `--checksum-algorithm` and
keeps existing checksums. `--checksum-algorithm` cannot be combined with
`--lock` or `--skip-download`, which do not calculate checksums.

## Stubs that install mise {#bootstrap}

Add `--bootstrap` to write a bash script instead of a TOML file, for teammates
who may not have mise:

```sh
mise generate tool-stub ./bin/rg --bootstrap \
  --platform-url https://github.com/BurntSushi/ripgrep/releases/download/15.2.0/ripgrep-15.2.0-x86_64-unknown-linux-musl.tar.gz
```

The script uses `mise` from `PATH` or `~/.local/bin/mise`. If neither exists,
it installs mise into `~/.local/bin`, then runs the stub, which is embedded in
the script as comments. `--bootstrap-version 2026.10.4` pins the mise version it
installs.

## Lock a stub {#locked-tool-stub}

Locking a backend stub records a concrete version and the platform download
metadata its backend provides, in the `mise.lock` of the nearest project config
above the stub. Installs then use the recorded URLs, verify the recorded
checksums, and work in locked mode (`--locked` or `MISE_LOCKED=1`). Stored URLs
skip release discovery on later installs, but they do not remove a private
download's authentication or every backend's verification requests.

<a id="locking-a-stub"></a>

`mise generate tool-stub` writes only HTTP stubs, and `--lock` works on any
existing stub, so write a backend stub by hand first:

```sh
mkdir -p bin
cat > bin/node <<'STUB'
#!/usr/bin/env -S mise tool-stub
tool = "node"
version = "24"
STUB
chmod +x bin/node

# Resolve the version and record platform URLs and checksums in mise.lock
mise generate tool-stub ./bin/node --lock
```

The stub keeps its version request (`24`), just as `mise.toml` does, and
`mise.lock` records the resolved version for the lockfile's existing
platforms, or the default platforms for a new lockfile. `--lock` fails when no
project config is found above the stub.

```toml [mise.lock]
lockfile_version = 3
tool-stubs = ["bin/node"]

[[tools.node]]
version = "24.21.0"
backend = "core:node"
specifiers = ["24"]
# ...platform URLs and checksums
```

`tool-stubs` lists the stubs whose entries the lockfile holds, relative to the
lockfile. No config declares those tools, so the list is how `mise lock` knows
to keep their entries and refresh them with the rest of the lockfile. When a
listed stub is deleted, `mise lock` removes it from the list and prunes its
entry.

A stub finds its lockfile from where the stub file lives, not from the
directory it runs in, and a symlinked stub is followed to its real location.
`./bin/node` and a `~/.local/bin/node` symlink to it both use the project's
`mise.lock` from any working directory. Local and environment configs
(`mise.local.toml`, `mise.{env}.toml`) are not used, so a committed stub locks
the same way on every machine. In locked mode, a stub without an entry for the
current platform is rejected like any unlocked tool.

### Bumping a locked version {#bumping-a-locked-version}

Run `--lock` again to pick up a newer version; it resolves the stub's request
from scratch. Pass `--version` to change the request itself:

```sh
mise generate tool-stub ./bin/node --lock --version 26
```

`mise lock --bump` also re-resolves the requests of the stubs listed in the
lockfile.

## Run a stub {#usage}

Stubs written by `mise generate tool-stub` are already executable. Run
`chmod +x` only on stubs you write by hand.

Each run goes through mise, which caches the resolved executable until the stub
changes. In a tight loop, run the script under `mise exec` and call the tool
directly.

To debug a stub, run it through [`mise tool-stub`](/cli/tool-stub.html):

```sh
mise tool-stub ./bin/rg --version
```

### On Windows {#on-windows}

Windows cannot run a shebang script, so `mise generate tool-stub` also writes a
`.cmd` launcher beside the stub. Run `.\bin\rg.cmd`, or `rg` when `bin` is on
`PATH`, and Windows finds the launcher through `PATHEXT`.

mise writes the launcher whenever the stub could run on Windows: when `os`
includes Windows or is unset, and when the stub has a `[platforms.windows-*]`
table or no platform tables. A stub limited to Linux and macOS gets no
launcher, and neither does a stub whose name already ends in `.cmd`, `.bat` or
`.exe`. The launcher is written on every platform, so a stub generated on Linux
and committed to a repository still works for someone who clones it on
Windows. If a stub stops supporting Windows, regenerating it removes the
launcher. Only launchers mise generated are removed; a launcher you wrote is
left alone.

The extension-less stub stays as well, so Git Bash and Cygwin can run it
through its shebang.

## Stubs and mise prune {#pruning}

Running a stub records it in `~/.local/state/mise/tracked-stubs`, the same way
mise tracks config files it has used. [`mise prune`](/cli/prune.html) keeps the
tool versions that a tracked stub needs. A stub protects its tool only after it
has run at least once on the machine, and once the stub file is deleted, its
versions can be pruned again unless something else needs them.

## A `mise exec` wrapper instead {#alternative-creating-simple-stubs-with-mise-x}

For a simple case, a shell script that calls [`mise exec`](/cli/exec.html)
does the same job without TOML:

```sh
mkdir -p ./bin
cat > ./bin/node <<'SCRIPT'
#!/usr/bin/env bash
exec mise exec node@24 -- node "$@"
SCRIPT
chmod +x ./bin/node
```

`node@24` selects the version, and `node "$@"` names the executable and passes
the caller's arguments along. These wrappers need bash and mise, and they
record no download metadata; use a tool stub when you need platform URLs or
lock data.
