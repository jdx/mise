# nix Backend

This backend allows mise to install any package (from the >120_000 available) from the [nixpkgs](https://github.com/NixOS/nixpkgs/) repository. It uses the NixHub Index (see bellow) to search for packages versions.

The code for this is inside the mise repository at [`./src/backend/nix.rs`](https://github.com/jdx/mise/blob/main/src/backend/nix.rs).

The only requirement is a system where `nix` is installed. Nix can be installed on any Linux, MacOS, and Windows (WSL2).

If you are using [NixOS](https://nixos.org/download/) you are all set.
However if you wish to install Nix on any other operating system, we recommend using the [Determinate Nix Installer](https://determinate.systems/nix-installer/).

## Usage

The following example will install the `go` package.
(read bellow how nix uses attribute-paths to identify installables)

```
mise use -g nix:go@1.24.1
```

or in toml format:

```toml
[tools]
"nix:go" = "1.24.1"
```

## A note on nixpkgs and package versions

The nixpkgs repository is a huge collection of programs (> 120_000 packages) that are constantly evolving and getting updated by the nix contributors to match their latest version. Because of this mono-repo nature, in order to find what versions have ever been available for a package, you would need to track which repository revision introduced each package version change.

Fortunately for us, there are a couple of services and tools that provide such
historical version information:

- [Lazamar Index](https://lazamar.co.uk/nix-versions/) - Provides historical versions of packages tracked by `channel` (ie, a particular nixpkgs branch.) This index is built using the public information produced by the [nixpkgs Hydra](https://hydra.nixos.org/project/nixpkgs) CI tool that is constantly building each new nixpkgs revision.

- [NixHub](https://www.nixhub.io/) is a version search service provided by [Jetify](https://www.jetify.com/) for their [devbox](https://www.jetify.com/devbox) product. However NixHub's [JSON API](https://www.jetify.com/docs/nixhub/) is free to use and might be more up to date than the Lazamar index.

The mise nix-backend uses NixHub JSON API to search for packages versions.
In the future it could also support Lazamar index via an option, but for now
using the NixHub API seems sufficient.

## Installable Attribute-Paths

In order to install a package from nixpkgs, you need to know its attribute-path.

You can think of the nixpkgs repository as a very-huge json object, but
written in the nix-language. An attribute-path represents the keys on that
tree used to access a particular installable package.

Most packages have simple attribute-paths are pretty much guessable, like
`emacs`, `ruby`, `cargo`, `nodejs`.

However some other packages depend on an specific runtime or are part of a
particular package set. For those, the attribute-path contains the `.` dot
separated path leading to the installable package.

An example is the `pip` program which is available under `python312Packages.pip` or `python311Packages.pip`.

In order to find those non-guessable attribute-paths, you can use the official
[Nix Search](https://search.nixos.org/packages) website.

Or if you prefer the command-line, the [nix-search-cli](https://github.com/peterldowns/nix-search-cli) program can query the Nix Search' index.

```
# Searching all packages that provide programs with rust anywhere on its name.
nix run nixpkgs#nix-search-cli -- --program "*rust*"
```
