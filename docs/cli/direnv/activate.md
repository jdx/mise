# `mise direnv activate`

**Usage**: `mise direnv activate`

**Source code**: [`src/cli/direnv/activate.rs`](https://github.com/jdx/mise/blob/main/src/cli/direnv/activate.rs)

Output direnv function to use mise inside direnv

See <https://mise.jdx.dev/direnv.html> for more information

Because this generates the legacy files based on currently installed plugins,
you should run this command after installing new plugins. Otherwise
direnv may not know to update environment variables when legacy file versions change.

Examples:

    mise direnv activate > ~/.config/direnv/lib/use_mise.sh
    echo 'use mise' > .envrc
    direnv allow
