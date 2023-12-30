# IDE Integration

IDEs work better with shims than they do environment variable modifications. The simplest way is
to add the rtx shim directory to PATH.

For IntelliJ and VSCode—and likely others, you can modify `~/.zprofile`
with the following:

```
export PATH="$HOME/.local/share/rtx/shims:$PATH"
```

This won't work for all of rtx's functionality. For example, arbitrary env vars in `[env]` will only be set
if a shim is executed. For this we need tighter integration with the IDE and a custom plugin. If you feel
ambitious, take a look at existing direnv extensions for your IDE and see if you can modify it to work for rtx.
Direnv and rtx work similarly and there should be a direnv extension that can be used as a starting point.

Alternatively, you may be able to get tighter integration with a direnv extension and using the
[`use_rtx`](#direnv) direnv function.
