# Package Manager Plugins

Package manager plugins extend [`[bootstrap.packages]`](/bootstrap/packages/)
without adding a manager to mise core. They are useful for machine-global
state owned by another tool, such as VS Code extensions, Helm plugins, krew
plugins, and GitHub CLI extensions.

Declare the plugin source and packages together. The `example/*` repository
URLs below are **placeholders** for syntax only — replace them with maintained,
installable plugin repositories before running these commands:

```toml
[bootstrap.plugins]
vscode = "https://github.com/example/mise-vscode-extensions" # placeholder
krew = "https://github.com/example/mise-krew" # placeholder

[bootstrap.packages]
"vscode:ms-python.python" = "latest"
"krew:ctx" = "latest"
```

`mise bootstrap` installs declared package plugins first, applies built-in
package managers, installs `[tools]`, then applies plugin managers. This lets a
plugin declare a host command such as `code`, `helm`, `kubectl`, or `gh` that is
provided by global `[tools]` entries. Package-plugin hooks include the process
PATH, mise shims, and global tool paths; project-only tool paths are not added
as a separate dependency toolset. Install the host tool globally or ensure it
is on the hook's PATH. Installing an extension is separate from installing its host.

For an existing configuration, start with `mise bootstrap --dry-run` to inspect
the phase order. The narrower `plugins apply` installs plugins themselves;
`packages apply` expects the plugin and its host dependencies to be ready.

The narrower commands are also available:

```sh
mise bootstrap plugins status
mise bootstrap plugins status --missing
mise bootstrap plugins apply
mise bootstrap packages status
mise bootstrap packages apply
mise bootstrap packages prune --manager vscode --dry-run
```

You can install a plugin without declaring it:

```sh
# placeholder URL — replace with a real package-plugin repository
mise plugins install package:vscode https://github.com/example/mise-vscode-extensions
```

Run as the user whose application state should change. A successful install in
one user's VS Code, Helm, or GitHub CLI profile does not configure every user's
profile on the host.

Package plugins install into the host application's own state directory. They
do not create mise installs or shims, never elevate with `sudo`, and are not
affected by `system_packages.sudo`. The `system_packages.managers` setting is
name-based and can include or exclude plugin managers just like built-ins.

Plugins may implement `PackageUninstall` to support the explicit destructive
command `mise bootstrap packages prune --manager <plugin>`. mise removes only
packages it observed transitioning from missing to installed during a plugin
install; packages that were already present are never claimed. Prune also keeps
packages referenced by the current config or trusted, loadable tracked configs.
Removing a config entry alone does not uninstall host-managed state.

See [Package Plugin Development](/package-plugin-development.html) to create a
plugin.
