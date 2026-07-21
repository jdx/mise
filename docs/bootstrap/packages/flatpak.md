# Flatpak

Flatpak applications and runtimes installed system-wide via the
[`flatpak`](https://docs.flatpak.org/en/latest/flatpak-command-reference.html) CLI.

```toml
[bootstrap.packages]
"flatpak:org.mozilla.firefox" = "latest"
"flatpak:org.gnome.Builder" = "latest"
```

Flatpak packages are part of `[bootstrap.packages]`, just like apt packages,
Homebrew formulae, and Mac App Store apps. The package name is an application
or runtime ID accepted by `flatpak install` and `flatpak update`.

mise does not install Flatpak or configure remotes implicitly. Install the
`flatpak` CLI and add the required remote (commonly Flathub) before applying
the config:

```sh
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
mise bootstrap packages use flatpak:org.mozilla.firefox
```

## Commands

```sh
mise bootstrap packages status --manager flatpak
mise bootstrap packages apply --manager flatpak
mise bootstrap packages upgrade --manager flatpak
```

mise manages the system-wide Flatpak installation. `apply` runs
`flatpak install --system --noninteractive <id>` for missing packages, while
`upgrade` runs `flatpak update --system --noninteractive <id>` for installed
packages. Flatpak resolves the configured ID from the system remotes.

Flatpak does not expose installation of arbitrary historical versions through
these commands, so version pins are not supported. Use `"latest"` in config.

The manager is Linux-only and requires `flatpak` on `PATH`. On other platforms,
or when the command is missing, shared configs list Flatpak entries as skipped.
