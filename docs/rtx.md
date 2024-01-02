# Coming from rtx

`mise` was formerly called `rtx`. The name was changed to avoid confusion with nvidia's
line of graphics cards.

To upgrade from `rtx` to `mise`, simply install `mise` and it should automatically
migrate its internal directories, moving `~/.local/share/rtx` to `~/.local/share/mise`
and `~/.config/rtx` to `~/.config/mise` (if the destination does not exist).

`mise` will continue reading `.rtx.toml` files for some time but that eventually will
be deprecaated so please rename them to `.mise.toml`.

I apologize if this migration is not seamless however I think moving to a name that
is easier to search for and avoids confusion is better for everyone.

