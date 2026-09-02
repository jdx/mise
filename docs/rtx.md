# Coming from rtx

`mise` was formerly called `rtx`. The name was changed to avoid confusion with Nvidia's
line of graphics cards. This wasn't a legal issue, just general confusion: when people
first heard about the project or saw it posted, they didn't realize it was a CLI tool. It
was also difficult to search for on Google, Twitter, Slack, and elsewhere. This was the top
complaint about `rtx`, and many people were fairly outspoken about disliking the name for
this reason. `rtx` was supposed to be a working title that I intended to change but never
got around to. This change should have happened earlier, when there were fewer users, and
I apologize for not doing it sooner knowing that it was likely to be necessary at some point.

To upgrade from `rtx` to `mise`, install `mise` and it should automatically
migrate its internal directories, moving `~/.local/share/rtx/installs/*` to `~/.local/share/mise/installs/*`
(skipping Python and Ruby, which cannot be moved), `~/.local/share/rtx/plugins` to `~/.local/share/mise/plugins`,
and `~/.config/rtx` to `~/.config/mise` (if the destination does not exist). Python and Ruby
will need to be reinstalled with `mise install`.

`mise` will continue reading `.rtx.toml` files for some time, but that will eventually
be deprecated, so please rename them to `mise.toml`. `mise` will not read `RTX_*`
env vars, so those will need to be changed to `MISE_*`. Anything using a local `.rtx` or
`.config/rtx` directory will need to be moved to `.mise`/`.config/mise`.

I apologize if this migration is not seamless; however, I think moving to a name that
is easier to search for and avoids confusion is better for everyone. I also apologize
for it being abrupt—I couldn't think of a way to "slow roll" this change out
while also keeping the GitHub repo.

Users of the `rtx-action` GitHub action will need to switch to `mise-action` (and also
bump the major version to v2).

If you build infrastructure where users may still be calling `rtx activate` in their
shell rc scripts, you can create a symlink `ln -s /path/to/mise /path/to/rtx` so
`rtx activate` still functions.

For <https://mise.run>, we're using `~/.local/bin/mise`
as the executable path instead of the old `~/.local/share/rtx/bin/mise`
to keep things a bit cleaner. You can still use the old location if you like by setting
`MISE_INSTALL_PATH`.

If you use shims, a `mise reshim` will be necessary to update the shims.

Thanks for trying out my little CLI tool, by the way. I find this project incredibly
fulfilling to work on, and I love seeing people have success with it. I have a
tremendous passion for building dev tools, and the ideas in `mise` are the product of
thinking about building a tool like this for over a decade.

If you aren't happy with `mise` or the way I'm running this project, even in a tiny way,
please let me know. You can [contact me privately](/about#contact) if you like. I certainly
won't take offense, and I would rather you say something than nothing. Otherwise
I'll never know.
