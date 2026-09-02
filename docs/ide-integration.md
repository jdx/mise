# IDE Integration

Code editors and IDEs work differently from interactive shells.

Usually, they either inherit the environment from your current shell (as when you start them from a terminal with `nvim .` or `code .`) or have [their own way](https://github.com/microsoft/vscode-docs/blob/906acccd6180d8425577f8297ed29e221ad3daca/docs/supporting/faq.md?plain=1#L238) to set up the environment.

Once the IDE is running, it won't reload the environment variables or the `PATH` provided by `mise` when you update your mise configuration files, so the default `mise activate` method cannot be relied on to set up the editor.

There are a few ways to make `mise` work with your editor:

- Some editors or IDE plugins support `mise` directly and let you select the tool or SDK path from the IDE settings. This gives you access to the tool binaries but doesn't load the environment variables.
- Most editors (and language plugins) look for tools on the `PATH` and run them in the context of your project, so adding the `mise` shims to the `PATH` may be enough (see [below](#adding-shims-to-path-default-shell)). This runs the tool provided by mise and loads the environment variables.
- In other cases, you may need to manually enter the path to the tools provided by `mise` in the IDE settings. Find it with [`mise which <tool>`](./cli/which.md) or [`mise where`](./cli/where). If the plugin supports it, you can also provide the path to the tool shim (e.g. `~/.local/share/mise/shims/node`), which also loads the environment variables when the tool runs.
- Finally, some custom plugins have been developed to work with `mise`. You can find them in the [IDE Plugins](#ide-plugins) section.

## Adding shims to PATH in your default shell profile {#adding-shims-to-path-default-shell}

IDEs work better with [shims](./dev-tools/shims) than with environment variable modifications. The simplest approach is
to add the mise shim directory to `PATH`.

For IntelliJ and VSCode—and likely others—you can modify your default shell's login (or "profile")
script. Find your default shell with:

::: code-group

```shell [macos]
dscl . -read /Users/$USER UserShell
```

```shell [linux]
getent passwd $USER | cut -d: -f7
```

:::

You can change your default shell with `chsh -s /path/to/shell`, but you may need
to add it to `/etc/shells` first. Once you know the right shell, modify the appropriate file:

::: code-group

```zsh
# ~/.zprofile
eval "$(mise activate zsh --shims)"
```

```bash
# ~/.bash_profile or ~/.bash_login or ~/.profile
eval "$(mise activate bash --shims)"
```

```fish
# ~/.config/fish/config.fish
if status is-interactive
  mise activate fish | source
else
  mise activate fish --shims | source
end
```

:::

::: warning
Do not use /bin/bash or /usr/bin/bash on macOS. bash is complicated, decades old, and mise cannot use as many of its features.
Unless you consider yourself a bash expert and know why I (and Apple, for that matter) advise against it, use zsh on macOS.
:::

On Linux this file is read when you log into the machine, so changes take effect only after you log out and back in. See [VSCode](#vscode) below
for how to get VSCode to read the login file.

This assumes that `mise` is on `PATH`. If it is not, use the absolute path
(e.g. `eval "$($HOME/.local/bin/mise activate zsh --shims)"`).

Here are examples showing VSCode and IntelliJ using the `node` provided by `mise`:

::: tabs
=== VSCode

![vscode using shims](./shims-vscode.png)

=== IntelliJ
![intellij using shims](./shims-intellij.png)
:::

As mentioned above, using `shims` doesn't work with all mise features. For example, arbitrary [env vars](./environments/) in `[env]` are
only set when a shim is executed. Supporting them requires tighter integration with the IDE or a custom plugin.

## IDE Plugins

Here are some community plugins that have been developed to work with `mise`:

- Emacs: [mise.el](https://github.com/liuyinz/mise.el)
- IntelliJ: [intellij-mise](https://github.com/134130/intellij-mise)
- VSCode: [mise-vscode](https://github.com/hverlin/mise-vscode)

## Vim

```vim
" Prepend mise shims to PATH
let $PATH = $HOME . '/.local/share/mise/shims:' . $PATH
```

## Neovim

```lua
-- Prepend mise shims to PATH
vim.env.PATH = vim.env.HOME .. "/.local/share/mise/shims:" .. vim.env.PATH
```

For better Treesitter and LSP integration, see the [neovim cookbook](./mise-cookbook/neovim.md).

## Emacs

### Traditional shims way

```lisp
;; CLI tools installed by Mise
;; See: https://www.emacswiki.org/emacs/ExecPath
(setenv "PATH" (concat (getenv "PATH") ":/home/user/.local/share/mise/shims"))
(setq exec-path (append exec-path '("/home/user/.local/share/mise/shims")))
```

### Use with package [mise.el](https://github.com/eki3z/mise.el)

<https://github.com/eki3z/mise.el>

> A GNU Emacs library which uses the mise tool to determine per-directory/project environment variables and then set those environment variables on a per-buffer basis.

```lisp
(require 'mise)
(add-hook 'after-init-hook #'global-mise-mode)
```

## JetBrains Editors (IntelliJ, RustRover, PyCharm, WebStorm, RubyMine, GoLand, etc)

### IntelliJ Plugin

<https://github.com/134130/intellij-mise>

This plugin can automatically configure the IDE to use the tools provided by mise. It also has some support for running mise tasks and loading environment variables in run configurations.

### Direct SDK selection

Some JetBrains IDEs (or language plugins) support `mise` directly, allowing you to select the SDK version from the IDE settings.
Example for Java:

![SDK settings](./intellij-sdk-selection.png)

### SDK selection using asdf layout

Some plugins cannot yet find SDKs installed by `mise` but do support asdf.
In that case, a workaround is to symlink the mise tool directory, which has the same layout as asdf:

```sh
ln -s ~/.local/share/mise ~/.asdf
```

They should then show up in Project Settings:

![project settings](https://github.com/jdx/mise-docs/assets/216188/b34a0e3f-7af8-45c9-85b8-2c72bd1dc226)

For node (and possibly other languages), the setting is under "Languages & Frameworks":

![languages & frameworks](https://github.com/jdx/mise-docs/assets/216188/9926be1c-ab88-451a-8ace-edf2dac564b5)

## VSCode

### VSCode Automation Profile for macOS

Unlike Linux, macOS does not read the login shell profile (`~/.profile` or `~/.zprofile`) when you log into the machine. You'll likely
want to add this setting to your VSCode config so it loads your shims:

```json
    "terminal.integrated.automationProfile.osx": {
        "path": "/usr/bin/zsh",
        "args": ["--login"]
    }
```

:::tip
You can also use `["--login", "--interactive"]` if you want to include `~/.zshrc`.
:::

### VSCode Plugin

The [VSCode plugin](https://marketplace.visualstudio.com/items?itemName=hverlin.mise-vscode) can configure other extensions for you, so you don't need to modify your shell profile to add the shims to `PATH`.

It also provides features such as:

- Automatic configuration of other extensions to use tools provided by `mise`
- Management of `mise` tasks, tools, and environment variables directly from VSCode
- Loading of environment variables from `mise.toml` files in VSCode
- Autocompletion and snippets for `mise.toml` files
- Integration with VSCode tasks

<https://github.com/hverlin/mise-vscode/> ([Documentation](https://hverlin.github.io/mise-vscode/))

### Use [`mise exec`](./cli/exec) in launch Configuration

While modifying your default shell profile is likely the easiest solution, you can also configure
the tools in `launch.json`:

::: details mise exec launch.json example

```json
{
  "configurations": [
    {
      "type": "node",
      "request": "launch",
      "name": "Launch Program",
      "program": "${file}",
      "args": [],
      "osx": {
        "runtimeExecutable": "mise"
      },
      "linux": {
        "runtimeExecutable": "mise"
      },
      "runtimeArgs": ["exec", "--", "node"]
    }
  ]
}
```

:::

## Xcode

Xcode projects can run system commands from script build phases and schemes. Because Xcode sandboxes
script execution with `/usr/bin/sandbox-exec`, don't expect mise and its
automatically activated tools to work out of the box. First, add `$(SRCROOT)/mise.toml` to the
list of **Input files** so that Xcode allows reads of that file. Then use `mise activate` to
activate the tools you need:

```bash
# -C ensures that Mise loads the configuration from the Mise configuration
# file in the project's root directory.
eval "$($HOME/.local/bin/mise activate -C $SRCROOT bash --shims)"

swiftlint
```
