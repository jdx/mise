---
description: "Package mise for a distribution: choose build features, turn off self-update, ship update instructions, and test the package."
---

# Packaging mise

A package manager that installs mise should own its updates, so turn off
`mise self-update` in your package and tell users how to update instead.
Self-update stays on unless the package does one of the things below.

Each method makes `mise doctor` report `self_update_available: no`, makes
`mise self-update` fail instead of replacing the binary, and stops the
automatic updates that [`self_update.auto`](/configuration/settings.html#self_update.auto)
turns on. mise prints your update instructions, if you ship them, when
`mise self-update` fails, when `mise version` or `mise doctor` finds a newer
release, and when a project's
[`min_version`](/configuration.html#minimum-mise-version) asks for a newer
mise. Without instructions, those last two messages say "self-update is
disabled for this install, update mise the same way you installed it".

## Build features {#build-features}

`cargo build --release` builds with three default Cargo features:

| Feature             | Effect                                                                                 |
| ------------------- | -------------------------------------------------------------------------------------- |
| `native-tls`        | TLS through the platform library, which is OpenSSL on Linux                            |
| `vfox/vendored-lua` | Compiles the Lua 5.1 that vfox plugins run on; without it, mise links a system Lua 5.1 |
| `self_update`       | The `mise self-update` implementation                                                  |

The `rustls` and `rustls-native-roots` features use rustls in place of
`native-tls`.

The build needs Rust at or above the `rust-version` in `Cargo.toml`, a C
compiler, and, with `native-tls` on Linux, `pkg-config` and the OpenSSL
headers. See [build dependencies](/contributing.html#build-dependencies).

## Disable self-update at build time {#disable-at-build-time}

Build without the `self_update` feature. This example keeps native TLS and the
bundled Lua:

```sh
cargo build --release --no-default-features --features native-tls,vfox/vendored-lua
```

The `self-update` subcommand still exists, so scripts that call it get a clear
error instead of "unknown command". It prints your update instructions, if the
package ships them, and fails with
`mise's self-update feature has been disabled at build time, cannot update`.
This is the only method that `mise self-update --force` cannot get past.

## Paths mise checks {#install-prefix}

The marker and instructions files below are looked up relative to the install
prefix. mise finds it from its own executable: it resolves symlinks, then takes
the directory two levels up, so `/usr/bin/mise` gives `/usr`.

## Disable self-update with a marker file {#disable-with-a-marker-file}

Install an empty `.disable-self-update` file at any one of these paths under the
prefix:

- `lib/.disable-self-update` (Homebrew uses this one)
- `lib/mise/.disable-self-update` (the AUR `mise-bin` package uses this one)
- `lib64/mise/.disable-self-update`

## Ship update instructions {#ship-update-instructions}

An instructions file also turns off self-update, and gives users the command to
run instead. Install it at any one of:

- `lib/mise-self-update-instructions.toml`
- `lib/mise/mise-self-update-instructions.toml`
- `lib64/mise/mise-self-update-instructions.toml`

mise prints the file's `message`. Without a `message` key, it prints the value
of the first other key in alphabetical order.

```toml
# Debian and Ubuntu (apt)
message = "To update mise from the APT repository, run:\n\n  sudo apt update && sudo apt install --only-upgrade mise\n"
```

```toml
# Fedora and CentOS Stream (dnf)
message = "To update mise from COPR, run:\n\n  sudo dnf upgrade mise\n"
```

Set `MISE_SELF_UPDATE_INSTRUCTIONS` to a file path to use that file instead of
searching the prefix.

## Override the result for testing {#overriding-the-outcome}

`MISE_SELF_UPDATE_AVAILABLE=false` turns off self-update without installing
anything, and `MISE_SELF_UPDATE_AVAILABLE=true` turns it back on even when a
marker or instructions file is present. Neither has an effect on a build
without the `self_update` feature.

`mise self-update --force` skips the availability check, so a user who passes it
replaces the binary even when a marker file, an instructions file, or
`MISE_SELF_UPDATE_AVAILABLE=false` is in effect. Treat these runtime methods as
"do not update by default", not as a hard block.

## Test a package {#testing-packaging}

Test packaging changes in a disposable container or machine for the target
distribution. The examples below need a running Docker engine. Start the
container from your host shell, then run the installation commands inside it.
You are root there and `sudo` is not installed, so leave `sudo` off the
commands you copy. These checks install from the published repositories; to
test a package you built, copy it into the container and install it there.

### Ubuntu and Debian (apt) {#ubuntu-apt}

```sh
docker run -ti --rm ubuntu bash
```

The image has no package lists, so run `apt-get update` first. Then follow the
[apt instructions](/installing-mise.html#apt) and run `mise --version`. To test
the PPA instead, run `apt-get install -y software-properties-common` first; it
provides `add-apt-repository`.

### Fedora (dnf) {#fedora-dnf}

```sh
docker run -ti --rm fedora bash
```

Inside the container, follow the [Fedora instructions](/installing-mise.html#dnf-fedora),
then run `mise --version`.

### RHEL 9 (dnf) {#rhel-dnf}

```sh
docker run -ti --rm registry.access.redhat.com/ubi9/ubi:latest bash
```

Inside the container, follow the
[RHEL 9 instructions](/installing-mise.html#dnf-el9), then run
`mise --version`. RHEL 9 needs the `centos-stream+epel-next-9` COPR chroot;
`dnf copr enable` without it looks for an `epel-9` build that does not exist.
