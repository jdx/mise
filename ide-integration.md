# IDE Integration

IDEs work better with shims than they do environment variable modifications. The simplest way is
to add the mise shim directory to PATH.

For IntelliJ and VSCode—and likely others, you can modify `~/.zprofile`
with the following:

```sh
export PATH="$HOME/.local/share/mise/shims:$PATH"
```

This won't work for all of mise's functionality. For example, arbitrary env vars in `[env]` will only be set
if a shim is executed. For this we need tighter integration with the IDE and a custom plugin. If you feel
ambitious, take a look at existing direnv extensions for your IDE and see if you can modify it to work for mise.
Direnv and mise work similarly and there should be a direnv extension that can be used as a starting point.

Alternatively, you may be able to get tighter integration with a direnv extension and using the
[`use_mise`](/direnv) direnv function.


## emacs

```lisp
;; CLI tools installed by Mise
;; See: https://www.emacswiki.org/emacs/ExecPath
(setenv "PATH" (concat (getenv "PATH") ":/home/user/.local/share/mise/shims"))
(setq exec-path (append exec-path '("/home/user/.local/share/mise/shims")))
```

## Xcode

Xcode projects can system commands from script build phases and schemes. Since Xcode doesn't source your shell profile the tool versions pinned by your project's Mise configuration file are not automatically resolved through the `$PATH` environment variable.
To work around this, you can use the `mise x`:

```bash
mise x -C $SRCROOT swiftlint
```

Note that the `-C` flag ensures that Mise will use the project's root directory, where the Mise configuration file lives, as the working directory for the command. Note that in schemes pre and post action scripts, you'll have to enable "Provide build settings from" to inherit the `$SRCROOT` environment variable.

## [YOUR IDE HERE]

I am not a heavy IDE user. I use JetBrains products but I don't actually
like to execute code directly inside of them often so I don't have much
personal advice to offer for IDEs generally. That said, people often
ask about how to get their IDE to work with mise so if you've done this
for your IDE, please consider sending a PR to this page with some
instructions (however rough they are, starting somewhere is better than
nothing).
