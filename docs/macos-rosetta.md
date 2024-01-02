# Running mise under macOS Rosetta

If you have a need to run tools as x86_64 on Apple Silicon, this can be done with mise however you'll currently 
need to use the x86_64 version of mise itself. There is an [outstanding issue](https://github.com/jdx/mise/issues/405)
to support this with an env var like RTX_ARCH=x86_64 to make it more seamless.

A common reason for doing this is to support compiling node <=14.

First, you'll need a copy of mise that's built for x86_64:

```sh
$ mkdir -p ~/.local/share/mise/bin
$ curl https://mise.jdx.dev/mise-latest-macos-x64 > ~/.local/share/mise/bin/mise-x64
$ chmod +x ~/.local/share/mise/bin/mise-x64
$ ~/.local/share/mise/bin/mise-x64 --version
mise 2024.x.x
```

::: warning
If `~/.local/share/mise/bin` is not in PATH, you'll need to prefix all commands with `~/.local/share/mise/bin/mise-x64`.
:::

Now you can use `mise-x64` to install tools:

```sh
$ mise-x64 use -g node@20
```
