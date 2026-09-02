# About

`mise` (pronounced "meez") or "mise-en-place" is a development environment setup tool.
The name refers to a French culinary phrase that roughly translates to "setup" or "put in place".
The idea is that before you begin cooking, you should have all your utensils and ingredients
ready and in their place.

`mise` does the same for your projects. Using its `mise.toml` config file,
you'll have a consistent way to set up and interact with your projects no matter what
language they're written in.

Its functionality is grouped into three categories, described below.

`mise` installs and manages dev tools/runtimes like node, python, or terraform. It
simplifies installing these tools and lets you specify which version to use in each
project. `mise` supports [hundreds](/plugins.md) of dev tools.

`mise` manages environment variables, letting you specify configuration like
`AWS_ACCESS_KEY_ID` that differs between projects. It can also
automatically activate a [Python virtualenv](/lang/python) when you enter a project.

`mise` is a task runner that lets developers share common tasks within
a project and makes things like running tasks on file changes
easy.

## Contact

`mise` was initially created by [Jeff Dickey](https://jdx.dev). The goal is
to make local development of software easy and consistent across languages. Jeff
has spent many years building dev tools and thinking about the problems that `mise`
addresses.

This project is a labor of love. Jeff created it because he wanted to make
your life as a developer easier. We hope you find it useful. Feedback is a massive
driver for us. If you have anything positive or negative to say—even if it's just
to say hi—please reach out on [Twitter](https://twitter.com/jdxcode),
[Mastodon](https://fosstodon.org/@jdx), [Discord](https://discord.gg/UBa7pJUN7Z),
or `jdx at this domain`.
