# IDE Integration

IDEs work better with shims than they do environment variable modifications. The simplest way is
to add the mise shim directory to PATH.

For IntelliJ and VSCode—and likely others, you can modify your default shell's profile
script. Your default shell can be found with:

* macos – `dscl . -read /Users/$USER UserShell`
* linux – `getent passwd $USER | cut -d: -f7`

You can change your default shell with `chsh -s /path/to/shell` but you may need
to first add it to `/etc/shells`.

Once you know the right one, modify the appropriate file:

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

This assumes that `mise` is on PATH. If it is not, you'll need to use the absolute path (
e.g.: `eval "$($HOME/.local/bin/mise activate zsh)"`).

This won't work for all of mise's functionality. For example, arbitrary env vars in `[env]` will
only be set
if a shim is executed. For this we need tighter integration with the IDE and a custom plugin. If you
feel
ambitious, take a look at existing direnv extensions for your IDE and see if you can modify it to
work for mise.
Direnv and mise work similarly and there should be a direnv extension that can be used as a starting
point.

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

## emacs

- Traditional shims way

```lisp
;; CLI tools installed by Mise
;; See: https://www.emacswiki.org/emacs/ExecPath
(setenv "PATH" (concat (getenv "PATH") ":/home/user/.local/share/mise/shims"))
(setq exec-path (append exec-path '("/home/user/.local/share/mise/shims")))
```

- Use with package [mise.el](https://github.com/liuyinz/mise.el)

```lisp
(require 'mise)
(add-hook 'after-init-hook #'global-mise-mode)
```

## Xcode

Xcode projects can run system commands from script build phases and schemes. Since Xcode sandboxes
the execution of the script using the tool `/usr/bin/sandbox-exec`, don't expect Mise and the
automatically-activated tools to work out of the box. First, you'll need to
add `$(SRCROOT)/.mise.toml` to the list of **Input files**. This is necessary for Xcode to allow
reads to that file. Then, you can use `mise activate` to activate the tools you need:

```bash
# -C ensures that Mise loads the configuration from the Mise configuration 
# file in the project's root directory.
eval "$($HOME/.local/bin/mise activate -C $SRCROOT bash --shims)"

swiftlint
```

## JetBrains Editors (IntelliJ, RustRover, PyCharm, WebStorm, RubyMine, GoLand, etc)

Some JetBrains IDEs have direct support for mise, others have support for asdf which can be used by
first symlinking the mise tool directory which is the
same layout as asdf:

```sh
ln -s ~/.local/share/mise ~/.asdf
```

Then they should show up on in Project Settings:

![project settings](https://github.com/jdx/mise-docs/assets/216188/b34a0e3f-7af8-45c9-85b8-2c72bd1dc226)

Or in the case of node (possibly other languages), it's under "Languages & Frameworks":

![languages & frameworks](https://github.com/jdx/mise-docs/assets/216188/9926be1c-ab88-451a-8ace-edf2dac564b5)

## VSCode

While modifying `~/.zprofile` is likely the easiest solution, you can also set
the tools in `launch.json`:

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
      "runtimeArgs": [
        "x",
        "--",
        "node"
      ]
    }
  ]
}
```

## [YOUR IDE HERE]

I am not a heavy IDE user. I use JetBrains products but I don't actually
like to execute code directly inside of them often so I don't have much
personal advice to offer for IDEs generally. That said, people often
ask about how to get their IDE to work with mise so if you've done this
for your IDE, please consider sending a PR to this page with some
instructions (however rough they are, starting somewhere is better than
nothing).

Also if you've found a setup that you prefer to what is listed here consider
adding it as a suggestion.
