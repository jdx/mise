# Installing mise

If you are new to `mise`, follow the [Getting Started](/getting-started) guide first.

## Installation Methods

Choose one installation method, verify the executable, then configure your
shell if you want automatic project activation. Use the same package manager
for future updates when it owns your mise installation.

| Platform         | Recommended    | Alternative     |
| ---------------- | -------------- | --------------- |
| macOS            | mise.run       | Homebrew        |
| Linux            | mise.run       | System packages |
| Windows          | Scoop          | winget          |
| Any (Rust users) | cargo binstall | cargo install   |
| CI/Docker        | mise.run       | GitHub Releases |

The official single-binary release installed by `mise.run` is the preferred method on macOS and
Linux. These binaries are built with mise's optimized release profile and can be updated immediately
with `mise self-update`. Prefer them over third-party package builds: the Homebrew formula can be
substantially slower and larger, and package-manager releases may also trail a mise release.

::: tip Which methods auto-update?
Package managers (apt, dnf, brew, pacman, etc.) update mise when you update system packages. Official standalone installations support `mise self-update`; a build or
package may disable it. Updating mise itself is separate from `mise upgrade`,
which updates managed tools.

For installations that support `mise self-update`, automatic updates can be enabled globally:

```sh
mise settings set auto_update true
```

