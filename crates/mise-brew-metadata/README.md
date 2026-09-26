# mise-brew-metadata

Homebrew formula metadata models and API lookup used by [mise](https://mise.jdx.dev/).

The crate fetches formulae from the Homebrew JSON API, resolves aliases and old
names, and exposes the bottle and source metadata used by mise's package manager.
