# mise-brew-metadata

Homebrew formula and cask metadata models used by [mise](https://mise.jdx.dev/).

The crate parses formula, bottle, source, cask artifact, and tap URL metadata.
mise owns API fetching, cask installation, and evaluation of tap Ruby definitions
when published metadata is absent.

This is an internal component of mise; its API may change between releases.
