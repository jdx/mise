# About

`mise` (pronounced "meez") or "mise-en-place" is a development environment setup tool.
The name refers to a French culinary phrase that roughly translates to "setup" or "put in place".
The idea is that before one begins cooking, they should have all their utensils and ingredients
ready to go in their place.

`mise` does the same for your projects. Using its `.mise.toml` config file,
you'll have a consistent way to setup and interact with your projects no matter what
language they're written in.

Its functionality is grouped into 3 categories described below.

`mise` installs and manages dev tools/runtimes like node, python, or terraform both
simplifying installing these tools and allowing you to specify which version of these
tools to use in different projects. `mise` supports [hundreds](/plugins.md) of dev tools.

`mise` manages environment variables letting you specify configuration like
`AWS_ACCESS_KEY_ID` that may differ between projects. It can also be used to
automatically activate a [Python virtualenv](/lang/python) when entering projects too.

`mise` is a task runner that can be used to share common tasks within
a project among developers and make things like running tasks on file changes
easy.

## Contact

`mise` is mostly built and maintained by me, [Jeff Dickey](https://jdx.dev). The goal is
to make local development of software easy and consistent across languages. I
have spent many years building dev tools and thinking about the problems that `mise`
addresses.

I try to use the first-person in these docs since the reality is it's generally me
writing them and I think it makes it more interesting having a bit of my personality
in the text.

This project is simply a labor of love. I am making it because I want to make
your life as a developer easier. I hope you find it useful. Feedback is a massive
driver for me. If you have anything positive or negative to say-even if it's just
to say hi-please reach out to me either on [Twitter](https://twitter.com/jdxcode),
[Mastodon](https://fosstodon.org/@jdx), [Discord](https://discord.gg/UBa7pJUN7Z),
or `jdx at this domain`.
