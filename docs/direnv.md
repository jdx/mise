---
---

# direnv

[direnv](https://direnv.net) and rtx both manage environment variables based on directory. Because they both analyze
the current environment variables before and after their respective "hook" commands are run, they can sometimes conflict with each other.

If you have an issue, it's likely to do with the ordering of PATH. This means it would
really only be a problem if you were trying to manage the same tool with direnv and rtx. For example,
you may use `layout python` in an `.envrc` but also be maintaining a `.tool-versions` file with python
in it as well.

A more typical usage of direnv would be to set some arbitrary environment variables, or add unrelated
binaries to PATH. In these cases, rtx will not interfere with direnv.

### rtx inside of direnv (`use rtx` in `.envrc`)

If you do encounter issues with `rtx activate`, or just want to use direnv in an alternate way,
this is a simpler setup that's less likely to cause issues—at the cost of functionality.

This may be required if you want to use direnv's `layout python` with rtx. Otherwise there are
situations where rtx will override direnv's PATH. `use rtx` ensures that direnv always has control.

To do this, first use `rtx` to build a `use_rtx` function that you can use in `.envrc` files:

```sh
rtx direnv activate > ~/.config/direnv/lib/use_rtx.sh
```

Now in your `.envrc` file add the following:

```sh
use rtx
```

direnv will now call rtx to export its environment variables. You'll need to make sure to add `use_rtx`
to all projects that use rtx (or use direnv's `source_up` to load it from a subdirectory). You can also add `use rtx` to `~/.config/direnv/direnvrc`.

Note that in this method direnv typically won't know to refresh `.tool-versions` files
unless they're at the same level as a `.envrc` file. You'll likely always want to have
a `.envrc` file next to your `.tool-versions` for this reason. To make this a little
easier to manage, I encourage _not_ actually using `.tool-versions` at all, and instead
setting environment variables entirely in `.envrc`:

```sh
export RTX_NODE_VERSION=20.0.0
export RTX_PYTHON_VERSION=3.11
```

Of course if you use `rtx activate`, then these steps won't have been necessary and you can use rtx
as if direnv was not used.

If you continue to struggle, you can also try using the [shims method](./shims).

### Do you need direnv?

While making rtx compatible with direnv is, and will always be a major goal of this project, I also
want rtx to be capable of replacing direnv if needed. This is why rtx includes support for managing
env vars and [virtualenv](https://github.com/jdx/rtx/blob/main/docs/python.md#experimental-automatic-virtualenv-creationactivation)
for python using `.rtx.toml`.

If you find you continue to need direnv, please open an issue and let me know what it is to see if
it's something rtx could support. rtx will never be as capable as direnv with a DSL like `.envrc`,
but I think we can handle enough common use cases to make that unnecessary for most people.
