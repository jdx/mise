---
description: "Extend bootstrap packages with plugins for VS Code extensions, Helm plugins, and similar state."
---

# Package manager plugins

Package manager plugins add managers to
[`[bootstrap.packages]`](/bootstrap/packages/) for machine-wide state that
another program owns, such as VS Code extensions, Helm plugins, krew plugins,
and GitHub CLI extensions.

Declare each plugin's repository under `[bootstrap.plugins]` and its packages
under `[bootstrap.packages]`. Replace the `example/...` URLs with the
repositories of the plugins you use:

```toml
[bootstrap.plugins]
vscode = "https://github.com/example/mise-vscode-extensions"
krew = "https://github.com/example/mise-krew"

[bootstrap.packages]
"vscode:ms-python.python" = "latest"
"krew:ctx" = "latest"
```

Install the plugins, then their packages, with the matching phases of
[`mise bootstrap`](/bootstrap.html):

```sh
mise bootstrap --only plugins,packages,tools --dry-run
mise bootstrap --only plugins,packages,tools
```

## How bootstrap runs plugins

`mise bootstrap` installs plugins first, then built-in packages, then
`[tools]`, and applies plugin packages last. That way a plugin can rely on a
host command such as `code`, `helm`, `kubectl`, or `gh` that comes from your
global `[tools]`. Plugins see your `PATH`, mise's shims, and the global tools,
but not tools declared only in a project's `mise.toml`, so install the host
command globally. A plugin that needs a command it cannot find reports itself
unavailable.

To run the steps separately, install the plugins with
`mise bootstrap plugins apply`, then run `mise bootstrap packages apply` once
the host commands are installed:

```sh
mise bootstrap plugins status --missing
mise bootstrap plugins apply
mise bootstrap packages status
mise bootstrap packages apply
```

You can also install a plugin without declaring it:

```sh
mise plugins install package:vscode https://github.com/example/mise-vscode-extensions
```

## What plugins change

A plugin installs into the host program's own state, such as VS Code's
extensions directory. It creates no mise installs or shims. Run mise as the
user whose state should change: installing an extension into one user's VS Code
does not configure it for other users on the machine.

Plugins never use sudo and are not affected by
[`system_packages.sudo`](/configuration/settings.html#system_packages.sudo).
[`system_packages.managers`](/configuration/settings.html#system_packages.managers)
matches plugin managers by name, as it does built-in ones.

Whether a plugin accepts version pins, and on which platforms it runs, is up to
the plugin. A pin on a plugin that does not support pins is reported and
skipped, as with built-in managers. Plugins do not support
`state = "absent"`.

## Prune

[`mise bootstrap packages prune --manager <plugin>`](/cli/bootstrap/packages/prune.html)
uninstalls packages that no configuration declares, when the plugin supports
uninstall:

```sh
mise bootstrap packages prune --manager vscode --dry-run
```

mise removes only packages it saw change from missing to installed when it
installed them through the plugin. Packages that were already present, or that
you installed yourself, are never removed. Prune also keeps packages declared
by the current configuration or any trusted config file mise tracks. Deleting
an entry alone does not uninstall anything.

To write a plugin, including uninstall support for prune, see the
[package manager plugin guide](/package-plugin-development.html).
