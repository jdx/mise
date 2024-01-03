# Coming from rtx

`mise` was formerly called `rtx`. The name was changed to avoid confusion with Nvidia's
line of graphics cards.

To upgrade from `rtx` to `mise`, simply install `mise` and it should automatically
migrate its internal directories, moving `~/.local/share/rtx` to `~/.local/share/mise`
and `~/.config/rtx` to `~/.config/mise` (if the destination does not exist).

`mise` will continue reading `.rtx.toml` files for some time but that eventually will
be deprecated so please rename them to `.mise.toml`. `mise` will not read from `RTX_*`
env vars so those will need to be changed to `MISE_*`. Anything using a local `.rtx` or
`.config/rtx` directory will need to be moved to `.mise`/`.config/mise`.

I apologize if this migration is not seamless however I think moving to a name that
is easier to search for and avoids confusion is better for everyone.

Users of the `rtx-action` GitHub action will need to switch to `mise-action` (and also
bump the major version to v2).
