# Package Manager Plugins

Package manager plugins extend [`[bootstrap.packages]`](/bootstrap/packages/)
without adding a manager to mise core. They are useful for machine-global
state owned by another tool, such as VS Code extensions, Helm plugins, krew
plugins, and GitHub CLI extensions.

Declare the plugin source and packages together:

```toml
[bootstrap.plugins]
vscode = "https://github.com/mise-plugins/mise-vscode-extensions"
krew = "https://github.com/mise-plugins/mise-krew"

[bootstrap.packages]
"vscode:ms-python.python" = "latest"
"krew:ctx" = "latest"
```

`mise bootstrap` installs declared package plugins first, applies built-in
package managers, installs `[tools]`, then applies plugin managers. This lets a
plugin declare a host command such as `code`, `helm`, `kubectl`, or `gh` that is
provided by the same config's global `[tools]` entries.

The narrower commands are also available:

```sh
mise bootstrap plugins status
mise bootstrap plugins status --missing
mise bootstrap plugins apply
mise bootstrap packages status
mise bootstrap packages apply
```

You can install a plugin without declaring it:

```sh
mise plugin install package:vscode https://github.com/mise-plugins/mise-vscode-extensions
```

Package plugins install into the host application's own state directory. They
do not create mise installs or shims, never elevate with `sudo`, and are not
affected by `system_packages.sudo`. The `system_packages.managers` setting is
name-based and can include or exclude plugin managers just like built-ins.

Package removal and pruning are not supported in the first version of this API.
Removing a config entry does not uninstall host-managed state.

See [Package Plugin Development](/package-plugin-development.html) to create a
plugin.
