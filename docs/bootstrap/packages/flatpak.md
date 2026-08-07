# Flatpak

Flatpak applications and runtimes installed system-wide or for the current user via the
[`flatpak`](https://docs.flatpak.org/en/latest/flatpak-command-reference.html) CLI.

```toml
[bootstrap.packages]
"flatpak:org.mozilla.firefox" = "latest"
"flatpak-user:org.gnome.Builder" = "latest"
```

Use the `flatpak` manager for the default system-wide installation and
`flatpak-user` for the current user's installation. The scopes are separate,
so a config can manage packages in either or both, including the same ID in
both scopes. The package name is an application or runtime ID accepted by
`flatpak install` and `flatpak update`.

mise does not install Flatpak or configure remotes implicitly. Install the
`flatpak` CLI and add the required remote (commonly Flathub) before applying
the config:

```sh
flatpak remote-add --system --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
mise bootstrap packages use flatpak:org.mozilla.firefox
mise bootstrap packages use flatpak-user:org.gnome.Builder
```

## Commands

```sh
mise bootstrap packages status --manager flatpak
mise bootstrap packages status --manager flatpak-user
mise bootstrap packages apply --manager flatpak
mise bootstrap packages apply --manager flatpak-user
mise bootstrap packages upgrade --manager flatpak
mise bootstrap packages upgrade --manager flatpak-user
```

mise always passes an explicit scope to Flatpak so status, installation, and
upgrades operate on the same installation. `flatpak` passes `--system`, while
`flatpak-user` passes `--user`. Flatpak resolves the configured ID from the
remotes configured for that scope.

Flatpak does not expose installation of arbitrary historical versions through
these commands, so version pins are not supported. Use `"latest"` in config.

The manager is Linux-only and requires `flatpak` on `PATH`. On other platforms,
or when the command is missing, shared configs list Flatpak entries as skipped.
