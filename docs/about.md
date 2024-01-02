# About mise-en-place

`mise` (pronounced "meez") or "mise-en-place" is a development environment setup tool.
The name refers to a French culinary phrase that roughly translates to "setup" or "put in place".
The idea is that before one begins cooking, they should have all their utensils and ingredients
ready to go in their place.

`mise` does the same for your projects. You create a `.mise.toml` file in your project
and no matter what language it's written in, you'll be able to
set it up and run tasks for it with common commands like `mise run lint` or `mise run test`.
This file can either be checked into a project to share amongst developers, or it can
be created by an individual and not committed for personal configuration.

`mise` installs and manages dev tools/runtimes like node, python, or terraform both
simplifying installing these tools and allowing you to specify which version of these
tools to use in different projects.

`mise` also manages environment variables letting you specify configuration like
`AWS_ACCESS_KEY_ID` that may differ between projects.

Lastly, `mise` is a task runner that can be used to share common tasks within
a project among developers and make things like running tasks on file changes
easy.
