# Project Roadmap

Issues
marked ["enhancements"](https://github.com/jdx/mise/issues?q=is%3Aissue+is%3Aopen+label%3Aenhancement)
are the best way to read about ideas for future
functionality. As far as general scope however, these are likely going to be focuses for 2024:

* Tasks - this is the newest headline feature of mise and needs to be refined, tested, and iterated
  on before it can come out of experimental
* Documentation website - we've outgrown what is mostly a single README
* Supply chain hardening - securing mise is very important and this topic has had a lot of interest
  from the community. We plan to make several improvements on this front
* Improved python development - better virtualenv integration, precompiled python binaries, and
  other areas are topics that frequently come up to improve
* Improved plugin development - it's unclear what we'll do exactly but in general we want to make
  the experience of vending tools for asdf/mise to be better and safer.
* GUI/TUI - While we're all big CLI fans, it still would be great to better visualize what tools are
  available, what your configuration is, and other things via some kind of UI.

## Versioning

mise uses [Calver](https://calver.org/) versioning (`2024.1.0`).
Breaking changes will be few but when they do happen,
they will be communicated in the CLI with plenty of notice whenever possible.

Rather than have SemVer major releases to communicate change in large releases,
new functionality and changes can be opted-into with settings like `experimental = true`.
This way plugin authors and users can
test out new functionality immediately without waiting for a major release.

The numbers in Calver (YYYY.MM.RELEASE) simply represent the date of the release—not compatibility
or how many new features were added.
Each release will be small and incremental.

## Anti-goals

* Dependency management - mise expects you to have system dependencies (like openssl or readline)
  already setup and configured. This makes it different than tools like nix which manage all
  dependencies for you. While this seems like an obvious downside, it actually ends up making mise
  far easier to use than nix. That said, we would like to make managing system dependencies easier
  where we can but this is likely going to be simply via better docs and error messages.
* DevOps tooling - mise is designed with local development in mind. While there are certainly many
  devs using it for production/server roles which we support and encourage, that will never be the
  our focus on the roadmap. Building a better ansible/terraform/kubernetes just isn't the goal.
* Remote task caching - turbopack, moonrepo, and many others are trying to solve this (major)
  problem. mise's task runner will likely always just be a simple convenience around executing
  scripts.
