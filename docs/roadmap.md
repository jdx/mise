# Roadmap

Issues
marked ["enhancements"](https://github.com/jdx/mise/issues?q=is%3Aissue+is%3Aopen+label%3Aenhancement)
are the best way to read about ideas for future
functionality. As far as general scope however, these are likely going to be focuses for 2025:

- Removing experimental flag on features - several features are still marked as experimental. My hope
  is all features will be GA by the end of 2025.
- Supply chain hardening - much progress was made here by adopting ubi and aqua and switching to those backends
  for the majority of tools. In 2025, we'll continue migrating more tools where possible away from asdf.
  Aqua tools now include native verification support for SLSA provenance, Cosign signatures, and GitHub attestations without requiring external dependencies.
- Tasks improvements - tasks came out of experimental at the end of 2024 but there are still features
  that I'd like to see from tasks such as prompts and error handling.
- Hook improvements - hooks are very new in mise and still experimental. I suspect the design of hooks
  will change a bit as we learn more about how they are used. It's unclear what exactly will happen here right now.
- Improved python development - python improved a lot with better venv support and the precompiled
  binaries provided by Astral. As users are adopting this more we're learning about how mise can still
  be further improved for python development—which is the most complicated tool to support in mise by far.
  Where possible, the plan is to leverage uv as much as we can since they're the real experts when it
  comes to the python ecosystem.
- Further Windows support - non-WSL Windows support was added in 2024 but it is not heavily used. There are
  definitely bugs and gaps with Windows remaining but we should be able to get Windows much closer to UNIX
  by the end of the year. More testing on Windows would be a big help here.
- GUI/TUI - A few commands in mise make use of a TUI like `mise run`, `mise use`, and `mise up -i`,
  I'd like to see more done with these type of UIs in 2025.

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

- Dependency management - mise expects you to have system dependencies (like openssl or readline)
  already setup and configured. This makes it different than tools like nix which manage all
  dependencies for you. While this seems like an obvious downside, it actually ends up making mise
  far easier to use than nix. That said, we would like to make managing system dependencies easier
  where we can but this is likely going to be simply via better docs and error messages.
- DevOps tooling - mise is designed with local development in mind. While there are certainly many
  devs using it for production/server roles which we support and encourage, that will never be the
  our focus on the roadmap. Building a better ansible/terraform/kubernetes just isn't the goal.
- Remote task caching - turbopack, moonrepo, and many others are trying to solve this (major)
  problem. mise's task runner will likely always just be a simple convenience around executing
  scripts.
