---
description: "Install mise on macOS, Linux, or Windows, verify the executable, and keep it up to date."
---

# Installing mise

Pick an installation method for your platform, check the executable, then
[set up your shell](/shell-setup.html). For a guided first project, see
[Getting started](/getting-started.html).

## Choose a method {#installation-methods}

| Platform | Recommended                                                        | Alternatives                                                                                      |
| -------- | ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------- |
| macOS    | [mise.run](#mise-run)                                              | [Homebrew](#homebrew), [MacPorts](#macports), [packslip](#packslip) (Apple silicon)               |
| Linux    | [mise.run](#mise-run)                                              | [packslip](#packslip), [apt](#apt), [dnf](#dnf), [pacman](#pacman), [apk](#apk), [others](#linux) |
| Windows  | [winget](#windows-winget)                                          | [Scoop](#windows-scoop), [Chocolatey](#windows-chocolatey), [packslip](#packslip)                 |
| CI       | [mise-action or a committed wrapper](/continuous-integration.html) |                                                                                                   |
| Docker   | [Official images](/mise-cookbook/docker.html)                      |                                                                                                   |

mise.run, packslip and [GitHub Releases](#github-releases) install the official
release binaries, which are built with mise's optimized release profile and
update with `mise self-update`. Package-manager builds can trail a release, and
the Homebrew formula's build can be noticeably slower and larger.

Use one method per machine. Two installations on `PATH` can leave an older
binary in use; see [Verify the executable](#verify-the-executable).

## Official release binaries {#recommended}

### Install script (mise.run) {#mise-run}

```sh
curl -fsSL https://mise.run | sh
```

The script installs the executable to `~/.local/bin/mise`. That directory does
not need to be on `PATH`: once you [activate mise](/shell-setup.html), mise
adds its own directory. To choose another path (its parent directory must be
writable by your user):

```sh
curl -fsSL https://mise.run | MISE_INSTALL_PATH="$HOME/bin/mise" sh
```

Without `MISE_VERSION`, the script selects the newest stable release published
at least 24 hours before it runs, and keeps an existing executable at the
install path when that one is the same release or newer. Set `MISE_VERSION` for
a reproducible install:

```sh
curl -fsSL https://mise.run | MISE_VERSION=v2026.10.4 sh
```

#### Install and activate in one step {#shell-specific-installation-activation}

The shell-specific endpoints install mise and append activation to that shell's
startup file:

::: code-group

```sh [zsh]
curl -fsSL https://mise.run/zsh | sh
# adds activation to ${ZDOTDIR:-$HOME}/.zshrc
```

```sh [bash]
curl -fsSL https://mise.run/bash | sh
# adds activation to ~/.bashrc
```

```sh [fish]
curl -fsSL https://mise.run/fish | sh
# adds activation to ~/.config/fish/config.fish
```

:::

They skip the append when their own marker comment is already in the file.
They do not recognize activation that you or a package manager added another
way, so check the file first if you set up activation before.

#### Installer options {#installer-options}

The script reads these environment variables, not TOML settings:

| Variable                               | Effect                                                                                                                                                          |
| -------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `MISE_INSTALL_PATH`                    | Executable path. Defaults to `~/.local/bin/mise`.                                                                                                               |
| `MISE_VERSION`                         | Release to install, such as `v2026.10.4`. Bypasses the release-age delay.                                                                                       |
| `MISE_SELF_UPDATE_MINIMUM_RELEASE_AGE` | Skip releases newer than this: an integer with `s`, `m`, `h`, `d` or `w`, such as `7d`, or `0s` for none. Falls back to `MISE_MINIMUM_RELEASE_AGE`, then `24h`. |
| `MISE_INSTALL_SKIP_IF_EXISTS`          | With `1`, skip the download when the executable at the install path is already the selected version.                                                            |
| `MISE_INSTALL_MUSL`                    | With `1`, install the static musl build.                                                                                                                        |
| `MISE_INSTALL_OS`, `MISE_INSTALL_ARCH` | Override the detected platform, such as `MISE_INSTALL_ARCH=x64` to install the Intel build on Apple silicon. Add `-musl` to the architecture for a musl build.  |
| `MISE_INSTALL_EXT`                     | Archive format, `tar.zst` or `tar.gz`. Defaults to `tar.zst` when `zstd` is available.                                                                          |
| `MISE_INSTALL_FROM_GITHUB`             | With `1`, download from GitHub Releases instead of mise.jdx.dev.                                                                                                |
| `MISE_INSTALL_HELP`                    | With `0`, do not print the activation hint after installing.                                                                                                    |
| `MISE_DEBUG`, `MISE_QUIET`             | With `1`, print debug output or only errors.                                                                                                                    |

#### Verify the install script {#verify-the-install-script}

To check the script's signature before running it:

```sh
gpg --keyserver hkps://keys.openpgp.org --recv-keys 24853EC9F655CE80B48E6C3A8B81C9D17413A06D
curl -fsSL -o install.sh.sig https://mise.jdx.dev/install.sh.sig
gpg --output install.sh --decrypt install.sh.sig
```

Confirm that GPG reports a valid signature by the release key with fingerprint
`24853EC9F655CE80B48E6C3A8B81C9D17413A06D`. If the download or the check fails,
stop and do not run the output. After a successful check:

```sh
sh ./install.sh
```

#### Supported platforms {#supported-platforms}

The script installs one of these builds:

- `macos-x64`
- `macos-arm64`
- `linux-x64`
- `linux-x64-musl`
- `linux-arm64`
- `linux-arm64-musl`
- `linux-armv7`
- `linux-armv7-musl`

[GitHub Releases](#github-releases) has the same builds plus `windows-x64` and
`windows-arm64`.

The `linux-x64`, `linux-arm64` and `linux-armv7` builds are dynamically linked
and need glibc 2.18 or newer. The `-musl` builds are static and do not need
glibc; use them on musl systems such as Alpine Linux and on systems with an
older glibc.

The script selects a musl build on musl systems. On glibc systems it selects the
glibc build without checking the glibc version, so on a glibc older than 2.18
select the musl build yourself:

```sh
curl -fsSL https://mise.run | MISE_INSTALL_MUSL=1 sh
```

::: details Policy for raising the glibc minimum

Before mise raises its glibc minimum, every distribution with an older version
must have reached the end of standard vendor support. Extended-support programs,
such as Ubuntu ESM, RHEL ELS, and SUSE LTSS, do not extend this period. Users of
those systems can use static musl builds.

The following table lists standard support end dates for several distributions:

| glibc  | Distros                                                | Standard support ends                    |
| ------ | ------------------------------------------------------ | ---------------------------------------- |
| ≤ 2.27 | RHEL 7, Ubuntu 16.04 / 18.04, Debian 9, Amazon Linux 2 | ended (last: Amazon Linux 2, 2026-06-30) |
| 2.28   | Debian 10                                              | ended 2024-06-30                         |
| 2.28   | RHEL 8, Rocky 8, AlmaLinux 8                           | 2029-05-31                               |
| 2.31   | Ubuntu 20.04, Debian 11                                | ended (last: Debian 11, 2026-08-31)      |
| 2.34   | Amazon Linux 2023                                      | 2029-06-30                               |
| 2.34   | RHEL 9, Rocky 9, AlmaLinux 9                           | 2032-05-31                               |
| 2.35   | Ubuntu 22.04                                           | 2027-06-01                               |

The minimum remains glibc 2.18. These dates inform future compatibility
changes; they do not set a release schedule. The same support policy applies to
distributions omitted from the table.

:::

For other platforms, [build from source with Cargo](#cargo).

### packslip {#packslip}

[packslip](https://packslip.dev) installs mise's signed upstream release without
running a mise install script. It verifies the Sigstore signature and
transparency-log entry against mise's GitHub repository, checks the archive's
digest and size, and exposes the `mise` executable from the complete archive.

Use packslip 1.5.1 or newer on Linux x64 or arm64, macOS arm64, or Windows x64
or arm64. Intel Macs need another installation method.

Install packslip from its signed
[APT or RPM repository](https://packslip.dev/docs/distributions/) or one of the
other methods in its [getting started guide](https://packslip.dev/docs/getting-started/),
then install mise:

```sh
packslip install github.com/jdx/mise --pin ps1_nlhmwtfeufglxv5myvwvronk7a
~/.local/bin/mise --version
```

packslip prints the installed command paths: `~/.local/bin/mise` for an ordinary
Unix user and `/usr/local/bin/mise` for root. It does not change `PATH` or your
shell files.

`--pin` is the fingerprint of mise's GitHub repository, so packslip accepts only
mise's own releases even if the repository is renamed or another repository
takes its name. Without `--pin`, packslip trusts the repository GitHub reports
for the name on first use and holds later installs on that machine to it.

Without `--version`, or with `--version latest`, packslip installs the current
stable release. To install a specific release:

```sh
packslip install github.com/jdx/mise --version 2026.10.4 \
  --pin ps1_nlhmwtfeufglxv5myvwvronk7a
```

`mise self-update` works on this installation, and rerunning `packslip install`
replaces mise with the release you request. packslip does not update itself or
mise on its own. See [packslip's install guide](https://packslip.dev/docs/bootstrap/)
for system scope, destination overrides and trust settings, and the
[Docker page](/mise-cookbook/docker.html#bootstrap-with-packslip) for keeping a
pinned packslip while mise moves to new releases.

## macOS {#macos}

The [install script](#mise-run) is the recommended method on macOS.

### Homebrew {#homebrew}

```sh
brew install mise
```

Homebrew builds mise from source with its own settings; the official binaries
from [mise.run](#mise-run) are faster and arrive sooner. The formula installs
shell completions and activates mise in fish automatically.
[Homebrew formula](https://formulae.brew.sh/formula/mise)

### MacPorts {#macports}

```sh
sudo port install mise
```

[MacPorts port](https://ports.macports.org/port/mise/)

## Linux {#linux}

The [install script](#mise-run) is the recommended method on Linux. The packages
below update with the rest of your system.

### apt {#apt}

On Debian 11+ and Ubuntu 22.04+, enable the mise repository with extrepo:

```sh
sudo apt install -y extrepo
sudo extrepo enable mise
sudo apt update
sudo apt install -y mise
```

On Ubuntu 26.04+, you can use the PPA instead:

```sh
sudo add-apt-repository -y ppa:jdxcode/mise
sudo apt update
sudo apt install -y mise
```

### dnf {#dnf}

mise is in the [jdxcode/mise COPR](https://copr.fedorainfracloud.org/coprs/jdxcode/mise/).

#### Fedora 43+ and RHEL 10, CentOS Stream 10 and their rebuilds {#dnf-fedora}

```sh
sudo dnf copr enable jdxcode/mise
sudo dnf install mise
```

#### RHEL 9, CentOS Stream 9, AlmaLinux 9 and Rocky 9 {#dnf-el9}

RHEL 9's Rust is too old to build mise, so enable the CentOS Stream 9 build,
which runs on all of these. Without the chroot name, `dnf copr enable` looks
for an `epel-9` build, which does not exist:

```sh
sudo dnf copr enable jdxcode/mise centos-stream+epel-next-9
sudo dnf install mise
```

### yum (RHEL 8 and rebuilds) {#yum}

```sh
sudo yum install -y yum-utils
sudo yum-config-manager --add-repo https://mise.jdx.dev/rpm/mise.repo
sudo yum install -y mise
```

### zypper {#zypper}

```sh
sudo wget https://mise.jdx.dev/rpm/mise.repo -O /etc/zypp/repos.d/mise.repo
sudo zypper refresh
sudo zypper install mise
```

### pacman {#pacman}

On Arch Linux:

```sh
sudo pacman -S mise
```

[Arch package](https://archlinux.org/packages/extra/x86_64/mise/)

### apk {#apk}

On Alpine Linux, mise is in the
[community repository](https://gitlab.alpinelinux.org/alpine/aports/-/blob/master/community/mise/APKBUILD):

```sh
apk add mise
```

::: warning Alpine source-build default is deprecated
On Alpine, mise compiles tools from source by default. Since 2026.8.0 it warns
about this, and 2027.8.0 switches the default to precompiled binaries. To keep
compiling, set [`all_compile = true`](/configuration/settings.html#all_compile).
:::

### Nix {#nix}

With nixpkgs 24.05 or later:

```sh
nix-env -iA nixpkgs.mise
```

To try it without a persistent installation, run
`nix-shell -p mise --run "mise --version"`.

The mise repository is also a flake:

```nix
inputs.mise.url = "github:jdx/mise";
# then add inputs.mise.packages.${system}.mise to your packages
```

::: warning NixOS source-build default is deprecated
On NixOS, mise compiles tools from source by default. Since 2026.8.0 it warns
about this, and 2027.8.0 switches the default to precompiled binaries, which
need [nix-ld](https://github.com/Mic92/nix-ld). To keep compiling, set
[`all_compile = true`](/configuration/settings.html#all_compile).
:::

### Snap {#snap-linux}

```sh
sudo snap install mise --classic
```

[snapcraft.io page](https://snapcraft.io/mise)

## Windows {#windows}

winget is the recommended method on Windows. After installing, set up
[PowerShell activation](/shell-setup.html#powershell), or put the shims
directory on `Path` for cmd.exe as described in
[Windows shells](/shell-setup.html#windows).

### winget {#windows-winget}

```powershell
winget install jdx.mise
```

[winget manifest](https://github.com/microsoft/winget-pkgs/tree/master/manifests/j/jdx/mise)

### Scoop {#windows-scoop}

```powershell
scoop install mise
```

Scoop puts `mise` on `Path` through its own command shim. It does not add mise's
tool shims directory, so set up activation or shims separately.
[Scoop manifest](https://github.com/ScoopInstaller/Main/blob/master/bucket/mise.json)

### Chocolatey {#windows-chocolatey}

```powershell
choco install mise
```

The [Chocolatey package](https://community.chocolatey.org/packages/mise) can lag
official releases; check its version before choosing it.

### Manual install on Windows {#windows-manual}

Download `mise-v<version>-windows-x64.zip` (or `windows-arm64`) and
`SHASUMS256.txt` from [GitHub Releases](https://github.com/jdx/mise/releases).
In PowerShell, print the hash of the zip:

```powershell
(Get-FileHash mise-v2026.10.4-windows-x64.zip -Algorithm SHA256).Hash
```

It must match the `./mise-v2026.10.4-windows-x64.zip` line in
`SHASUMS256.txt`; PowerShell prints it in uppercase. With `minisign` installed,
you can also check the signature of `SHASUMS256.txt` as shown under
[Manual download](#github-releases). Then extract the `mise\bin` folder, which
holds `mise.exe` and `mise-shim.exe`, and add it to your `Path`. Keep the two
files together: mise copies `mise-shim.exe` to create its shims.

## Language package managers {#language-package-managers}

### Cargo {#cargo}

Source builds need a Rust toolchain that meets the release's `rust-version`,
plus the platform's compiler and native libraries. See
[build dependencies](/contributing.html#build-dependencies).

```sh
cargo install --locked mise
```

[cargo-binstall](https://github.com/cargo-bins/cargo-binstall) installs a
prebuilt binary instead of compiling:

```sh
cargo install --locked cargo-binstall
cargo binstall mise
```

To build the latest commit on `main`:

```sh
cargo install --locked mise --git https://github.com/jdx/mise --branch main
```

### npm {#npm}

The `mise` npm package distributes the precompiled binary; it is not a Node.js
library. It suits JavaScript projects that set up mise through `package.json`
or `npx`. Install `mise`, not the older `@jdxcode/mise` package.

```sh
npm install -g mise
```

npx runs mise without a global npm install. npm caches the download, and tools
mise installs go to mise's data directory as usual:

```sh
npx --yes mise exec python@3.14 -- python --version
```

[npm package](https://www.npmjs.com/package/mise)

## Containers {#docker}

Official images are published to `ghcr.io/jdx/mise` and Docker Hub
(`jdxcode/mise`) for Linux amd64 and arm64. Use the `-debian` tag as a CI or
development image, or copy the static binary from the scratch image with
`COPY --from=`. See [Docker](/mise-cookbook/docker.html) for tags, digest
pinning and Dockerfiles.

## Manual download {#github-releases}

Choose a release and the file for your platform from
[GitHub Releases](https://github.com/jdx/mise/releases). For example, to
download the Linux x64 executable into the current directory:

```sh
mise_version=2026.10.4
mise_platform=linux-x64
curl -fL -o mise "https://github.com/jdx/mise/releases/download/v${mise_version}/mise-v${mise_version}-${mise_platform}"
```

Each release also ships `SHASUMS256.txt`, signed with minisign
(`SHASUMS256.txt.minisig`) and GPG (`SHASUMS256.asc`). Check the signature of
the checksum file, then the download against it. These commands need `minisign`
and `sha256sum` (on macOS, use `shasum -a 256 -c` in place of `sha256sum -c`)
and reuse the variables and the `mise` file from above:

```sh
base="https://github.com/jdx/mise/releases/download/v${mise_version}"
curl -fL -O "$base/SHASUMS256.txt" -O "$base/SHASUMS256.txt.minisig"
minisign -Vm SHASUMS256.txt -P RWTC3g8W3z4RZK3V3qv7fa1QY4JEWyBtqIHW+85QlJpZc5yG+uNYNBSZ &&
  grep " ./mise-v${mise_version}-${mise_platform}$" SHASUMS256.txt | sed 's| ./mise-.*| mise|' | sha256sum -c
```

The minisign public key is
[`minisign.pub`](https://github.com/jdx/mise/blob/main/minisign.pub) in the
repository. Then install the executable to a directory you can write to:

```sh
mkdir -p ~/.local/bin
install -m 755 ./mise ~/.local/bin/mise
~/.local/bin/mise --version
```

## Verify the executable {#verify-the-executable}

```sh
mise --version
mise doctor
```

Before activation, a default mise.run installation runs as
`~/.local/bin/mise --version` and `~/.local/bin/mise doctor`. If the version is
not the one you installed, check which copy runs with `command -v mise` on Unix
or `Get-Command mise` in PowerShell.

mise keeps tools, caches and state under your home directory; see
[Directories](/directories.html) to change the locations. For other problems,
see [Troubleshooting](/troubleshooting.html).

## Shells {#shells}

To activate mise in bash, zsh, fish, PowerShell, Nushell, Xonsh or Elvish, and
to install shell completions, see [Shell setup](/shell-setup.html).

## Updating mise {#updating}

If a package manager installed mise, update it with that package manager.
Otherwise run [`mise self-update`](/cli/self-update.html), which also updates
installed plugins unless you pass `--no-plugins`. `mise upgrade` updates your
tools, not mise.

To update automatically, enable
[`self_update.auto`](/configuration/settings.html#self_update.auto) in your
global config:

```sh
mise settings self_update.auto=true
```

mise then checks for a new release at the interval set by
[`self_update.check_duration`](/configuration/settings.html#self_update.check_duration).
When one is available, it installs it before an eligible interactive command and
runs the command again with the new binary. It skips the check in CI, in
offline modes, in non-interactive sessions and for installations whose packager
disabled self-update.

`mise self-update` and automatic updates skip releases younger than
[`self_update.minimum_release_age`](/configuration/settings.html#self_update.minimum_release_age),
which falls back to [`minimum_release_age`](/security.html#minimum-release-age).
`mise self-update 2026.10.4` installs a named release without that delay. Before
replacing the binary, mise verifies the release's signatures; see
[packslip verification](/dev-tools/packslip-verification.html#self-update) for
what it checks.

Organizations can serve updates from a mirror of approved releases with
[`self_update.repository`](/configuration/settings.html#self_update.repository)
and [`self_update.api_url`](/configuration/settings.html#self_update.api_url).
The mirror must keep the official release file names and signatures, and
`api_url` must use HTTPS. Private repositories and GitHub Enterprise use mise's
[GitHub token](/dev-tools/github-tokens.html) resolution. Set these in the
global or system config; mise ignores them in a project config:

```toml [~/.config/mise/config.toml]
[settings.self_update]
repository = "myorg/mise-mirror"
api_url = "https://github.example.com/api/v3"
```

mise talks to registries and release APIs that change over time, so keep it
current. When a project needs a newer mise, set
[`min_version`](/configuration.html#minimum-mise-version) instead of pinning
one mise version for everyone. Distribution packagers can disable
`mise self-update` and point users to their package manager; see
[Packaging mise](/packaging.html).

## Uninstalling {#uninstalling}

Use the package manager that installed mise to remove a package-managed CLI.
For a standalone installation, preview the removal first:

```sh
mise implode --dry-run
```

`mise implode` removes the CLI, installed tools, cache and state, including the
system data directory when present. It keeps the user config directory unless
you pass `--config`. Check the listed paths before running it without
`--dry-run`; environment variables can change them.

Then remove the activation lines from your shell startup files and any
completion files you installed. Project `mise.toml` files, and host packages
installed by bootstrap, are not part of mise's data and stay in place. See
[Directories](/directories.html) for the storage paths.