mise then periodically checks before eligible interactive commands, installs a newer release without
updating plugins, and re-runs the original command with the new binary. Configure the interval with
[`auto_update_check_duration`](/configuration/settings.html#auto_update_check_duration).

Organizations can direct manual and automatic self-updates to a curated GitHub release mirror by
setting [`self_update.repository`](/configuration/settings.html#self_update.repository). Private
repositories and GitHub Enterprise use mise's existing GitHub token resolution. Mirrored archives
must retain the official file names and embedded mise signatures. The API URL must use HTTPS:

```toml
[settings.self_update]
repository = "myorg/mise-mirror"
api_url = "https://api.github.com"
```

These settings are global-only: set them in the user-global or system configuration, not a project
configuration.
:::

::: tip Keep mise up to date
mise connects to many external registries and backends, such as aqua, GitHub releases, language package registries, and system package managers. Those services change over time, so mise works best when the CLI is kept on a recent version.

Projects and organizations should generally set a [`min_version`](/configuration.html#minimum-mise-version) when they need a newer mise feature instead of locking every user to a specific mise executable. While there are ways to pin or bootstrap a particular mise version, locking users to one mise version is generally discouraged. A fixed mise version can be useful in controlled CI builds, but it needs a
planned update process as upstream registries evolve. `min_version` lets a
project require a feature while allowing users to keep their CLI current.
:::

### <https://mise.run> {#mise-run}

`mise` does not need to be on `PATH`. If you run the activate script in your shell's rc file,
mise adds itself to `PATH` automatically.

```sh
curl -fsSL https://mise.run | sh
```

To choose another executable path (its parent must be writable by your user):

```sh
curl -fsSL https://mise.run | MISE_INSTALL_PATH=/usr/local/bin/mise sh
```

#### Shell-specific installation + activation

For a more streamlined setup, use the shell-specific endpoints, which install mise and configure activation in your shell's configuration file:

::: code-group

```sh [zsh]
curl -fsSL https://mise.run/zsh | sh
# Installs mise and adds activation to ~/.zshrc
```

```sh [bash]
curl -fsSL https://mise.run/bash | sh
# Installs mise and adds activation to ~/.bashrc
```

```sh [fish]
curl -fsSL https://mise.run/fish | sh
# Installs mise and adds activation to ~/.config/fish/config.fish
```

:::

These shell-specific installers will:

- Install mise using the same logic as the main installer
- Append activation to the selected shell's configuration (`ZDOTDIR` is honored for zsh; fish uses `~/.config/fish/config.fish`)
- Skip that append when the same shell installer's marker is already present

If activation was added manually or by a package manager, inspect the file
first: the installer marker check does not detect every equivalent hook.

Options:

- `MISE_DEBUG=1` – enable debug logging
- `MISE_QUIET=1` – disable non-error output
- `MISE_INSTALL_PATH=/some/path` – change the binary path (default: `~/.local/bin/mise`)
- `MISE_VERSION=v2025.12.0` – install a specific version
- `MISE_INSTALL_SKIP_IF_EXISTS=1` – skip the download/install if the mise binary at the install path already matches the requested version

To verify the install script hasn't been tampered with:

```sh
gpg --keyserver hkps://keys.openpgp.org --recv-keys 24853EC9F655CE80B48E6C3A8B81C9D17413A06D
curl -fsSL -o install.sh.sig https://mise.jdx.dev/install.sh.sig
gpg --output install.sh --decrypt install.sh.sig
```

Confirm that GPG reports a valid signature by the release key with fingerprint
`24853EC9F655CE80B48E6C3A8B81C9D17413A06D`. If download or verification fails,
stop; do not run the output. After successful verification:

```sh
sh ./install.sh
```

::: tip
Unless you change the version with `MISE_VERSION`, the install script is pinned to whatever the latest
version was when it was downloaded, with checksums inside the file. Downloading the script and committing it to
a project is therefore a great way to ensure that anyone who installs with it fetches the exact same mise binary.
:::

Supported OS/arch:

- `macos-x64`
- `macos-arm64`
- `linux-x64`
- `linux-x64-musl`
- `linux-arm64`
- `linux-arm64-musl`
- `linux-armv7`
- `linux-armv7-musl`

If you need something else, compile it with `cargo install mise` (see below).

### apk

For Alpine Linux:

```sh
apk add mise
```

_mise lives in
the [community repository](https://gitlab.alpinelinux.org/alpine/aports/-/blob/master/community/mise/APKBUILD)._

::: warning Alpine source-build default is deprecated
Alpine currently compiles tools from source by default. This automatic behavior is deprecated:
affected source installs warn beginning in mise 2026.8.0, and the default will switch to
precompiled binaries in mise 2027.8.0. To keep compiling from source, set
[`all_compile = true`](/configuration/settings.html#all_compile) explicitly.
:::

### apt

On Ubuntu 26.04+, mise is available via a PPA:

```sh
sudo add-apt-repository -y ppa:jdxcode/mise
sudo apt update
sudo apt install -y mise
```

On Debian 11+ and Ubuntu 22.04+, the mise repository can be enabled with extrepo:

```sh
sudo apt install -y extrepo
sudo extrepo enable mise
sudo apt update
sudo apt install -y mise
```

### pacman

For Arch Linux:

```sh
sudo pacman -S mise
```

[Arch package](https://archlinux.org/packages/extra/x86_64/mise/)

### Cargo

Source builds need a Rust toolchain meeting the selected release's
`rust-version` and the platform's compiler and native library prerequisites.
See [contributing](/contributing.html) for the build dependencies. Build with Cargo:

```sh
cargo install --locked mise
```

Do it faster with [cargo-binstall](https://github.com/cargo-bins/cargo-binstall):

```sh
cargo install --locked cargo-binstall
cargo binstall mise
```

Build from the latest commit on main:

```sh
cargo install --locked mise --git https://github.com/jdx/mise --branch main
```

### dnf

#### Fedora 41+, CentOS Stream 9+, RHEL 10+

```sh
sudo dnf copr enable jdxcode/mise
sudo dnf install mise
```

#### RHEL 9 / AlmaLinux 9 / Rocky 9

RHEL 9 AppStream is currently frozen at Rust 1.88, which is older than mise's
minimum supported Rust version. Use the CentOS Stream 9 build instead — the
resulting binary works on RHEL 9 derivatives:

```sh
sudo dnf copr enable jdxcode/mise centos-stream+epel-next-9
sudo dnf install mise
```

[COPR package page](https://copr.fedorainfracloud.org/coprs/jdxcode/mise/)

### Snap (Linux)

```sh
sudo snap install mise --classic
```

[snapcraft.io page](https://snapcraft.io/mise)

### Docker

See the [Docker cookbook](/mise-cookbook/docker) for tips on using mise with Docker.

::: details Example Dockerfile

Put a `mise.toml` declaring `node = "24"` under `[tools]` in the Docker build
context. This example copies that configuration, installs its tools, and uses
`mise exec` for the container command. Add any other configuration files,
lockfile, hook inputs, or application files that your real project needs.

```dockerfile
FROM debian:13-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

ENV MISE_INSTALL_PATH=/usr/local/bin/mise
RUN curl -fsSL https://mise.run -o /tmp/install-mise.sh \
    && sh /tmp/install-mise.sh \
    && rm /tmp/install-mise.sh

WORKDIR /app
COPY mise.toml ./mise.toml
RUN mise trust && mise install

ENTRYPOINT ["mise", "exec", "--"]
CMD ["node", "--version"]
```

:::

### Homebrew

::: warning
The Homebrew formula is convenient, but it is not the preferred installation method. Homebrew builds
mise separately from the official, more optimized release binaries. For the best performance and
fastest access to new releases, use the [`mise.run`](#mise-run) installer instead.
:::

```sh
brew install mise
```

[Homebrew formula](https://formulae.brew.sh/formula/mise)

### npm

mise is available on npm as a precompiled binary. It isn't a Node.js package—it is only distributed
via npm. This is useful for JS projects that want to set up mise via `package.json` or `npx`.

```sh
npm install -g mise
```

Use npx to run mise without adding it as a permanent global npm package. npm
caches its download, and any tools mise installs remain in mise's data directory:

```sh
npx --yes mise exec python@3.14 -- python --version
```

[npm package](https://www.npmjs.com/package/mise)

The legacy [`@jdxcode/mise`](https://www.npmjs.com/package/@jdxcode/mise) package is still published.

### GitHub Releases

Choose a version and the matching OS/architecture artifact from
[GitHub Releases](https://github.com/jdx/mise/releases). For example, to download
a Linux x64 executable to a temporary working directory:

```sh
mise_version=2026.9.1
mise_platform=linux-x64
curl -fL -o mise "https://github.com/jdx/mise/releases/download/v${mise_version}/mise-v${mise_version}-${mise_platform}"
```

Change both values for your chosen release and platform. Verify the artifact
against that release's checksum/signature metadata before installing it. The
`mise.run` installer handles platform selection and checksum checking for you.

After verifying a downloaded Unix executable, install it to a user-writable path:

```sh
mkdir -p ~/.local/bin
install -m 755 ./mise ~/.local/bin/mise
~/.local/bin/mise --version
```

### MacPorts

```sh
sudo port install mise
```

[MacPorts port](https://ports.macports.org/port/mise/)

### nix

For the Nix package manager, at release 24.05 or later:

```sh
nix-env -iA nixpkgs.mise
```

To try the Nixpkgs package without a persistent installation, run
`nix-shell -p mise --run "mise --version"`.

This repository also exposes a flake package at
`inputs.mise.packages.${system}.mise` when your flake declares a `mise` input
pointing to `github:jdx/mise`. The attribute is a Nix expression, not a shell command.

::: warning NixOS source-build default is deprecated
NixOS currently compiles tools from source by default. This automatic behavior is deprecated:
affected source installs warn beginning in mise 2026.8.0, and the default will switch to
precompiled binaries in mise 2027.8.0. Enable [nix-ld](https://github.com/Mic92/nix-ld) before that
change. To keep compiling from source, set
[`all_compile = true`](/configuration/settings.html#all_compile) explicitly.
:::

### yum (RHEL 8, CentOS Stream 8, Amazon Linux 2)

```sh
sudo yum install -y yum-utils
sudo yum-config-manager --add-repo https://mise.jdx.dev/rpm/mise.repo
sudo yum install -y mise
```

### zypper

```sh
sudo wget https://mise.jdx.dev/rpm/mise.repo -O /etc/zypp/repos.d/mise.repo
sudo zypper refresh
sudo zypper install mise
```

### Windows - Scoop

Scoop exposes the `mise` executable through its own command shim. Configure
[shell activation](#shells) or [mise's tool shims](/dev-tools/shims.html) separately;
the current Scoop manifest does not add mise's tool-shims directory to PATH.

```sh
scoop install mise
```

[Scoop manifest](https://github.com/ScoopInstaller/Main/blob/master/bucket/mise.json)

### Windows - winget

```sh
winget install jdx.mise
```

[winget manifest](https://github.com/microsoft/winget-pkgs/tree/master/manifests/j/jdx/mise)

### Windows - Chocolatey

::: info
Check the [Chocolatey package](https://community.chocolatey.org/packages/mise)
version before choosing it; it can lag official releases.
:::

```sh
choco install mise
```

### Windows - manual

Download the latest release from [GitHub](https://github.com/jdx/mise/releases) and add the binary
to your PATH.

If your shell does not support `mise activate`, add the shims directory (by default `%LOCALAPPDATA%\mise\shims`) to PATH.

## Verify the executable

```sh
mise --version
mise doctor
```

For the default `mise.run` installation before activation, use
`~/.local/bin/mise --version`. If the version is unexpected, check which copy is
running with `command -v mise` on Unix or `Get-Command mise` in PowerShell.
Having two installation methods on PATH can leave an older binary in use.

## Shells

The examples assume `mise` is on PATH. For a default `mise.run` installation,
use `~/.local/bin/mise` in the activation line instead. Add one activation line
to the startup file you actually use; avoid appending duplicates.

### Bash

```sh
echo 'eval "$(mise activate bash)"' >> ~/.bashrc
```

### Zsh

```sh
echo 'eval "$(mise activate zsh)"' >> "${ZDOTDIR-$HOME}/.zshrc"
```

### Fish

```sh
mkdir -p ~/.config/fish
echo 'mise activate fish | source' >> ~/.config/fish/config.fish
```

::: tip
For Homebrew and possibly other installs, mise is activated automatically, so
this step is not necessary.

See [`MISE_FISH_AUTO_ACTIVATE=1`](/configuration#mise-fish-auto-activate-1) for more information.
:::

### PowerShell

Use PowerShell's `$PROFILE` variable for the current host's profile. Create it
when missing, then add the activation line once:

```powershell
if (-not (Test-Path $PROFILE)) {
    New-Item -ItemType Directory -Force (Split-Path -Parent $PROFILE) | Out-Null
    New-Item -ItemType File -Path $PROFILE | Out-Null
}
$activation = '(&mise activate pwsh) | Out-String | Invoke-Expression'
if (-not (Select-String -Path $PROFILE -SimpleMatch $activation -Quiet)) {
    Add-Content $PROFILE $activation
}
```

See [PowerShell profiles](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_profiles)
when you use different profiles for terminals, editors, or PowerShell versions.

### Nushell

Nushell loads activation as a generated module. Add this to `env.nu` (located
at `$nu.env-path`) so the module exists before `config.nu` is parsed:

```nushell
let mise_path = $nu.default-config-dir | path join mise.nu
^mise activate nu | save $mise_path --force
```

Add this to `config.nu` (located at `$nu.config-path`):

```nushell
use ($nu.default-config-dir | path join mise.nu)
```

Restart Nushell after saving both files. If `mise` is not on PATH, use its
absolute executable path in the `env.nu` command. The module is regenerated at
startup so it follows mise upgrades.

### Xonsh

Add the following to `~/.xonshrc` or the Xonsh config file you use:

```xonsh
execx($(mise activate xonsh))
```

For a default `mise.run` installation before mise is on PATH, use
`execx($(~/.local/bin/mise activate xonsh))` instead. Restart Xonsh after saving.

mise updates Xonsh's environment and the process environment. If your own
startup code changes PATH, keep those views consistent so subprocesses resolve
the same commands as the shell.

### Elvish

Add the following to your `rc.elv`:

```shell
var mise: = (ns [&])
eval (mise activate elvish | slurp) &ns=$mise: &on-end={|ns| set mise: = $ns }
mise:activate
```

Optionally alias `mise` to `mise:mise` for seamless integration of `mise {activate,deactivate,shell}`:

```shell
edit:add-var mise~ {|@args| mise:mise $@args }
```

### Something else?

Adding a new shell is not hard since very little shell code is
in this project.
[See here](https://github.com/jdx/mise/tree/main/src/shell) for how
the others are implemented. If your shell isn't currently supported,
I'd be happy to help you get it integrated.

## Autocompletion

::: tip
Some installation methods automatically install autocompletion scripts.
:::

The [`mise completion`](/cli/completion.html) command can generate autocompletion scripts for your shell.

The instructions below complete mise itself. For commands installed through the
packslip backend, see [tool completions and skills](/dev-tools/packslip-resources.html).
The generated scripts are self-contained and do not require the separate `usage` CLI.

The simplest way to install the completion script is:

```shell
mise completion bash --install
```

Replace `bash` with `zsh`, `fish`, or `powershell` for your shell. Alternatively, choose the path yourself:

::: code-group

```sh [bash]
# This requires bash-completion to be installed
mkdir -p ~/.local/share/bash-completion/completions/
mise completion bash > ~/.local/share/bash-completion/completions/mise
```

```sh [zsh]
# Generate into a directory owned by your user:
mkdir -p ~/.zfunc
mise completion zsh > ~/.zfunc/_mise

# Add these to .zshrc, with fpath before your existing compinit call:
# fpath=(~/.zfunc $fpath)
# autoload -Uz compinit
# compinit
```

```sh [fish]
mkdir -p ~/.config/fish/completions
mise completion fish > ~/.config/fish/completions/mise.fish
```

:::

Then source your shell's rc file or restart your shell.

## Troubleshooting

If you encounter issues after installation, run:

```sh
mise doctor
```

This diagnoses common problems with your mise setup. See [mise doctor](/cli/doctor) for more information.

## Uninstalling

Use the package manager that installed mise to remove a package-managed CLI.
For a standalone installation, preview the removal first:

```sh
mise implode --dry-run
```

`mise implode` removes the CLI, installed tools, cache, and state, including the
system data directory when present. It keeps the user configuration directory
unless `--config` is passed. Inspect the listed paths before running without
`--dry-run`; these may be customized by environment variables.

Remove activation lines from shell startup files and any completion files you
installed separately. Project `mise.toml` files and host packages installed by
bootstrap are separate from mise's tool data. See [directories](/directories.html)
for the configured storage paths.
