# Coming from rtx

`mise` was formerly called `rtx`. The name was changed to avoid confusion with Nvidia's
line of graphics cards. This wasn't a legal issue, but just general confusion. When
people first hear about the project or see it posted they wouldn't realize it was talking
about a CLI tool. It was a bit difficult to search for on Google but also places like
Twitter and in Slack searches and things. This was the top complaint about `rtx` and
many people were fairly outspoken about disliking the name for this reason. `rtx` was
supposed to be a working title that I intended to change but never got around to doing.
This change should've happened earlier when there were fewer users and I apologize for
not having done that sooner knowing that this was likely going to be necessary at some point.

To upgrade from `rtx` to `mise`, simply install `mise` and it should automatically
migrate its internal directories, moving `~/.local/share/rtx/installs/*` to `~/.local/share/mise/installs/*`
(skipping python & ruby which cannot be moved), `~/.local/share/rtx/plugins` to `~/.local/share/mise/plugins`,
and `~/.config/rtx` to `~/.config/mise` (if the destination does not exist). Python and Ruby
installs will need to be reinstalled with `mise install`.

`mise` will continue reading `.rtx.toml` files for some time but that eventually will
be deprecated so please rename them to `.mise.toml`. `mise` will not read from `RTX_*`
env vars so those will need to be changed to `MISE_*`. Anything using a local `.rtx` or
`.config/rtx` directory will need to be moved to `.mise`/`.config/mise`.

I apologize if this migration is not seamless however I think moving to a name that
is easier to search for and avoids confusion is better for everyone. I also apologize
for it being abrupt—I simply couldn't think of a way to "slow roll" this change out
while also keeping the GitHub repo.

Users of the `rtx-action` GitHub action will need to switch to `mise-action` (and also
bump the major version to v2).

If you build infrastructure where users may still be calling `rtx activate` in their
shell rc scripts, you can create a symlink `ln -s /path/to/mise /path/to/rtx` so
`rtx activate` still functions.

For <https://mise.run>, we're using `~/.local/bin/mise`
as the executable PATH instead of the old directory `~/.local/share/rtx/bin/mise`
to keep things a bit cleaner. You can still use the old style if you like by setting
`MISE_INSTALL_PATH`.

If you use shims, a `mise reshim` will be necessary to update the shims.

Thanks for trying out my little CLI tool by the way. I find this project incredibly
fulfilling to work on and seeing people have success using. I have
tremendous passion for building dev tools and the ideas in `mise` are the product of
me thinking about building a tool like this for over a decade.

If you aren't happy with `mise` or the way I'm running this project, even in a tiny way,
please let me know. You can [contact me privately](/about#contact) if you like. I certainly
won't take offense and I would prefer you say something rather than nothing. Otherwise
I'll never know.
