# `mise direnv`

- **Usage**: `mise direnv <SUBCOMMAND>`
- **Source code**: [`src/cli/direnv.rs`](https://github.com/jdx/mise/blob/main/src/cli/direnv.rs)

Output direnv function to use mise inside direnv

See <https://mise.jdx.dev/direnv.html> for more information

Because this generates the legacy files based on currently installed plugins,
you should run this command after installing new plugins. Otherwise
direnv may not know to update environment variables when legacy file versions change.

## Subcommands

- [`mise direnv activate`](/cli/direnv/activate.md)
