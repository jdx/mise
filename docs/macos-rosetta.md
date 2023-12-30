# Running rtx under macOS Rosetta

If you have a need to run tools as x86_64 on Apple Silicon, this can be done with rtx however you'll currently 
need to use the x86_64 version of rtx itself. There is an [outstanding issue](https://github.com/jdx/rtx/issues/405)
to support this with an env var like RTX_ARCH=x86_64 to make it more seamless.

A common reason for doing this is to support compiling node <=14.

First, you'll need a copy of rtx that's built for x86_64:

```sh
$ mkdir -p ~/.local/share/rtx/bin
$ curl https://rtx.jdx.dev/rtx-latest-macos-x64 > ~/.local/share/rtx/bin/rtx-x64
$ chmod +x ~/.local/share/rtx/bin/rtx-x64
$ ~/.local/share/rtx/bin/rtx-x64 --version
rtx 2024.x.x
```

::: warning
If `~/.local/share/rtx/bin` is not in PATH, you'll need to prefix all commands with `~/.local/share/rtx/bin/rtx-x64`.
:::

Now you can use `rtx-x64` to install tools:

```sh
$ rtx-x64 use -g node@20
```
