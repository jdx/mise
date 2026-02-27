# Changelog

## [2026.2.22](https://github.com/jdx/mise/compare/v2026.2.21..v2026.2.22) - 2026-02-27

### 🚀 Features

- add `--outdated` flag to `mise plugins ls` by @jdx in [#8360](https://github.com/jdx/mise/pull/8360)

### 🐛 Bug Fixes

- **(github)** resolve rename_exe search dir for archives with bin/ subdirectory by @jdx in [#8358](https://github.com/jdx/mise/pull/8358)
- **(install)** skip tools=true env directives during backend installation by @jdx in [#8356](https://github.com/jdx/mise/pull/8356)
- **(ruby)** resolve correct Windows checksums in lockfile by @jdx in [#8357](https://github.com/jdx/mise/pull/8357)

### 📦 Registry

- switch terradozer backend to github fork by @chenrui333 in [#8365](https://github.com/jdx/mise/pull/8365)

### Chore

- **(release)** fix duplicated version prefix in release title by @jdx in [#8359](https://github.com/jdx/mise/pull/8359)

### New Contributors

- @chenrui333 made their first contribution in [#8365](https://github.com/jdx/mise/pull/8365)

### 📦 Aqua Registry Updates

#### New Packages (1)

- [`huseyinbabal/taws`](https://github.com/huseyinbabal/taws)

#### Updated Packages (2)

- [`block/goose`](https://github.com/block/goose)
- [`pre-commit/pre-commit`](https://github.com/pre-commit/pre-commit)

## [2026.2.21](https://github.com/jdx/mise/compare/v2026.2.20..v2026.2.21) - 2026-02-26

### 🐛 Bug Fixes

- **(exec)** respect PATH order for virtualenv resolution in mise x by @jdx in [#8342](https://github.com/jdx/mise/pull/8342)
- **(task)** revert process group changes that cause hangs with nested mise tasks by @jdx in [#8347](https://github.com/jdx/mise/pull/8347)
- **(task)** resolve vars from subdirectory configs for monorepo tasks by @jdx in [#8343](https://github.com/jdx/mise/pull/8343)
- **(task)** resolve dependencies before prepare to fix monorepo glob deps by @jdx in [#8353](https://github.com/jdx/mise/pull/8353)
- python noarch with Conda backend by @wolfv in [#8349](https://github.com/jdx/mise/pull/8349)

### New Contributors

- @wolfv made their first contribution in [#8349](https://github.com/jdx/mise/pull/8349)

### 📦 Aqua Registry Updates

#### New Packages (3)

- [`alexhallam/tv`](https://github.com/alexhallam/tv)
- [`arcanist-sh/hx`](https://github.com/arcanist-sh/hx)
- [`dathere/qsv`](https://github.com/dathere/qsv)

#### Updated Packages (3)

- [`astral-sh/ruff`](https://github.com/astral-sh/ruff)
- [`caarlos0/fork-cleaner`](https://github.com/caarlos0/fork-cleaner)
- [`rhysd/actionlint`](https://github.com/rhysd/actionlint)

## [2026.2.20](https://github.com/jdx/mise/compare/v2026.2.19..v2026.2.20) - 2026-02-25

### 🚀 Features

- **(conda)** replace custom backend with rattler crates by @jdx in [#8325](https://github.com/jdx/mise/pull/8325)
- **(task)** enforce per-task timeout configuration by @tvararu in [#8250](https://github.com/jdx/mise/pull/8250)
- **(vsix)** added vsix archives to http backend by @sosumappu in [#8306](https://github.com/jdx/mise/pull/8306)
- add core dotnet plugin for .NET SDK management by @jdx in [#8326](https://github.com/jdx/mise/pull/8326)

### 🐛 Bug Fixes

- **(conda)** preserve conda_packages on locked install and fix temp file race by @jdx in [#8335](https://github.com/jdx/mise/pull/8335)
- **(conda)** deduplicate repodata records to fix solver error on Linux by @jdx in [#8337](https://github.com/jdx/mise/pull/8337)
- **(env)** include watch_files in fast-path early exit check by @jdx in [#8317](https://github.com/jdx/mise/pull/8317)
- **(env)** clear fish completions when setting/unsetting shell aliases by @jdx in [#8324](https://github.com/jdx/mise/pull/8324)
- **(lockfile)** prevent lockfile writes when --locked is set by @jdx in [#8308](https://github.com/jdx/mise/pull/8308)
- **(lockfile)** prune orphan tool entries on mise lock by @mackwic in [#8265](https://github.com/jdx/mise/pull/8265)
- **(lockfile)** error on contradictory locked=true + lockfile=false config by @jdx in [#8329](https://github.com/jdx/mise/pull/8329)
- **(regal)** Update package location by @charlieegan3 in [#8315](https://github.com/jdx/mise/pull/8315)
- **(release)** strip markdown heading prefix from communique release title by @jdx in [#8303](https://github.com/jdx/mise/pull/8303)
- **(schema)** enforce additionalProperties constraint for env by @adamliang0 in [#8328](https://github.com/jdx/mise/pull/8328)

### 📚 Documentation

- Remove incorrect oh-my-zsh plugin ordering comment by @bvosk in [#8323](https://github.com/jdx/mise/pull/8323)
- require AI disclosure on GitHub comments by @jdx in [#8330](https://github.com/jdx/mise/pull/8330)

### 📦 Registry

- add `oxfmt` by @taoufik07 in [#8316](https://github.com/jdx/mise/pull/8316)

### New Contributors

- @adamliang0 made their first contribution in [#8328](https://github.com/jdx/mise/pull/8328)
- @tvararu made their first contribution in [#8250](https://github.com/jdx/mise/pull/8250)
- @bvosk made their first contribution in [#8323](https://github.com/jdx/mise/pull/8323)
- @taoufik07 made their first contribution in [#8316](https://github.com/jdx/mise/pull/8316)
- @charlieegan3 made their first contribution in [#8315](https://github.com/jdx/mise/pull/8315)
- @sosumappu made their first contribution in [#8306](https://github.com/jdx/mise/pull/8306)

### 📦 Aqua Registry Updates

#### New Packages (3)

- [`Tyrrrz/FFmpegBin`](https://github.com/Tyrrrz/FFmpegBin)
- [`elixir-lang/expert`](https://github.com/elixir-lang/expert)
- [`erikjuhani/basalt`](https://github.com/erikjuhani/basalt)

#### Updated Packages (5)

- [`caarlos0/fork-cleaner`](https://github.com/caarlos0/fork-cleaner)
- [`firecow/gitlab-ci-local`](https://github.com/firecow/gitlab-ci-local)
- [`jackchuka/mdschema`](https://github.com/jackchuka/mdschema)
- [`kunobi-ninja/kunobi-releases`](https://github.com/kunobi-ninja/kunobi-releases)
- [`peco/peco`](https://github.com/peco/peco)

## [2026.2.19](https://github.com/jdx/mise/compare/v2026.2.18..v2026.2.19) - 2026-02-22

### 🐛 Bug Fixes

- **(docs)** correct ripgrep command by @nguyenvulong in [#8299](https://github.com/jdx/mise/pull/8299)
- **(task)** skip setpgid for TTY stdin to fix interactive tasks by @jdx in [#8301](https://github.com/jdx/mise/pull/8301)
- clean up empty parent install dir on failed install by @jdx in [#8302](https://github.com/jdx/mise/pull/8302)

### Chore

- **(release)** run communique via mise x for PATH resolution by @jdx in [#8294](https://github.com/jdx/mise/pull/8294)

### 📦 Aqua Registry Updates

#### New Packages (2)

- [`kubie-org/kubie`](https://github.com/kubie-org/kubie)
- [`steipete/gogcli`](https://github.com/steipete/gogcli)

## [2026.2.18](https://github.com/jdx/mise/compare/v2026.2.17..v2026.2.18) - 2026-02-21

### 🚀 Features

- **(install)** auto-lock all platforms after tool installation by @jdx in [#8277](https://github.com/jdx/mise/pull/8277)

### 🐛 Bug Fixes

- **(config)** respect --yes flag for config trust prompts by @jdx in [#8288](https://github.com/jdx/mise/pull/8288)
- **(exec)** strip shims from PATH on Unix to prevent infinite recursion by @jdx in [#8276](https://github.com/jdx/mise/pull/8276)
- **(install)** validate --locked before --dry-run short-circuit by @altendky in [#8290](https://github.com/jdx/mise/pull/8290)
- **(release)** refresh PATH after mise up in release-plz by @jdx in [#8292](https://github.com/jdx/mise/pull/8292)
- **(schema)** replace unevaluatedProperties with additionalProperties by @jdx in [#8285](https://github.com/jdx/mise/pull/8285)
- **(task)** avoid duplicated stderr on task failure in replacing mode by @jdx in [#8275](https://github.com/jdx/mise/pull/8275)
- **(task)** use process groups to kill child process trees on Unix by @jdx in [#8279](https://github.com/jdx/mise/pull/8279)
- **(task)** run depends_post tasks even when parent task fails by @jdx in [#8274](https://github.com/jdx/mise/pull/8274)
- **(task)** suggest similar commands when mistyping a CLI subcommand by @jdx in [#8286](https://github.com/jdx/mise/pull/8286)
- **(task)** execute monorepo subdirectory prepare steps from root by @jdx in [#8291](https://github.com/jdx/mise/pull/8291)
- **(upgrade)** don't force-reinstall already installed versions by @jdx in [#8282](https://github.com/jdx/mise/pull/8282)
- **(watch)** restore terminal state after watchexec exits by @jdx in [#8273](https://github.com/jdx/mise/pull/8273)

### 📚 Documentation

- clarify that MISE_CEILING_PATHS excludes the ceiling directory itself by @jdx in [#8283](https://github.com/jdx/mise/pull/8283)

### Chore

- replace gen-release-notes script with communique by @jdx in [#8289](https://github.com/jdx/mise/pull/8289)

### New Contributors

- @altendky made their first contribution in [#8290](https://github.com/jdx/mise/pull/8290)

### 📦 Aqua Registry Updates

#### New Packages (4)

- [`Skarlso/crd-to-sample-yaml`](https://github.com/Skarlso/crd-to-sample-yaml)
- [`kunobi-ninja/kunobi-releases`](https://github.com/kunobi-ninja/kunobi-releases)
- [`swanysimon/markdownlint-rs`](https://github.com/swanysimon/markdownlint-rs)
- [`tmux/tmux-builds`](https://github.com/tmux/tmux-builds)

#### Updated Packages (2)

- [`firecow/gitlab-ci-local`](https://github.com/firecow/gitlab-ci-local)
- [`k1LoW/runn`](https://github.com/k1LoW/runn)

## [2026.2.17](https://github.com/jdx/mise/compare/v2026.2.16..v2026.2.17) - 2026-02-19

### 🚀 Features

- **(prepare)** update mtime of outputs after command is run by @halms in [#8243](https://github.com/jdx/mise/pull/8243)

### 🐛 Bug Fixes

- **(install)** use backend bin paths for per-tool postinstall hooks by @jdx in [#8234](https://github.com/jdx/mise/pull/8234)
- **(use)** write to config.toml instead of config.local.toml by @jdx in [#8240](https://github.com/jdx/mise/pull/8240)
- default legacy .mise.backend installs to non-explicit by @jean-humann in [#8245](https://github.com/jdx/mise/pull/8245)

### 🚜 Refactor

- **(config)** consolidate flat task_* settings into nested task.* by @jdx in [#8239](https://github.com/jdx/mise/pull/8239)

### Chore

- **(prepare)** refactor common code into ProviderBase by @halms in [#8246](https://github.com/jdx/mise/pull/8246)

### 📦 Aqua Registry Updates

#### Updated Packages (1)

- [`namespacelabs/foundation/nsc`](https://github.com/namespacelabs/foundation/nsc)

## [2026.2.16](https://github.com/jdx/mise/compare/v2026.2.15..v2026.2.16) - 2026-02-17

### 🚀 Features

- **(mcp)** add run_task tool for executing mise tasks by @joaommartins in [#8179](https://github.com/jdx/mise/pull/8179)
- **(node)** suggest setting node.flavor if the flavor is not found in mirror by @risu729 in [#8206](https://github.com/jdx/mise/pull/8206)

### 🐛 Bug Fixes

- **(java)** sort order for shorthand versions by @roele in [#8197](https://github.com/jdx/mise/pull/8197)
- **(node)** migrate env vars to settings by @risu729 in [#8200](https://github.com/jdx/mise/pull/8200)
- **(registry)** apply overrides in shims by @risu729 in [#8199](https://github.com/jdx/mise/pull/8199)
- migrate MISE_CARGO_BINSTALL_ONLY to settings by @risu729 in [#8202](https://github.com/jdx/mise/pull/8202)

### 📚 Documentation

- **(doctor)** fix subcommand in an example by @risu729 in [#8198](https://github.com/jdx/mise/pull/8198)

### 📦 Registry

- add github backend for typst by @3w36zj6 in [#8210](https://github.com/jdx/mise/pull/8210)

### Chore

- **(test)** disable flaky Forgejo e2e test by @jdx in [#8211](https://github.com/jdx/mise/pull/8211)

## [2026.2.15](https://github.com/jdx/mise/compare/v2026.2.14..v2026.2.15) - 2026-02-17

### 🚀 Features

- **(task)** stream keep-order output in real-time per task by @jdx in [#8164](https://github.com/jdx/mise/pull/8164)

### 🐛 Bug Fixes

- **(aqua)** resolve lockfile artifacts for target platform (fix discussion #7479) by @mackwic in [#8183](https://github.com/jdx/mise/pull/8183)
- **(exec)** strip shims from PATH to prevent recursive shim execution by @jdx in [#8189](https://github.com/jdx/mise/pull/8189)
- **(hook-env)** preserve PATH reordering done after activation by @jdx in [#8190](https://github.com/jdx/mise/pull/8190)
- **(lockfile)** resolve version aliases before lockfile lookup by @jdx in [#8194](https://github.com/jdx/mise/pull/8194)
- **(registry)** set helm-diff archive bin name to diff by @jean-humann in [#8173](https://github.com/jdx/mise/pull/8173)
- **(task)** improve source freshness checks with dynamic task dirs by @rooperuu in [#8169](https://github.com/jdx/mise/pull/8169)
- **(task)** resolve global tasks when running from monorepo root by @jdx in [#8192](https://github.com/jdx/mise/pull/8192)
- **(task)** prevent wildcard glob `test:*` from matching parent task `test` by @jdx in [#8165](https://github.com/jdx/mise/pull/8165)
- **(task)** resolve task_config.includes relative to config root by @jdx in [#8193](https://github.com/jdx/mise/pull/8193)
- **(upgrade)** skip untrusted tracked configs during upgrade by @jdx in [#8195](https://github.com/jdx/mise/pull/8195)

### 🚜 Refactor

- use enum for npm.pacakge_manager by @risu729 in [#8180](https://github.com/jdx/mise/pull/8180)

### 📚 Documentation

- **(plugins)** replace node/asdf-nodejs examples with vfox plugins by @jdx in [#8191](https://github.com/jdx/mise/pull/8191)

### ⚡ Performance

- call npm view only once by @risu729 in [#8181](https://github.com/jdx/mise/pull/8181)

### New Contributors

- @jean-humann made their first contribution in [#8173](https://github.com/jdx/mise/pull/8173)
- @mackwic made their first contribution in [#8183](https://github.com/jdx/mise/pull/8183)
- @rooperuu made their first contribution in [#8169](https://github.com/jdx/mise/pull/8169)

### 📦 Aqua Registry Updates

#### New Packages (2)

- [`BetterDiscord/cli`](https://github.com/BetterDiscord/cli)
- [`glossia.ai/cli`](https://github.com/glossia.ai/cli)

## [2026.2.14](https://github.com/jdx/mise/compare/v2026.2.13..v2026.2.14) - 2026-02-16

### 🚀 Features

- **(vfox)** allow plugins to request env var redaction via MiseEnvResult by @jdx in [#8166](https://github.com/jdx/mise/pull/8166)
- add a default_host setting for rust by @aacebedo in [#8154](https://github.com/jdx/mise/pull/8154)
- add github_content package support for aqua backend by @risu729 in [#8147](https://github.com/jdx/mise/pull/8147)
- support devEngines.runtime in deno by @risu729 in [#8144](https://github.com/jdx/mise/pull/8144)

### 🐛 Bug Fixes

- **(asset_matcher)** penalize vsix files by @risu729 in [#8151](https://github.com/jdx/mise/pull/8151)
- **(edit)** strip formatting whitespace from TOML values in `mise edit` by @jdx in [#8162](https://github.com/jdx/mise/pull/8162)
- **(install)** improve --locked support for python and ubi backends by @jdx in [#8163](https://github.com/jdx/mise/pull/8163)
- **(npm)** suppress npm update notifier while npm install by @risu729 in [#8152](https://github.com/jdx/mise/pull/8152)
- **(schema)** add task_templates, extends, and timeout by @risu729 in [#8145](https://github.com/jdx/mise/pull/8145)

### 🚜 Refactor

- **(registry)** remove [key=value] options syntax from backends by @risu729 in [#8146](https://github.com/jdx/mise/pull/8146)

### 📚 Documentation

- **(settings)** remove wrong tip for github_attestations by @risu729 in [#8158](https://github.com/jdx/mise/pull/8158)

### Chore

- **(release-plz)** delete embedded plugins directory before update by @risu729 in [#8149](https://github.com/jdx/mise/pull/8149)
- adds necessary env var to the mcp help message in cli by @joaommartins in [#8133](https://github.com/jdx/mise/pull/8133)

### New Contributors

- @joaommartins made their first contribution in [#8133](https://github.com/jdx/mise/pull/8133)

### 📦 Aqua Registry Updates

#### New Packages (5)

- [`containers/podlet`](https://github.com/containers/podlet)
- [`hickford/git-credential-azure`](https://github.com/hickford/git-credential-azure)
- [`hickford/git-credential-oauth`](https://github.com/hickford/git-credential-oauth)
- [`kovetskiy/mark`](https://github.com/kovetskiy/mark)
- [`openbao/openbao/bao`](https://github.com/openbao/openbao/bao)

## [2026.2.13](https://github.com/jdx/mise/compare/v2026.2.12..v2026.2.13) - 2026-02-15

### 📦️ Dependency Updates

- bump sigstore-verification to 0.2 by @jdx in [e8897c9](https://github.com/jdx/mise/commit/e8897c9fbc873fe272495a65e5a88b04b97f3b6d)

### 📦 Aqua Registry Updates

#### New Packages (1)

- [`k1LoW/tcmux`](https://github.com/k1LoW/tcmux)

#### Updated Packages (1)

- [`jdx/usage`](https://github.com/jdx/usage)

## [2026.2.12](https://github.com/jdx/mise/compare/v2026.2.11..v2026.2.12) - 2026-02-14

### 🚀 Features

- **(java)** add a java.shorthand_vendor setting by @roele in [#8134](https://github.com/jdx/mise/pull/8134)

### 📦 Aqua Registry Updates

#### New Packages (4)

- [`IvanIsCoding/celq`](https://github.com/IvanIsCoding/celq)
- [`postfinance/topf`](https://github.com/postfinance/topf)
- [`runkids/skillshare`](https://github.com/runkids/skillshare)
- [`sandreas/tone`](https://github.com/sandreas/tone)

## [2026.2.11](https://github.com/jdx/mise/compare/v2026.2.10..v2026.2.11) - 2026-02-12

### 🚀 Features

- **(env)** support array access for multiple tool versions in tera templates by @jdx in [#8129](https://github.com/jdx/mise/pull/8129)

### 🐛 Bug Fixes

- **(hook-env)** watch files accessed by tera template functions by @jdx in [#8122](https://github.com/jdx/mise/pull/8122)

### 📦 Registry

- added mutagen by @tony-sol in [#8125](https://github.com/jdx/mise/pull/8125)
- add communique by @jdx in [#8126](https://github.com/jdx/mise/pull/8126)

## [2026.2.10](https://github.com/jdx/mise/compare/v2026.2.9..v2026.2.10) - 2026-02-12

### 🚀 Features

- **(activate)** add shims directory as fallback when auto-install is enabled by @ctaintor in [#8106](https://github.com/jdx/mise/pull/8106)
- **(env)** add `tools` variable to tera template context by @jdx in [#8108](https://github.com/jdx/mise/pull/8108)
- **(set)** add --stdin flag for multiline environment variables by @jdx in [#8110](https://github.com/jdx/mise/pull/8110)

### 🐛 Bug Fixes

- **(backend)** improve conda patchelf and dependency resolution for complex packages by @jdx in [#8087](https://github.com/jdx/mise/pull/8087)
- **(ci)** fix validate-new-tools grep pattern for test field by @jdx in [#8100](https://github.com/jdx/mise/pull/8100)
- **(config)** make MISE_OFFLINE work correctly by gracefully skipping network calls by @jdx in [#8109](https://github.com/jdx/mise/pull/8109)
- **(github)** skip v prefix for "latest" version by @jdx in [#8105](https://github.com/jdx/mise/pull/8105)
- **(gitlab)** resolve tool options from config for aliased tools by @jdx in [#8084](https://github.com/jdx/mise/pull/8084)
- **(install)** use version_expr for Flutter to fix version resolution by @jdx in [#8081](https://github.com/jdx/mise/pull/8081)
- **(registry)** add Linux support for tuist by @fortmarek in [#8102](https://github.com/jdx/mise/pull/8102)
- **(release)** write release notes to file instead of capturing stdout by @jdx in [#8086](https://github.com/jdx/mise/pull/8086)
- **(release)** make release notes editorialization non-blocking by @jdx in [#8116](https://github.com/jdx/mise/pull/8116)
- **(upgrade)** tools are not uninstalled properly due to outdated symlink by @roele in [#8099](https://github.com/jdx/mise/pull/8099)
- **(upgrade)** ensure uninstallation failure does not leave invalid symlinks by @roele in [#8101](https://github.com/jdx/mise/pull/8101)
- SLSA for in-toto statement with no signatures by @gerhard in [#8094](https://github.com/jdx/mise/pull/8094)
- Vfox Plugin Auto-Installation for Environment Directives by @pose in [#8035](https://github.com/jdx/mise/pull/8035)

### 📚 Documentation

- use mise activate for PowerShell in getting-started by @rileychh in [#8112](https://github.com/jdx/mise/pull/8112)

### 📦 Registry

- add conda backend for mysql by @jdx in [#8080](https://github.com/jdx/mise/pull/8080)
- add conda backends for 10 asdf-only tools by @jdx in [#8083](https://github.com/jdx/mise/pull/8083)
- added podman-tui by @tony-sol in [#8098](https://github.com/jdx/mise/pull/8098)

### Chore

- sort settings.toml alphabetically and add test by @jdx in [#8111](https://github.com/jdx/mise/pull/8111)

### New Contributors

- @ctaintor made their first contribution in [#8106](https://github.com/jdx/mise/pull/8106)
- @rileychh made their first contribution in [#8112](https://github.com/jdx/mise/pull/8112)
- @fortmarek made their first contribution in [#8102](https://github.com/jdx/mise/pull/8102)
- @pose made their first contribution in [#8035](https://github.com/jdx/mise/pull/8035)
- @gerhard made their first contribution in [#8094](https://github.com/jdx/mise/pull/8094)

### 📦 Aqua Registry Updates

#### New Packages (2)

- [`entireio/cli`](https://github.com/entireio/cli)
- [`rmitchellscott/reManager`](https://github.com/rmitchellscott/reManager)

#### Updated Packages (1)

- [`atuinsh/atuin`](https://github.com/atuinsh/atuin)

## [2026.2.9](https://github.com/jdx/mise/compare/v2026.2.8..v2026.2.9) - 2026-02-10

### 🚀 Features

- auto-select no-YJIT Ruby on older glibc systems by @jdx in [#8069](https://github.com/jdx/mise/pull/8069)

### 🐛 Bug Fixes

- **(shim)** update mise-shim.exe during self-update on Windows by @jdx in [#8075](https://github.com/jdx/mise/pull/8075)
- Bump xx to 2.5 by @erickt in [#8077](https://github.com/jdx/mise/pull/8077)

### 📚 Documentation

- **(ruby)** remove experimental language for precompiled binaries by @jdx in [#8073](https://github.com/jdx/mise/pull/8073)

### New Contributors

- @erickt made their first contribution in [#8077](https://github.com/jdx/mise/pull/8077)

### 📦 Aqua Registry Updates

#### Updated Packages (1)

- [`carthage-software/mago`](https://github.com/carthage-software/mago)

## [2026.2.8](https://github.com/jdx/mise/compare/v2026.2.7..v2026.2.8) - 2026-02-09

### 🚀 Features

- **(node)** support package.json as idiomatic version file by @jdx in [#8059](https://github.com/jdx/mise/pull/8059)
- **(ruby)** graduate precompiled ruby from experimental (gradual rollout) by @jdx in [#8052](https://github.com/jdx/mise/pull/8052)
- add --dry-run-code flag to exit non-zero when there is work to do by @jdx in [#8063](https://github.com/jdx/mise/pull/8063)

### 🐛 Bug Fixes

- **(core)** respect MISE_ARCH override in bun and erlang plugins by @jdx in [#8062](https://github.com/jdx/mise/pull/8062)
- **(hooks)** resolve 12 community-reported hooks issues by @jdx in [#8058](https://github.com/jdx/mise/pull/8058)
- accept key=value format in set/add subcommands by @jdx in [#8053](https://github.com/jdx/mise/pull/8053)

### 📚 Documentation

- bump action versions in GitHub Actions examples by @muzimuzhi in [#8065](https://github.com/jdx/mise/pull/8065)
- add opengraph meta tags by @jdx in [#8066](https://github.com/jdx/mise/pull/8066)

### 📦️ Dependency Updates

- upgrade toml to 0.9 and toml_edit to 0.24 (TOML 1.1) by @jdx in [#8057](https://github.com/jdx/mise/pull/8057)

### 📦 Registry

- add quicktype (npm:quicktype) by @zdunecki in [#8054](https://github.com/jdx/mise/pull/8054)
- use inline table for test definitions by @jdx in [#8056](https://github.com/jdx/mise/pull/8056)

## [2026.2.7](https://github.com/jdx/mise/compare/v2026.2.6..v2026.2.7) - 2026-02-08

### 🚀 Features

- **(shim)** add native .exe shim mode for Windows by @jdx in [#8045](https://github.com/jdx/mise/pull/8045)

### 🐛 Bug Fixes

- **(install)** preserve config options and registry defaults by @jdx in [#8044](https://github.com/jdx/mise/pull/8044)
- **(link)** linked versions override lockfile during resolution by @jdx in [#8050](https://github.com/jdx/mise/pull/8050)
- **(release)** preserve aqua-registry sections in changelog across releases by @jdx in [#8047](https://github.com/jdx/mise/pull/8047)
- ls --all-sources shows duplicate entries by @roele in [#8042](https://github.com/jdx/mise/pull/8042)

### 📚 Documentation

- replace "inherit" terminology with config layering by @jdx in [#8046](https://github.com/jdx/mise/pull/8046)

### 📦 Registry

- switch oxlint to npm backend by default by @risu729 in [#8038](https://github.com/jdx/mise/pull/8038)
- add orval (npm:orval) by @zdunecki in [#8051](https://github.com/jdx/mise/pull/8051)

### New Contributors

- @zdunecki made their first contribution in [#8051](https://github.com/jdx/mise/pull/8051)

## [2026.2.6](https://github.com/jdx/mise/compare/v2026.2.5..v2026.2.6) - 2026-02-07

### 🚀 Features

- **(env)** add shell-style variable expansion in env values by @jdx in [#8029](https://github.com/jdx/mise/pull/8029)
- **(list)** add --all-sources flag to list command by @TylerHillery in [#8019](https://github.com/jdx/mise/pull/8019)

### 🐛 Bug Fixes

- **(gem)** Windows support for gem backend by @my1e5 in [#8031](https://github.com/jdx/mise/pull/8031)
- **(gem)** revert gem.rs script newline change by @my1e5 in [#8034](https://github.com/jdx/mise/pull/8034)
- **(lock)** write tools to lockfile matching their source config by @jdx in [#8012](https://github.com/jdx/mise/pull/8012)
- **(ls)** sort sources deterministically in --all-sources output by @jdx in [#8037](https://github.com/jdx/mise/pull/8037)
- **(task)** auto-install tools from mise.toml for file tasks by @jdx in [#8030](https://github.com/jdx/mise/pull/8030)

### 📚 Documentation

- fix wrong positions of `mise run` flags by @muzimuzhi in [#8036](https://github.com/jdx/mise/pull/8036)

### 📦️ Dependency Updates

- update ghcr.io/jdx/mise:copr docker digest to 3e00d7d by @renovate[bot] in [#8023](https://github.com/jdx/mise/pull/8023)
- update ghcr.io/jdx/mise:alpine docker digest to 0ced1b3 by @renovate[bot] in [#8022](https://github.com/jdx/mise/pull/8022)

### 📦 Registry

- add tirith ([github:sheeki03/tirith](https://github.com/sheeki03/tirith)) by @sheeki03 in [#8024](https://github.com/jdx/mise/pull/8024)
- add mas by @TyceHerrman in [#8032](https://github.com/jdx/mise/pull/8032)

### Security

- **(deps)** update time crate to 0.3.47 to fix RUSTSEC-2026-0009 by @jdx in [#8026](https://github.com/jdx/mise/pull/8026)

### New Contributors

- @sheeki03 made their first contribution in [#8024](https://github.com/jdx/mise/pull/8024)
- @TylerHillery made their first contribution in [#8019](https://github.com/jdx/mise/pull/8019)

### 📦 Aqua Registry Updates

#### New Packages (1)

- [`kubernetes-sigs/kubectl-validate`](https://github.com/kubernetes-sigs/kubectl-validate)

#### Updated Packages (6)

- [`flux-iac/tofu-controller/tfctl`](https://github.com/flux-iac/tofu-controller/tfctl)
- [`gogs/gogs`](https://github.com/gogs/gogs)
- [`j178/prek`](https://github.com/j178/prek)
- [`syncthing/syncthing`](https://github.com/syncthing/syncthing)
- [`tuist/tuist`](https://github.com/tuist/tuist)
- [`yaml/yamlscript`](https://github.com/yaml/yamlscript)
## [2026.2.5](https://github.com/jdx/mise/compare/v2026.2.4..v2026.2.5) - 2026-02-06

### 🐛 Bug Fixes

- **(lock)** remove experimental flag requirement for lockfiles by @jdx in [#8011](https://github.com/jdx/mise/pull/8011)

### Chore

- add tone calibration to release notes prompt by @jdx in [#8010](https://github.com/jdx/mise/pull/8010)

## [2026.2.4](https://github.com/jdx/mise/compare/v2026.2.3..v2026.2.4) - 2026-02-05

### 🐛 Bug Fixes

- **(env)** resolve sourced env for tool templates by @corymhall in [#7895](https://github.com/jdx/mise/pull/7895)
- **(npm)** only declare the configured package manager as a dependency by @jdx in [#7995](https://github.com/jdx/mise/pull/7995)
- **(upgrade)** respect use_locked_version when checking tracked configs by @jdx in [#7997](https://github.com/jdx/mise/pull/7997)
- ignore MISE_TOOL_VERSION in env var parsing by @jdx in [#8004](https://github.com/jdx/mise/pull/8004)

### New Contributors

- @corymhall made their first contribution in [#7895](https://github.com/jdx/mise/pull/7895)

## [2026.2.3](https://github.com/jdx/mise/compare/v2026.2.2..v2026.2.3) - 2026-02-04

### 🐛 Bug Fixes

- **(install)** allow pipx/npm/cargo/asdf backends to work in locked mode by @jdx in [#7985](https://github.com/jdx/mise/pull/7985)

### 📦️ Dependency Updates

- update bytes to 1.11.1 to fix RUSTSEC-2026-0007 by @jdx in [#7986](https://github.com/jdx/mise/pull/7986)

### 📦 Registry

- add mermaid-ascii by @TyceHerrman in [#7984](https://github.com/jdx/mise/pull/7984)
- add godot ([aqua:godotengine/godot](https://github.com/godotengine/godot)) by @dmarcoux in [#7989](https://github.com/jdx/mise/pull/7989)
- add julia (http:julia) by @quinnj in [#7990](https://github.com/jdx/mise/pull/7990)

### New Contributors

- @quinnj made their first contribution in [#7990](https://github.com/jdx/mise/pull/7990)
- @dmarcoux made their first contribution in [#7989](https://github.com/jdx/mise/pull/7989)

## [2026.2.2](https://github.com/jdx/mise/compare/v2026.2.1..v2026.2.2) - 2026-02-03

### 🚀 Features

- **(asset-matcher)** enable `mingw-w64` detection for windows packages by @lchagnoleau in [#7981](https://github.com/jdx/mise/pull/7981)
- **(crates/vfox)** add download_path to BackendInstall context by @malept in [#7959](https://github.com/jdx/mise/pull/7959)
- **(python)** rework `python.uv_venv_auto` setting by @halms in [#7905](https://github.com/jdx/mise/pull/7905)
- add "Did you mean?" suggestions and inactive tool warnings by @jdx in [#7965](https://github.com/jdx/mise/pull/7965)

### 🐛 Bug Fixes

- **(hook-env)** skip remote version fetching for uninstalled tools in prefer-offline mode by @jdx in [#7976](https://github.com/jdx/mise/pull/7976)
- **(install.sh)** Corret `setup` to `set up` by @gogolok in [#7980](https://github.com/jdx/mise/pull/7980)
- retry spawn on ETXTBSY (Text file busy) by @jdx in [#7964](https://github.com/jdx/mise/pull/7964)
- improve ToolOptions parsing to support comma separated values by @roele in [#7971](https://github.com/jdx/mise/pull/7971)

### 📚 Documentation

- improve plugin documentation with comparisons and template links by @jdx in [#7962](https://github.com/jdx/mise/pull/7962)

### 📦️ Dependency Updates

- bump hyper-util, system-configuration, lru, aws-sdk, and others by @jdx in [#7977](https://github.com/jdx/mise/pull/7977)

### Chore

- **(vfox)** add LuaCATS type definitions for plugin IDE support by @jdx in [#7961](https://github.com/jdx/mise/pull/7961)
- **(vfox)** add `download_path` to `BackendInstallCtx` type defintion by @malept in [#7973](https://github.com/jdx/mise/pull/7973)
- add stylua linting for vfox plugin Lua files by @jdx in [#7960](https://github.com/jdx/mise/pull/7960)
- use system Rust for PPA builds on Ubuntu 26.04+ by @jdx in [#7956](https://github.com/jdx/mise/pull/7956)

### New Contributors

- @gogolok made their first contribution in [#7980](https://github.com/jdx/mise/pull/7980)

## [2026.2.1](https://github.com/jdx/mise/compare/v2026.2.0..v2026.2.1) - 2026-02-02

### 🚀 Features

- **(generate)** implement --index flag and use task names for task-docs --multi by @jdx in [#7944](https://github.com/jdx/mise/pull/7944)
- **(plugins)** warn when env plugin shadows a registry tool by @jdx in [#7953](https://github.com/jdx/mise/pull/7953)
- **(tool-stub)** add --lock flag to generate tool-stub by @jdx in [#7948](https://github.com/jdx/mise/pull/7948)
- **(vfox)** add log module for Lua plugins by @jdx in [#7949](https://github.com/jdx/mise/pull/7949)
- **(vfox)** switch Lua runtime from Lua 5.1 to Luau by @jdx in [#7954](https://github.com/jdx/mise/pull/7954)

### 🐛 Bug Fixes

- **(build)** upgrade cross images to :main for C++17 support by @jdx in [#7958](https://github.com/jdx/mise/pull/7958)
- **(build)** update glibc check to match new cross image baseline by @jdx in [fc1247e](https://github.com/jdx/mise/commit/fc1247e84b91957e4d6e6841be3af7a95f242625)
- **(registry)** handle file:// URLs in normalize_remote by @jdx in [#7947](https://github.com/jdx/mise/pull/7947)
- **(vfox)** fix LuaLS warnings in test fixtures and add linting by @jdx in [#7946](https://github.com/jdx/mise/pull/7946)

### 🚜 Refactor

- unify deprecated_at! macro with warn and remove versions by @jdx in [#7957](https://github.com/jdx/mise/pull/7957)

### 🧪 Testing

- remove unnecessary end-of-test cleanup from e2e tests by @jdx in [#7950](https://github.com/jdx/mise/pull/7950)

### ◀️ Revert

- Revert "fix(build): update glibc check to match new cross image baseline" by @jdx in [0774bf9](https://github.com/jdx/mise/commit/0774bf99d4a2ab2a30553a7db09f79223cdc5aa6)
- Revert "fix(build): upgrade cross images to :main for C++17 support " by @jdx in [8dcca08](https://github.com/jdx/mise/commit/8dcca086e87c1b29343e2842c6c68ec949dd60f4)
- Revert "feat(vfox): switch Lua runtime from Lua 5.1 to Luau " by @jdx in [8b4322d](https://github.com/jdx/mise/commit/8b4322d693702890e268d9c1e9309536ffdbd8fc)

## [2026.2.0](https://github.com/jdx/mise/compare/v2026.1.12..v2026.2.0) - 2026-02-01

### 🚀 Features

- **(edit)** add interactive config editor (`mise edit`) by @jdx in [#7930](https://github.com/jdx/mise/pull/7930)
- **(lockfile)** graduate lockfiles from experimental by @jdx in [#7929](https://github.com/jdx/mise/pull/7929)
- **(task)** add support for usage values in task confirm dialog by @roele in [#7924](https://github.com/jdx/mise/pull/7924)
- **(task)** improve source freshness checking with edge case handling by @jdx in [#7932](https://github.com/jdx/mise/pull/7932)

### 🐛 Bug Fixes

- **(activate)** preserve ordering of paths appended after mise activate by @jdx in [#7919](https://github.com/jdx/mise/pull/7919)
- **(install)** sort failed installations for deterministic error output by @jdx in [#7936](https://github.com/jdx/mise/pull/7936)
- **(lockfile)** preserve URL and prefer sha256 when merging platform info by @jdx in [#7923](https://github.com/jdx/mise/pull/7923)
- **(lockfile)** add atomic writes and cache invalidation by @jdx in [#7927](https://github.com/jdx/mise/pull/7927)
- **(release)** add mise-interactive-config to release-plz publish workflow by @jdx in [#7940](https://github.com/jdx/mise/pull/7940)
- **(release)** handle mise-interactive-config schema during cargo publish by @jdx in [#7942](https://github.com/jdx/mise/pull/7942)
- **(release)** include mise.json in mise-interactive-config package by @jdx in [3689a4a](https://github.com/jdx/mise/commit/3689a4a83c7e886ba15082f99674ebc6398056e3)
- **(task)** discover and run shebang file tasks on Windows by @jdx in [#7941](https://github.com/jdx/mise/pull/7941)
- **(templates)** use sha256 for hash filter instead of blake3 by @jdx in [#7925](https://github.com/jdx/mise/pull/7925)
- **(upgrade)** respect tracked configs when pruning old versions by @jdx in [#7926](https://github.com/jdx/mise/pull/7926)

### 🚜 Refactor

- **(progress)** migrate from indicatif to clx by @jdx in [#7928](https://github.com/jdx/mise/pull/7928)

### 📚 Documentation

- improve clarity on uvx and pipx dependencies by @ygormutti in [#7878](https://github.com/jdx/mise/pull/7878)

### ⚡ Performance

- **(install)** use Kahn's algorithm for dependency scheduling by @jdx in [#7933](https://github.com/jdx/mise/pull/7933)
- use Aho-Corasick for efficient redaction by @jdx in [#7931](https://github.com/jdx/mise/pull/7931)

### 🧪 Testing

- remove flaky test_http_version_list test by @jdx in [#7934](https://github.com/jdx/mise/pull/7934)

### Chore

- use github backend instead of ubi in mise.lock by @jdx in [#7922](https://github.com/jdx/mise/pull/7922)

### New Contributors

- @ygormutti made their first contribution in [#7878](https://github.com/jdx/mise/pull/7878)

## [2026.1.12](https://github.com/jdx/mise/compare/v2026.1.11..v2026.1.12) - 2026-01-31

### 🐛 Bug Fixes

- **(task)** resolve monorepo task includes relative to config file directory by @jdx in [#7917](https://github.com/jdx/mise/pull/7917)
- disable autocrlf on git clone to fix WSL issues by @jdx in [#7916](https://github.com/jdx/mise/pull/7916)

### 📚 Documentation

- **(tasks)** add bash array pattern for variadic args by @jdx in [#7914](https://github.com/jdx/mise/pull/7914)

## [2026.1.11](https://github.com/jdx/mise/compare/v2026.1.10..v2026.1.11) - 2026-01-30

### 🚀 Features

- **(config)** load local .config/miserc.toml too by @scop in [#7896](https://github.com/jdx/mise/pull/7896)
- **(vfox)** pass constructed env to module hooks for cmd.exec by @jdx in [#7908](https://github.com/jdx/mise/pull/7908)

### 🐛 Bug Fixes

- **(cache)** resolve correct cache path when clearing GitHub backend tools by @jdx in [#7907](https://github.com/jdx/mise/pull/7907)
- **(ci)** handle aqua subpath backends in grace period check by @jdx in [3dcf20c](https://github.com/jdx/mise/commit/3dcf20cd3054e5038e7e81a736dfe9187656eead)
- **(github)** support macOS .app bundle binary discovery by @jdx in [#7885](https://github.com/jdx/mise/pull/7885)
- **(prepare)** scope prepare providers to their defining config file by @jdx in [#7889](https://github.com/jdx/mise/pull/7889)
- **(registry)** update daytona asset pattern for new release naming by @jdx in [#7897](https://github.com/jdx/mise/pull/7897)
- **(registry)** revert daytona asset pattern to simple naming by @jdx in [#7911](https://github.com/jdx/mise/pull/7911)
- **(run)** show task info instead of executing when --help used without usage spec by @jdx in [#7893](https://github.com/jdx/mise/pull/7893)
- **(task)** fix wait_for with env overrides and re-render outputs by @jdx in [#7888](https://github.com/jdx/mise/pull/7888)
- use correct MISE comment format and remove invalid usage field by @jdx in [89d87ba](https://github.com/jdx/mise/commit/89d87ba1331a60ffa5c1f55ee8da96e6be59c59c)
- add #USAGE arg spec for check-release-failures task by @jdx in [8bfab84](https://github.com/jdx/mise/commit/8bfab845d310ec26337f24f154e2b63fc12cdf86)
- use mise tool --json instead of regex parsing registry files by @jdx in [22d1513](https://github.com/jdx/mise/commit/22d1513eb7a3e97f1f806960dafa3a06732eafbb)

### 🚜 Refactor

- consolidate retry + grace period logic into mise task by @jdx in [794efb4](https://github.com/jdx/mise/commit/794efb40ff0b8c5be1bdbb78146b7358b255fac7)

### 📚 Documentation

- **(gitlab)** explain MISE_GITLAB_TOKEN for private repo by @lchagnoleau in [#7902](https://github.com/jdx/mise/pull/7902)

### ⚡ Performance

- **(exec)** reduce startup overhead for `mise x` by @jdx in [#7890](https://github.com/jdx/mise/pull/7890)
- **(install)** replace per-tool .mise.backend with consolidated manifest by @jdx in [#7892](https://github.com/jdx/mise/pull/7892)

### 📦️ Dependency Updates

- update docker/login-action digest to c94ce9f by @renovate[bot] in [#7899](https://github.com/jdx/mise/pull/7899)
- update alpine:edge docker digest to 9a341ff by @renovate[bot] in [#7898](https://github.com/jdx/mise/pull/7898)
- update ghcr.io/jdx/mise:alpine docker digest to 3a67fe5 by @renovate[bot] in [#7900](https://github.com/jdx/mise/pull/7900)
- update ghcr.io/jdx/mise:copr docker digest to 4dc8174 by @renovate[bot] in [#7901](https://github.com/jdx/mise/pull/7901)
- update ghcr.io/jdx/mise:deb docker digest to af0c0b5 by @renovate[bot] in [#7903](https://github.com/jdx/mise/pull/7903)
- update ghcr.io/jdx/mise:rpm docker digest to a53bf55 by @renovate[bot] in [#7904](https://github.com/jdx/mise/pull/7904)

### Chore

- **(ci)** only auto-merge release PRs with substantive changes by @jdx in [#7884](https://github.com/jdx/mise/pull/7884)
- **(ci)** ignore test-tool failures from recent upstream releases on release branch by @jdx in [9ca4e8d](https://github.com/jdx/mise/commit/9ca4e8d273da94a8ef1fcdaa3ba4489bd8561a37)
- **(registry)** disable buck2 test due to checksum race condition by @jdx in [a658eaa](https://github.com/jdx/mise/commit/a658eaa6d5ce56040c48dcb4a3f34b6abb15cca9)

### New Contributors

- @autofix-ci[bot] made their first contribution
- @lchagnoleau made their first contribution in [#7902](https://github.com/jdx/mise/pull/7902)

## [2026.1.9](https://github.com/jdx/mise/compare/v2026.1.8..v2026.1.9) - 2026-01-28

### 🚀 Features

- **(doctor)** add backend mismatch warnings by @jdx in [#7847](https://github.com/jdx/mise/pull/7847)
- **(http)** add rename_exe support for archive extraction by @jdx in [#7874](https://github.com/jdx/mise/pull/7874)
- **(http)** send x-mise-ci header for CI environment tracking by @jdx in [#7875](https://github.com/jdx/mise/pull/7875)
- **(install)** auto-install plugins from [plugins] config section by @jdx in [#7856](https://github.com/jdx/mise/pull/7856)
- **(registry)** add vercel by @mikecurtis in [#7844](https://github.com/jdx/mise/pull/7844)
- **(task)** support glob patterns in task_config.includes by @jdx in [#7870](https://github.com/jdx/mise/pull/7870)
- **(task)** add task templates for reusable task definitions by @jdx in [#7873](https://github.com/jdx/mise/pull/7873)

### 🐛 Bug Fixes

- **(backend)** change registry mismatch log from info to debug by @jdx in [#7858](https://github.com/jdx/mise/pull/7858)
- **(ci)** use squash merge for auto-merge-release workflow by @jdx in [7e5e71e](https://github.com/jdx/mise/commit/7e5e71e3b065518f08c2fb187789c13b594c0c9d)
- **(ci)** remove --auto flag to merge immediately when CI passes by @jdx in [23ed2ed](https://github.com/jdx/mise/commit/23ed2edf03d7f28640b0516e1f02d0f240626d4e)
- **(github)** select platform-matching provenance file for SLSA verification by @jdx in [#7853](https://github.com/jdx/mise/pull/7853)
- **(go)** filter out version "1" from available versions by @jdx in [#7871](https://github.com/jdx/mise/pull/7871)
- **(install)** skip CurDir components when detecting archive structure by @jdx in [#7868](https://github.com/jdx/mise/pull/7868)
- **(pipx)** ensure Python minor version symlink exists for postinstall hooks by @jdx in [#7869](https://github.com/jdx/mise/pull/7869)
- **(registry)** prevent duplicate -stable suffix in Flutter download URLs by @jdx in [#7872](https://github.com/jdx/mise/pull/7872)
- **(task)** pass env to usage parser for env-backed arguments by @jdx in [#7848](https://github.com/jdx/mise/pull/7848)
- **(task)** propagate MISE_ENV to child tasks when using -E flag by @jdx in [06ee776](https://github.com/jdx/mise/commit/06ee77643fcebfe03b37e6a235cf4f1c258a4e34)
- **(vfox-dotnet)** use os.execute() to fix Windows installation by @prodrigues1912 in [#7843](https://github.com/jdx/mise/pull/7843)

### 📚 Documentation

- update cache-behavior with env_cache information by @jdx in [#7849](https://github.com/jdx/mise/pull/7849)

### ◀️ Revert

- remove task inheritance from parent configs in monorepos by @jdx in [#7851](https://github.com/jdx/mise/pull/7851)
- Revert "fix(ci): remove --auto flag to merge immediately when CI passes" by @jdx in [0606187](https://github.com/jdx/mise/commit/06061878d2abfd5194425f11f7a47237cd5039e3)

### 📦 Registry

- add mago ([aqua:carthage-software/mago](https://github.com/carthage-software/mago)) by @scop in [#7845](https://github.com/jdx/mise/pull/7845)

### Chore

- **(ci)** auto-merge release branch into main daily at 4am CST by @jdx in [#7852](https://github.com/jdx/mise/pull/7852)

### New Contributors

- @mikecurtis made their first contribution in [#7844](https://github.com/jdx/mise/pull/7844)
- @prodrigues1912 made their first contribution in [#7843](https://github.com/jdx/mise/pull/7843)

## [2026.1.8](https://github.com/jdx/mise/compare/v2026.1.7..v2026.1.8) - 2026-01-27

### 🐛 Bug Fixes

- **(aqua)** invalidate lockfile when asset doesn't match registry by @jdx in [#7830](https://github.com/jdx/mise/pull/7830)
- **(aqua)** add warnings when version tag lookup fails by @jdx in [#7831](https://github.com/jdx/mise/pull/7831)
- **(github)** penalize Windows-specific extensions on non-Windows platforms by @jdx in [#7838](https://github.com/jdx/mise/pull/7838)
- **(task)** resolve monorepo task env vars in usage spec by @jdx in [#7832](https://github.com/jdx/mise/pull/7832)
- **(task)** support dotted keys and deep-merge in file task headers by @jdx in [#7840](https://github.com/jdx/mise/pull/7840)
- don't thank @jdx in LLM-generated release notes by @jdx in [#7835](https://github.com/jdx/mise/pull/7835)
- ensure that idiomatic and toolversions show in ls --local by @offbyone in [#7836](https://github.com/jdx/mise/pull/7836)

### 🚜 Refactor

- **(registry)** split registry.toml into one file per tool by @jdx in [#7820](https://github.com/jdx/mise/pull/7820)

### 📚 Documentation

- improve conventional commit guidance in CLAUDE.md by @jdx in [cbf2f74](https://github.com/jdx/mise/commit/cbf2f7472a8aea858fc8008a30aedfd10f5f6382)

### 📦️ Dependency Updates

- lock file maintenance by @renovate[bot] in [#7826](https://github.com/jdx/mise/pull/7826)
- lock file maintenance by @renovate[bot] in [#7827](https://github.com/jdx/mise/pull/7827)

### Chore

- **(ci)** add CI failure feedback to pr-closer workflow by @jdx in [#7821](https://github.com/jdx/mise/pull/7821)
- **(ci)** add FORGEJO_TOKEN for Codeberg API authentication by @jdx in [#7841](https://github.com/jdx/mise/pull/7841)

### Registry

- **(claude)** add aqua backend as default by @jdx in [#7842](https://github.com/jdx/mise/pull/7842)

## [2026.1.7](https://github.com/jdx/mise/compare/v2026.1.6..v2026.1.7) - 2026-01-25

### 🐛 Bug Fixes

- **(backend)** resolve registry mismatch for previously installed tools by @smorimoto in [#7773](https://github.com/jdx/mise/pull/7773)
- **(env_cache)** use cached watch_files to avoid plugin re-execution by @jdx in [#7817](https://github.com/jdx/mise/pull/7817)
- **(github)** handle projectname@version tag format by @jdx in [#7788](https://github.com/jdx/mise/pull/7788)
- **(http)** add fromJSON/keys to version_expr for HashiCorp tools by @jdx in [#7816](https://github.com/jdx/mise/pull/7816)

### 📚 Documentation

- **(contributing)** correct ripgrep command by @nguyenvulong in [#7805](https://github.com/jdx/mise/pull/7805)
- **(contributing)** update hk usages by @muzimuzhi in [#7797](https://github.com/jdx/mise/pull/7797)

### 📦 Registry

- add claude-powerline by @TyceHerrman in [#7798](https://github.com/jdx/mise/pull/7798)
- add rpk by @artemklevtsov in [#7802](https://github.com/jdx/mise/pull/7802)

### New Contributors

- @smorimoto made their first contribution in [#7773](https://github.com/jdx/mise/pull/7773)
- @nguyenvulong made their first contribution in [#7805](https://github.com/jdx/mise/pull/7805)

## [2026.1.6](https://github.com/jdx/mise/compare/v2026.1.5..v2026.1.6) - 2026-01-21

### 🚀 Features

- **(config)** add miette diagnostics for TOML parsing errors by @jdx in [#7764](https://github.com/jdx/mise/pull/7764)
- **(env)** add environment caching with module cacheability support by @jdx in [#7761](https://github.com/jdx/mise/pull/7761)

### 🐛 Bug Fixes

- **(prepare)** handle freshness check for auto-created venvs by @jdx in [#7770](https://github.com/jdx/mise/pull/7770)
- **(release)** use colon separator in release titles by @jdx in [#7765](https://github.com/jdx/mise/pull/7765)
- **(release)** drop Fedora 41 from COPR build (EOL) by @TobiX in [#7771](https://github.com/jdx/mise/pull/7771)
- **(release)** bump version until unused when publishing subcrates by @jdx in [#7787](https://github.com/jdx/mise/pull/7787)
- **(tasks)** include task tools in env resolution cache check by @jdx in [#7786](https://github.com/jdx/mise/pull/7786)
- rust lockfile by @vadimpiven in [#7780](https://github.com/jdx/mise/pull/7780)
- Ensure tool stubs have dependency toolset paths as well by @thejcannon in [#7777](https://github.com/jdx/mise/pull/7777)

### 🚜 Refactor

- improve filetask field parsing tests and parser by @makp0 in [#7751](https://github.com/jdx/mise/pull/7751)

### 📚 Documentation

- improve CLAUDE.md with additional development guidance by @jdx in [#7763](https://github.com/jdx/mise/pull/7763)
- drop architecture from Debian sources.list by @TobiX in [#7772](https://github.com/jdx/mise/pull/7772)

### 📦 Registry

- use aqua for zprint by @scop in [#7767](https://github.com/jdx/mise/pull/7767)
- add miller ([aqua:johnkerl/miller](https://github.com/johnkerl/miller)) by @kit494way in [#7782](https://github.com/jdx/mise/pull/7782)
- add atlas-community ([aqua:ariga/atlas/community](https://github.com/ariga/atlas/community)) by @akanter in [#7784](https://github.com/jdx/mise/pull/7784)

### Security

- remove insecure registry-comment workflow by @jdx in [#7769](https://github.com/jdx/mise/pull/7769)

## [2026.1.5](https://github.com/jdx/mise/compare/v2026.1.4..v2026.1.5) - 2026-01-19

### 🚀 Features

- **(complete)** add PowerShell completion support by @jdx in [#7746](https://github.com/jdx/mise/pull/7746)
- **(release)** add LLM-generated prose summary to release notes by @jdx in [#7737](https://github.com/jdx/mise/pull/7737)
- **(vfox)** add semver Lua module for version sorting by @jdx in [#7739](https://github.com/jdx/mise/pull/7739)
- **(vfox)** add rolling release support with checksum tracking by @jdx in [#7757](https://github.com/jdx/mise/pull/7757)
- dry filetask parsing and validation by @makp0 in [#7738](https://github.com/jdx/mise/pull/7738)

### 🐛 Bug Fixes

- **(completions)** bump usage-cli to 2.13.1 for PowerShell support by @jdx in [#7756](https://github.com/jdx/mise/pull/7756)
- schema missing env required string variant by @vadimpiven in [#7734](https://github.com/jdx/mise/pull/7734)
- validate unknown fields in filetask headers by @makp0 in [#7733](https://github.com/jdx/mise/pull/7733)
- disable schemacrawler test by @jdx in [#7743](https://github.com/jdx/mise/pull/7743)
- replace double forward slash with single slash in get_task_lists by @collinstevens in [#7744](https://github.com/jdx/mise/pull/7744)
- require LLM for release notes and include aqua section by @jdx in [#7745](https://github.com/jdx/mise/pull/7745)
- preserve {{ version }} in tool options during config load by @jdx in [#7755](https://github.com/jdx/mise/pull/7755)

### 📚 Documentation

- add documentation URL structure guidance to CLAUDE.md by @jdx in [#7740](https://github.com/jdx/mise/pull/7740)
- add pitchfork promotion by @jdx in [#7747](https://github.com/jdx/mise/pull/7747)

### 📦️ Dependency Updates

- relax version constraints and update dependencies by @jdx in [#7736](https://github.com/jdx/mise/pull/7736)
- lock file maintenance by @renovate[bot] in [#7749](https://github.com/jdx/mise/pull/7749)

### Chore

- bump xx to 2.3.1 by @jdx in [#7753](https://github.com/jdx/mise/pull/7753)

### New Contributors

- @collinstevens made their first contribution in [#7744](https://github.com/jdx/mise/pull/7744)
- @makp0 made their first contribution in [#7738](https://github.com/jdx/mise/pull/7738)
- @vadimpiven made their first contribution in [#7734](https://github.com/jdx/mise/pull/7734)

## [2026.1.4](https://github.com/jdx/mise/compare/v2026.1.3..v2026.1.4) - 2026-01-17

### 🚀 Features

- **(conda)** add dependency locking for reproducible installations by @jdx in [#7708](https://github.com/jdx/mise/pull/7708)
- **(http)** add JSON filter syntax for version extraction by @jdx in [#7707](https://github.com/jdx/mise/pull/7707)
- **(http)** add version_expr support and Tera templating by @jdx in [#7723](https://github.com/jdx/mise/pull/7723)
- **(task)** add [monorepo].config_roots for explicit config root listing by @jdx in [#7705](https://github.com/jdx/mise/pull/7705)
- **(task)** support env vars in task dependencies by @jdx in [#7724](https://github.com/jdx/mise/pull/7724)

### 🐛 Bug Fixes

- **(conda)** fix hardcoded library paths in conda packages by @jdx in [#7713](https://github.com/jdx/mise/pull/7713)
- **(env)** avoid venv/go backend deadlock during env resolution by @stk0vrfl0w in [#7696](https://github.com/jdx/mise/pull/7696)
- **(locked)** exempt tool stubs from lockfile requirements by @jdx in [#7729](https://github.com/jdx/mise/pull/7729)
- **(python)** sort CPython versions at end of ls-remote output by @jdx in [#7721](https://github.com/jdx/mise/pull/7721)
- **(task)** resolve remote task files before display and validation commands by @yannrouillard in [#7681](https://github.com/jdx/mise/pull/7681)
- **(task)** support monorepo paths in `mise tasks deps` by @chadxz in [#7699](https://github.com/jdx/mise/pull/7699)
- **(task)** resolve all monorepo path hints in deps by @chadxz in [#7698](https://github.com/jdx/mise/pull/7698)

### 📚 Documentation

- remove outdated roadmap page by @jdx in [#7726](https://github.com/jdx/mise/pull/7726)

### ⚡ Performance

- **(task)** fix task-ls cached performance regression by @jdx in [#7716](https://github.com/jdx/mise/pull/7716)

### 📦️ Dependency Updates

- replace dependency @tsconfig/node22 with @tsconfig/node24 by @renovate[bot] in [#7618](https://github.com/jdx/mise/pull/7618)

### 📦 Registry

- add aqua backend for smithy by @jdx in [#7661](https://github.com/jdx/mise/pull/7661)
- remove low-usage asdf plugins by @jdx in [#7701](https://github.com/jdx/mise/pull/7701)
- disable mirrord test by @jdx in [#7703](https://github.com/jdx/mise/pull/7703)
- use vfox-dotnet as default backend by @jdx in [#7704](https://github.com/jdx/mise/pull/7704)
- use vfox-lua as default lua backend by @jdx in [#7706](https://github.com/jdx/mise/pull/7706)
- add vfox backend for redis by @jdx in [#7709](https://github.com/jdx/mise/pull/7709)
- use vfox-postgres as default postgres backend by @jdx in [#7710](https://github.com/jdx/mise/pull/7710)
- use github backend for kotlin by @jdx in [#7711](https://github.com/jdx/mise/pull/7711)
- add vfox backend for leiningen by @jdx in [#7714](https://github.com/jdx/mise/pull/7714)
- use pipx backend for meson by @jdx in [#7712](https://github.com/jdx/mise/pull/7712)
- use github backend for crystal by @jdx in [#7715](https://github.com/jdx/mise/pull/7715)
- use conda backend for sqlite by @jdx in [#7718](https://github.com/jdx/mise/pull/7718)
- use conda backend for make by @jdx in [#7719](https://github.com/jdx/mise/pull/7719)
- swift-package-list use github backend by @jdx in [#7720](https://github.com/jdx/mise/pull/7720)

### Chore

- increase macos release build timeout to 90 minutes by @jdx in [#7725](https://github.com/jdx/mise/pull/7725)

### New Contributors

- @yannrouillard made their first contribution in [#7681](https://github.com/jdx/mise/pull/7681)
- @stk0vrfl0w made their first contribution in [#7696](https://github.com/jdx/mise/pull/7696)

## [2026.1.3](https://github.com/jdx/mise/compare/v2026.1.2..v2026.1.3) - 2026-01-16

### 🚀 Features

- **(s3)** add S3 backend for private artifact storage by @jdx in [#7668](https://github.com/jdx/mise/pull/7668)
- **(upgrade)** use installed_tool completer for mise upgrade by @jdx in [#7670](https://github.com/jdx/mise/pull/7670)
- **(upgrade)** add --exclude flag to mise upgrade command by @jdx in [#7669](https://github.com/jdx/mise/pull/7669)
- add no hooks and no env flags by @aacebedo in [#7560](https://github.com/jdx/mise/pull/7560)

### 🐛 Bug Fixes

- **(backend)** allow upgrading vfox backend tools with symlinked installations by @TyceHerrman in [#7012](https://github.com/jdx/mise/pull/7012)
- **(backend)** reject architecture mismatches in asset selection by @jdx in [#7672](https://github.com/jdx/mise/pull/7672)
- **(backend)** canonicalize symlink target before installs check by @jdx in [#7671](https://github.com/jdx/mise/pull/7671)
- **(npm)** avoid circular dependency when npm is in dependencies by @AprilNEA in [#7644](https://github.com/jdx/mise/pull/7644)
- **(self-update)** skip update when already at latest version by @jdx in [#7666](https://github.com/jdx/mise/pull/7666)
- fall back to GITHUB_TOKEN for github.com by @subdigital in [#7667](https://github.com/jdx/mise/pull/7667)
- GitHub token fallback by @subdigital in [#7673](https://github.com/jdx/mise/pull/7673)
- inherit tasks from parent configs in monorepos by @chadxz in [#7643](https://github.com/jdx/mise/pull/7643)

### 📚 Documentation

- **(contributing)** update registry examples by @scop in [#7660](https://github.com/jdx/mise/pull/7660)
- **(contributing)** update registry PR title rule by @scop in [#7663](https://github.com/jdx/mise/pull/7663)
- remove 404 link from contributing by @opswole in [#7692](https://github.com/jdx/mise/pull/7692)
- clarify that backend plugins should sort the version list by @ofalvai in [#7680](https://github.com/jdx/mise/pull/7680)

### 📦️ Dependency Updates

- update ghcr.io/jdx/mise:alpine docker digest to 11f659e by @renovate[bot] in [#7685](https://github.com/jdx/mise/pull/7685)
- update ghcr.io/jdx/mise:copr docker digest to 3adaea4 by @renovate[bot] in [#7686](https://github.com/jdx/mise/pull/7686)
- update ghcr.io/jdx/mise:deb docker digest to 8bbca53 by @renovate[bot] in [#7687](https://github.com/jdx/mise/pull/7687)
- update ghcr.io/jdx/mise:rpm docker digest to de81415 by @renovate[bot] in [#7688](https://github.com/jdx/mise/pull/7688)
- update mcr.microsoft.com/devcontainers/rust:1 docker digest to 282e805 by @renovate[bot] in [#7690](https://github.com/jdx/mise/pull/7690)
- update rust docker digest to bed2d7f by @renovate[bot] in [#7691](https://github.com/jdx/mise/pull/7691)

### 📦 Registry

- add oh-my-posh by @scop in [#7659](https://github.com/jdx/mise/pull/7659)
- add bibtex-tidy (npm:bibtex-tidy) by @3w36zj6 in [#7677](https://github.com/jdx/mise/pull/7677)
- remove misconfigured bin_path option from kscript by @risu729 in [#7693](https://github.com/jdx/mise/pull/7693)

### New Contributors

- @AprilNEA made their first contribution in [#7644](https://github.com/jdx/mise/pull/7644)
- @opswole made their first contribution in [#7692](https://github.com/jdx/mise/pull/7692)
- @subdigital made their first contribution in [#7673](https://github.com/jdx/mise/pull/7673)
- @aacebedo made their first contribution in [#7560](https://github.com/jdx/mise/pull/7560)

## [2026.1.2](https://github.com/jdx/mise/compare/v2026.1.1..v2026.1.2) - 2026-01-13

### 🐛 Bug Fixes

- **(backend)** filter pre-release versions with latest + install_before by @koh-sh in [#7631](https://github.com/jdx/mise/pull/7631)
- **(backend)** detect .artifactbundle.zip files in asset selection by @swizzlr in [#7657](https://github.com/jdx/mise/pull/7657)
- **(docs)** formatting in configuration hierarchy section by @jonathanagustin in [#7638](https://github.com/jdx/mise/pull/7638)
- **(http)** enhance fetch_versions to to fallback to config for tool options by @roele in [#7655](https://github.com/jdx/mise/pull/7655)
- **(npm)** migrate npm publish to OIDC trusted publishing by @jdx in [#7607](https://github.com/jdx/mise/pull/7607)
- **(registry)** correct checkmake version test pattern by @jdx in [#7632](https://github.com/jdx/mise/pull/7632)
- **(release)** handle empty grep result in aqua-registry changelog by @jdx in [f45b4c6](https://github.com/jdx/mise/commit/f45b4c66d752c8e31ca103e42eda37710afd9d00)
- **(self-update)** self-update fails across year boundary due to semver mismatch by @jdx in [#7611](https://github.com/jdx/mise/pull/7611)
- **(tasks)** fix tool inheritance from intermediate parents by @chadxz in [#7637](https://github.com/jdx/mise/pull/7637)
- add `-test` to VERSION_REGEX prerelease filter by @belgio99 in [#7647](https://github.com/jdx/mise/pull/7647)

### 📚 Documentation

- **(tasks)** remove documentation for unimplemented features by @turbocrime in [#7599](https://github.com/jdx/mise/pull/7599)
- update `mise aliases` references to `mise tool-alias` by @muzimuzhi in [#7615](https://github.com/jdx/mise/pull/7615)
- use call operator in PowerShell profile example by @shina1024 in [#7639](https://github.com/jdx/mise/pull/7639)
- replace ASCII .pub key with binary .gpg for signed-by on Ubuntu/Debian by @gmalinowski in [#7649](https://github.com/jdx/mise/pull/7649)
- add missing word by @henrebotha in [#7653](https://github.com/jdx/mise/pull/7653)

### 🛡️ Security

- **(security)** prevent code execution from untrusted fork in registry-comment workflow by @jdx in [4a2441e](https://github.com/jdx/mise/commit/4a2441e81649c37dc05354246f9c9c192b6e8180)

### ◀️ Revert

- Revert "fix(release): handle empty grep result in aqua-registry changelog" by @jdx in [522ffdc](https://github.com/jdx/mise/commit/522ffdcb0627c31d60bf0b7f11ae5341896ccfc9)
- Revert "chore(release): include manually updated aqua-registry entries in the changelog " by @jdx in [1ebb943](https://github.com/jdx/mise/commit/1ebb9436d8b32c8dacf2ceca4d4c7a341f1a3bcb)

### 📦️ Dependency Updates

- update ghcr.io/jdx/mise:alpine docker digest to fbfffcf by @renovate[bot] in [#7619](https://github.com/jdx/mise/pull/7619)
- lock file maintenance by @renovate[bot] in [#7646](https://github.com/jdx/mise/pull/7646)

### 📦 Registry

- add hatoo/oha tool by @jylenhof in [#7633](https://github.com/jdx/mise/pull/7633)

### Chore

- **(registry)** fix registry comment workflow by @risu729 in [#7554](https://github.com/jdx/mise/pull/7554)
- **(release)** include manually updated aqua-registry entries in the changelog by @risu729 in [#7603](https://github.com/jdx/mise/pull/7603)

### New Contributors

- @swizzlr made their first contribution in [#7657](https://github.com/jdx/mise/pull/7657)
- @belgio99 made their first contribution in [#7647](https://github.com/jdx/mise/pull/7647)
- @gmalinowski made their first contribution in [#7649](https://github.com/jdx/mise/pull/7649)
- @chadxz made their first contribution in [#7637](https://github.com/jdx/mise/pull/7637)
- @shina1024 made their first contribution in [#7639](https://github.com/jdx/mise/pull/7639)
- @jonathanagustin made their first contribution in [#7638](https://github.com/jdx/mise/pull/7638)
- @turbocrime made their first contribution in [#7599](https://github.com/jdx/mise/pull/7599)

## [2026.1.1](https://github.com/jdx/mise/compare/v2026.1.0..v2026.1.1) - 2026-01-08

### 🚀 Features

- **(config)** add .miserc.toml for early initialization settings by @jdx in [#7596](https://github.com/jdx/mise/pull/7596)
- allow to include tasks from git repositories by @vmaleze in [#7582](https://github.com/jdx/mise/pull/7582)

### 🐛 Bug Fixes

- **(config)** mise use writes to lowest precedence config file by @jdx in [#7598](https://github.com/jdx/mise/pull/7598)
- **(python)** sort miniconda versions by conda version instead of version string by @jdx in [#7595](https://github.com/jdx/mise/pull/7595)
- Rust channel updates installing twice by @rjvkn in [#7565](https://github.com/jdx/mise/pull/7565)
- use Bearer instead of token in authorization headers by @risu729 in [#7593](https://github.com/jdx/mise/pull/7593)

### 📚 Documentation

- **(url-replacements)** document auth behaviour with url replacements by @risu729 in [#7592](https://github.com/jdx/mise/pull/7592)
- correct spelling in walkthrough.md by @tomhoover in [#7581](https://github.com/jdx/mise/pull/7581)

### 📦 Registry

- Revert "fix(registry): fix biome test to handle version prefix" by @risu729 in [#7586](https://github.com/jdx/mise/pull/7586)
- use aqua backend for ty by @risu729 in [#7539](https://github.com/jdx/mise/pull/7539)
- update opencode's org from sst to anomalyco by @graelo in [#7594](https://github.com/jdx/mise/pull/7594)

### New Contributors

- @graelo made their first contribution in [#7594](https://github.com/jdx/mise/pull/7594)
- @tomhoover made their first contribution in [#7581](https://github.com/jdx/mise/pull/7581)
- @vmaleze made their first contribution in [#7582](https://github.com/jdx/mise/pull/7582)

## [2026.1.0](https://github.com/jdx/mise/compare/v2025.12.13..v2026.1.0) - 2026-01-07

### 🚀 Features

- **(hooks)** add tool context env vars to postinstall hooks by @jdx in [#7521](https://github.com/jdx/mise/pull/7521)
- **(sops)** support standard SOPS environment variables by @yordis in [#7461](https://github.com/jdx/mise/pull/7461)
- **(tasks)** Add disable_spec_from_run_scripts setting by @iamkroot in [#7471](https://github.com/jdx/mise/pull/7471)
- **(tasks)** Add task_show_full_cmd setting by @iamkroot in [#7344](https://github.com/jdx/mise/pull/7344)
- **(tasks)** enable naked task completions and ::: separator by @jdx in [#7524](https://github.com/jdx/mise/pull/7524)
- add Forgejo backend by @roele in [#7469](https://github.com/jdx/mise/pull/7469)
- override node bundled npm by specified version of npm by @risu729 in [#7559](https://github.com/jdx/mise/pull/7559)

### 🐛 Bug Fixes

- **(aqua)** fix tree-sitter bin path regression by @risu729 in [#7535](https://github.com/jdx/mise/pull/7535)
- **(ci)** exclude subcrate tags from release workflow by @jdx in [#7517](https://github.com/jdx/mise/pull/7517)
- **(e2e)** remove hardcoded year from version check by @jdx in [#7584](https://github.com/jdx/mise/pull/7584)
- **(github)** asset matcher does not handle mixed archive/binary assets properly by @roele in [#7566](https://github.com/jdx/mise/pull/7566)
- **(github)** prioritize .zip on windows by @risu729 in [#7568](https://github.com/jdx/mise/pull/7568)
- **(github)** prefer .zip over non-archive extensions on linux by @risu729 in [#7587](https://github.com/jdx/mise/pull/7587)
- **(npm)** always use hoisted installs of bun by @sushichan044 in [#7542](https://github.com/jdx/mise/pull/7542)
- **(npm)** suppress NPM_CONFIG_UPDATE_NOTIFIER by @risu729 in [#7556](https://github.com/jdx/mise/pull/7556)
- **(registry)** fix biome test to handle version prefix by @jdx in [#7585](https://github.com/jdx/mise/pull/7585)
- **(tasks)** load monorepo task dirs without config by @matixlol in [#7478](https://github.com/jdx/mise/pull/7478)
- force reshim when windows_shim_mode is hardlink by @roele in [#7537](https://github.com/jdx/mise/pull/7537)
- simple .tar files are not extracted properly by @roele in [#7567](https://github.com/jdx/mise/pull/7567)
- quiet kerl update output by @iloveitaly in [#7467](https://github.com/jdx/mise/pull/7467)

### 📚 Documentation

- **(registry)** remove ubi backend from preferred backends list by @risu729 in [#7555](https://github.com/jdx/mise/pull/7555)
- **(tasks)** remove advanced usage specs sections from toml-tasks.md by @risu729 in [#7538](https://github.com/jdx/mise/pull/7538)
- fix invalid config section `[aliases]` by @muzimuzhi in [#7518](https://github.com/jdx/mise/pull/7518)
- Fix path to GitLab backend source by @henrebotha in [#7529](https://github.com/jdx/mise/pull/7529)
- Fix path to GitLab backend source by @henrebotha in [#7531](https://github.com/jdx/mise/pull/7531)
- update `mise --version` output by @muzimuzhi in [#7530](https://github.com/jdx/mise/pull/7530)

### 🧪 Testing

- **(win)** use pester in backend tests by @risu729 in [#7536](https://github.com/jdx/mise/pull/7536)
- update e2e tests to use `[tool_alias]` instead of `[alias]` by @muzimuzhi in [#7520](https://github.com/jdx/mise/pull/7520)

### 📦️ Dependency Updates

- update alpine:edge docker digest to ea71a03 by @renovate[bot] in [#7545](https://github.com/jdx/mise/pull/7545)
- update docker/setup-buildx-action digest to 8d2750c by @renovate[bot] in [#7546](https://github.com/jdx/mise/pull/7546)
- update ghcr.io/jdx/mise:copr docker digest to 23f4277 by @renovate[bot] in [#7548](https://github.com/jdx/mise/pull/7548)
- update ghcr.io/jdx/mise:alpine docker digest to 0adc211 by @renovate[bot] in [#7547](https://github.com/jdx/mise/pull/7547)
- lock file maintenance by @renovate[bot] in [#7211](https://github.com/jdx/mise/pull/7211)
- lock file maintenance by @renovate[bot] in [#7572](https://github.com/jdx/mise/pull/7572)
- replace dependency @tsconfig/node18 with @tsconfig/node20 by @renovate[bot] in [#7543](https://github.com/jdx/mise/pull/7543)
- replace dependency @tsconfig/node20 with @tsconfig/node22 by @renovate[bot] in [#7544](https://github.com/jdx/mise/pull/7544)

### 📦 Registry

- add zarf by @joonas in [#7525](https://github.com/jdx/mise/pull/7525)
- update aws-vault to maintained fork by @h3y6e in [#7527](https://github.com/jdx/mise/pull/7527)
- fix claude backend http for windows-x64 by @granstrand in [#7540](https://github.com/jdx/mise/pull/7540)
- add sqlc by @phm07 in [#7570](https://github.com/jdx/mise/pull/7570)
- use spm backend for swift-package-list by @risu729 in [#7569](https://github.com/jdx/mise/pull/7569)
- add npm (npm:npm) by @risu729 in [#7557](https://github.com/jdx/mise/pull/7557)
- add github backend for tmux by @ll-nick in [#7472](https://github.com/jdx/mise/pull/7472)

### Chore

- **(release)** update Changelog for v2025.12.13 by @muzimuzhi in [#7522](https://github.com/jdx/mise/pull/7522)

### New Contributors

- @ll-nick made their first contribution in [#7472](https://github.com/jdx/mise/pull/7472)
- @sushichan044 made their first contribution in [#7542](https://github.com/jdx/mise/pull/7542)
- @phm07 made their first contribution in [#7570](https://github.com/jdx/mise/pull/7570)
- @granstrand made their first contribution in [#7540](https://github.com/jdx/mise/pull/7540)
- @h3y6e made their first contribution in [#7527](https://github.com/jdx/mise/pull/7527)
- @matixlol made their first contribution in [#7478](https://github.com/jdx/mise/pull/7478)

## [2025.12.13](https://github.com/jdx/mise/compare/v2025.12.12..v2025.12.13) - 2025-12-30

### 🚀 Features

- **(ruby)** set PKG_CONFIG_PATH for native gem extensions by @jdx in [#7457](https://github.com/jdx/mise/pull/7457)
- **(tera)** add haiku() function for random name generation by @jdx in [#7399](https://github.com/jdx/mise/pull/7399)
- **(vfox)** pass tool options to EnvKeys hook by @jdx in [#7447](https://github.com/jdx/mise/pull/7447)
- implement independent versioning for subcrates by @jdx in [#7402](https://github.com/jdx/mise/pull/7402)
- Move iTerm to OSC9;4 supported terminals by @Maks3w in [#7485](https://github.com/jdx/mise/pull/7485)

### 🐛 Bug Fixes

- **(ci)** improve GHA cache efficiency and fix registry-ci bug by @jdx in [#7404](https://github.com/jdx/mise/pull/7404)
- **(ci)** use !cancelled() instead of always() for registry-ci by @jdx in [#7435](https://github.com/jdx/mise/pull/7435)
- **(ci)** bump taiki-e/install-action 2.61.10 to 2.65.5 by @kvokka in [#7496](https://github.com/jdx/mise/pull/7496)
- **(e2e)** use explicit asdf backend for zprint in plugin_install test by @jdx in [#7440](https://github.com/jdx/mise/pull/7440)
- **(github)** use GITHUB_TOKEN for attestation verification by @jdx in [#7446](https://github.com/jdx/mise/pull/7446)
- **(go)** filter out go pre-release versions in ls-remote by @roele in [#7488](https://github.com/jdx/mise/pull/7488)
- **(hooks)** revert per-tool hook execution by @just-be-dev in [#7509](https://github.com/jdx/mise/pull/7509)
- **(release)** sync subcrate versions and use YYYY.MM.0 calver by @jdx in [#7516](https://github.com/jdx/mise/pull/7516)
- **(schema)** add shell_alias definition by @anp in [#7441](https://github.com/jdx/mise/pull/7441)
- **(schema)** add prepare config by @risu729 in [#7497](https://github.com/jdx/mise/pull/7497)
- **(test)** update backend_arg test to use clojure instead of poetry by @jdx in [#7436](https://github.com/jdx/mise/pull/7436)
- use vfox backend for poetry and fix related tests by @jdx in [#7445](https://github.com/jdx/mise/pull/7445)

### 📚 Documentation

- **(prepare)** add all source files to sources by @risu729 in [#7498](https://github.com/jdx/mise/pull/7498)
- add link to COPR package page for Fedora/RHEL by @jdx in [bc8ac73](https://github.com/jdx/mise/commit/bc8ac732e3bdecfd12affd7b8c54cdebcdb87da1)
- improve installation documentation by @jdx in [#7403](https://github.com/jdx/mise/pull/7403)
- add comprehensive glossary by @jdx in [#7401](https://github.com/jdx/mise/pull/7401)
- use `mise run` uniformly in its examples by @muzimuzhi in [#7444](https://github.com/jdx/mise/pull/7444)
- update source file for asset autodetection by @muzimuzhi in [#7513](https://github.com/jdx/mise/pull/7513)

### 🧪 Testing

- **(ci)** validate GitHub token from pool with API call by @jdx in [#7459](https://github.com/jdx/mise/pull/7459)
- rename duplicate 'ci' job names for clarity by @jdx in [#7398](https://github.com/jdx/mise/pull/7398)
- add token pool integration for rate limit distribution by @jdx in [#7397](https://github.com/jdx/mise/pull/7397)

### 📦️ Dependency Updates

- replace dependency @tsconfig/node18 with @tsconfig/node20 by @renovate[bot] in [#7450](https://github.com/jdx/mise/pull/7450)
- pin rui314/setup-mold action to 725a879 by @renovate[bot] in [#7449](https://github.com/jdx/mise/pull/7449)

### 📦 Registry

- add github backend for swiftformat by @jdx in [#7396](https://github.com/jdx/mise/pull/7396)
- use pipx backend for azure-cli by @jdx in [#7406](https://github.com/jdx/mise/pull/7406)
- use pipx backend for dvc by @jdx in [#7413](https://github.com/jdx/mise/pull/7413)
- add github backend for zprint by @jdx in [#7410](https://github.com/jdx/mise/pull/7410)
- use gem backend for cocoapods by @jdx in [#7411](https://github.com/jdx/mise/pull/7411)
- use pipx backend for gallery-dl by @jdx in [#7409](https://github.com/jdx/mise/pull/7409)
- add aqua backends for HashiCorp tools by @jdx in [#7408](https://github.com/jdx/mise/pull/7408)
- use npm backend for danger-js by @jdx in [#7407](https://github.com/jdx/mise/pull/7407)
- use pipx backend for pipenv by @jdx in [#7415](https://github.com/jdx/mise/pull/7415)
- use pipx backend for poetry by @jdx in [#7416](https://github.com/jdx/mise/pull/7416)
- add github backend for xcodegen ([github:yonaskolb/XcodeGen](https://github.com/yonaskolb/XcodeGen)) by @jdx in [#7417](https://github.com/jdx/mise/pull/7417)
- use npm backend for heroku by @jdx in [#7418](https://github.com/jdx/mise/pull/7418)
- add aqua backend for setup-envtest by @jdx in [#7421](https://github.com/jdx/mise/pull/7421)
- add github backend for xcresultparser ([github:a7ex/xcresultparser](https://github.com/a7ex/xcresultparser)) by @jdx in [#7422](https://github.com/jdx/mise/pull/7422)
- add aqua backend for tomcat by @jdx in [#7423](https://github.com/jdx/mise/pull/7423)
- use npm backend for serverless by @jdx in [#7424](https://github.com/jdx/mise/pull/7424)
- add github backend for daytona ([github:daytonaio/daytona](https://github.com/daytonaio/daytona)) by @jdx in [#7412](https://github.com/jdx/mise/pull/7412)
- add github backend for flyway ([github:flyway/flyway](https://github.com/flyway/flyway)) by @jdx in [#7414](https://github.com/jdx/mise/pull/7414)
- add github backend for schemacrawler ([github:schemacrawler/SchemaCrawler](https://github.com/schemacrawler/SchemaCrawler)) by @jdx in [#7419](https://github.com/jdx/mise/pull/7419)
- add github backend for codeql by @jdx in [#7420](https://github.com/jdx/mise/pull/7420)
- use pipx backend for mitmproxy by @jdx in [#7425](https://github.com/jdx/mise/pull/7425)
- use pipx backend for sshuttle by @jdx in [#7426](https://github.com/jdx/mise/pull/7426)
- add github backend for quarkus by @jdx in [#7428](https://github.com/jdx/mise/pull/7428)
- add github backend for smithy by @jdx in [#7430](https://github.com/jdx/mise/pull/7430)
- add github backend for xchtmlreport ([github:XCTestHTMLReport/XCTestHTMLReport](https://github.com/XCTestHTMLReport/XCTestHTMLReport)) by @jdx in [#7431](https://github.com/jdx/mise/pull/7431)
- add github backend for grails by @jdx in [#7429](https://github.com/jdx/mise/pull/7429)
- use npm backend for esy by @jdx in [#7434](https://github.com/jdx/mise/pull/7434)
- add github backend for micronaut by @jdx in [#7433](https://github.com/jdx/mise/pull/7433)
- add github backend for dome by @jdx in [#7432](https://github.com/jdx/mise/pull/7432)
- use vfox backend for poetry by @jdx in [#7438](https://github.com/jdx/mise/pull/7438)
- add vfox backend for pipenv by @jdx in [#7439](https://github.com/jdx/mise/pull/7439)
- use github backend for xchtmlreport by @jdx in [#7442](https://github.com/jdx/mise/pull/7442)
- use npm backend for purty by @jdx in [#7443](https://github.com/jdx/mise/pull/7443)
- add micromamba tool definition by @rjvkn in [#7475](https://github.com/jdx/mise/pull/7475)
- add github backend for rumdl by @kvokka in [#7494](https://github.com/jdx/mise/pull/7494)
- add github backend for ty by @kvokka in [#7495](https://github.com/jdx/mise/pull/7495)
- add kopia by @ldrouard in [#7501](https://github.com/jdx/mise/pull/7501)
- add d2 by @icholy in [#7514](https://github.com/jdx/mise/pull/7514)

### Chore

- **(docker)** add Node LTS to mise Docker image by @jdx in [#7405](https://github.com/jdx/mise/pull/7405)
- rename mise-tools to mise-versions by @jdx in [ab3e1b8](https://github.com/jdx/mise/commit/ab3e1b8e64c2aa881c43af7636d6b492c6001e6f)
- s/mise task/mise tasks/g in docs and tests by @muzimuzhi in [#7400](https://github.com/jdx/mise/pull/7400)
- update singular/plural forms for word "task" by @muzimuzhi in [#7448](https://github.com/jdx/mise/pull/7448)

### New Contributors

- @icholy made their first contribution in [#7514](https://github.com/jdx/mise/pull/7514)
- @Maks3w made their first contribution in [#7485](https://github.com/jdx/mise/pull/7485)
- @muzimuzhi made their first contribution in [#7513](https://github.com/jdx/mise/pull/7513)
- @just-be-dev made their first contribution in [#7509](https://github.com/jdx/mise/pull/7509)
- @kvokka made their first contribution in [#7495](https://github.com/jdx/mise/pull/7495)
- @rjvkn made their first contribution in [#7475](https://github.com/jdx/mise/pull/7475)
- @anp made their first contribution in [#7441](https://github.com/jdx/mise/pull/7441)

## [2025.12.12](https://github.com/jdx/mise/compare/v2025.12.11..v2025.12.12) - 2025-12-18

### 🚀 Features

- **(backend)** add version timestamps for spm, conda, and gem backends by @jdx in [#7383](https://github.com/jdx/mise/pull/7383)
- **(backend)** add security features to github backend by @jdx in [#7387](https://github.com/jdx/mise/pull/7387)
- **(ruby)** add GitHub attestation verification for precompiled binaries by @jdx in [#7382](https://github.com/jdx/mise/pull/7382)

### 🐛 Bug Fixes

- **(aqua)** improve security feature detection by @jdx in [#7385](https://github.com/jdx/mise/pull/7385)
- **(github)** use version_prefix when fetching release for SLSA verification by @jdx in [#7391](https://github.com/jdx/mise/pull/7391)

### 🚜 Refactor

- **(vfox)** remove submodules, embed plugins directly by @jdx in [#7389](https://github.com/jdx/mise/pull/7389)

### 🧪 Testing

- **(registry)** add final ci job as merge gate by @jdx in [#7390](https://github.com/jdx/mise/pull/7390)
- split unit job to speed up macOS CI by @jdx in [#7388](https://github.com/jdx/mise/pull/7388)

## [2025.12.11](https://github.com/jdx/mise/compare/v2025.12.10..v2025.12.11) - 2025-12-18

### 🚀 Features

- **(alias)** rename alias to tool-alias, add shell-alias command by @jdx in [#7357](https://github.com/jdx/mise/pull/7357)
- **(upgrade)** display summary of upgraded tools by @jdx in [#7372](https://github.com/jdx/mise/pull/7372)
- **(vfox)** embed vfox plugin Lua code in binary by @jdx in [#7369](https://github.com/jdx/mise/pull/7369)

### 🐛 Bug Fixes

- **(aqua)** add start_operations for progress reporting by @jdx in [#7354](https://github.com/jdx/mise/pull/7354)
- **(github)** improve asset detection for distro-specific and Swift artifacts by @jdx in [#7347](https://github.com/jdx/mise/pull/7347)
- **(github)** clean up static_helpers.rs and fix archive bin= option by @jdx in [#7366](https://github.com/jdx/mise/pull/7366)
- **(http)** add start_operations for progress reporting by @jdx in [#7355](https://github.com/jdx/mise/pull/7355)
- **(lockfile)** place lockfile alongside config file by @jdx in [#7360](https://github.com/jdx/mise/pull/7360)
- **(progress)** add start_operations to core plugins by @jdx in [#7351](https://github.com/jdx/mise/pull/7351)
- **(ruby-install)** Use ruby_install_bin to update by @calebhearth in [#7350](https://github.com/jdx/mise/pull/7350)
- **(rust)** add release_url for rust versions by @jdx in [#7373](https://github.com/jdx/mise/pull/7373)
- **(schema)** add `tool_alias`, mark `alias` as deprecated by @SKalt in [#7358](https://github.com/jdx/mise/pull/7358)
- **(toolset)** filter tools by OS in list_current_versions by @jdx in [#7356](https://github.com/jdx/mise/pull/7356)
- **(ubi)** only show deprecation warning during installation by @jdx in [#7380](https://github.com/jdx/mise/pull/7380)
- **(ui)** remove noisy "record size" message during install by @jdx in [#7381](https://github.com/jdx/mise/pull/7381)
- update mise-versions URL to use /tools/ prefix by @jdx in [#7378](https://github.com/jdx/mise/pull/7378)

### 🚜 Refactor

- **(backend)** unified AssetMatcher with checksum fetching by @jdx in [#7370](https://github.com/jdx/mise/pull/7370)
- **(backend)** deprecate ubi backend in favor of github by @jdx in [#7374](https://github.com/jdx/mise/pull/7374)
- **(toolset)** decompose mod.rs into smaller modules by @jdx in [#7371](https://github.com/jdx/mise/pull/7371)

### 🧪 Testing

- **(e2e)** fix and rename ubi and vfox_embedded_override tests by @jdx in [052ea40](https://github.com/jdx/mise/commit/052ea40b29468f05fbc425cc5a4c20ebda077253)

### 📦 Registry

- add vfox-gcloud backend for gcloud by @jdx in [#7349](https://github.com/jdx/mise/pull/7349)
- convert amplify to use github backend by @jdx in [#7365](https://github.com/jdx/mise/pull/7365)
- add github backend for djinni tool by @jdx in [#7363](https://github.com/jdx/mise/pull/7363)
- switch glab to native gitlab backend by @jdx in [#7364](https://github.com/jdx/mise/pull/7364)
- add s5cmd by @jdx in [#7376](https://github.com/jdx/mise/pull/7376)

### Chore

- **(registry)** disable flaky tests for gitu and ktlint by @jdx in [64151cb](https://github.com/jdx/mise/commit/64151cb3fb1e517b2c80aa2179b24c4bd55ff34a)
- resolve clippy warnings and add stricter CI check by @jdx in [#7367](https://github.com/jdx/mise/pull/7367)
- suppress dead_code warnings in asset_matcher module by @jdx in [#7377](https://github.com/jdx/mise/pull/7377)

### New Contributors

- @calebhearth made their first contribution in [#7350](https://github.com/jdx/mise/pull/7350)

## [2025.12.10](https://github.com/jdx/mise/compare/v2025.12.9..v2025.12.10) - 2025-12-16

### 🐛 Bug Fixes

- **(backend)** fix fuzzy_match_filter regex for vendor-prefixed versions by @jdx in [#7332](https://github.com/jdx/mise/pull/7332)
- **(backend)** use backend delegation for install-time option filtering by @jdx in [#7335](https://github.com/jdx/mise/pull/7335)
- **(duration)** support calendar units in relative durations for --before flag by @Copilot in [#7337](https://github.com/jdx/mise/pull/7337)
- **(gem)** improve shebang compatibility for precompiled Ruby by @jdx in [#7336](https://github.com/jdx/mise/pull/7336)
- **(gem)** handle RubyGems polyglot shebang format by @jdx in [#7340](https://github.com/jdx/mise/pull/7340)
- **(pipx)** use minor version symlink for venv Python by @jdx in [#7339](https://github.com/jdx/mise/pull/7339)
- **(registry)** prefer claude-code latest over stale stable by @jdx in [#7334](https://github.com/jdx/mise/pull/7334)
- **(upgrade)** only check specified tools when upgrading with tool args by @jdx in [#7331](https://github.com/jdx/mise/pull/7331)

### 📚 Documentation

- Revise alias example for task execution by @azais-corentin in [#7338](https://github.com/jdx/mise/pull/7338)

## [2025.12.9](https://github.com/jdx/mise/compare/v2025.12.8..v2025.12.9) - 2025-12-16

### 🚀 Features

- **(aqua)** add tuist aqua backend by @jdx in [#7323](https://github.com/jdx/mise/pull/7323)
- **(ls-remote)** add release_url to VersionInfo for --json output by @jdx in [#7322](https://github.com/jdx/mise/pull/7322)
- **(prepare)** add `mise prepare` command for dependency preparation by @jdx in [#7281](https://github.com/jdx/mise/pull/7281)
- **(registry)** add aqua backend for zigmod by @jdx in [#7319](https://github.com/jdx/mise/pull/7319)

### 🐛 Bug Fixes

- **(e2e)** fix flaky test_prepare go provider test by @jdx in [0e2ef73](https://github.com/jdx/mise/commit/0e2ef73f9ae91072efd5abbbbe9d82e932472e79)
- **(go)** restore git ls-remote for version listing by @jdx in [#7324](https://github.com/jdx/mise/pull/7324)

### 📦 Registry

- use github backend for sourcery by @jdx in [#7327](https://github.com/jdx/mise/pull/7327)
- use github backend for swiftgen by @jdx in [#7326](https://github.com/jdx/mise/pull/7326)

## [2025.12.8](https://github.com/jdx/mise/compare/v2025.12.7..v2025.12.8) - 2025-12-15

### 🚀 Features

- **(conda)** add dependency resolution for conda packages by @jdx in [#7280](https://github.com/jdx/mise/pull/7280)
- **(go)** add created_at support to ls-remote --json by @jdx in [#7305](https://github.com/jdx/mise/pull/7305)
- **(hook-env)** add hook_env.cache_ttl and hook_env.chpwd_only settings for NFS optimization by @jdx in [#7312](https://github.com/jdx/mise/pull/7312)
- **(hooks)** add MISE_TOOL_NAME and MISE_TOOL_VERSION to preinstall/postinstall hooks by @jdx in [#7311](https://github.com/jdx/mise/pull/7311)
- **(shell_alias)** add shell_alias support for cross-shell aliases by @jdx in [#7316](https://github.com/jdx/mise/pull/7316)
- **(tool)** add security field to mise tool --json by @jdx in [#7303](https://github.com/jdx/mise/pull/7303)
- add --before flag for date-based version filtering by @jdx in [#7298](https://github.com/jdx/mise/pull/7298)

### 🐛 Bug Fixes

- **(aqua)** support cosign v3 bundle verification by @jdx in [#7314](https://github.com/jdx/mise/pull/7314)
- **(config)** use correct config_root in tera context for hooks by @jdx in [#7309](https://github.com/jdx/mise/pull/7309)
- **(nu)** fix nushell deactivation script on Windows by @fu050409 in [#7213](https://github.com/jdx/mise/pull/7213)
- **(python)** apply uv_venv_create_args in auto-venv code path by @jdx in [#7310](https://github.com/jdx/mise/pull/7310)
- **(shell)** escape exe path in activation scripts for paths with spaces by @jdx in [#7315](https://github.com/jdx/mise/pull/7315)
- **(task)** parallelize exec_env loading to fix parallel task execution by @jdx in [#7313](https://github.com/jdx/mise/pull/7313)
- track downloads for python and java by @jdx in [#7304](https://github.com/jdx/mise/pull/7304)
- include full tool ID in download track by @jdx in [#7320](https://github.com/jdx/mise/pull/7320)

### 📚 Documentation

- Switch `postinstall` code to be shell-agnostic by @thejcannon in [#7317](https://github.com/jdx/mise/pull/7317)

### 🧪 Testing

- **(e2e)** disable debug mode by default for windows-e2e by @jdx in [#7318](https://github.com/jdx/mise/pull/7318)

### New Contributors

- @fu050409 made their first contribution in [#7213](https://github.com/jdx/mise/pull/7213)

## [2025.12.7](https://github.com/jdx/mise/compare/v2025.12.6..v2025.12.7) - 2025-12-14

### 🚀 Features

- **(java)** add created_at support to ls-remote --json by @jdx in [#7297](https://github.com/jdx/mise/pull/7297)
- **(ls-remote)** add created_at timestamps to ls-remote --json for more backends by @jdx in [#7295](https://github.com/jdx/mise/pull/7295)
- **(ls-remote)** add created_at timestamps to ls-remote --json for core plugins by @jdx in [#7294](https://github.com/jdx/mise/pull/7294)
- **(registry)** add --json flag to registry command by @jdx in [#7290](https://github.com/jdx/mise/pull/7290)
- **(ruby)** add created_at timestamps to ls-remote --json by @jdx in [#7296](https://github.com/jdx/mise/pull/7296)

### 🐛 Bug Fixes

- **(spm)** recursively update submodules after checkout by @JFej in [#7292](https://github.com/jdx/mise/pull/7292)
- prioritize raw task output over task_output setting by @skorfmann in [#7286](https://github.com/jdx/mise/pull/7286)

### New Contributors

- @skorfmann made their first contribution in [#7286](https://github.com/jdx/mise/pull/7286)
- @JFej made their first contribution in [#7292](https://github.com/jdx/mise/pull/7292)

## [2025.12.6](https://github.com/jdx/mise/compare/v2025.12.5..v2025.12.6) - 2025-12-14

### 🚀 Features

- add anonymous download tracking for tool popularity stats by @jdx in [#7289](https://github.com/jdx/mise/pull/7289)

### 🐛 Bug Fixes

- add --compressed flag to curl for Swift GPG keys by @jdx in [7bc1273](https://github.com/jdx/mise/commit/7bc1273e78c9a1b58e0c987f5f2560f498efd2d4)

### 📚 Documentation

- add Versions link to nav bar by @jdx in [#7283](https://github.com/jdx/mise/pull/7283)
- add mise-tools link to nav bar by @jdx in [#7285](https://github.com/jdx/mise/pull/7285)

## [2025.12.5](https://github.com/jdx/mise/compare/v2025.12.4..v2025.12.5) - 2025-12-13

### 🚀 Features

- **(ls-remote)** add --json flag with created_at timestamps by @jdx in [#7279](https://github.com/jdx/mise/pull/7279)

### 🐛 Bug Fixes

- **(config)** respect MISE_CONFIG_DIR when set to non-default location by @jdx in [#7271](https://github.com/jdx/mise/pull/7271)
- **(http)** move http-tarballs from cache to data directory by @jdx in [#7273](https://github.com/jdx/mise/pull/7273)
- **(pipx)** expand wildcards in install command for backend tools by @jdx in [#7275](https://github.com/jdx/mise/pull/7275)
- **(tasks)** position-based flag parsing for `mise run` by @jdx in [#7278](https://github.com/jdx/mise/pull/7278)
- **(tera)** handle empty strings in path filters by @jdx in [#7276](https://github.com/jdx/mise/pull/7276)
- **(vfox)** make mise_env and mise_path hooks optional by @jdx in [#7274](https://github.com/jdx/mise/pull/7274)

### 📚 Documentation

- **(ruby)** add precompiled binaries documentation by @jdx in [#7269](https://github.com/jdx/mise/pull/7269)

## [2025.12.4](https://github.com/jdx/mise/compare/v2025.12.3..v2025.12.4) - 2025-12-13

### 🚀 Features

- **(copr)** add Fedora 43 support by @jdx in [#7265](https://github.com/jdx/mise/pull/7265)
- **(ruby)** add precompiled binary support by @jdx in [#7263](https://github.com/jdx/mise/pull/7263)

## [2025.12.3](https://github.com/jdx/mise/compare/v2025.12.2..v2025.12.3) - 2025-12-13

### 🚀 Features

- **(ui)** add color_theme setting for light terminal support by @bishopmatthew in [#7257](https://github.com/jdx/mise/pull/7257)

### 🐛 Bug Fixes

- **(node)** add newlines between GPG keys in fetch script by @jdx in [#7262](https://github.com/jdx/mise/pull/7262)
- **(run)** truncate task description to first line in run selector by @jdx in [#7256](https://github.com/jdx/mise/pull/7256)
- unset -f bash functions by @agriffis in [#7072](https://github.com/jdx/mise/pull/7072)

### 📚 Documentation

- fix type of mise_env in templates by @risu729 in [#7261](https://github.com/jdx/mise/pull/7261)

### 🧪 Testing

- add empty secret redaction test by @risu729 in [#7260](https://github.com/jdx/mise/pull/7260)

### 📦️ Dependency Updates

- update ghcr.io/jdx/mise:copr docker digest to af06edf by @renovate[bot] in [#7245](https://github.com/jdx/mise/pull/7245)
- update ghcr.io/jdx/mise:alpine docker digest to 3ca5ebd by @renovate[bot] in [#7244](https://github.com/jdx/mise/pull/7244)
- update ghcr.io/jdx/mise:rpm docker digest to bdc5d0d by @renovate[bot] in [#7247](https://github.com/jdx/mise/pull/7247)
- update ghcr.io/jdx/mise:deb docker digest to f73d7ef by @renovate[bot] in [#7246](https://github.com/jdx/mise/pull/7246)
- update mcr.microsoft.com/devcontainers/rust:1 docker digest to 884de39 by @renovate[bot] in [#7249](https://github.com/jdx/mise/pull/7249)
- update jdx/mise-action digest to 146a281 by @renovate[bot] in [#7248](https://github.com/jdx/mise/pull/7248)

### Chore

- **(registry)** retry only failed tools by @risu729 in [#7251](https://github.com/jdx/mise/pull/7251)

### New Contributors

- @agriffis made their first contribution in [#7072](https://github.com/jdx/mise/pull/7072)
- @bishopmatthew made their first contribution in [#7257](https://github.com/jdx/mise/pull/7257)

## [2025.12.2](https://github.com/jdx/mise/compare/v2025.12.1..v2025.12.2) - 2025-12-11

### 🐛 Bug Fixes

- **(node)** fetch GPG keys from nodejs/release-keys repo by @jdx in [#7242](https://github.com/jdx/mise/pull/7242)
- **(release)** run fetch-gpg-keys before build by @jdx in [2608caf](https://github.com/jdx/mise/commit/2608cafec410befc911f53181850fbc720bc33ce)
- **(tasks)** disable ctrl-c exit behavior during mise run by @jdx in [#7232](https://github.com/jdx/mise/pull/7232)

### 📦 Registry

- added werf by @tony-sol in [#7230](https://github.com/jdx/mise/pull/7230)

## [2025.12.1](https://github.com/jdx/mise/compare/v2025.12.0..v2025.12.1) - 2025-12-08

### 🚀 Features

- **(npm)** support pnpm as a package manager for npm backend by @risu729 in [#7214](https://github.com/jdx/mise/pull/7214)
- **(tool-stubs)** add --bootstrap flag to mise generate tool-stub by @jdx in [#7203](https://github.com/jdx/mise/pull/7203)

### 🐛 Bug Fixes

- **(alpine)** increase alpine release timeout to 60 minutes by @jdx in [#7188](https://github.com/jdx/mise/pull/7188)
- **(bun)** use x64-baseline for aarch64 on Windows by @roele in [#7190](https://github.com/jdx/mise/pull/7190)
- **(tools)** allow using env vars in tools by @antonsergeyev in [#7205](https://github.com/jdx/mise/pull/7205)
- add cfg(feature = "self_update") to statics only used by that feature by @jdx in [#7185](https://github.com/jdx/mise/pull/7185)

### 📚 Documentation

- Update registry.md by @jdx in [ad11ad1](https://github.com/jdx/mise/commit/ad11ad14705b2adac5c874f15fef4cc74652e26f)

### 📦️ Dependency Updates

- update ghcr.io/jdx/mise:alpine docker digest to 2909cce by @renovate[bot] in [#7196](https://github.com/jdx/mise/pull/7196)
- update fedora:43 docker digest to 6cd815d by @renovate[bot] in [#7195](https://github.com/jdx/mise/pull/7195)
- update ghcr.io/jdx/mise:deb docker digest to 1893530 by @renovate[bot] in [#7198](https://github.com/jdx/mise/pull/7198)
- update ghcr.io/jdx/mise:copr docker digest to 0447a85 by @renovate[bot] in [#7197](https://github.com/jdx/mise/pull/7197)

### 📦 Registry

- add Supabase CLI to registry.toml by @bodadotsh in [#7206](https://github.com/jdx/mise/pull/7206)
- add cmake aqua backend by @mangkoran in [#7186](https://github.com/jdx/mise/pull/7186)

### New Contributors

- @antonsergeyev made their first contribution in [#7205](https://github.com/jdx/mise/pull/7205)
- @bodadotsh made their first contribution in [#7206](https://github.com/jdx/mise/pull/7206)

## [2025.12.0](https://github.com/jdx/mise/compare/v2025.11.11..v2025.12.0) - 2025-12-04

### 🚀 Features

- **(config)** add support for netrc by @RobotSupervisor in [#7164](https://github.com/jdx/mise/pull/7164)
- **(lock)** add resolve_lock_info to core backends for checksum fetching by @jdx in [#7180](https://github.com/jdx/mise/pull/7180)
- **(ruby)** Install ruby from a zip file over HTTPS by @KaanYT in [#7167](https://github.com/jdx/mise/pull/7167)
- **(tasks)** add `usage` args to Tera context in run scripts by @iamkroot in [#7041](https://github.com/jdx/mise/pull/7041)

### 🐛 Bug Fixes

- **(lock)** validate platform qualifiers when reading from lockfile by @jdx in [#7181](https://github.com/jdx/mise/pull/7181)
- **(task)** retry shebang scripts on ETXTBUSY by @iamkroot in [#7162](https://github.com/jdx/mise/pull/7162)
- **(ui)** remove duplicate 'mise' prefix in verbose footer output by @jdx in [#7174](https://github.com/jdx/mise/pull/7174)

### 📦️ Dependency Updates

- bump usage-lib to 2.9.0 by @jdx in [#7177](https://github.com/jdx/mise/pull/7177)

### 📦 Registry

- remove duplicated ubi and github backends from gping by @risu729 in [#7144](https://github.com/jdx/mise/pull/7144)
- disable bashly test (not working in CI) by @jdx in [#7173](https://github.com/jdx/mise/pull/7173)
- disable cfn-lint test (failing in CI) by @jdx in [#7176](https://github.com/jdx/mise/pull/7176)

### Chore

- add fd to mise.toml by @blampe in [#7178](https://github.com/jdx/mise/pull/7178)

### New Contributors

- @RobotSupervisor made their first contribution in [#7164](https://github.com/jdx/mise/pull/7164)

## [2025.11.11](https://github.com/jdx/mise/compare/v2025.11.10..v2025.11.11) - 2025-11-30

### 🚀 Features

- **(backend)** add filter_bins option to github/gitlab backends by @risu729 in [#7105](https://github.com/jdx/mise/pull/7105)
- **(ci)** auto-close PRs from non-maintainers by @jdx in [#7108](https://github.com/jdx/mise/pull/7108)
- **(conda)** add conda backend for installing packages from conda-forge by @jdx in [#7139](https://github.com/jdx/mise/pull/7139)
- **(github)** add rename_exe option and switch elm, opam, yt-dlp from ubi by @jdx in [#7140](https://github.com/jdx/mise/pull/7140)
- **(install)** add --locked flag for strict lockfile mode by @jdx in [#7098](https://github.com/jdx/mise/pull/7098)
- **(lock)** implement cross-platform lockfile generation by @jdx in [#7091](https://github.com/jdx/mise/pull/7091)
- **(lockfile)** add options field for tool artifact identity by @jdx in [#7092](https://github.com/jdx/mise/pull/7092)
- **(lockfile)** add env field and local lockfile support by @jdx in [#7099](https://github.com/jdx/mise/pull/7099)
- **(lockfile)** add URL support for deno, go, and zig backends by @jdx in [#7112](https://github.com/jdx/mise/pull/7112)
- **(lockfile)** add URL support for vfox backend by @jdx in [#7114](https://github.com/jdx/mise/pull/7114)
- **(lockfile)** add multi-platform checksums without downloading tarballs by @jdx in [#7113](https://github.com/jdx/mise/pull/7113)

### 🐛 Bug Fixes

- **(backend)** allow platform-specific strip_components by @risu729 in [#7106](https://github.com/jdx/mise/pull/7106)
- **(backend)** prefer path root for bin path if it contains an executable by @risu729 in [#7151](https://github.com/jdx/mise/pull/7151)
- **(bash)** avoid deactivate error on (no)unset PROMPT_COMMAND by @scop in [#7096](https://github.com/jdx/mise/pull/7096)
- **(ci)** use updatedAt instead of createdAt for stale PR detection by @jdx in [#7109](https://github.com/jdx/mise/pull/7109)
- **(config)** increase fetch_remote_versions_timeout default to 20s by @jdx in [#7157](https://github.com/jdx/mise/pull/7157)
- **(github)** search subdirectories for executables in discover_bin_paths by @jdx in [#7138](https://github.com/jdx/mise/pull/7138)
- **(lockfile)** combine api_url with asset_pattern for GitHub release URLs by @jdx in [#7111](https://github.com/jdx/mise/pull/7111)

### 🚜 Refactor

- **(lock)** simplify lockfile to always use array format by @jdx in [#7093](https://github.com/jdx/mise/pull/7093)
- **(lockfile)** use compact inline table format by @jdx in [#7141](https://github.com/jdx/mise/pull/7141)

### 📚 Documentation

- **(gitlab)** document rename_exe option also for gitlab backend by @risu729 in [#7149](https://github.com/jdx/mise/pull/7149)
- **(lockfile)** update documentation for recent lockfile changes by @jdx in [#7107](https://github.com/jdx/mise/pull/7107)
- **(node)** use config_root in _.path for pnpm example by @risu729 in [#7146](https://github.com/jdx/mise/pull/7146)
- **(registry)** add github/gitlab backends to the preferred backends list by @risu729 in [#7148](https://github.com/jdx/mise/pull/7148)
- **(registry)** add url mappings for all backends by @risu729 in [#7147](https://github.com/jdx/mise/pull/7147)

### 📦️ Dependency Updates

- update docker/metadata-action digest to c299e40 by @renovate[bot] in [#7101](https://github.com/jdx/mise/pull/7101)
- update ghcr.io/jdx/mise:alpine docker digest to 693c5f6 by @renovate[bot] in [#7102](https://github.com/jdx/mise/pull/7102)
- update ghcr.io/jdx/mise:deb docker digest to 9985cab by @renovate[bot] in [#7104](https://github.com/jdx/mise/pull/7104)
- update ghcr.io/jdx/mise:copr docker digest to 564d8e1 by @renovate[bot] in [#7103](https://github.com/jdx/mise/pull/7103)
- update rust crate ubi to 0.8.4 by @risu729 in [#7154](https://github.com/jdx/mise/pull/7154)

### 📦 Registry

- add aqua backend as primary for e1s by @jdx in [#7115](https://github.com/jdx/mise/pull/7115)
- add gem backend for bashly by @jdx in [6af6607](https://github.com/jdx/mise/commit/6af6607393a198feb1078e3ec3bc06146e82a23d)
- switch 1password from asdf to vfox backend by @jdx in [#7116](https://github.com/jdx/mise/pull/7116)
- add vfox backend for bfs by @jdx in [#7126](https://github.com/jdx/mise/pull/7126)
- add github backend for btrace by @jdx in [#7129](https://github.com/jdx/mise/pull/7129)
- add github backend for cf by @jdx in [#7131](https://github.com/jdx/mise/pull/7131)
- add vfox backend for bpkg by @jdx in [#7130](https://github.com/jdx/mise/pull/7130)
- switch apollo-ios from asdf to github backend by @jdx in [#7118](https://github.com/jdx/mise/pull/7118)
- add vfox backend for chromedriver by @jdx in [#7134](https://github.com/jdx/mise/pull/7134)
- switch superhtml, vespa-cli, xcsift from ubi to github backend by @jdx in [#7137](https://github.com/jdx/mise/pull/7137)
- add vfox backend for clickhouse by @jdx in [#7136](https://github.com/jdx/mise/pull/7136)
- switch chicken to vfox plugin by @jdx in [#7135](https://github.com/jdx/mise/pull/7135)
- switch chezscheme from asdf to vfox backend by @jdx in [#7132](https://github.com/jdx/mise/pull/7132)
- add vfox backend for carthage by @jdx in [#7133](https://github.com/jdx/mise/pull/7133)
- switch azure-functions-core-tools from asdf to vfox backend by @jdx in [#7128](https://github.com/jdx/mise/pull/7128)
- switch aapt2 to vfox backend by @jdx in [#7117](https://github.com/jdx/mise/pull/7117)
- switch ant to vfox backend by @jdx in [#7119](https://github.com/jdx/mise/pull/7119)
- switch asciidoctorj from asdf to vfox backend by @jdx in [#7121](https://github.com/jdx/mise/pull/7121)
- switch awscli-local to pipx backend by @jdx in [#7120](https://github.com/jdx/mise/pull/7120)
- add omnictl by @risu729 in [#7145](https://github.com/jdx/mise/pull/7145)
- remove pnpm asdf plugin from fallback by @risu729 in [#7143](https://github.com/jdx/mise/pull/7143)
- switch tanzu to github backend by @jdx in [#7124](https://github.com/jdx/mise/pull/7124)
- switch android-sdk to vfox plugin by @jdx in [#7127](https://github.com/jdx/mise/pull/7127)
- add vfox backend for ag (The Silver Searcher) by @jdx in [#7122](https://github.com/jdx/mise/pull/7122)
- add gem backend for bashly by @jdx in [#7125](https://github.com/jdx/mise/pull/7125)

### Chore

- **(registry)** ignore deleted tools in test-tool workflow by @risu729 in [#7081](https://github.com/jdx/mise/pull/7081)
- **(release)** show registry section last in changelog by @jdx in [#7156](https://github.com/jdx/mise/pull/7156)
- update mise.lock with checksums by @jdx in [71e9123](https://github.com/jdx/mise/commit/71e9123efac62924b5804e1f56e61400adf22470)
- disable cancel-in-progress for test workflow on main branch by @risu729 in [#7152](https://github.com/jdx/mise/pull/7152)

## [2025.11.10](https://github.com/jdx/mise/compare/v2025.11.9..v2025.11.10) - 2025-11-27

### 🐛 Bug Fixes

- **(docs)** link gitlab backended tools in registry by @risu729 in [#7078](https://github.com/jdx/mise/pull/7078)

### 🚜 Refactor

- **(hook-env)** derive config_subdirs from config filenames by @risu729 in [#7080](https://github.com/jdx/mise/pull/7080)

### 📦 Registry

- enable symlink_bins for aws-sam by @risu729 in [#7082](https://github.com/jdx/mise/pull/7082)
- use cargo backend for tokei to support latest version by @risu729 in [#7086](https://github.com/jdx/mise/pull/7086)
- add SonarSource/sonar-scanner-cli by @kapitoshka438 in [#7087](https://github.com/jdx/mise/pull/7087)

### New Contributors

- @kapitoshka438 made their first contribution in [#7087](https://github.com/jdx/mise/pull/7087)

## [2025.11.9](https://github.com/jdx/mise/compare/v2025.11.8..v2025.11.9) - 2025-11-27

### 🚀 Features

- **(aqua)** add symlink_bins option to filter exposed binaries by @jdx in [#7076](https://github.com/jdx/mise/pull/7076)

### 🐛 Bug Fixes

- **(aqua)** skip whitespace before pipe token in template parser by @jdx in [#7069](https://github.com/jdx/mise/pull/7069)
- **(docs)** link github backends to github repo URLs by @SKalt in [#7071](https://github.com/jdx/mise/pull/7071)

### 📚 Documentation

- update node examples from 22 to 24 by @jdx in [#7074](https://github.com/jdx/mise/pull/7074)

### ⚡ Performance

- **(hook-env)** add fast-path to skip initialization when nothing changed by @jdx in [#7073](https://github.com/jdx/mise/pull/7073)

### 📦 Registry

- add charmbracelet/crush by @ev-the-dev in [#7075](https://github.com/jdx/mise/pull/7075)

### New Contributors

- @ev-the-dev made their first contribution in [#7075](https://github.com/jdx/mise/pull/7075)
- @SKalt made their first contribution in [#7071](https://github.com/jdx/mise/pull/7071)

## [2025.11.8](https://github.com/jdx/mise/compare/v2025.11.7..v2025.11.8) - 2025-11-26

### 🚀 Features

- **(plugins)** Install a plugin from a zip file over HTTPS by @KaanYT in [#6992](https://github.com/jdx/mise/pull/6992)
- **(registry)** add tool options support for http backend by @jdx in [#7061](https://github.com/jdx/mise/pull/7061)

### 🐛 Bug Fixes

- **(core)** trim `core:` prefix in unalias_backend by @kou029w in [#7040](https://github.com/jdx/mise/pull/7040)
- **(exec)** make `mise x tool@latest` auto-install actual latest version by @jdx in [#7064](https://github.com/jdx/mise/pull/7064)
- **(go)** use -mod=readonly for go install by @joonas in [#7052](https://github.com/jdx/mise/pull/7052)
- **(npm)** handle v-prefixed versions correctly by @jdx in [#7062](https://github.com/jdx/mise/pull/7062)
- **(tasks)** add missing task fields to JSON output by @roele in [#7044](https://github.com/jdx/mise/pull/7044)
- semver in aqua by @lucasew in [#7018](https://github.com/jdx/mise/pull/7018)
- use the musl version if installing in Android (Termux) by @lucasew in [#7027](https://github.com/jdx/mise/pull/7027)
- empty enable_tools crash by @moshen in [#7035](https://github.com/jdx/mise/pull/7035)

### 📚 Documentation

- add MISE and USAGE syntax hl queries to neovim cookbook by @okuuva in [#7047](https://github.com/jdx/mise/pull/7047)
- use local assets for screenshots by @okuuva in [#7056](https://github.com/jdx/mise/pull/7056)
- remove GitHub issues link from roadmap by @jdx in [6897286](https://github.com/jdx/mise/commit/689728642b386e197a549ea8b5dd591c3b950b42)

### 📦️ Dependency Updates

- update docker/metadata-action digest to 318604b by @renovate[bot] in [#7033](https://github.com/jdx/mise/pull/7033)
- update actions/checkout digest to 34e1148 by @renovate[bot] in [#7032](https://github.com/jdx/mise/pull/7032)
- lock file maintenance by @renovate[bot] in [#7048](https://github.com/jdx/mise/pull/7048)

### 📦 Registry

- add blender by @lucasew in [#7014](https://github.com/jdx/mise/pull/7014)
- add vespa-cli by @buinauskas in [#7037](https://github.com/jdx/mise/pull/7037)
- fix vespa-cli order by @buinauskas in [#7038](https://github.com/jdx/mise/pull/7038)
- add scooter by @TyceHerrman in [#7039](https://github.com/jdx/mise/pull/7039)
- Prefer github backend for allure by @TobiX in [#7049](https://github.com/jdx/mise/pull/7049)

### Chore

- upgrade actionlint to 1.7.9 and fix lint issues by @jdx in [#7065](https://github.com/jdx/mise/pull/7065)

### New Contributors

- @joonas made their first contribution in [#7052](https://github.com/jdx/mise/pull/7052)
- @KaanYT made their first contribution in [#6992](https://github.com/jdx/mise/pull/6992)
- @kou029w made their first contribution in [#7040](https://github.com/jdx/mise/pull/7040)
- @moshen made their first contribution in [#7035](https://github.com/jdx/mise/pull/7035)
- @buinauskas made their first contribution in [#7038](https://github.com/jdx/mise/pull/7038)
- @lucasew made their first contribution in [#7014](https://github.com/jdx/mise/pull/7014)

## [2025.11.7](https://github.com/jdx/mise/compare/v2025.11.6..v2025.11.7) - 2025-11-20

### 🚀 Features

- **(exec)** ensure MISE_ENV is set in spawned shell when specified via -E flag by @ceelian in [#7007](https://github.com/jdx/mise/pull/7007)

### 🐛 Bug Fixes

- **(fig)** resolve __dirname error in ES module by @jdx in [#7021](https://github.com/jdx/mise/pull/7021)
- **(go)** Don't allow auto mod=vendor mode by @mariduv in [#7006](https://github.com/jdx/mise/pull/7006)
- **(nushell)** test `use` not `source`, fix pipeline parse error by @jokeyrhyme in [#7013](https://github.com/jdx/mise/pull/7013)
- **(tasks)** make file paths relative to config location and templateable by @halms in [#7005](https://github.com/jdx/mise/pull/7005)

### 📦 Registry

- added nelm by @tony-sol in [#7020](https://github.com/jdx/mise/pull/7020)

### Chore

- **(deny)** add exclusion for number_prefix by @jdx in [e955ecb](https://github.com/jdx/mise/commit/e955ecbb733d61ef1d0b522a979a7d1998ec8061)

### New Contributors

- @mariduv made their first contribution in [#7006](https://github.com/jdx/mise/pull/7006)
- @ceelian made their first contribution in [#7007](https://github.com/jdx/mise/pull/7007)

## [2025.11.6](https://github.com/jdx/mise/compare/v2025.11.5..v2025.11.6) - 2025-11-18

### 🐛 Bug Fixes

- **(nushell)** add missing `| parse env | update-env` for deactivation operations by @jokeyrhyme in [#6994](https://github.com/jdx/mise/pull/6994)
- **(pwsh)** wrap the executable path with double quotes by @leosuncin in [#6993](https://github.com/jdx/mise/pull/6993)
- in `activate bash` output, wrap mise executable path in single-quotes by @cspotcode in [#7002](https://github.com/jdx/mise/pull/7002)
- On Windows, preserve/proxy the exit code of tools, to match behavior on Unix by @cspotcode in [#7001](https://github.com/jdx/mise/pull/7001)

### 📚 Documentation

- simplify apt instructions by @scop in [#6986](https://github.com/jdx/mise/pull/6986)
- update idiomatic version files enablement info by @scop in [#6985](https://github.com/jdx/mise/pull/6985)
- registry notability explanation by @jdx in [8f9ab15](https://github.com/jdx/mise/commit/8f9ab15e18d8cf0983d08a1f14b04511c999d681)

### 🧪 Testing

- **(aqua)** remove biome test due to version incompatibility by @jdx in [#7000](https://github.com/jdx/mise/pull/7000)

### 📦️ Dependency Updates

- lock file maintenance by @renovate[bot] in [#6997](https://github.com/jdx/mise/pull/6997)

### 📦 Registry

- add tbls by @artemklevtsov in [#6987](https://github.com/jdx/mise/pull/6987)
- add kubeswitch tool and add test for ruff by @jylenhof in [#6990](https://github.com/jdx/mise/pull/6990)

### New Contributors

- @cspotcode made their first contribution in [#7001](https://github.com/jdx/mise/pull/7001)
- @jokeyrhyme made their first contribution in [#6994](https://github.com/jdx/mise/pull/6994)
- @artemklevtsov made their first contribution in [#6987](https://github.com/jdx/mise/pull/6987)
- @leosuncin made their first contribution in [#6993](https://github.com/jdx/mise/pull/6993)

## [2025.11.5](https://github.com/jdx/mise/compare/v2025.11.4..v2025.11.5) - 2025-11-15

### 🚀 Features

- **(http)** Add 'format' to http backend by @thejcannon in [#6957](https://github.com/jdx/mise/pull/6957)

### 🐛 Bug Fixes

- **(bootstrap)** wrong directory on first run by @vmeurisse in [#6971](https://github.com/jdx/mise/pull/6971)
- **(tasks)** fix nested colons with `mise task edit` by @jdx in [#6978](https://github.com/jdx/mise/pull/6978)
- Use compatible env flags by @thejcannon in [#6964](https://github.com/jdx/mise/pull/6964)
- Flush vfox download buffer by @blampe in [#6969](https://github.com/jdx/mise/pull/6969)

### 📚 Documentation

- `arch()` template is `x64` by @thejcannon in [#6967](https://github.com/jdx/mise/pull/6967)
- update section headers in getting-started.md by @JunichiroKohari in [#6980](https://github.com/jdx/mise/pull/6980)

### New Contributors

- @JunichiroKohari made their first contribution in [#6980](https://github.com/jdx/mise/pull/6980)
- @blampe made their first contribution in [#6969](https://github.com/jdx/mise/pull/6969)
- @thejcannon made their first contribution in [#6964](https://github.com/jdx/mise/pull/6964)

## [2025.11.4](https://github.com/jdx/mise/compare/v2025.11.3..v2025.11.4) - 2025-11-13

### 🚀 Features

- **(gem-backend)** use gem command for backend operations by @andrewthauer in [#6650](https://github.com/jdx/mise/pull/6650)
- **(tasks)** add `mise task validate` command for task validation by @jdx in [#6958](https://github.com/jdx/mise/pull/6958)
- Add `--skip-deps` flag to run specified tasks, skipping dependencies by @hverlin in [#6894](https://github.com/jdx/mise/pull/6894)

### 🐛 Bug Fixes

- **(cli)** intercept --help flag to show task help instead of executing task by @jdx in [#6955](https://github.com/jdx/mise/pull/6955)
- **(cli)** handle `mise help` without requiring tasks by @jdx in [#6961](https://github.com/jdx/mise/pull/6961)
- **(pwsh)** remove __MISE_DIFF env var instead of __MISE_WATCH on deactivate by @IMXEren in [#6886](https://github.com/jdx/mise/pull/6886)
- remove temporary files after install by @vmeurisse in [#6948](https://github.com/jdx/mise/pull/6948)

### 📚 Documentation

- **(snapcraft)** update `summary` & `description` shown in snapcraft.io by @phanect in [#6926](https://github.com/jdx/mise/pull/6926)
- Change package example in go.md by @nachtjasmin in [#6862](https://github.com/jdx/mise/pull/6862)
- paranoid mode does not untrust global config by @iloveitaly in [#6952](https://github.com/jdx/mise/pull/6952)

### 📦️ Dependency Updates

- lock file maintenance by @renovate[bot] in [#6932](https://github.com/jdx/mise/pull/6932)

### 📦 Registry

- add xcsift by @alexey1312 in [#6923](https://github.com/jdx/mise/pull/6923)
- add tools: magika & xxh by @IceCodeNew in [#6909](https://github.com/jdx/mise/pull/6909)
- add aliases to aqua-backend tools by @IceCodeNew in [#6910](https://github.com/jdx/mise/pull/6910)

### Chore

- bump cargo deps by @jdx in [#6960](https://github.com/jdx/mise/pull/6960)

### New Contributors

- @iloveitaly made their first contribution in [#6952](https://github.com/jdx/mise/pull/6952)
- @nachtjasmin made their first contribution in [#6862](https://github.com/jdx/mise/pull/6862)
- @IceCodeNew made their first contribution in [#6910](https://github.com/jdx/mise/pull/6910)
- @alexey1312 made their first contribution in [#6923](https://github.com/jdx/mise/pull/6923)

## [2025.11.3](https://github.com/jdx/mise/compare/v2025.11.2..v2025.11.3) - 2025-11-07

### 🚀 Features

- **(aqua)** support `Asset` template for cosign and slsa verification by @risu729 in [#6875](https://github.com/jdx/mise/pull/6875)
- improve task info support with experimental_monorepo_root by @hverlin in [#6881](https://github.com/jdx/mise/pull/6881)

### 🐛 Bug Fixes

- **(clippy)** resolve comparison and derivable impl warnings by @jdx in [#6924](https://github.com/jdx/mise/pull/6924)
- **(config)** add `mise/config.local.toml` to config paths by @risu729 in [#6882](https://github.com/jdx/mise/pull/6882)
- **(java)** unable to install JDKs of release type EA by @roele in [#6907](https://github.com/jdx/mise/pull/6907)
- interactive task selection when monorepo tasks are enabled by @halms in [#6891](https://github.com/jdx/mise/pull/6891)

### 📚 Documentation

- **(security)** use long-form GPG key fingerprint in installation docs by @jdx in [#6885](https://github.com/jdx/mise/pull/6885)

### 📦 Registry

- rename yt-dlp bin by @risu729 in [#6883](https://github.com/jdx/mise/pull/6883)
- use aqua backend for slsa-verifier by @risu729 in [#6872](https://github.com/jdx/mise/pull/6872)
- added devcontainer-cli by @moisesmorillo in [#6888](https://github.com/jdx/mise/pull/6888)
- add amazon-ecs-cli by @ducvuongpham in [#6898](https://github.com/jdx/mise/pull/6898)
- add helm-ls by @ldrouard in [#6899](https://github.com/jdx/mise/pull/6899)
- add ubi backend and test for oxipng, change aqua backend by @ldrouard in [#6900](https://github.com/jdx/mise/pull/6900)

### Chore

- update Java LTS to 25 by @sargunv in [#6897](https://github.com/jdx/mise/pull/6897)

### New Contributors

- @halms made their first contribution in [#6891](https://github.com/jdx/mise/pull/6891)
- @sargunv made their first contribution in [#6897](https://github.com/jdx/mise/pull/6897)
- @ducvuongpham made their first contribution in [#6898](https://github.com/jdx/mise/pull/6898)

## [2025.11.2](https://github.com/jdx/mise/compare/v2025.11.1..v2025.11.2) - 2025-11-03

### 🚀 Features

- **(cli)** switch manpage generation from clap_mangen to usage by @jdx in [#6868](https://github.com/jdx/mise/pull/6868)
- **(task)** add selective stream suppression for silent configuration by @jdx in [#6851](https://github.com/jdx/mise/pull/6851)

### 🐛 Bug Fixes

- **(backend)** support platform-specific bin and bin_path configuration by @dragoscirjan in [#6853](https://github.com/jdx/mise/pull/6853)
- **(generate)** update git pre-commit script to use null separator by @azais-corentin in [#6874](https://github.com/jdx/mise/pull/6874)
- **(stubs)** lookup for aqua tools stubs fails because of tool options by @roele in [#6867](https://github.com/jdx/mise/pull/6867)
- **(task)** resolve aliases correctly for monorepo tasks by @jdx in [#6857](https://github.com/jdx/mise/pull/6857)
- **(task)** prevent MISE_TASK_OUTPUT from propagating to nested tasks by @jdx in [#6860](https://github.com/jdx/mise/pull/6860)
- **(tasks)** simplify task command display to show only first line by @jdx in [#6863](https://github.com/jdx/mise/pull/6863)
- **(tasks)** implement smart flag routing for task arguments by @jdx in [#6861](https://github.com/jdx/mise/pull/6861)
- **(xonsh)** prevent KeyError when activating in nested shells by @jdx in [#6856](https://github.com/jdx/mise/pull/6856)
- Don't set empty env var if decryption fails with age.strict=false by @iamkroot in [#6847](https://github.com/jdx/mise/pull/6847)

### 🚜 Refactor

- **(task)** split run.rs into modular task execution pipeline by @jdx in [#6852](https://github.com/jdx/mise/pull/6852)

### 📚 Documentation

- **(cli)** integrate clap-sort to validate subcommand ordering by @jdx in [#6865](https://github.com/jdx/mise/pull/6865)

### 📦️ Dependency Updates

- lock file maintenance by @renovate[bot] in [#6873](https://github.com/jdx/mise/pull/6873)

### 📦 Registry

- rename mise-haskell -> asdf-haskell by @jdx in [#6859](https://github.com/jdx/mise/pull/6859)

### New Contributors

- @azais-corentin made their first contribution in [#6874](https://github.com/jdx/mise/pull/6874)
- @dragoscirjan made their first contribution in [#6853](https://github.com/jdx/mise/pull/6853)

## [2025.11.1](https://github.com/jdx/mise/compare/v2025.11.0..v2025.11.1) - 2025-11-01

### 🚀 Features

- **(age)** add strict mode for non-strict decryption mode by @iamkroot in [#6838](https://github.com/jdx/mise/pull/6838)
- **(vfox)** add support for specifying attestation metadata in the preinstall return value by @malept in [#6839](https://github.com/jdx/mise/pull/6839)

### 🐛 Bug Fixes

- **(activate)** prevent hash table errors during deactivation by @jdx in [#6846](https://github.com/jdx/mise/pull/6846)
- **(install)** error on non-existent tools in `mise install` by @jdx in [#6844](https://github.com/jdx/mise/pull/6844)

### 📦 Registry

- Disable libsql-server on Windows by @jayvdb in [#6837](https://github.com/jdx/mise/pull/6837)
- add infisical by @jdx in [#6845](https://github.com/jdx/mise/pull/6845)

## [2025.11.0](https://github.com/jdx/mise/compare/v2025.10.21..v2025.11.0) - 2025-11-01

### 🐛 Bug Fixes

- **(activate)** reset PATH when activate is called multiple times by @jdx in [#6829](https://github.com/jdx/mise/pull/6829)
- **(env)** preserve user-configured PATH entries from env._.path by @jdx in [#6835](https://github.com/jdx/mise/pull/6835)
- store tool options for all backends in metadata by @roele in [#6807](https://github.com/jdx/mise/pull/6807)

### 📚 Documentation

- fix usage spec syntax from 'option' to 'flag' by @jdx in [#6834](https://github.com/jdx/mise/pull/6834)

### 📦️ Dependency Updates

- update ghcr.io/jdx/mise:alpine docker digest to 7351bbe by @renovate[bot] in [#6826](https://github.com/jdx/mise/pull/6826)
- update ghcr.io/jdx/mise:deb docker digest to 3a847f2 by @renovate[bot] in [#6828](https://github.com/jdx/mise/pull/6828)
- update ghcr.io/jdx/mise:copr docker digest to 546dffb by @renovate[bot] in [#6827](https://github.com/jdx/mise/pull/6827)
- pin jdx/mise-action action to e3d7b8d by @renovate[bot] in [#6825](https://github.com/jdx/mise/pull/6825)

## [2025.10.21](https://github.com/jdx/mise/compare/v2025.10.20..v2025.10.21) - 2025-10-30

### 🐛 Bug Fixes

- **(cli)** show friendly error when --cd path does not exist by @jdx in [#6818](https://github.com/jdx/mise/pull/6818)
- **(env)** prevent PATH corruption when paths are interleaved with original PATH by @jdx in [#6821](https://github.com/jdx/mise/pull/6821)
- **(node)** update lts version by @risu729 in [#6816](https://github.com/jdx/mise/pull/6816)
- **(schema,settings)** update type and descriptions for shell argument settings by @astrochemx in [#6805](https://github.com/jdx/mise/pull/6805)

### Chore

- update kerl to 4.4.0 by @rbino in [#6809](https://github.com/jdx/mise/pull/6809)

### New Contributors

- @astrochemx made their first contribution in [#6805](https://github.com/jdx/mise/pull/6805)
- @rbino made their first contribution in [#6809](https://github.com/jdx/mise/pull/6809)

## [2025.10.20](https://github.com/jdx/mise/compare/v2025.10.19..v2025.10.20) - 2025-10-29

### 🚀 Features

- Add MSVC asset matching on Windows by @trolleyman in [#6798](https://github.com/jdx/mise/pull/6798)

### 🐛 Bug Fixes

- **(cache)** exclude http backend tarballs from autoprune by @jdx in [#6806](https://github.com/jdx/mise/pull/6806)
- **(ci)** prevent release job from running when dependencies fail by @jdx in [#6804](https://github.com/jdx/mise/pull/6804)
- **(fish)** remove --move flag from fish_add_path to prevent PATH corruption by @jdx in [#6800](https://github.com/jdx/mise/pull/6800)
- **(tasks)** support local .config/mise/conf.d/*.toml tasks by @syhol in [#6792](https://github.com/jdx/mise/pull/6792)

### 📚 Documentation

- change 'claude-code' to 'claude' in examples by @bradleybuda in [#6801](https://github.com/jdx/mise/pull/6801)

### 📦 Registry

- add cpz and rmz by @sassdavid in [#6793](https://github.com/jdx/mise/pull/6793)

### New Contributors

- @trolleyman made their first contribution in [#6798](https://github.com/jdx/mise/pull/6798)
- @bradleybuda made their first contribution in [#6801](https://github.com/jdx/mise/pull/6801)

## [2025.10.19](https://github.com/jdx/mise/compare/v2025.10.18..v2025.10.19) - 2025-10-28

### 🚀 Features

- **(zig)** Download zig tarballs from vetted community mirrors when available. by @Maarrk in [#6670](https://github.com/jdx/mise/pull/6670)

### 🐛 Bug Fixes

- **(config)** respect auto_install=false for all installation contexts by @jdx in [#6788](https://github.com/jdx/mise/pull/6788)
- **(plugins)** incorrect tool versions installed for custom plugins by @roele in [#6765](https://github.com/jdx/mise/pull/6765)
- **(reqwest)** enable socks for self-update by @tony-sol in [#6775](https://github.com/jdx/mise/pull/6775)

### 📚 Documentation

- **(task)** Fix task flag definitions and examples by @syhol in [#6790](https://github.com/jdx/mise/pull/6790)
- **(task-arguments)** adds `# [USAGE]` syntax by @risu729 in [#6768](https://github.com/jdx/mise/pull/6768)
- enhance task documentation with syntax highlighting and corrections by @jdx in [#6777](https://github.com/jdx/mise/pull/6777)
- use triple single quotes for multiline run commands by @jdx in [#6791](https://github.com/jdx/mise/pull/6791)

### 🧪 Testing

- **(perf)** add warmup calls for benchmarks to fix incorrect numbers by @jdx in [#6789](https://github.com/jdx/mise/pull/6789)

### 📦️ Dependency Updates

- lock file maintenance by @renovate[bot] in [#6780](https://github.com/jdx/mise/pull/6780)

### 📦 Registry

- update bat-extras backends by @TyceHerrman in [#6784](https://github.com/jdx/mise/pull/6784)

## [2025.10.18](https://github.com/jdx/mise/compare/v2025.10.17..v2025.10.18) - 2025-10-25

### 🚀 Features

- **(task)** make leading colon optional for monorepo task references by @jdx in [#6763](https://github.com/jdx/mise/pull/6763)

### 🐛 Bug Fixes

- **(task)** resolve monorepo task dependencies with colons in task names by @jdx in [#6761](https://github.com/jdx/mise/pull/6761)
- Add clang and libs to nix nativeBuildInputs by @laozc in [#6760](https://github.com/jdx/mise/pull/6760)

### 📚 Documentation

- **(task)** deprecate Tera template functions for task arguments by @jdx in [#6764](https://github.com/jdx/mise/pull/6764)

## [2025.10.17](https://github.com/jdx/mise/compare/v2025.10.16..v2025.10.17) - 2025-10-24

### 🚀 Features

- **(plugins)** Implement missing `file.exists()` Lua function by @ofalvai in [#6754](https://github.com/jdx/mise/pull/6754)
- **(tasks)** Make tera templates available in usage by @iamkroot in [#6747](https://github.com/jdx/mise/pull/6747)
- use custom api_url for asset downloading in GHES setups by @talbx in [#6730](https://github.com/jdx/mise/pull/6730)

### 🐛 Bug Fixes

- **(env)** prioritize _.path after external PATH modifications by @jdx in [#6755](https://github.com/jdx/mise/pull/6755)
- incorrect task arguments with spaces on Windows by @nickbabcock in [#6744](https://github.com/jdx/mise/pull/6744)

### 📚 Documentation

- Add example of configuring tools in a file tasks by @richardthe3rd in [#6719](https://github.com/jdx/mise/pull/6719)
- Add NixOS tip about source compilation to install docs by @richardgill in [#6757](https://github.com/jdx/mise/pull/6757)

### ◀️ Revert

- fix(shell): prevent infinite loop in zsh command-not-found handler by @jdx in [#6758](https://github.com/jdx/mise/pull/6758)

### 📦️ Dependency Updates

- update ghcr.io/jdx/mise:copr docker digest to 7f6aee5 by @renovate[bot] in [#6750](https://github.com/jdx/mise/pull/6750)
- update ghcr.io/jdx/mise:alpine docker digest to f749e46 by @renovate[bot] in [#6749](https://github.com/jdx/mise/pull/6749)
- update ghcr.io/jdx/mise:rpm docker digest to 308b042 by @renovate[bot] in [#6752](https://github.com/jdx/mise/pull/6752)
- update ghcr.io/jdx/mise:deb docker digest to e28b4fd by @renovate[bot] in [#6751](https://github.com/jdx/mise/pull/6751)

### 📦 Registry

- add superhtml by @Maarrk in [#6742](https://github.com/jdx/mise/pull/6742)
- add opengrep by @vmeurisse in [#6745](https://github.com/jdx/mise/pull/6745)

### New Contributors

- @richardgill made their first contribution in [#6757](https://github.com/jdx/mise/pull/6757)
- @nickbabcock made their first contribution in [#6744](https://github.com/jdx/mise/pull/6744)
- @vmeurisse made their first contribution in [#6745](https://github.com/jdx/mise/pull/6745)
- @talbx made their first contribution in [#6730](https://github.com/jdx/mise/pull/6730)
- @Maarrk made their first contribution in [#6742](https://github.com/jdx/mise/pull/6742)

## [2025.10.16](https://github.com/jdx/mise/compare/v2025.10.15..v2025.10.16) - 2025-10-23

### 🚀 Features

- **(tasks)** modify usage spec parsing to return dummy strings by @iamkroot in [#6723](https://github.com/jdx/mise/pull/6723)
- include resolved sources in task templating context by @the-wondersmith in [#6180](https://github.com/jdx/mise/pull/6180)
- Add Tera function `absolute` by @iamkroot in [#6729](https://github.com/jdx/mise/pull/6729)

### 🐛 Bug Fixes

- **(cli)** respect os filter during upgrade by @iamkroot in [#6724](https://github.com/jdx/mise/pull/6724)

### 📚 Documentation

- fix RUNTIME.osType values in example snippet by @ofalvai in [#6732](https://github.com/jdx/mise/pull/6732)
- migrate issue links to GitHub discussions by @jdx in [#6740](https://github.com/jdx/mise/pull/6740)
- document Lua version by @ofalvai in [#6741](https://github.com/jdx/mise/pull/6741)

### New Contributors

- @ofalvai made their first contribution in [#6741](https://github.com/jdx/mise/pull/6741)
- @iamkroot made their first contribution in [#6729](https://github.com/jdx/mise/pull/6729)
- @the-wondersmith made their first contribution in [#6180](https://github.com/jdx/mise/pull/6180)

## [2025.10.15](https://github.com/jdx/mise/compare/v2025.10.14..v2025.10.15) - 2025-10-22

### 🚀 Features

- **(aqua)** use GitHub API digests for release asset checksums by @jdx in [#6720](https://github.com/jdx/mise/pull/6720)
- **(github)** use GitHub API digests for release asset checksums by @jdx in [#6721](https://github.com/jdx/mise/pull/6721)
- **(plugins)** automatically install backend plugins by @roele in [#6696](https://github.com/jdx/mise/pull/6696)
- **(tasks)** add choices to flag() and enable naked runs with task flags by @jdx in [#6707](https://github.com/jdx/mise/pull/6707)

### 🐛 Bug Fixes

- **(config)** show trust error instead of silently skipping untrusted configs by @jdx in [#6715](https://github.com/jdx/mise/pull/6715)
- **(env)** handle non-ASCII environment variables gracefully by @arnodirlam in [#6708](https://github.com/jdx/mise/pull/6708)
- **(nix)** add cmakeMinimal to nativeBuildInputs by @okuuva in [#6691](https://github.com/jdx/mise/pull/6691)
- **(tasks)** load project env vars for global tasks with dir="{{cwd}}" by @jdx in [#6717](https://github.com/jdx/mise/pull/6717)

### 📦️ Dependency Updates

- update gh to latest (2.82.1) by @jdx in [#6718](https://github.com/jdx/mise/pull/6718)

### New Contributors

- @arnodirlam made their first contribution in [#6708](https://github.com/jdx/mise/pull/6708)

## [2025.10.14](https://github.com/jdx/mise/compare/v2025.10.13..v2025.10.14) - 2025-10-21

### 🚀 Features

- **(tasks)** add env var support for args/flags in usage specs by @jdx in [#6704](https://github.com/jdx/mise/pull/6704)

### 🐛 Bug Fixes

- **(release)** prevent S3 rate limiting errors during CDN upload by @jdx in [#6705](https://github.com/jdx/mise/pull/6705)

### 📚 Documentation

- add comprehensive documentation for environment plugins by @jdx in [#6702](https://github.com/jdx/mise/pull/6702)

### 📦️ Dependency Updates

- bump mlua from 0.11.0-beta.3 to 0.11 by @jdx in [#6701](https://github.com/jdx/mise/pull/6701)

## [2025.10.13](https://github.com/jdx/mise/compare/v2025.10.12..v2025.10.13) - 2025-10-21

### 🐛 Bug Fixes

- **(revert)** fix(deps): update rust crate ubi to 0.8.2 by @nekrich in [#6700](https://github.com/jdx/mise/pull/6700)

### 📚 Documentation

- Add fnox as recommended secret management option by @jdx in [#6698](https://github.com/jdx/mise/pull/6698)

### New Contributors

- @nekrich made their first contribution in [#6700](https://github.com/jdx/mise/pull/6700)

## [2025.10.12](https://github.com/jdx/mise/compare/v2025.10.11..v2025.10.12) - 2025-10-20

### 🐛 Bug Fixes

- **(rust)** preserve original PATH entries when managing tool paths by @jdx in [#6689](https://github.com/jdx/mise/pull/6689)

### 📦️ Dependency Updates

- update rust crate ubi to 0.8.2 by @risu729 in [#6693](https://github.com/jdx/mise/pull/6693)

## [2025.10.11](https://github.com/jdx/mise/compare/v2025.10.10..v2025.10.11) - 2025-10-18

### 🚀 Features

- remove experimental labels from stable features by @jdx in [#6684](https://github.com/jdx/mise/pull/6684)

### 🐛 Bug Fixes

- **(tasks)** resolve :task patterns in run blocks for monorepo tasks by @LER0ever in [#6682](https://github.com/jdx/mise/pull/6682)

### 📚 Documentation

- Fix typo in comparison-to-asdf.md by @TobiX in [#6677](https://github.com/jdx/mise/pull/6677)

### 📦️ Dependency Updates

- update docker/dockerfile:1 docker digest to b6afd42 by @renovate[bot] in [#6675](https://github.com/jdx/mise/pull/6675)
- update fedora:43 docker digest to 2ad3073 by @renovate[bot] in [#6676](https://github.com/jdx/mise/pull/6676)

### New Contributors

- @LER0ever made their first contribution in [#6682](https://github.com/jdx/mise/pull/6682)

## [2025.10.10](https://github.com/jdx/mise/compare/v2025.10.9..v2025.10.10) - 2025-10-16

### 🐛 Bug Fixes

- **(backend)** sync parent directory after removing incomplete marker by @EronWright in [#6668](https://github.com/jdx/mise/pull/6668)
- **(tasks)** improve error message for untrusted config files by @jdx in [#6672](https://github.com/jdx/mise/pull/6672)
- fix(deps) Revert "fix(deps): update rust crate ubi to 0.8 " by @swgillespie in [#6652](https://github.com/jdx/mise/pull/6652)

### New Contributors

- @swgillespie made their first contribution in [#6652](https://github.com/jdx/mise/pull/6652)
- @EronWright made their first contribution in [#6668](https://github.com/jdx/mise/pull/6668)

## [2025.10.9](https://github.com/jdx/mise/compare/v2025.10.8..v2025.10.9) - 2025-10-15

### 🐛 Bug Fixes

- **(docs)** add missing config file path by @haellsigh in [#6658](https://github.com/jdx/mise/pull/6658)
- **(task)** resolve monorepo dependency chains with local task references by @jdx in [#6665](https://github.com/jdx/mise/pull/6665)
- **(ui)** add terminal detection for OSC 9;4 progress sequences by @jdx in [#6657](https://github.com/jdx/mise/pull/6657)

### 📚 Documentation

- fix aqua package info in CHANGELOG.md by @jdx in [#6664](https://github.com/jdx/mise/pull/6664)

### New Contributors

- @haellsigh made their first contribution in [#6658](https://github.com/jdx/mise/pull/6658)

## [2025.10.8](https://github.com/jdx/mise/compare/v2025.10.7..v2025.10.8) - 2025-10-13

### 🚀 Features

- **(plugins)** more archiver extensions by @blaubaer in [#6644](https://github.com/jdx/mise/pull/6644)

### 🐛 Bug Fixes

- **(cli)** make `mise //foo` equivalent to `mise run //foo` by @neongreen in [#6641](https://github.com/jdx/mise/pull/6641)
- **(config)** load MISE_ENV configs for monorepo tasks by @jdx in [#6624](https://github.com/jdx/mise/pull/6624)
- improve ... pattern matching for monorepo tasks by @neongreen in [#6635](https://github.com/jdx/mise/pull/6635)

### 🛡️ Security

- **(security)** use HTTPS instead of HTTP for version hosts by @jdx in [#6638](https://github.com/jdx/mise/pull/6638)

### 📦️ Dependency Updates

- update rust crate ubi to 0.8 by @risu729 in [#6637](https://github.com/jdx/mise/pull/6637)

### 📦 Registry

- add codex (`npm:@openai/codex`) by @risu729 in [#6634](https://github.com/jdx/mise/pull/6634)
- add tests (1password-certstrap) by @risu729 in [#6592](https://github.com/jdx/mise/pull/6592)

### New Contributors

- @neongreen made their first contribution in [#6641](https://github.com/jdx/mise/pull/6641)

## [2025.10.7](https://github.com/jdx/mise/compare/v2025.10.6..v2025.10.7) - 2025-10-10

### 🚀 Features

- **(config)** Add a ceiling to how mise searchs for config & tasks by @richardthe3rd in [#6041](https://github.com/jdx/mise/pull/6041)
- **(release)** include aqua-registry updates in changelog and release notes by @jdx in [#6623](https://github.com/jdx/mise/pull/6623)

### 🐛 Bug Fixes

- **(task)** use config_root instead of project_root for task base path by @risu729 in [#6609](https://github.com/jdx/mise/pull/6609)
- **(task)** resolve project tasks in run blocks using TaskLoadContext by @jdx in [#6614](https://github.com/jdx/mise/pull/6614)
- **(task)** dont truncate task message when CI is set by @roele in [#6620](https://github.com/jdx/mise/pull/6620)
- **(tasks)** restore MISE_ENV environment inheritance for tasks by @glasser in [#6621](https://github.com/jdx/mise/pull/6621)
- **(ui)** prevent OSC 9;4 progress sequences from being written to non-terminals by @jdx in [#6615](https://github.com/jdx/mise/pull/6615)

### 📦 Registry

- add lazyssh by @TyceHerrman in [#6610](https://github.com/jdx/mise/pull/6610)

### Chore

- remove cosign/slsa-verifier from mise.toml by @jdx in [#6616](https://github.com/jdx/mise/pull/6616)

### New Contributors

- @richardthe3rd made their first contribution in [#6041](https://github.com/jdx/mise/pull/6041)

## [2025.10.6](https://github.com/jdx/mise/compare/v2025.10.5..v2025.10.6) - 2025-10-08

### 🚀 Features

- add OSC 9;4 terminal progress indicators by @jdx in [#6584](https://github.com/jdx/mise/pull/6584)
- make progress bar a footer by @jdx in [#6590](https://github.com/jdx/mise/pull/6590)

### 🐛 Bug Fixes

- **(task)** preserve ubi tool options during auto-install by @jdx in [#6600](https://github.com/jdx/mise/pull/6600)
- unify project_root and config_root resolution by @risu729 in [#6588](https://github.com/jdx/mise/pull/6588)

### 🚜 Refactor

- **(exec)** remove redundant tty check for auto-install by @jdx in [#6589](https://github.com/jdx/mise/pull/6589)
- remove duplicated task loads by @risu729 in [#6594](https://github.com/jdx/mise/pull/6594)

### 📦 Registry

- add vfox-mongod by @blaubaer in [#6586](https://github.com/jdx/mise/pull/6586)

### New Contributors

- @blaubaer made their first contribution in [#6586](https://github.com/jdx/mise/pull/6586)

## [2025.10.5](https://github.com/jdx/mise/compare/v2025.10.4..v2025.10.5) - 2025-10-07

### 🐛 Bug Fixes

- **(docs)** improve favicon support for Safari by @jdx in [#6567](https://github.com/jdx/mise/pull/6567)
- **(github)** download assets via API to respect GITHUB_TOKEN by @roele in [#6496](https://github.com/jdx/mise/pull/6496)
- **(task)** load toml tasks in `task_config.includes` in system/global config and monorepo subdirs by @risu729 in [#6545](https://github.com/jdx/mise/pull/6545)
- **(task)** handle dots in monorepo directory names correctly by @jdx in [#6571](https://github.com/jdx/mise/pull/6571)

### 📚 Documentation

- **(readme)** add GitHub Issues & Discussions section by @rsyring in [#6573](https://github.com/jdx/mise/pull/6573)
- **(tasks)** create dedicated monorepo tasks documentation by @jdx in [#6561](https://github.com/jdx/mise/pull/6561)
- **(tasks)** enhance monorepo documentation with tool comparisons by @jdx in [#6563](https://github.com/jdx/mise/pull/6563)

### 📦 Registry

- add jules by @alefteris in [#6568](https://github.com/jdx/mise/pull/6568)

## [2025.10.4](https://github.com/jdx/mise/compare/v2025.10.3..v2025.10.4) - 2025-10-06

### 🐛 Bug Fixes

- **(installing-mise.md)** broken link by @equirosa in [#6555](https://github.com/jdx/mise/pull/6555)
- **(task)** remote git tasks now properly inherit tools from parent configs by @jdx in [#6558](https://github.com/jdx/mise/pull/6558)
- **(tasks)** restore tool loading from idiomatic version files by @jdx in [#6559](https://github.com/jdx/mise/pull/6559)

### 🚜 Refactor

- **(task)** remove duplicated codes by @risu729 in [#6553](https://github.com/jdx/mise/pull/6553)

### New Contributors

- @equirosa made their first contribution in [#6555](https://github.com/jdx/mise/pull/6555)

## [2025.10.3](https://github.com/jdx/mise/compare/v2025.10.2..v2025.10.3) - 2025-10-06

### 🚀 Features

- **(tasks)** add experimental monorepo task support with target paths by @jdx in [#6535](https://github.com/jdx/mise/pull/6535)
- **(tasks)** respect local config_roots for monorepo tasks by @jdx in [#6552](https://github.com/jdx/mise/pull/6552)
- support latest suffix for Java, Python and Ruby flavoured versions by @roele in [#6533](https://github.com/jdx/mise/pull/6533)

### 🐛 Bug Fixes

- **(aqua)** decode filename extracted from url by @risu729 in [#6536](https://github.com/jdx/mise/pull/6536)
- **(snapcraft)** use classic confinement by @phanect in [#6542](https://github.com/jdx/mise/pull/6542)
- **(task)** fix task pattern matching and add :task syntax for monorepos by @risu729 in [#6549](https://github.com/jdx/mise/pull/6549)
- **(tasks)** validate monorepo setup before running monorepo tasks by @jdx in [#6551](https://github.com/jdx/mise/pull/6551)
- Add bash option in example by @Its-Just-Nans in [#6541](https://github.com/jdx/mise/pull/6541)
- suppress ignore crate logs by @risu729 in [#6547](https://github.com/jdx/mise/pull/6547)

### 📚 Documentation

- Update Python virtual environment documentation by @Konfekt in [#6538](https://github.com/jdx/mise/pull/6538)

### 📦 Registry

- added cloudflare wrangler by @moisesmorillo in [#6534](https://github.com/jdx/mise/pull/6534)

### Chore

- **(hk)** bump to v1.18.1 by @jdx in [#6546](https://github.com/jdx/mise/pull/6546)

### Hk

- bump to 1.18.1 by @jdx in [0ab65cd](https://github.com/jdx/mise/commit/0ab65cd9c6827fd4738e5184be6d743f94be34b2)

### New Contributors

- @Konfekt made their first contribution in [#6538](https://github.com/jdx/mise/pull/6538)
- @moisesmorillo made their first contribution in [#6534](https://github.com/jdx/mise/pull/6534)

## [2025.10.2](https://github.com/jdx/mise/compare/v2025.10.1..v2025.10.2) - 2025-10-03

### 🐛 Bug Fixes

- **(shell)** prevent infinite loop in zsh command-not-found handler by @yordis in [#6516](https://github.com/jdx/mise/pull/6516)
- **(snapcraft)** add missing home plug for the home directory access permission by @phanect in [#6525](https://github.com/jdx/mise/pull/6525)
- **(vfox)** implement headers support on http mod by @BasixKOR in [#6521](https://github.com/jdx/mise/pull/6521)
- set MIX_HOME and MIX_ARCHIVES when using the elixir plugin by @numso in [#6504](https://github.com/jdx/mise/pull/6504)

### 📦️ Dependency Updates

- update docker/login-action digest to 5e57cd1 by @renovate[bot] in [#6522](https://github.com/jdx/mise/pull/6522)
- update fedora:43 docker digest to 2c0d72b by @renovate[bot] in [#6523](https://github.com/jdx/mise/pull/6523)

### Security

- verify macOS binary signature during self-update by @jdx in [#6528](https://github.com/jdx/mise/pull/6528)

### New Contributors

- @yordis made their first contribution in [#6516](https://github.com/jdx/mise/pull/6516)
- @numso made their first contribution in [#6504](https://github.com/jdx/mise/pull/6504)
- @BasixKOR made their first contribution in [#6521](https://github.com/jdx/mise/pull/6521)

## [2025.10.1](https://github.com/jdx/mise/compare/v2025.10.0..v2025.10.1) - 2025-10-03

### 🚀 Features

- **(snapcraft)** add snap package by @phanect in [#6472](https://github.com/jdx/mise/pull/6472)

### 🐛 Bug Fixes

- **(cache)** remove duplicate bytes in prune output by @risu729 in [#6515](https://github.com/jdx/mise/pull/6515)

### 📦 Registry

- add tombi by @TyceHerrman in [#6520](https://github.com/jdx/mise/pull/6520)

### Chore

- **(copr)** increase COPR publish timeout by 60 minutes by @Copilot in [#6512](https://github.com/jdx/mise/pull/6512)

### New Contributors

- @phanect made their first contribution in [#6472](https://github.com/jdx/mise/pull/6472)

## [2025.10.0](https://github.com/jdx/mise/compare/v2025.9.25..v2025.10.0) - 2025-10-01

### 🚀 Features

- change idiomatic_version_file to default disabled by @jdx in [#6501](https://github.com/jdx/mise/pull/6501)

### 🐛 Bug Fixes

- **(self-update)** add missing functions to self_update stub by @jdx in [#6502](https://github.com/jdx/mise/pull/6502)
- **(set)** allow --prompt flag to work with `mise set` by @jdx in [#6485](https://github.com/jdx/mise/pull/6485)

### 📚 Documentation

- **(hooks)** clarify pre/post-install hooks description. by @minusfive in [#6497](https://github.com/jdx/mise/pull/6497)
- remove link to issue by @jdx in [e13d980](https://github.com/jdx/mise/commit/e13d98012fda05e5032b7dfc18f562c28f140cf9)

### 🧪 Testing

- **(e2e)** remove deprecated MISE_LEGACY_VERSION_FILE assertions by @jdx in [#6505](https://github.com/jdx/mise/pull/6505)

### 📦 Registry

- add code by @TyceHerrman in [#6492](https://github.com/jdx/mise/pull/6492)

### New Contributors

- @minusfive made their first contribution in [#6497](https://github.com/jdx/mise/pull/6497)

## [2025.9.25](https://github.com/jdx/mise/compare/v2025.9.24..v2025.9.25) - 2025-09-30

### 🐛 Bug Fixes

- **(auto-install)** support installing non-active backend versions by @jdx in [#6484](https://github.com/jdx/mise/pull/6484)
- **(install)** remove duplicate 'mise' text in install header by @jdx in [#6489](https://github.com/jdx/mise/pull/6489)
- **(task)** prevent hang when tasks with multiple dependencies fail by @stempler in [#6481](https://github.com/jdx/mise/pull/6481)

### 🧪 Testing

- **(e2e)** use local HTTP server instead of httpbin.org for tool-stub tests by @jdx in [#6488](https://github.com/jdx/mise/pull/6488)

### 📦 Registry

- prefer k3s from Aqua over ASDF plugin by @TobiX in [#6486](https://github.com/jdx/mise/pull/6486)

### Chore

- **(ci)** prevent release workflow from running on release branch pushes by @jdx in [#6490](https://github.com/jdx/mise/pull/6490)
- **(ci)** parallelize release workflow to start e2e tests earlier by @jdx in [#6491](https://github.com/jdx/mise/pull/6491)

### New Contributors

- @stempler made their first contribution in [#6481](https://github.com/jdx/mise/pull/6481)

## [2025.9.24](https://github.com/jdx/mise/compare/v2025.9.23..v2025.9.24) - 2025-09-29

### 🚀 Features

- **(age)** support age encrypted env vars in mise.toml files by @jdx in [#6463](https://github.com/jdx/mise/pull/6463)

### 🐛 Bug Fixes

- **(vfox)** integrate `parse_legacy_file` into backend by @malept in [#6471](https://github.com/jdx/mise/pull/6471)

### 📦 Registry

- add ggshield by @TyceHerrman in [#6435](https://github.com/jdx/mise/pull/6435)
- add jaq by @TyceHerrman in [#6434](https://github.com/jdx/mise/pull/6434)

## [2025.9.23](https://github.com/jdx/mise/compare/v2025.9.22..v2025.9.23) - 2025-09-28

### 🚀 Features

- **(env)** add support for required environment variables by @jdx in [#6461](https://github.com/jdx/mise/pull/6461)

### 🐛 Bug Fixes

- **(set)** unify config file resolution for mise set and mise use by @jdx in [#6467](https://github.com/jdx/mise/pull/6467)

### Chore

- **(clippy)** replace &Box<dyn SingleReport> with &dyn SingleReport by @jdx in [#6465](https://github.com/jdx/mise/pull/6465)

## [2025.9.22](https://github.com/jdx/mise/compare/v2025.9.21..v2025.9.22) - 2025-09-28

### 🚀 Features

- **(backend)** add environment variable override for tool backends by @jdx in [#6456](https://github.com/jdx/mise/pull/6456)
- add a http_retries setting to define number of retry attempts by @roele in [#6444](https://github.com/jdx/mise/pull/6444)

### 📦 Registry

- re-enable tests by @risu729 in [#6454](https://github.com/jdx/mise/pull/6454)
- restore comments and tests by @risu729 in [#6378](https://github.com/jdx/mise/pull/6378)
- add github backend for graphite by @jdx in [#6455](https://github.com/jdx/mise/pull/6455)

## [2025.9.21](https://github.com/jdx/mise/compare/v2025.9.20..v2025.9.21) - 2025-09-27

### 🚀 Features

- **(cache)** add mise cache path command by @jdx in [#6442](https://github.com/jdx/mise/pull/6442)
- **(github)** add support for compressed binaries and Buck2 to registry by @jdx in [#6439](https://github.com/jdx/mise/pull/6439)

### 🐛 Bug Fixes

- **(http)** bump mtime when extracting tarballs to cache by @jdx in [#6438](https://github.com/jdx/mise/pull/6438)

### 🧪 Testing

- **(vfox)** eliminate flaky remote host dependencies in tests by @jdx in [#6447](https://github.com/jdx/mise/pull/6447)
- **(vfox)** improve test_download_file reliability by @jdx in [#6450](https://github.com/jdx/mise/pull/6450)
- optimize remote task tests with local server by @jdx in [#6443](https://github.com/jdx/mise/pull/6443)
- optimize git remote task tests with local repositories by @jdx in [#6441](https://github.com/jdx/mise/pull/6441)
- mark slow e2e tests and add runtime warnings by @jdx in [#6449](https://github.com/jdx/mise/pull/6449)

### 📦 Registry

- remove incorrect bin_path from balena-cli by @jdx in [#6445](https://github.com/jdx/mise/pull/6445)
- disable oxlint test temporarily by @jdx in [#6446](https://github.com/jdx/mise/pull/6446)

### Chore

- **(ci)** run release workflow on PRs to main for branch protection by @jdx in [#6448](https://github.com/jdx/mise/pull/6448)

## [2025.9.20](https://github.com/jdx/mise/compare/v2025.9.19..v2025.9.20) - 2025-09-26

### 🚀 Features

- **(spm)** add support for self-hosted and GitLab repositories by @roele in [#6358](https://github.com/jdx/mise/pull/6358)
- add instructions for self-update by @jdx in [#6433](https://github.com/jdx/mise/pull/6433)

### 🐛 Bug Fixes

- **(doctor)** exclude tools not supported on current os by @risu729 in [#6422](https://github.com/jdx/mise/pull/6422)
- **(json-schema)** remove settings/additionalProperties by @tpansino in [#6420](https://github.com/jdx/mise/pull/6420)
- **(task)** prevent hang when nested tasks fail by @jdx in [#6430](https://github.com/jdx/mise/pull/6430)
- **(ubi)** filter versions with tag_regex before trimming v prefixes by @risu729 in [#6421](https://github.com/jdx/mise/pull/6421)
- allow strip_archive_path_components to strip a dir containing the same filename by @risu729 in [#6405](https://github.com/jdx/mise/pull/6405)

### 📦️ Dependency Updates

- update ghcr.io/jdx/mise:alpine docker digest to a64d8b4 by @renovate[bot] in [#6426](https://github.com/jdx/mise/pull/6426)
- update actions/cache digest to 0057852 by @renovate[bot] in [#6425](https://github.com/jdx/mise/pull/6425)
- update ghcr.io/jdx/mise:deb docker digest to af96f8e by @renovate[bot] in [#6428](https://github.com/jdx/mise/pull/6428)
- update ghcr.io/jdx/mise:copr docker digest to 0f98c77 by @renovate[bot] in [#6427](https://github.com/jdx/mise/pull/6427)

### 📦 Registry

- use version_prefix for github backends by @risu729 in [#6409](https://github.com/jdx/mise/pull/6409)
- fix hivemind by @mnm364 in [#6431](https://github.com/jdx/mise/pull/6431)
- revert opam/k3kcli backends to ubi by @risu729 in [#6406](https://github.com/jdx/mise/pull/6406)

## [2025.9.19](https://github.com/jdx/mise/compare/v2025.9.18..v2025.9.19) - 2025-09-25

### 🚀 Features

- **(github)** filter remote versions by version_prefix by @risu729 in [#6408](https://github.com/jdx/mise/pull/6408)
- Remove experimental labels for GitHub and HTTP backends by @Copilot in [#6415](https://github.com/jdx/mise/pull/6415)

### 🐛 Bug Fixes

- **(backend)** make pre-tools env vars available in postinstall hooks by @jdx in [#6418](https://github.com/jdx/mise/pull/6418)

### 🧪 Testing

- **(vfox)** replace flaky external tests with local dummy plugin by @jdx in [#6403](https://github.com/jdx/mise/pull/6403)

### 📦 Registry

- fix mise-ghcup plugin managed tools descriptions by @risu729 in [#6411](https://github.com/jdx/mise/pull/6411)
- add Tinymist by @3w36zj6 in [#6412](https://github.com/jdx/mise/pull/6412)
- revert djinni backend to ubi by @risu729 in [#6410](https://github.com/jdx/mise/pull/6410)

### New Contributors

- @Copilot made their first contribution in [#6415](https://github.com/jdx/mise/pull/6415)

## [2025.9.18](https://github.com/jdx/mise/compare/v2025.9.17..v2025.9.18) - 2025-09-25

### 🚀 Features

- **(config)** support vars in tool versions by @jdx in [#6401](https://github.com/jdx/mise/pull/6401)
- **(template)** add read_file() function by @jdx in [#6400](https://github.com/jdx/mise/pull/6400)

### 🐛 Bug Fixes

- **(aqua)** support github_artifact_attestations.enabled by @risu729 in [#6372](https://github.com/jdx/mise/pull/6372)
- use /c instead of -c on windows in postinstall hook by @risu729 in [#6397](https://github.com/jdx/mise/pull/6397)

### 🧪 Testing

- **(test-tool)** uninstall all versions and clear cache before installation by @jdx in [#6393](https://github.com/jdx/mise/pull/6393)

### 📦 Registry

- replace amplify-cli github backend with ubi by @eggplants in [#6396](https://github.com/jdx/mise/pull/6396)

### New Contributors

- @eggplants made their first contribution in [#6396](https://github.com/jdx/mise/pull/6396)

## [2025.9.17](https://github.com/jdx/mise/compare/v2025.9.16..v2025.9.17) - 2025-09-24

### 🚀 Features

- **(java)** add support for Liberica NIK releases by @roele in [#6382](https://github.com/jdx/mise/pull/6382)

### 🐛 Bug Fixes

- **(toolset)** handle underflow in version_sub function by @koh-sh in [#6389](https://github.com/jdx/mise/pull/6389)

### 📚 Documentation

- document MISE_ENV behavior for global/system configs by @jdx in [#6385](https://github.com/jdx/mise/pull/6385)

### New Contributors

- @jc00ke made their first contribution in [#6386](https://github.com/jdx/mise/pull/6386)
- @koh-sh made their first contribution in [#6389](https://github.com/jdx/mise/pull/6389)

## [2025.9.16](https://github.com/jdx/mise/compare/v2025.9.15..v2025.9.16) - 2025-09-22

### 🐛 Bug Fixes

- **(aqua)** remove blake3 support from aqua checksum algorithms by @risu729 in [#6370](https://github.com/jdx/mise/pull/6370)
- **(aqua)** remove cosign and slsa-verifier dependencies by @risu729 in [#6371](https://github.com/jdx/mise/pull/6371)
- **(aqua)** remove cosign.experimental by @risu729 in [#6376](https://github.com/jdx/mise/pull/6376)
- **(file)** handle GNU sparse files and tar crate extraction issues by @jdx in [#6380](https://github.com/jdx/mise/pull/6380)

### 📚 Documentation

- minisign doesn't require cli by @risu729 in [#6369](https://github.com/jdx/mise/pull/6369)

### 📦 Registry

- use npm backend for zbctl by @risu729 in [#6379](https://github.com/jdx/mise/pull/6379)

### Chore

- ignore renovate new bot name by @risu729 in [#6364](https://github.com/jdx/mise/pull/6364)

## [2025.9.15](https://github.com/jdx/mise/compare/v2025.9.14..v2025.9.15) - 2025-09-21

### 🚀 Features

- add env propagation by @Its-Just-Nans in [#6342](https://github.com/jdx/mise/pull/6342)

### 🐛 Bug Fixes

- **(aqua)** improve GitHub token handling for sigstore verification by @jdx in [#6351](https://github.com/jdx/mise/pull/6351)
- **(backend)** change dependency checks to warnings instead of errors by @jdx in [#6363](https://github.com/jdx/mise/pull/6363)
- **(npm)** improve error message when npm/bun is not installed by @jdx in [#6359](https://github.com/jdx/mise/pull/6359)
- **(vfox)** enable TLS support for reqwest to fix CI tests by @jdx in [#6356](https://github.com/jdx/mise/pull/6356)

### 🚜 Refactor

- **(registry)** convert to nested TOML sections format by @jdx in [#6361](https://github.com/jdx/mise/pull/6361)

### 🧪 Testing

- **(e2e)** resolve mise via PATH in backend missing deps test by @jdx in [#6362](https://github.com/jdx/mise/pull/6362)
- **(vfox)** replace flaky external HTTP tests with local mock server by @jdx in [#6354](https://github.com/jdx/mise/pull/6354)

### 📦️ Dependency Updates

- pin dependencies by @renovate[bot] in [#6243](https://github.com/jdx/mise/pull/6243)

### 📦 Registry

- add missing cargo backends by @jayvdb in [#6307](https://github.com/jdx/mise/pull/6307)

### Chore

- **(install.sh)** add `MISE_INSTALL_MUSL` to force installing musl variants on Linux by @malept in [#6355](https://github.com/jdx/mise/pull/6355)

## [2025.9.14](https://github.com/jdx/mise/compare/v2025.9.13..v2025.9.14) - 2025-09-20

### 🐛 Bug Fixes

- fix an issue where Swift could not be installed on arm64 Ubuntu by @lish82 in [#6348](https://github.com/jdx/mise/pull/6348)

### Chore

- use cross to build on linux by @jdx in [#6346](https://github.com/jdx/mise/pull/6346)

### New Contributors

- @lish82 made their first contribution in [#6348](https://github.com/jdx/mise/pull/6348)

## [2025.9.13](https://github.com/jdx/mise/compare/v2025.9.12..v2025.9.13) - 2025-09-19

### 🚀 Features

- **(aqua)** integrate native sigstore-verification for security verification by @jdx in [#6332](https://github.com/jdx/mise/pull/6332)
- **(docs)** improve search result readability with lighter teal background by @jdx in [#6328](https://github.com/jdx/mise/pull/6328)
- **(ui)** update logo as favicon and fix hover transitions by @jdx in [#6325](https://github.com/jdx/mise/pull/6325)
- **(vfox)** add file.read lua function by @malept in [#6333](https://github.com/jdx/mise/pull/6333)
- add documentation for "Environment in tasks" #5134 #5638 by @Its-Just-Nans in [#6329](https://github.com/jdx/mise/pull/6329)

### 🐛 Bug Fixes

- **(github)** correctly paginate releases/tags for private repos by @malept in [#6318](https://github.com/jdx/mise/pull/6318)
- **(hk)** exclude aqua-registry from prettier linting by @jdx in [#6327](https://github.com/jdx/mise/pull/6327)
- **(ui)** improve GitHub star badge layout and alignment by @jdx in [#6326](https://github.com/jdx/mise/pull/6326)

### 📚 Documentation

- change 'hello.py' to 'main.py' in python.md by @my1e5 in [#6319](https://github.com/jdx/mise/pull/6319)
- customize VitePress theme with unique branding by @jdx in [#6324](https://github.com/jdx/mise/pull/6324)

### 📦️ Dependency Updates

- update taiki-e/install-action digest to 0aa4f22 by @renovate[bot] in [#6334](https://github.com/jdx/mise/pull/6334)
- update rust crate comfy-table to v7.2.1 by @renovate[bot] in [#6335](https://github.com/jdx/mise/pull/6335)
- update rust crate console to v0.16.1 by @renovate[bot] in [#6336](https://github.com/jdx/mise/pull/6336)
- update rust crate indexmap to v2.11.4 by @renovate[bot] in [#6337](https://github.com/jdx/mise/pull/6337)

### 📦 Registry

- remove deprecated virtualos by @jdx in [166379f](https://github.com/jdx/mise/commit/166379f71c79fccacfc980dd14d4e18642c7d1e5)
- add trufflehog ([aqua:trufflesecurity/trufflehog](https://github.com/trufflesecurity/trufflehog)) by @risu729 in [#6316](https://github.com/jdx/mise/pull/6316)

### Chore

- fixing typos by @Its-Just-Nans in [#6331](https://github.com/jdx/mise/pull/6331)

### New Contributors

- @Its-Just-Nans made their first contribution in [#6331](https://github.com/jdx/mise/pull/6331)
- @my1e5 made their first contribution in [#6319](https://github.com/jdx/mise/pull/6319)

## [2025.9.12](https://github.com/jdx/mise/compare/v2025.9.11..v2025.9.12) - 2025-09-16

### 🐛 Bug Fixes

- **(ci)** properly exclude aqua-registry files from hk linting by @jdx in [42d7758](https://github.com/jdx/mise/commit/42d7758d157317088ac5194ac26eefc7fc1ba4f8)

### Chore

- **(release)** embed aqua-registry under crate and publish like vfox by @jdx in [#6306](https://github.com/jdx/mise/pull/6306)
- ignore aqua-registry assets from prettier by @jdx in [047d77b](https://github.com/jdx/mise/commit/047d77be35fea0b3277342cb6383a1873bca19a5)
- disable "useless cat" shellcheck by @jdx in [a6def59](https://github.com/jdx/mise/commit/a6def59fe945028934fed0694df2b4daeeaaf478)

## [2025.9.11](https://github.com/jdx/mise/compare/v2025.9.10..v2025.9.11) - 2025-09-16

### 🚀 Features

- **(ci)** run all registry tools when workflow_dispatch is triggered by @jdx in [#6295](https://github.com/jdx/mise/pull/6295)
- **(cli)** handle non-existent working directories gracefully by @jdx in [#6304](https://github.com/jdx/mise/pull/6304)
- **(set)** add -E/--env flag to mise set command by @jdx in [#6291](https://github.com/jdx/mise/pull/6291)
- **(tasks)** make auto outputs default by @risu729 in [#6137](https://github.com/jdx/mise/pull/6137)

### 🐛 Bug Fixes

- align crate versions to fix release-plz script by @jdx in [5a464e9](https://github.com/jdx/mise/commit/5a464e98b56f49200e69ce2665ed896c74b206e3)

### 🚜 Refactor

- **(aqua)** extract aqua registry into internal subcrate by @jdx in [#6293](https://github.com/jdx/mise/pull/6293)

### 📚 Documentation

- fix mkdir paths by @risu729 in [#6298](https://github.com/jdx/mise/pull/6298)
- fix rust profile default by @risu729 in [#6305](https://github.com/jdx/mise/pull/6305)

### 📦 Registry

- indicate aws-cli is v2 by @jayvdb in [#6300](https://github.com/jdx/mise/pull/6300)

### Chore

- **(aqua-registry)** remove unused aqua-registry files by @jdx in [#6294](https://github.com/jdx/mise/pull/6294)
- **(release)** make release-plz idempotent for existing crate versions by @jdx in [dbdfd6a](https://github.com/jdx/mise/commit/dbdfd6a713852a1d55a6bb8376d2996545e128ce)
- **(release)** skip publishing mise when aqua-registry is a path dep by @jdx in [47efffd](https://github.com/jdx/mise/commit/47efffdfc0316100f41c6c077d17fd6014663f4f)
- keep aqua-registry LICENSE file by @risu729 in [#6297](https://github.com/jdx/mise/pull/6297)

### New Contributors

- @jayvdb made their first contribution in [#6300](https://github.com/jdx/mise/pull/6300)

## [2025.9.10](https://github.com/jdx/mise/compare/v2025.9.9..v2025.9.10) - 2025-09-13

### 🐛 Bug Fixes

- **(tool-stub)** detect binary names from single-file downloads by @jdx in [#6281](https://github.com/jdx/mise/pull/6281)

### 🚜 Refactor

- allow any collection types in deserialize_arr by @risu729 in [#6282](https://github.com/jdx/mise/pull/6282)
- use deserialize_arr for task runs by @risu729 in [#6253](https://github.com/jdx/mise/pull/6253)

### 📚 Documentation

- **(getting-started)** add backends step with diagram, github example, env vars and simple tasks by @jdx in [#6288](https://github.com/jdx/mise/pull/6288)
- simplify OS detection in tool plugin development by @jdx in [#6287](https://github.com/jdx/mise/pull/6287)
- update backend plugin template references by @jdx in [942f5eb](https://github.com/jdx/mise/commit/942f5eb1436fef38920836347218984200b07386)

### 📦️ Dependency Updates

- update rust crate chrono to v0.4.42 by @renovate[bot] in [#6274](https://github.com/jdx/mise/pull/6274)
- update taiki-e/install-action digest to 0154864 by @renovate[bot] in [#6273](https://github.com/jdx/mise/pull/6273)

### 📦 Registry

- use asdf to install semver-tool by @jylenhof in [#6233](https://github.com/jdx/mise/pull/6233)

### Chore

- **(schema)** fix schema for subtasks by @risu729 in [#6289](https://github.com/jdx/mise/pull/6289)
- update render:schema task by @risu729 in [#6275](https://github.com/jdx/mise/pull/6275)

## [2025.9.9](https://github.com/jdx/mise/compare/v2025.9.8..v2025.9.9) - 2025-09-11

### 🐛 Bug Fixes

- **(backend)** make HTTP installs atomic and serialize with cache lock by @jdx in [#6259](https://github.com/jdx/mise/pull/6259)
- **(env)** allow nested env._.path directives by @risu729 in [#6160](https://github.com/jdx/mise/pull/6160)
- **(env)** disallow nested env objects by @risu729 in [#6268](https://github.com/jdx/mise/pull/6268)
- **(schema)** allow nested arrays in task.depends by @risu729 in [#6265](https://github.com/jdx/mise/pull/6265)
- **(task)** resolve jobs=1 hang and keep-order panic; improve Ctrl-C handling by @jdx in [#6264](https://github.com/jdx/mise/pull/6264)
- **(tasks)** stop CLI group after first failure without --continue-on-error by @jdx in [#6270](https://github.com/jdx/mise/pull/6270)

### 📚 Documentation

- fixed toml issues in URL replacements settings documentation by @ThomasSteinbach in [#6269](https://github.com/jdx/mise/pull/6269)

### Chore

- **(schema)** strict oneOf branches and DRY env_directive in schemas by @jdx in [#6271](https://github.com/jdx/mise/pull/6271)
- add schema linter by @risu729 in [#6267](https://github.com/jdx/mise/pull/6267)

## [2025.9.8](https://github.com/jdx/mise/compare/v2025.9.7..v2025.9.8) - 2025-09-10

### 🐛 Bug Fixes

- **(tasks)** prevent hang when task fails in sequence by @jdx in [#6260](https://github.com/jdx/mise/pull/6260)
- **(version)** fetch mise version if cached version is older than the current by @risu729 in [#6252](https://github.com/jdx/mise/pull/6252)

### 📦️ Dependency Updates

- update rhysd/action-setup-vim action to v1.4.3 by @renovate[bot] in [#6249](https://github.com/jdx/mise/pull/6249)

## [2025.9.7](https://github.com/jdx/mise/compare/v2025.9.6..v2025.9.7) - 2025-09-09

### 🐛 Bug Fixes

- **(env)** allow mixed map for env._.file by @risu729 in [#6148](https://github.com/jdx/mise/pull/6148)
- **(tasks)** restore parallel starts with poetry via list_paths cache and stable exec-env cache by @jdx in [#6237](https://github.com/jdx/mise/pull/6237)
- add 'unknown' to the list of OS patterns by @efussi in [#6235](https://github.com/jdx/mise/pull/6235)
- propagate errors from backend installs by @jdx in [#6236](https://github.com/jdx/mise/pull/6236)

### 📦️ Dependency Updates

- update taiki-e/install-action digest to 0c5db7f by @renovate[bot] in [#6244](https://github.com/jdx/mise/pull/6244)
- update golang docker tag to v1.25.1 by @renovate[bot] in [#6247](https://github.com/jdx/mise/pull/6247)
- update dependency vitepress to v1.6.4 by @renovate[bot] in [#6246](https://github.com/jdx/mise/pull/6246)

### New Contributors

- @efussi made their first contribution in [#6235](https://github.com/jdx/mise/pull/6235)

## [2025.9.6](https://github.com/jdx/mise/compare/v2025.9.5..v2025.9.6) - 2025-09-08

### 🚀 Features

- **(backend)** add Backend trait methods for metadata fetching by @jdx in [#6228](https://github.com/jdx/mise/pull/6228)
- **(core)** implement metadata fetching for Node.js and Bun by @jdx in [#6230](https://github.com/jdx/mise/pull/6230)
- **(mise-test-tool)** add release scripts for automated GitHub releases by @jdx in [bd0eadd](https://github.com/jdx/mise/commit/bd0eadde5fff3897cda47d533c02cfe8e2b20048)
- **(platform)** implement platform parsing and CLI integration by @jdx in [#6227](https://github.com/jdx/mise/pull/6227)
- migrate tools from ubi to github backend which work by @jdx in [#6232](https://github.com/jdx/mise/pull/6232)

### 🐛 Bug Fixes

- **(task)** use terminal width instead of hardcoded 60-char limit for task display by @jdx in [#6218](https://github.com/jdx/mise/pull/6218)
- **(task)** use terminal width instead of hardcoded 60-char limit for task display by @jdx in [#6220](https://github.com/jdx/mise/pull/6220)
- nix flake build failure on macOS by @okuuva in [#6223](https://github.com/jdx/mise/pull/6223)
- only use multi-version syntax in mise.lock by @jdx in [#6224](https://github.com/jdx/mise/pull/6224)

### 🧪 Testing

- **(e2e)** add comprehensive parallel task execution test for issue #5451 by @jdx in [#6221](https://github.com/jdx/mise/pull/6221)

### Chore

- added .cursor/environment.json by @jdx in [dc6b145](https://github.com/jdx/mise/commit/dc6b1455967c650b4f960316830b63072998977c)
- init agent-os by @jdx in [81af40e](https://github.com/jdx/mise/commit/81af40ece5a8e1481b3a4ebf0de8a401fb7685ad)
- agent-os analyze by @jdx in [9625f58](https://github.com/jdx/mise/commit/9625f58112d4f22d299c1352a3e85f036435f21c)

## [2025.9.5](https://github.com/jdx/mise/compare/v2025.9.4..v2025.9.5) - 2025-09-06

### 🚀 Features

- **(task)** add timeout support for task execution by @jdx in [#6216](https://github.com/jdx/mise/pull/6216)
- **(task)** sub-tasks in run lists by @jdx in [#6212](https://github.com/jdx/mise/pull/6212)

### 🐛 Bug Fixes

- **(task)** remove MISE_TASK_UNNEST functionality by @jdx in [#6217](https://github.com/jdx/mise/pull/6217)

### Chore

- fix npm publish action by @jdx in [14f4b09](https://github.com/jdx/mise/commit/14f4b09982cfa09139f172f302939f46d2cb0872)
- fix cloudflare release action by @jdx in [00afa25](https://github.com/jdx/mise/commit/00afa2563d4368963bcacce11ebddbe05f45b4d7)
- fix git-cliff for release notes by @jdx in [15a9aed](https://github.com/jdx/mise/commit/15a9aede95c8ad953842c206df3b6c9a3960100f)

## [2025.9.4](https://github.com/jdx/mise/compare/v2025.9.3..v2025.9.4) - 2025-09-06

### Chore

- fix git-cliff on release by @jdx in [3c388f2](https://github.com/jdx/mise/commit/3c388f28e6cce6084f86e1805ace3aede594405b)

## [2025.9.3](https://github.com/jdx/mise/compare/v2025.9.2..v2025.9.3) - 2025-09-06

### 🚀 Features

- **(backend)** improve http error when platform url missing; list available platforms by @jdx in [#6200](https://github.com/jdx/mise/pull/6200)
- **(cli)** support scoped packages for all backend types by @earlgray283 in [#6213](https://github.com/jdx/mise/pull/6213)
- **(http)** add URL replacement feature for HTTP requests by @ThomasSteinbach in [#6207](https://github.com/jdx/mise/pull/6207)

### 🐛 Bug Fixes

- **(backend)** preserve arch underscores in platform keys by @jdx in [#6202](https://github.com/jdx/mise/pull/6202)
- **(task)** resolve hanging issue with multiple depends_post by @jdx in [#6206](https://github.com/jdx/mise/pull/6206)
- couldn't download node binary in Alpine, even if it exists in the mirror url by @Hazer in [#5972](https://github.com/jdx/mise/pull/5972)
- **breaking** use config_root for env._.path by @jdx in [#6204](https://github.com/jdx/mise/pull/6204)
- bugfix for paths that include spaces by @karim-elkholy in [#6210](https://github.com/jdx/mise/pull/6210)

### 📚 Documentation

- improve release notes generation by @jdx in [#6197](https://github.com/jdx/mise/pull/6197)
- fix release changelog contributor reporting by @jdx in [#6201](https://github.com/jdx/mise/pull/6201)

### Chore

- use fine-grained gh token by @jdx in [#6208](https://github.com/jdx/mise/pull/6208)
- use settings.local.json for claude config by @jdx in [fd0fba9](https://github.com/jdx/mise/commit/fd0fba9fadb5ea36371283dbcda9a4f6264f96cd)

### New Contributors

- @ThomasSteinbach made their first contribution in [#6207](https://github.com/jdx/mise/pull/6207)
- @earlgray283 made their first contribution in [#6213](https://github.com/jdx/mise/pull/6213)
- @karim-elkholy made their first contribution in [#6210](https://github.com/jdx/mise/pull/6210)
- @Hazer made their first contribution in [#5972](https://github.com/jdx/mise/pull/5972)

## [2025.9.2](https://github.com/jdx/mise/compare/v2025.9.1..v2025.9.2) - 2025-09-05

### 🐛 Bug Fixes

- **(ci)** set required environment variables for npm publishing by @jdx in [#6189](https://github.com/jdx/mise/pull/6189)
- **(release)** clean up extra newlines in release notes formatting by @jdx in [#6190](https://github.com/jdx/mise/pull/6190)
- **(release)** add proper newline after New Contributors section in cliff template by @jdx in [#6194](https://github.com/jdx/mise/pull/6194)
- **(release)** fix changelog formatting to remove extra blank lines by @jdx in [#6195](https://github.com/jdx/mise/pull/6195)
- **(release)** restore proper newline after New Contributors section by @jdx in [#6196](https://github.com/jdx/mise/pull/6196)

### 🚜 Refactor

- **(ci)** split release workflow into separate specialized workflows by @jdx in [#6193](https://github.com/jdx/mise/pull/6193)

### Chore

- **(release)** require GitHub Actions environment for release-plz script by @jdx in [#6191](https://github.com/jdx/mise/pull/6191)

## [2025.9.1](https://github.com/jdx/mise/compare/v2025.9.0..v2025.9.1) - 2025-09-05

### 🐛 Bug Fixes

- python nested venv path order by @elvismacak in [#6124](https://github.com/jdx/mise/pull/6124)
- resolve immutable release workflow and VERSION file timing issues by @jdx in [#6187](https://github.com/jdx/mise/pull/6187)

### New Contributors

- @elvismacak made their first contribution in [#6124](https://github.com/jdx/mise/pull/6124)

## [2025.9.0](https://github.com/jdx/mise/compare/v2025.8.21..v2025.9.0) - 2025-09-05

### 🚀 Features

- allow set/unset backend aliases by @roele in [#6172](https://github.com/jdx/mise/pull/6172)

### 🐛 Bug Fixes

- **(aqua)** respect order of asset_strs by @risu729 in [#6143](https://github.com/jdx/mise/pull/6143)
- **(java)** treat freebsd as linux (assuming linux compatability) by @roele in [#6161](https://github.com/jdx/mise/pull/6161)
- **(nushell/windows)** Fix $env.PATH getting converted to a string by @zackyancey in [#6157](https://github.com/jdx/mise/pull/6157)
- **(sync)** create uv_versions_path dir if it doesn't exist by @risu729 in [#6142](https://github.com/jdx/mise/pull/6142)
- **(ubi)** show relevent error messages for v-prefixed tags by @risu729 in [#6183](https://github.com/jdx/mise/pull/6183)
- remove nodejs/golang alias migrate code by @risu729 in [#6141](https://github.com/jdx/mise/pull/6141)
- mise activate not working on powershell v5 by @L0RD-ZER0 in [#6168](https://github.com/jdx/mise/pull/6168)

### 📚 Documentation

- **(task)** remove word "additional" to avoid confusions by @risu729 in [#6159](https://github.com/jdx/mise/pull/6159)

### Chore

- update Cargo.lock by @risu729 in [#6184](https://github.com/jdx/mise/pull/6184)

### New Contributors

- @zackyancey made their first contribution in [#6157](https://github.com/jdx/mise/pull/6157)

## [2025.8.21](https://github.com/jdx/mise/compare/v2025.8.20..v2025.8.21) - 2025-08-27

### 🚀 Features

- allow use of templates in task confirmation by @roele in [#6129](https://github.com/jdx/mise/pull/6129)

### 🐛 Bug Fixes

- task confirmation does not handle SIGINT appropriately by @roele in [#6126](https://github.com/jdx/mise/pull/6126)

### 📚 Documentation

- Split run command so that copy button is useful by @anujdeshpande in [#6099](https://github.com/jdx/mise/pull/6099)

### 📦 Registry

- prefer 1password asdf plugin for ls-remote by @risu729 in [#6116](https://github.com/jdx/mise/pull/6116)

### New Contributors

- @anujdeshpande made their first contribution in [#6099](https://github.com/jdx/mise/pull/6099)

## [2025.8.20](https://github.com/jdx/mise/compare/v2025.8.19..v2025.8.20) - 2025-08-22

### 🐛 Bug Fixes

- use fish_add_path when activating mise for fish shell by @roele in [#6074](https://github.com/jdx/mise/pull/6074)

## [2025.8.19](https://github.com/jdx/mise/compare/v2025.8.18..v2025.8.19) - 2025-08-22

### 🐛 Bug Fixes

- **(aqua)** bake in aliased registries by @risu729 in [#6105](https://github.com/jdx/mise/pull/6105)

### 📦 Registry

- update kubectl aqua alias by @malept in [#6107](https://github.com/jdx/mise/pull/6107)
- remove asdf plugin for watchexec by @risu729 in [#6106](https://github.com/jdx/mise/pull/6106)

## [2025.8.18](https://github.com/jdx/mise/compare/v2025.8.17..v2025.8.18) - 2025-08-22

### 🚀 Features

- **(env)** add --redacted and --values flags to env command by @jdx in [#6103](https://github.com/jdx/mise/pull/6103)

## [2025.8.17](https://github.com/jdx/mise/compare/v2025.8.16..v2025.8.17) - 2025-08-22

### 🐛 Bug Fixes

- **(aqua)** remove mise-versions aqua registry by @risu729 in [#6097](https://github.com/jdx/mise/pull/6097)

### 📚 Documentation

- fix invalid configuration by @kamontat in [#6088](https://github.com/jdx/mise/pull/6088)

### 📦️ Dependency Updates

- update apple-actions/import-codesign-certs digest to 95e84a1 by @renovate[bot] in [#6093](https://github.com/jdx/mise/pull/6093)
- update taiki-e/install-action digest to 36fe651 by @renovate[bot] in [#6094](https://github.com/jdx/mise/pull/6094)

### 📦 Registry

- remove asdf plugin for zoxide by @risu729 in [#6100](https://github.com/jdx/mise/pull/6100)

### Chore

- remove submodules option for actions/checkout by @risu729 in [#6090](https://github.com/jdx/mise/pull/6090)
- exclude aqua-registry from linguist stats by @risu729 in [#6098](https://github.com/jdx/mise/pull/6098)

### New Contributors

- @kamontat made their first contribution in [#6088](https://github.com/jdx/mise/pull/6088)

## [2025.8.16](https://github.com/jdx/mise/compare/v2025.8.15..v2025.8.16) - 2025-08-21

### Chore

- **(aqua-registry)** replace subtree logic with simpler `git clone` method by @jdx in [dd4947c](https://github.com/jdx/mise/commit/dd4947c49591ef3c0ac8372465bbfd1cde4ca946)
- remove vfox-npm submodule by @jdx in [c22f95b](https://github.com/jdx/mise/commit/c22f95b4c30a4415ee08830e17fa8bd5a7a59eb7)
- add vfox-npm by @jdx in [78c0972](https://github.com/jdx/mise/commit/78c0972a690eaf86eb6f5bbf2eabbe8a247890ea)

## [2025.8.15](https://github.com/jdx/mise/compare/v2025.8.14..v2025.8.15) - 2025-08-21

### Chore

- **(release-plz)** get `git status` by @jdx in [#6083](https://github.com/jdx/mise/pull/6083)
- add libbz2-dev to e2e test dependencies by @jdx in [#6080](https://github.com/jdx/mise/pull/6080)
- replace submodule with subtree by @risu729 in [#6082](https://github.com/jdx/mise/pull/6082)
- fix aqua-registry subtree by @jdx in [522f7f5](https://github.com/jdx/mise/commit/522f7f591dbfa01e537c294647ffdc2a2357c32c)

## [2025.8.14](https://github.com/jdx/mise/compare/v2025.8.13..v2025.8.14) - 2025-08-20

### 🚀 Features

- **(http)** auto-clean OS/arch suffixes from binary names by @jdx in [#6077](https://github.com/jdx/mise/pull/6077)
- **(install)** add --dry-run flag to show what would be installed by @jdx in [#6078](https://github.com/jdx/mise/pull/6078)

### 🐛 Bug Fixes

- **(python)** patching sysconfig data fails for RC versions by @roele in [#6069](https://github.com/jdx/mise/pull/6069)
- **(schema)** add missing `settings` type by @br3ndonland in [#6070](https://github.com/jdx/mise/pull/6070)

### Chore

- add liblzma-dev for e2e tests to avoid python-build warning by @jdx in [#6066](https://github.com/jdx/mise/pull/6066)

## [2025.8.13](https://github.com/jdx/mise/compare/v2025.8.12..v2025.8.13) - 2025-08-18

### 🐛 Bug Fixes

- clean up install progress and error output by @jdx in [#6063](https://github.com/jdx/mise/pull/6063)
- make header progress display at start of install by @jdx in [#6065](https://github.com/jdx/mise/pull/6065)

### Chore

- Upgrade ubi dependency by @suprememoocow in [#6061](https://github.com/jdx/mise/pull/6061)
- replace install_or_update_python_build by @jdx in [#6064](https://github.com/jdx/mise/pull/6064)

### New Contributors

- @suprememoocow made their first contribution in [#6061](https://github.com/jdx/mise/pull/6061)

## [2025.8.12](https://github.com/jdx/mise/compare/v2025.8.11..v2025.8.12) - 2025-08-17

### 🚀 Features

- respect PREFER_OFFLINE for aqua package metadata fetching by @jdx in [#6058](https://github.com/jdx/mise/pull/6058)

### 📚 Documentation

- fix backend_architecture docs by @risu729 in [#6027](https://github.com/jdx/mise/pull/6027)

### 📦️ Dependency Updates

- update amannn/action-semantic-pull-request digest to e32d7e6 by @renovate[bot] in [#6031](https://github.com/jdx/mise/pull/6031)
- update actions/checkout digest to 08eba0b by @renovate[bot] in [#6030](https://github.com/jdx/mise/pull/6030)
- update actions/cache digest to 0400d5f by @renovate[bot] in [#5957](https://github.com/jdx/mise/pull/5957)

### 📦 Registry

- support tenv idiomatic files by @risu729 in [#6050](https://github.com/jdx/mise/pull/6050)

### Chore

- check for warnings in gha with rust stable by @jdx in [#6055](https://github.com/jdx/mise/pull/6055)

## [2025.8.11](https://github.com/jdx/mise/compare/v2025.8.10..v2025.8.11) - 2025-08-17

### 🚀 Features

- **(task)** allow more #MISE comments patterns by @risu729 in [#6011](https://github.com/jdx/mise/pull/6011)

### 🐛 Bug Fixes

- prevent panic with task tera errors by @jdx in [#6046](https://github.com/jdx/mise/pull/6046)

### 📚 Documentation

- **(settings)** use php as an example for `disable_default_registry` by @risu729 in [#6025](https://github.com/jdx/mise/pull/6025)
- Update ide-integration.md by @jdx in [#6035](https://github.com/jdx/mise/pull/6035)
- Update ide-integration.md by @jdx in [#6040](https://github.com/jdx/mise/pull/6040)
- added openSUSE zypper install instructions by @lfromanini in [#6037](https://github.com/jdx/mise/pull/6037)
- update `contributing.md` for discussions by @br3ndonland in [#6047](https://github.com/jdx/mise/pull/6047)

### 📦 Registry

- add container-use ([aqua:dagger/container-use](https://github.com/dagger/container-use)) by @TyceHerrman in [#6029](https://github.com/jdx/mise/pull/6029)
- add prek ([aqua:j178/prek](https://github.com/j178/prek)) by @HenryZhang-ZHY in [#6023](https://github.com/jdx/mise/pull/6023)

### Chore

- fix warnings by @jdx in [#6043](https://github.com/jdx/mise/pull/6043)
- remove unused permissions in registry test by @risu729 in [#6044](https://github.com/jdx/mise/pull/6044)
- fix fish shell script in hk config by @jdx in [#6048](https://github.com/jdx/mise/pull/6048)

### New Contributors

- @br3ndonland made their first contribution in [#6047](https://github.com/jdx/mise/pull/6047)
- @HenryZhang-ZHY made their first contribution in [#6023](https://github.com/jdx/mise/pull/6023)
- @lfromanini made their first contribution in [#6037](https://github.com/jdx/mise/pull/6037)

## [2025.8.10](https://github.com/jdx/mise/compare/v2025.8.9..v2025.8.10) - 2025-08-14

### 🚀 Features

- **(aqua)** make bin paths executable by @risu729 in [#6010](https://github.com/jdx/mise/pull/6010)
- added header bar during `mise install` by @jdx in [#6022](https://github.com/jdx/mise/pull/6022)

### 🐛 Bug Fixes

- **(aqua)** improve warnings for packages without repo_owner and repo_name  (2nd attempt) by @risu729 in [#6009](https://github.com/jdx/mise/pull/6009)
- version prefix detection by @risu729 in [#5943](https://github.com/jdx/mise/pull/5943)
- respect MISE_DEFAULT_CONFIG_FILENAME by @risu729 in [#5899](https://github.com/jdx/mise/pull/5899)

### 📦 Registry

- enable kubecolor test by @risu729 in [#6008](https://github.com/jdx/mise/pull/6008)
- fix os specific backends for usage by @risu729 in [#6007](https://github.com/jdx/mise/pull/6007)
- use aqua backend for restish by @risu729 in [#5986](https://github.com/jdx/mise/pull/5986)
- add cfssljson ([aqua:cloudflare/cfssl/cfssljson](https://github.com/cloudflare/cfssl/cfssljson)) by @disintegrator in [#6013](https://github.com/jdx/mise/pull/6013)
- add claude-squad ([aqua:smtg-ai/claude-squad](https://github.com/smtg-ai/claude-squad)) by @TyceHerrman in [#5894](https://github.com/jdx/mise/pull/5894)

### New Contributors

- @disintegrator made their first contribution in [#6013](https://github.com/jdx/mise/pull/6013)

## [2025.8.9](https://github.com/jdx/mise/compare/v2025.8.8..v2025.8.9) - 2025-08-13

### 🚀 Features

- **(timeout)** show duration, URL, and config hint on timeouts; increase fetch timeout default to 10s by @jdx in [#5991](https://github.com/jdx/mise/pull/5991)

### 🐛 Bug Fixes

- **(aqua)** add executable permissions for zip-extracted binaries by @itochan in [#5998](https://github.com/jdx/mise/pull/5998)
- **(core)** auto-repair corrupted pyenv cache by recloning on update failure by @jdx in [#6003](https://github.com/jdx/mise/pull/6003)
- **(uv_venv)** fixes PATH ordering with `mise x` by @jdx in [#6005](https://github.com/jdx/mise/pull/6005)
- duplicate versions and validation in `mise tool` by @jdx in [#6001](https://github.com/jdx/mise/pull/6001)

### 📚 Documentation

- **(tools)** document per-tool postinstall option in [tools] by @jdx in [#5993](https://github.com/jdx/mise/pull/5993)
- Update install instructions for nushell by @Joniator in [#5981](https://github.com/jdx/mise/pull/5981)
- README.md typo by @jdx in [#5990](https://github.com/jdx/mise/pull/5990)

### ◀️ Revert

- Revert "docs: Update install instructions for nushell" by @jdx in [#5983](https://github.com/jdx/mise/pull/5983)
- Revert "fix(aqua): add executable permissions for zip-extracted binaries" by @jdx in [#6004](https://github.com/jdx/mise/pull/6004)

### 📦️ Dependency Updates

- update taiki-e/install-action digest to 2c73a74 by @renovate[bot] in [#5962](https://github.com/jdx/mise/pull/5962)
- update docker/metadata-action digest to c1e5197 by @renovate[bot] in [#5961](https://github.com/jdx/mise/pull/5961)
- update docker/login-action digest to 184bdaa by @renovate[bot] in [#5958](https://github.com/jdx/mise/pull/5958)

### 📦 Registry

- add vfox-yarn as primary yarn backend by @jdx in [#5982](https://github.com/jdx/mise/pull/5982)
- add missing description field for a lot of tools by @jylenhof in [#5966](https://github.com/jdx/mise/pull/5966)
- rename benthos to redpanda-connect by @risu729 in [#5984](https://github.com/jdx/mise/pull/5984)
- rename coq to rocq by @risu729 in [#5985](https://github.com/jdx/mise/pull/5985)

### Chore

- cargo up by @jdx in [#5992](https://github.com/jdx/mise/pull/5992)

### New Contributors

- @Joniator made their first contribution in [#5981](https://github.com/jdx/mise/pull/5981)
- @jylenhof made their first contribution in [#5966](https://github.com/jdx/mise/pull/5966)

## [2025.8.8](https://github.com/jdx/mise/compare/v2025.8.7..v2025.8.8) - 2025-08-11

### 📚 Documentation

- add documentation for os field in tool configuration by @jdx in [#5947](https://github.com/jdx/mise/pull/5947)

### 📦 Registry

- add bob ([aqua:MordechaiHadad/bob](https://github.com/MordechaiHadad/bob)) by @TyceHerrman in [#5914](https://github.com/jdx/mise/pull/5914)
- support usage on FreeBSD by @risu729 in [#5973](https://github.com/jdx/mise/pull/5973)
- filter out installer for podman by @risu729 in [#5974](https://github.com/jdx/mise/pull/5974)
- use pipx aqua backend by @itochan in [#5971](https://github.com/jdx/mise/pull/5971)
- only use aqua backend for yarn on windows by @jdx in [#5978](https://github.com/jdx/mise/pull/5978)

### Chore

- **(ci)** accept @ in regular expressions for new registry PR titles by @mst-mkt in [#5969](https://github.com/jdx/mise/pull/5969)
- fix registry test filter by @risu729 in [#5942](https://github.com/jdx/mise/pull/5942)
- fix registry test by @risu729 in [#5953](https://github.com/jdx/mise/pull/5953)

### New Contributors

- @itochan made their first contribution in [#5971](https://github.com/jdx/mise/pull/5971)
- @mst-mkt made their first contribution in [#5969](https://github.com/jdx/mise/pull/5969)

## [2025.8.7](https://github.com/jdx/mise/compare/v2025.8.6..v2025.8.7) - 2025-08-06

### 🐛 Bug Fixes

- **(lockfile)** fix multiple lockfile issues with version management by @jdx in [#5907](https://github.com/jdx/mise/pull/5907)
- **(toolset)** properly handle MISE_ADD_PATH from plugins by @jdx in [#5937](https://github.com/jdx/mise/pull/5937)

### 📦 Registry

- add python to gcloud dependencies by @risu729 in [#5936](https://github.com/jdx/mise/pull/5936)

## [2025.8.6](https://github.com/jdx/mise/compare/v2025.8.5..v2025.8.6) - 2025-08-06

### 🚀 Features

- **(tool-stub)** improve stub generation with bin inference, error handling, and fetch mode by @jdx in [#5932](https://github.com/jdx/mise/pull/5932)

### 📦 Registry

- add resvg ([aqua:linebender/resvg](https://github.com/linebender/resvg)) by @TyceHerrman in [#5926](https://github.com/jdx/mise/pull/5926)
- add specstory ([aqua:specstoryai/getspecstory](https://github.com/specstoryai/getspecstory)) by @TyceHerrman in [#5927](https://github.com/jdx/mise/pull/5927)
- add oxker ([aqua:mrjackwills/oxker](https://github.com/mrjackwills/oxker)) by @TyceHerrman in [#5929](https://github.com/jdx/mise/pull/5929)
- add tssh ([aqua:trzsz/trzsz-ssh](https://github.com/trzsz/trzsz-ssh)) by @TyceHerrman in [#5928](https://github.com/jdx/mise/pull/5928)

## [2025.8.5](https://github.com/jdx/mise/compare/v2025.8.4..v2025.8.5) - 2025-08-05

### 🚀 Features

- **(ci)** enhance registry PR validation with strict format checking by @jdx in [#5897](https://github.com/jdx/mise/pull/5897)
- add Model Context Protocol (MCP) server command by @jdx in [#5920](https://github.com/jdx/mise/pull/5920)

### 🐛 Bug Fixes

- **(elixir)** support `.exenv-version` by @risu729 in [#5901](https://github.com/jdx/mise/pull/5901)
- **(env)** improve PATH handling for env._.path directives by @jdx in [#5922](https://github.com/jdx/mise/pull/5922)
- allow devcontainer creation without a git repository by @acesyde in [#5891](https://github.com/jdx/mise/pull/5891)

### 📦 Registry

- add tlrc ([aqua:tldr-pages/tlrc](https://github.com/tldr-pages/tlrc)) by @TyceHerrman in [#5895](https://github.com/jdx/mise/pull/5895)
- support `.terragrunt-version` by @risu729 in [#5903](https://github.com/jdx/mise/pull/5903)
- add lnav ([aqua:tstack/lnav](https://github.com/tstack/lnav)) by @TyceHerrman in [#5896](https://github.com/jdx/mise/pull/5896)
- use aqua backend for yarn by @risu729 in [#5902](https://github.com/jdx/mise/pull/5902)
- add dotenvx ([aqua:dotenvx/dotenvx](https://github.com/dotenvx/dotenvx)) by @TyceHerrman in [#5915](https://github.com/jdx/mise/pull/5915)
- update kubecolor ([aqua:kubecolor/kubecolor](https://github.com/kubecolor/kubecolor)) by @Darwiner in [#5887](https://github.com/jdx/mise/pull/5887)
- add oxlint ([aqua:oxc-project/oxc/oxlint](https://github.com/oxc-project/oxc/oxlint)) by @TyceHerrman in [#5919](https://github.com/jdx/mise/pull/5919)
- add container ([aqua:apple/container](https://github.com/apple/container)) by @TyceHerrman in [#5917](https://github.com/jdx/mise/pull/5917)
- support `.packer-version` by @risu729 in [#5900](https://github.com/jdx/mise/pull/5900)

### Chore

- add synchronize to registry_comment gha by @jdx in [cbb1429](https://github.com/jdx/mise/commit/cbb14294072e9cbd3b0b9f21b2cb0a993a71d5ff)
- fix registry_comment gha by @jdx in [7ce513b](https://github.com/jdx/mise/commit/7ce513be3efe60372f667f76570e16ce0d4a013f)
- run registry test only for changed tools by @risu729 in [#5905](https://github.com/jdx/mise/pull/5905)

### New Contributors

- @Darwiner made their first contribution in [#5887](https://github.com/jdx/mise/pull/5887)
- @zekefast made their first contribution in [#5912](https://github.com/jdx/mise/pull/5912)

## [2025.8.4](https://github.com/jdx/mise/compare/v2025.8.3..v2025.8.4) - 2025-08-03

### 🚀 Features

- **(tasks)** **breaking** Add environment variable directives for mise tasks by @jdx in [#5638](https://github.com/jdx/mise/pull/5638)

## [2025.8.3](https://github.com/jdx/mise/compare/v2025.8.2..v2025.8.3) - 2025-08-03

### 🚀 Features

- **(registry)** add atuin package to registry by @TyceHerrman in [#5883](https://github.com/jdx/mise/pull/5883)
- introduce registry commit type for new tool additions by @jdx in [#5884](https://github.com/jdx/mise/pull/5884)

### 🐛 Bug Fixes

- **(aqua,github)** make asset name matching case-insensitive by @jdx in [#5886](https://github.com/jdx/mise/pull/5886)

### 🚜 Refactor

- **(ci)** separate Alpine release into its own workflow by @jdx in [#5868](https://github.com/jdx/mise/pull/5868)

### 📚 Documentation

- **(changelog)** automate backend links in changelog by @jdx in [#5889](https://github.com/jdx/mise/pull/5889)

### ⚡ Performance

- reduce render env task calls by @jdx in [#5888](https://github.com/jdx/mise/pull/5888)

### 📦 Registry

- add git-lfs ([aqua:git-lfs/git-lfs](https://github.com/git-lfs/git-lfs)) by @TyceHerrman in [#5885](https://github.com/jdx/mise/pull/5885)

## [2025.8.2](https://github.com/jdx/mise/compare/v2025.8.1..v2025.8.2) - 2025-08-02

### 🚀 Features

- **(registry)** add jjui by @TyceHerrman in [#5877](https://github.com/jdx/mise/pull/5877)
- **(registry)** add trunk metalinter by @daveio in [#5875](https://github.com/jdx/mise/pull/5875)

### 🐛 Bug Fixes

- **(python)** Windows OS no longer suffixed with `-shared` by @malept in [#5879](https://github.com/jdx/mise/pull/5879)

### New Contributors

- @daveio made their first contribution in [#5875](https://github.com/jdx/mise/pull/5875)
- @TyceHerrman made their first contribution in [#5877](https://github.com/jdx/mise/pull/5877)

## [2025.8.1](https://github.com/jdx/mise/compare/v2025.8.0..v2025.8.1) - 2025-08-01

### 🐛 Bug Fixes

- node gpg keys by @jdx in [#5866](https://github.com/jdx/mise/pull/5866)

## [2025.8.0](https://github.com/jdx/mise/compare/v2025.7.32..v2025.8.0) - 2025-08-01

### 🚀 Features

- **(registry)** use npm backend for yarn by @mrazauskas in [#5745](https://github.com/jdx/mise/pull/5745)
- **(registry)** add codebuff tool by @zacheryph in [#5856](https://github.com/jdx/mise/pull/5856)

### 🐛 Bug Fixes

- **(go)** implement heuristic-based go module find logic by @risu729 in [#5851](https://github.com/jdx/mise/pull/5851)
- **(node)** Add NodeJS maintainer Antoine du Hamel's new GPG key by @chadlwilson in [#5862](https://github.com/jdx/mise/pull/5862)
- **(pipx)** align HTML backend with PEP 503 registry URL assignment by @acesyde in [#5853](https://github.com/jdx/mise/pull/5853)
- **(registry)** fix balena ubi backend options by @risu729 in [#5861](https://github.com/jdx/mise/pull/5861)
- **(registry)** add aqua backends to tools by @risu729 in [#5863](https://github.com/jdx/mise/pull/5863)

### 📚 Documentation

- fix uv_venv_create_args reference for python by @jasonraimondi in [#5854](https://github.com/jdx/mise/pull/5854)
- expand on env directive examples and formats by @syhol in [#5857](https://github.com/jdx/mise/pull/5857)

### ◀️ Revert

- Revert "docs: fix uv_venv_create_args reference for python" by @jdx in [#5859](https://github.com/jdx/mise/pull/5859)

### New Contributors

- @zacheryph made their first contribution in [#5856](https://github.com/jdx/mise/pull/5856)
- @chadlwilson made their first contribution in [#5862](https://github.com/jdx/mise/pull/5862)
- @jasonraimondi made their first contribution in [#5854](https://github.com/jdx/mise/pull/5854)

## [2025.7.32](https://github.com/jdx/mise/compare/v2025.7.31..v2025.7.32) - 2025-07-31

### 🚀 Features

- **(tool-stubs)** Add human readable comments to stub sizes by @jdx in [#5845](https://github.com/jdx/mise/pull/5845)
- **(tool-stubs)** improve binary path detection in tool stub generator by @jdx in [#5847](https://github.com/jdx/mise/pull/5847)

### 🐛 Bug Fixes

- **(aqua)** support `AND` operator in semver by @risu729 in [#5838](https://github.com/jdx/mise/pull/5838)
- **(cli)** remove empty [platforms] section from generated tool stubs by @jdx in [#5844](https://github.com/jdx/mise/pull/5844)
- **(tool-stubs)** remove comment line from tool stub generator by @jdx in [#5843](https://github.com/jdx/mise/pull/5843)
- **(tool-stubs)** Remove latest version from tool stubs by @jdx in [#5846](https://github.com/jdx/mise/pull/5846)
- **(tool-stubs)** allow -v flag to be passed through to tool stubs by @jdx in [#5848](https://github.com/jdx/mise/pull/5848)

## [2025.7.31](https://github.com/jdx/mise/compare/v2025.7.30..v2025.7.31) - 2025-07-29

### 🚀 Features

- **(tool-stubs)** append to existing tool-stub files instead of overwriting by @jdx in [#5835](https://github.com/jdx/mise/pull/5835)
- **(tool-stubs)** add auto-platform detection from URLs by @jdx in [#5836](https://github.com/jdx/mise/pull/5836)
- Add sops.strict setting for non-strict decryption mode by @pepicrft in [#5378](https://github.com/jdx/mise/pull/5378)

### 🐛 Bug Fixes

- **(tool-stub)** use URL hash as version for HTTP backend with "latest" by @jdx in [#5828](https://github.com/jdx/mise/pull/5828)
- **(tool-stubs)** fix -v and --help flags by @jdx in [#5829](https://github.com/jdx/mise/pull/5829)
- **(tool-stubs)** use 'checksum' field instead of 'blake3' in generated stubs by @jdx in [#5834](https://github.com/jdx/mise/pull/5834)
- dotnet SearchQueryService fallback by @acesyde in [#5824](https://github.com/jdx/mise/pull/5824)
- registry.toml - Specify sbt dependency on java by @jatcwang in [#5827](https://github.com/jdx/mise/pull/5827)

### 🧪 Testing

- remove has test which is failing by @jdx in [4aa9cc9](https://github.com/jdx/mise/commit/4aa9cc973acb1bc34df51f27333a226df3256b69)

### New Contributors

- @jatcwang made their first contribution in [#5827](https://github.com/jdx/mise/pull/5827)

## [2025.7.30](https://github.com/jdx/mise/compare/v2025.7.29..v2025.7.30) - 2025-07-29

### 🚀 Features

- **(registry)** add amp by @jahands in [#5814](https://github.com/jdx/mise/pull/5814)

### 🐛 Bug Fixes

- **(tool-stubs)** fix error messages when it can't find the bin by @jdx in [#5817](https://github.com/jdx/mise/pull/5817)
- misidentifying built-in backend as a plugin backend by @syhol in [#5822](https://github.com/jdx/mise/pull/5822)

### 📚 Documentation

- **(troubleshooting)** path limits on Windows by @W1M0R in [#5815](https://github.com/jdx/mise/pull/5815)

## [2025.7.29](https://github.com/jdx/mise/compare/v2025.7.28..v2025.7.29) - 2025-07-28

### 🐛 Bug Fixes

- **(cli)** stable path env for exec on windows by @W1M0R in [#5790](https://github.com/jdx/mise/pull/5790)
- **(tool-stubs)** platform-specific bin fields by @jdx in [#5812](https://github.com/jdx/mise/pull/5812)
- tool-stub generation with archive downloads by @jdx in [#5811](https://github.com/jdx/mise/pull/5811)

### 📦️ Dependency Updates

- update jdx/mise-action digest to c37c932 by @renovate[bot] in [#5784](https://github.com/jdx/mise/pull/5784)

### New Contributors

- @W1M0R made their first contribution in [#5790](https://github.com/jdx/mise/pull/5790)

## [2025.7.28](https://github.com/jdx/mise/compare/v2025.7.27..v2025.7.28) - 2025-07-27

### 🚀 Features

- **(http)** show retry after for github rate limit by @risu729 in [#5803](https://github.com/jdx/mise/pull/5803)
- **(registry)** add carapace by @jahands in [#5804](https://github.com/jdx/mise/pull/5804)
- **(registry)** add `hatch` by @hasansezertasan in [#5788](https://github.com/jdx/mise/pull/5788)
- tool-stubs by @jdx in [#5795](https://github.com/jdx/mise/pull/5795)
- used shared cache for http backend by @jdx in [#5808](https://github.com/jdx/mise/pull/5808)

### 🐛 Bug Fixes

- **(aqua)** avoid unnecessary head requests in version resolution by @risu729 in [#5800](https://github.com/jdx/mise/pull/5800)
- **(toolset)** use join_paths for MISE_ADD_PATH by @risu729 in [#5785](https://github.com/jdx/mise/pull/5785)
- check lib64 directories for .disable-self-update file by @jdx in [#5809](https://github.com/jdx/mise/pull/5809)

### 🚜 Refactor

- **(aqua)** move alternative backend suggestions into validate by @risu729 in [#5794](https://github.com/jdx/mise/pull/5794)

### 📚 Documentation

- **(tool-stubs)** added shebangs by @jdx in [2d37500](https://github.com/jdx/mise/commit/2d37500e309a61062fc0e821a38be98626176d5d)
- **(tool-stubs)** corrected url syntax by @jdx in [32627be](https://github.com/jdx/mise/commit/32627bec8b3df5060ea9f93dc50003126585e572)
- fix plugin-lua-modules docs to match the vfox lua_mod functions by @syhol in [#5792](https://github.com/jdx/mise/pull/5792)
- fix http backend tool options example by @roele in [#5802](https://github.com/jdx/mise/pull/5802)

### 📦️ Dependency Updates

- update taiki-e/install-action digest to 7fbb30f by @renovate[bot] in [#5786](https://github.com/jdx/mise/pull/5786)
- pin actions/checkout action to 11bd719 by @renovate[bot] in [#5783](https://github.com/jdx/mise/pull/5783)

### New Contributors

- @hasansezertasan made their first contribution in [#5788](https://github.com/jdx/mise/pull/5788)

## [2025.7.27](https://github.com/jdx/mise/compare/v2025.7.26..v2025.7.27) - 2025-07-24

### 🐛 Bug Fixes

- **(copr)** disable self-update by @jdx in [#5780](https://github.com/jdx/mise/pull/5780)
- **(link.md)** correct example comment in mise link documentation by @mmurdockk in [#5760](https://github.com/jdx/mise/pull/5760)
- use github releases in install.sh for non-current version by @jdx in [c2b1ef1](https://github.com/jdx/mise/commit/c2b1ef1c53d736e14fb64365aa1339dc955d6c59)

### New Contributors

- @mmurdockk made their first contribution in [#5760](https://github.com/jdx/mise/pull/5760)

## [2025.7.26](https://github.com/jdx/mise/compare/v2025.7.25..v2025.7.26) - 2025-07-24

### Chore

- use correct release dirname by @jdx in [c8e0b5b](https://github.com/jdx/mise/commit/c8e0b5b42f3d258ec977b68326461d2fc81c4724)

## [2025.7.25](https://github.com/jdx/mise/compare/v2025.7.24..v2025.7.25) - 2025-07-24

### Chore

- updated deps by @jdx in [#5771](https://github.com/jdx/mise/pull/5771)

## [2025.7.24](https://github.com/jdx/mise/compare/v2025.7.23..v2025.7.24) - 2025-07-24

### Chore

- add MISE_INSTALL_FROM_GITHUB option for mise.run by @jdx in [#5772](https://github.com/jdx/mise/pull/5772)

## [2025.7.22](https://github.com/jdx/mise/compare/v2025.7.21..v2025.7.22) - 2025-07-24

### 🚀 Features

- **(doctor)** display # of baked-in aqua registry tools by @jdx in [#5756](https://github.com/jdx/mise/pull/5756)
- **(lock)** `mise lock` enhancements by @jdx in [#5765](https://github.com/jdx/mise/pull/5765)
- registry.toml: add SST by @juxuanu in [#5758](https://github.com/jdx/mise/pull/5758)

### 🐛 Bug Fixes

- **(copr)** fix remaining issues by @jdx in [#5755](https://github.com/jdx/mise/pull/5755)

### 📚 Documentation

- add descriptions for all the tasks by @jdx in [#5764](https://github.com/jdx/mise/pull/5764)

### 📦️ Dependency Updates

- update fedora docker tag to v43 by @renovate[bot] in [#5159](https://github.com/jdx/mise/pull/5159)

### Chore

- **(copr)** chmod +x by @jdx in [71cf6ee](https://github.com/jdx/mise/commit/71cf6eee0d1766bbc214c6cf307b3d7ae300cd33)
- **(hyperfine)** temporarily remove uncached benchmarks since they are not reporting right by @jdx in [#5769](https://github.com/jdx/mise/pull/5769)
- added `mise` shim for devcontainer by @jdx in [#5768](https://github.com/jdx/mise/pull/5768)

### Task-configuration.md

- typo by @mustafa0x in [#5216](https://github.com/jdx/mise/pull/5216)

### New Contributors

- @mustafa0x made their first contribution in [#5216](https://github.com/jdx/mise/pull/5216)
- @juxuanu made their first contribution in [#5758](https://github.com/jdx/mise/pull/5758)

## [2025.7.21](https://github.com/jdx/mise/compare/v2025.7.20..v2025.7.21) - 2025-07-23

### 🚀 Features

- **(packaging)** add COPR publishing workflow and documentation by @jdx in [#5719](https://github.com/jdx/mise/pull/5719)

### 🐛 Bug Fixes

- **(pwsh)** resolve issue caused by previous #5732 patch (hardcoded path) by @IMXEren in [#5753](https://github.com/jdx/mise/pull/5753)
- copr docker building by @jdx in [#5748](https://github.com/jdx/mise/pull/5748)

### 📚 Documentation

- **(README)** mention project alexandria by @jdx in [681bc75](https://github.com/jdx/mise/commit/681bc751025a848411b7dff322cd14d9487dd59f)
- Removes invalid array in redaction example by @EverlastingBugstopper in [#5752](https://github.com/jdx/mise/pull/5752)
- document mise-versions app by @jdx in [785ef24](https://github.com/jdx/mise/commit/785ef24e65259b95f56ecccebe9463a8a0c37519)

### 🧪 Testing

- fix asset detector test on musl by @jdx in [#5744](https://github.com/jdx/mise/pull/5744)

### Chore

- use 302 redirects for curl installs by @jdx in [#5747](https://github.com/jdx/mise/pull/5747)

### New Contributors

- @EverlastingBugstopper made their first contribution in [#5752](https://github.com/jdx/mise/pull/5752)

## [2025.7.20](https://github.com/jdx/mise/compare/v2025.7.19..v2025.7.20) - 2025-07-22

### 🚀 Features

- use mise.run for rosetta tip by @jdx in [#5739](https://github.com/jdx/mise/pull/5739)

### 🐛 Bug Fixes

- **(npm)** use bin/ as bin_paths when installed with bun on windows by @risu729 in [#5725](https://github.com/jdx/mise/pull/5725)

### 📚 Documentation

- remove curl instructions by @jdx in [785d2f2](https://github.com/jdx/mise/commit/785d2f2fe4795b23cb196a70a0b7956707d40437)
- add back in supported os/arch combinations by @jdx in [87b86b0](https://github.com/jdx/mise/commit/87b86b0f4f756dd6b7116192214c25e2995e9939)

### Chore

- set redirect for curl installs by @jdx in [#5740](https://github.com/jdx/mise/pull/5740)
- reduce binary size for linux by @jdx in [#5741](https://github.com/jdx/mise/pull/5741)

## [2025.7.19](https://github.com/jdx/mise/compare/v2025.7.18..v2025.7.19) - 2025-07-22

### 🐛 Bug Fixes

- **(pwsh)** set console encoding to UTF-8 to prevent Unicode garbling by @IMXEren in [#5732](https://github.com/jdx/mise/pull/5732)
- **(registry)** set matching_regex for glab on Windows to pick the correct asset by @risu729 in [#5727](https://github.com/jdx/mise/pull/5727)

### 📚 Documentation

- **(config)** fix alias section name by @malept in [#5736](https://github.com/jdx/mise/pull/5736)
- fix typo in contributing commit message prefixes by @malept in [#5737](https://github.com/jdx/mise/pull/5737)

### Chore

- **(ppa)** wait for gh rate limit by @jdx in [#5721](https://github.com/jdx/mise/pull/5721)
- **(vfox-test)** set GITHUB_TOKEN by @jdx in [cdbb62b](https://github.com/jdx/mise/commit/cdbb62b0f63bcb0a3b650c1d49aefb8c9798c6aa)

### New Contributors

- @malept made their first contribution in [#5736](https://github.com/jdx/mise/pull/5736)

## [2025.7.18](https://github.com/jdx/mise/compare/v2025.7.17..v2025.7.18) - 2025-07-21

### 🚀 Features

- **(registry)** add `jsonschema` CLI tool by @mrazauskas in [#5714](https://github.com/jdx/mise/pull/5714)

### 🐛 Bug Fixes

- mise up parallel execution by @jdx in [#5591](https://github.com/jdx/mise/pull/5591)
- ppa releases by @jdx in [#5717](https://github.com/jdx/mise/pull/5717)

### 📚 Documentation

- add comprehensive CLAUDE.md for Claude Code guidance by @jdx in [#5718](https://github.com/jdx/mise/pull/5718)

### Chore

- ubuntu ppa by @jdx in [#5715](https://github.com/jdx/mise/pull/5715)

## [2025.7.17](https://github.com/jdx/mise/compare/v2025.7.16..v2025.7.17) - 2025-07-19

### 🚀 Features

- consolidate lockfile assets and add URL tracking by @jdx in [#5629](https://github.com/jdx/mise/pull/5629)

### 🐛 Bug Fixes

- **(registry)** use aqua backend for available tools by @risu729 in [#5707](https://github.com/jdx/mise/pull/5707)

### 📚 Documentation

- document auto_install behavior by @jdx in [#5697](https://github.com/jdx/mise/pull/5697)

### 🧪 Testing

- **(registry)** enable disabled tests by @risu729 in [#5708](https://github.com/jdx/mise/pull/5708)
- **(registry)** comment out failing maven test in configuration by @jdx in [ae3e62b](https://github.com/jdx/mise/commit/ae3e62b232ab974058cf7b7c7a05d05086f48e48)

## [2025.7.16](https://github.com/jdx/mise/compare/v2025.7.15..v2025.7.16) - 2025-07-18

### 🐛 Bug Fixes

- mise.run cloudflare worker publish by @jdx in [#5704](https://github.com/jdx/mise/pull/5704)

### Chore

- **(release)** increase timeout for macos tarballs by @jdx in [05e3a45](https://github.com/jdx/mise/commit/05e3a459982745f365d958501492430effab1fc0)
- disable tests for 2025.7.16 by @jdx in [30d3b97](https://github.com/jdx/mise/commit/30d3b974dc3893158c10bfac500ac671407214b3)

## [2025.7.15](https://github.com/jdx/mise/compare/v2025.7.14..v2025.7.15) - 2025-07-18

### 🧪 Testing

- added .release-skip-e2e functionality by @jdx in [#5698](https://github.com/jdx/mise/pull/5698)

## [2025.7.14](https://github.com/jdx/mise/compare/v2025.7.13..v2025.7.14) - 2025-07-18

### 🐛 Bug Fixes

- mise.run cloudflare worker syntax by @jdx in [#5693](https://github.com/jdx/mise/pull/5693)

### 📦️ Dependency Updates

- update rust crate tabled to 0.20 by @renovate[bot] in [#5688](https://github.com/jdx/mise/pull/5688)
- update rust crate indicatif to 0.18 by @renovate[bot] in [#5687](https://github.com/jdx/mise/pull/5687)

## [2025.7.13](https://github.com/jdx/mise/compare/v2025.7.12..v2025.7.13) - 2025-07-18

### 🚀 Features

- https://mise.run/{bash,zsh,fish} by @jdx in [#5677](https://github.com/jdx/mise/pull/5677)
- add opencode tool with description, backends, and test command by @nipuna-perera in [#5679](https://github.com/jdx/mise/pull/5679)

### 🐛 Bug Fixes

- don't follow symlink to ignore symlinks from deletion by @risu729 in [#5672](https://github.com/jdx/mise/pull/5672)
- update completions by @risu729 in [#5682](https://github.com/jdx/mise/pull/5682)
- NoMethodError with Bundler::Installer by @hsbt in [#5678](https://github.com/jdx/mise/pull/5678)

### 📚 Documentation

- fix typo in RUSTUP_TOOLCHAIN env variable name by @anderso in [#5673](https://github.com/jdx/mise/pull/5673)

### 📦️ Dependency Updates

- update jdx/mise-action digest to bfb9fa0 by @renovate[bot] in [#5681](https://github.com/jdx/mise/pull/5681)
- pin dependencies by @renovate[bot] in [#5680](https://github.com/jdx/mise/pull/5680)
- update rust crate console to 0.16 by @renovate[bot] in [#5685](https://github.com/jdx/mise/pull/5685)
- update taiki-e/install-action digest to 4fd6bde by @renovate[bot] in [#5684](https://github.com/jdx/mise/pull/5684)

### New Contributors

- @nipuna-perera made their first contribution in [#5679](https://github.com/jdx/mise/pull/5679)
- @hsbt made their first contribution in [#5678](https://github.com/jdx/mise/pull/5678)
- @anderso made their first contribution in [#5673](https://github.com/jdx/mise/pull/5673)

## [2025.7.12](https://github.com/jdx/mise/compare/v2025.7.11..v2025.7.12) - 2025-07-17

### 🐛 Bug Fixes

- **(file)** remove top level directories in strip_archive_path_components by @risu729 in [#5662](https://github.com/jdx/mise/pull/5662)
- **(npm)** run bun in install_path instead of using --cwd flag of bun by @risu729 in [#5656](https://github.com/jdx/mise/pull/5656)
- **(nushell)** fix `get -i` deprecation by @JoaquinTrinanes in [#5666](https://github.com/jdx/mise/pull/5666)

### ◀️ Revert

- Revert "fix(aqua): improve warnings for packages without repo_owner and repo_name " by @jdx in [#5668](https://github.com/jdx/mise/pull/5668)

### Chore

- update deps by @risu729 in [#5657](https://github.com/jdx/mise/pull/5657)
- update usage by @risu729 in [#5661](https://github.com/jdx/mise/pull/5661)

### New Contributors

- @JoaquinTrinanes made their first contribution in [#5666](https://github.com/jdx/mise/pull/5666)

## [2025.7.11](https://github.com/jdx/mise/compare/v2025.7.10..v2025.7.11) - 2025-07-16

### 🚀 Features

- support extracting 7z archives for static backends by @yjoer in [#5632](https://github.com/jdx/mise/pull/5632)

### 🐛 Bug Fixes

- **(aqua)** improve warnings for packages without repo_owner and repo_name by @risu729 in [#5644](https://github.com/jdx/mise/pull/5644)
- **(generate)** fix task docs inject by @risu729 in [#5651](https://github.com/jdx/mise/pull/5651)
- **(static)** support `strip_components` for zip files by @risu729 in [#5631](https://github.com/jdx/mise/pull/5631)
- private forges by @hamnis in [#5650](https://github.com/jdx/mise/pull/5650)

### 🚜 Refactor

- **(aqua)** move no_aset and error_message checks into validate by @risu729 in [#5649](https://github.com/jdx/mise/pull/5649)

### 📚 Documentation

- **(vfox)** replace deprecated asdf and vfox settings with disable_backends by @risu729 in [#5652](https://github.com/jdx/mise/pull/5652)
- tweak static backend docs by @jdx in [#5627](https://github.com/jdx/mise/pull/5627)

### 🧪 Testing

- **(e2e)** move test_github_auto_detect to correct directory by @risu729 in [#5640](https://github.com/jdx/mise/pull/5640)

### New Contributors

- @hamnis made their first contribution in [#5650](https://github.com/jdx/mise/pull/5650)

## [2025.7.10](https://github.com/jdx/mise/compare/v2025.7.9..v2025.7.10) - 2025-07-14

### 🐛 Bug Fixes

- **(backend)** avoid double untar by @jdx in [#5626](https://github.com/jdx/mise/pull/5626)
- **(github)** handle missing "v" prefix by @jdx in [#5625](https://github.com/jdx/mise/pull/5625)

### 📚 Documentation

- add asset autodetection documentation to GitHub/GitLab backends by @jdx in [#5623](https://github.com/jdx/mise/pull/5623)

## [2025.7.9](https://github.com/jdx/mise/compare/v2025.7.8..v2025.7.9) - 2025-07-14

### 🚀 Features

- **(shim)** prevent mise-specific flags from interfering with shim execution by @jdx in [#5616](https://github.com/jdx/mise/pull/5616)
- github asset auto-detection by @jdx in [#5622](https://github.com/jdx/mise/pull/5622)

### 🐛 Bug Fixes

- resolve GitHub alias tool name parsing and add platform-specific asset support by @jdx in [#5621](https://github.com/jdx/mise/pull/5621)

## [2025.7.8](https://github.com/jdx/mise/compare/v2025.7.7..v2025.7.8) - 2025-07-13

### 🚀 Features

- custom backends through plugins by @jdx in [#5579](https://github.com/jdx/mise/pull/5579)
- nested tool options by @jdx in [#5614](https://github.com/jdx/mise/pull/5614)

### 🐛 Bug Fixes

- accept platform_ or platforms_ in http/github backends by @jdx in [#5608](https://github.com/jdx/mise/pull/5608)

### 📚 Documentation

- correct toml syntax by @jdx in [#5609](https://github.com/jdx/mise/pull/5609)
- removed some markdownlint rules by @jdx in [#5615](https://github.com/jdx/mise/pull/5615)

## [2025.7.7](https://github.com/jdx/mise/compare/v2025.7.4..v2025.7.7) - 2025-07-13

### 🚀 Features

- add static backends (Github, GitLab, and HTTP) by @jdx in [#5602](https://github.com/jdx/mise/pull/5602)
- blake3 support by @jdx in [#5605](https://github.com/jdx/mise/pull/5605)

### 🐛 Bug Fixes

- **(e2e)** simplify test path handling logic by @jdx in [#5600](https://github.com/jdx/mise/pull/5600)
- skip gh release edit on dry run in release workflow by @jdx in [#5603](https://github.com/jdx/mise/pull/5603)

### 📚 Documentation

- **(cursor)** fix conventional commits rule formatting by @jdx in [#5597](https://github.com/jdx/mise/pull/5597)
- **(cursor)** add testing rule for mise codebase by @jdx in [#5598](https://github.com/jdx/mise/pull/5598)

### 🧪 Testing

- disable cmake test for now by @jdx in [d521c31](https://github.com/jdx/mise/commit/d521c31eff1675cd18333c5c258b5d41110fc81a)

### 📦️ Dependency Updates

- pin dependencies by @renovate[bot] in [#5511](https://github.com/jdx/mise/pull/5511)

### Chore

- **(release)** mark a release as draft until assets are added by @risu729 in [#5584](https://github.com/jdx/mise/pull/5584)
- added reverts to git-cliff by @jdx in [#5577](https://github.com/jdx/mise/pull/5577)
- reduce binary size for linux by @jdx in [#5587](https://github.com/jdx/mise/pull/5587)
- `cargo check` fixes by @jdx in [#5589](https://github.com/jdx/mise/pull/5589)
- Merge vfox.rs into jdx/mise monorepo by @jdx in [#5590](https://github.com/jdx/mise/pull/5590)
- Add cursor rule for conventional commits by @jdx in [#5592](https://github.com/jdx/mise/pull/5592)
- Create GitHub action for vfox.rs tests by @jdx in [#5593](https://github.com/jdx/mise/pull/5593)
- tweak paths for test-vfox workflow by @jdx in [0189372](https://github.com/jdx/mise/commit/0189372aadad456cdac459317bb96ae3987cfd15)
- set workspace resolver by @jdx in [#5606](https://github.com/jdx/mise/pull/5606)
- add workspace resolver = 3 by @jdx in [304547a](https://github.com/jdx/mise/commit/304547a0b9a324b5d925c45e2841cadc3f6e938b)
- fix release-plz with workspace by @jdx in [5b3be6e](https://github.com/jdx/mise/commit/5b3be6eb8f06c509964a2b030eccb2f6e006f398)
- only bump mise version for release-plz by @jdx in [8f14d10](https://github.com/jdx/mise/commit/8f14d1014d217c91c36a96beaad4565a3aaf567e)
- add cargo-release by @jdx in [f657db5](https://github.com/jdx/mise/commit/f657db512fdb7ea4f58ac98af729ac6495e61100)
- mise up by @jdx in [4872ae6](https://github.com/jdx/mise/commit/4872ae6b4d63de54de4ac93e72e9a3cd51e20c2e)
- fix release-plz with workspace by @jdx in [bdb7119](https://github.com/jdx/mise/commit/bdb71196d6930091c68a6198d445fa16e108f75e)
- set-version by @jdx in [82fcd4f](https://github.com/jdx/mise/commit/82fcd4f22116bb92e1e615d9f1c03723d02aaaba)
- set-version by @jdx in [54388a4](https://github.com/jdx/mise/commit/54388a419427c664e557aa4ea034e13a2443bb8e)
- set-version by @jdx in [fe0a0a9](https://github.com/jdx/mise/commit/fe0a0a93b27219bd132b39f1f0b522bed1ad2b51)
- set-version by @jdx in [d9f24e2](https://github.com/jdx/mise/commit/d9f24e2b45fb7a9f5c2b795b490ba64a8d9eb207)
- set-version by @jdx in [97f6f4f](https://github.com/jdx/mise/commit/97f6f4febaf03f7c0d6d754701308edeb2287b53)
- set-version by @jdx in [13296e1](https://github.com/jdx/mise/commit/13296e10947ea5a96768e07bd95d009e95bace32)
- set-version by @jdx in [587a707](https://github.com/jdx/mise/commit/587a70744c4127f92cfe9381e7e273ac101c4a4f)
- set-version by @jdx in [1e80d52](https://github.com/jdx/mise/commit/1e80d52144144aaebc804aeef17010980f3a0caf)

## [2025.7.4](https://github.com/jdx/mise/compare/v2025.7.3..v2025.7.4) - 2025-07-11

### 🐛 Bug Fixes

- **(aqua)** align version resolution logic in list_bin_paths by @risu729 in [#5562](https://github.com/jdx/mise/pull/5562)
- Xonsh integration by @jfmontanaro in [#5557](https://github.com/jdx/mise/pull/5557)

### 📚 Documentation

- create comprehensive architecture documentation suite and enhance development guides by @jdx in [d2b4a05](https://github.com/jdx/mise/commit/d2b4a050261b685279c502009f55a3e260b72ff9)

### ◀️ Revert

- Revert "fix(aqua): align version resolution logic in list_bin_paths" by @jdx in [#5574](https://github.com/jdx/mise/pull/5574)

### 📦️ Dependency Updates

- update rust crate bzip2 to 0.6 by @renovate[bot] in [#5568](https://github.com/jdx/mise/pull/5568)
- update rust crate clap_mangen to v0.2.28 by @renovate[bot] in [#5566](https://github.com/jdx/mise/pull/5566)
- update rust crate clap to v4.5.41 by @renovate[bot] in [#5565](https://github.com/jdx/mise/pull/5565)
- update rust crate taplo to 0.14 by @renovate[bot] in [#5158](https://github.com/jdx/mise/pull/5158)

### Chore

- added xonsh for release builds by @jdx in [#5561](https://github.com/jdx/mise/pull/5561)
- enable backtrace lines on panic by @jdx in [#5571](https://github.com/jdx/mise/pull/5571)
- shfmt update by @jdx in [67ee245](https://github.com/jdx/mise/commit/67ee24556f1533c508e422513399ae04ecf6bdaa)

### New Contributors

- @jfmontanaro made their first contribution in [#5557](https://github.com/jdx/mise/pull/5557)

## [2025.7.3](https://github.com/jdx/mise/compare/v2025.7.2..v2025.7.3) - 2025-07-10

### 🚀 Features

- **(registry)** add vfox by @risu729 in [#5551](https://github.com/jdx/mise/pull/5551)

### 🐛 Bug Fixes

- **(aqua)** show other backends suggestion for unsupported package types by @risu729 in [#5547](https://github.com/jdx/mise/pull/5547)
- **(registry)** use aqua and fix ubi options for yamlscript by @risu729 in [#5538](https://github.com/jdx/mise/pull/5538)
- **(registry)** add java and yq to android-sdk dependencies by @risu729 in [#5545](https://github.com/jdx/mise/pull/5545)
- **(schema)** broken $schema ref by @tpansino in [#5540](https://github.com/jdx/mise/pull/5540)
- auto_install_disable_tools env var by @jdx in [#5543](https://github.com/jdx/mise/pull/5543)
- do not overwrite github tokens environment variables by @risu729 in [#5546](https://github.com/jdx/mise/pull/5546)

### Chore

- update Cargo.lock by @risu729 in [#5549](https://github.com/jdx/mise/pull/5549)

### New Contributors

- @tpansino made their first contribution in [#5540](https://github.com/jdx/mise/pull/5540)

## [2025.7.2](https://github.com/jdx/mise/compare/v2025.7.1..v2025.7.2) - 2025-07-09

### 🚀 Features

- **(registry)** add zizmor by @risu729 in [#5519](https://github.com/jdx/mise/pull/5519)
- Add `self_update_available` to `mise doctor` output by @joehorsnell in [#5534](https://github.com/jdx/mise/pull/5534)

### 🐛 Bug Fixes

- **(aqua)** use the version in url to verify and install by @risu729 in [#5537](https://github.com/jdx/mise/pull/5537)
- **(registry)** use aqua for numbat, gokey, golines by @risu729 in [#5518](https://github.com/jdx/mise/pull/5518)
- `self-update` on MITM firewall (attempt #2) by @joehorsnell in [#5459](https://github.com/jdx/mise/pull/5459)
- mise panic in removed directory by @roele in [#5532](https://github.com/jdx/mise/pull/5532)

### 📚 Documentation

- update ubi tag_regex syntax by @grimm26 in [#5529](https://github.com/jdx/mise/pull/5529)

### 🧪 Testing

- disable yamlscript test by @jdx in [#5536](https://github.com/jdx/mise/pull/5536)

### New Contributors

- @grimm26 made their first contribution in [#5529](https://github.com/jdx/mise/pull/5529)

## [2025.7.1](https://github.com/jdx/mise/compare/v2025.7.0..v2025.7.1) - 2025-07-06

### 🚀 Features

- **(aqua)** add support for zst compressed assets by @andreabedini in [#5495](https://github.com/jdx/mise/pull/5495)
- **(registry)** import package descriptions from aqua and add os specifier for tuist by @matracey in [#5487](https://github.com/jdx/mise/pull/5487)

### 🐛 Bug Fixes

- **(aqua)** handle hard links in aqua packages (attempt #2) by @risu729 in [#5486](https://github.com/jdx/mise/pull/5486)
- **(aqua)** apply correct `version_override` by @risu729 in [#5474](https://github.com/jdx/mise/pull/5474)
- **(erlang)** fix install_precompiled method signature for unsupported os by @roele in [#5503](https://github.com/jdx/mise/pull/5503)
- **(java)** relax version filter regex for JetBrains builds by @roele in [#5508](https://github.com/jdx/mise/pull/5508)
- **(registry)** use aqua backend for bat by @risu729 in [#5490](https://github.com/jdx/mise/pull/5490)
- **(registry)** use pipx backend for aws-sam on windows by @risu729 in [#5491](https://github.com/jdx/mise/pull/5491)
- enhance self-update for musl targets by @roele in [#5502](https://github.com/jdx/mise/pull/5502)
- include arch and os settings in cache keys by @risu729 in [#5504](https://github.com/jdx/mise/pull/5504)

### 🧪 Testing

- **(registry)** enable youtube-dl test by @risu729 in [#5492](https://github.com/jdx/mise/pull/5492)

### 📦️ Dependency Updates

- update swatinem/rust-cache digest to 98c8021 by @renovate[bot] in [#5512](https://github.com/jdx/mise/pull/5512)

### New Contributors

- @matracey made their first contribution in [#5487](https://github.com/jdx/mise/pull/5487)
- @andreabedini made their first contribution in [#5495](https://github.com/jdx/mise/pull/5495)

## [2025.7.0](https://github.com/jdx/mise/compare/v2025.6.8..v2025.7.0) - 2025-07-01

### 🚀 Features

- **(registry)** adds gemini-cli by @risu729 in [#5447](https://github.com/jdx/mise/pull/5447)
- **(registry)** adds npm backended tools by @risu729 in [#5446](https://github.com/jdx/mise/pull/5446)
- **(registry)** add powershell alias by @risu729 in [#5449](https://github.com/jdx/mise/pull/5449)
- **(registry)** add dagu by @yottahmd in [#5476](https://github.com/jdx/mise/pull/5476)
- **(registry)** update aws-sam backends to include aqua source by @yashikota in [#5461](https://github.com/jdx/mise/pull/5461)
- **(registry)** use ubi backend for youtube-dl nightly releases by @risu729 in [#5466](https://github.com/jdx/mise/pull/5466)

### 🐛 Bug Fixes

- **(aqua)** update victoria-metrics package name casing by @shikharbhardwaj in [#5483](https://github.com/jdx/mise/pull/5483)
- **(aqua)** handle hard links in aqua packages by @risu729 in [#5463](https://github.com/jdx/mise/pull/5463)
- **(bun)** enhance architecture detection for musl targets by @roele in [#5450](https://github.com/jdx/mise/pull/5450)
- **(erlang)** use precompiled ubuntu binaries on GHA by @paradox460 in [#5439](https://github.com/jdx/mise/pull/5439)
- **(erlang)** add `install_precompiled` for unsupported os by @risu729 in [#5479](https://github.com/jdx/mise/pull/5479)
- **(registry)** use aqua backend for cargo-make by @risu729 in [#5465](https://github.com/jdx/mise/pull/5465)
- **(registry)** use aqua backends for all available tools by @risu729 in [#5467](https://github.com/jdx/mise/pull/5467)
- `parse_command` passing `-c` flag to cmd.exe by @IMXEren in [#5441](https://github.com/jdx/mise/pull/5441)

### 🧪 Testing

- **(registry)** disable bitwarden test by @risu729 in [#5468](https://github.com/jdx/mise/pull/5468)

### ◀️ Revert

- Revert "chore(deps): pin dependencies" by @jdx in [#5453](https://github.com/jdx/mise/pull/5453)
- Revert "fix(aqua): handle hard links in aqua packages" by @jdx in [#5485](https://github.com/jdx/mise/pull/5485)

### 📦️ Dependency Updates

- pin dependencies by @renovate[bot] in [#5443](https://github.com/jdx/mise/pull/5443)
- update jdx/mise-action digest to 5cb1df6 by @renovate[bot] in [#5444](https://github.com/jdx/mise/pull/5444)

### Chore

- disable automatic cargo up due to windows build failure in homedir crate by @jdx in [7570d0a](https://github.com/jdx/mise/commit/7570d0a95498d7b5626645fe3065429e19d0b26e)

### Ci

- **(test)** run `apt-get update` before `apt-get install` by @risu729 in [#5448](https://github.com/jdx/mise/pull/5448)

### New Contributors

- @yashikota made their first contribution in [#5461](https://github.com/jdx/mise/pull/5461)
- @yottahmd made their first contribution in [#5476](https://github.com/jdx/mise/pull/5476)
- @IMXEren made their first contribution in [#5441](https://github.com/jdx/mise/pull/5441)

## [2025.6.8](https://github.com/jdx/mise/compare/v2025.6.7..v2025.6.8) - 2025-06-26

### 🚀 Features

- **(java)** add support for tar.xz in Java core plugin to support RedHat JDKs by @roele in [#5354](https://github.com/jdx/mise/pull/5354)
- **(registry)** add osv-scanner by @scop in [#5413](https://github.com/jdx/mise/pull/5413)
- **(registry)** add scorecard by @scop in [#5410](https://github.com/jdx/mise/pull/5410)
- **(registry)** add docker cli by @acesyde in [#5344](https://github.com/jdx/mise/pull/5344)
- **(registry)** add claude code by @lelouvincx in [#5420](https://github.com/jdx/mise/pull/5420)
- **(registry)** add aws `cfn-lint` by @garysassano in [#5434](https://github.com/jdx/mise/pull/5434)
- added graphite by @jdx in [#5429](https://github.com/jdx/mise/pull/5429)

### 🐛 Bug Fixes

- **(erlang)** use precompiled binaries for linux ubuntu by @paradox460 in [#5402](https://github.com/jdx/mise/pull/5402)
- **(ubi)** checksum generation might fail if extract_all option is used by @roele in [#5394](https://github.com/jdx/mise/pull/5394)
- `self-update` on MITM firewall by @joehorsnell in [#5387](https://github.com/jdx/mise/pull/5387)
- lint warning by @jdx in [#5425](https://github.com/jdx/mise/pull/5425)
- only warn on toolset resolve errors by @jdx in [#5435](https://github.com/jdx/mise/pull/5435)

### 🚜 Refactor

- **(registry)** use pipx for semgrep by @scop in [#5423](https://github.com/jdx/mise/pull/5423)
- **(registry)** add backends and tests by @risu729 in [#5388](https://github.com/jdx/mise/pull/5388)

### ◀️ Revert

- Revert "fix: `self-update` on MITM firewall" by @jdx in [#5427](https://github.com/jdx/mise/pull/5427)

### Ci

- unpin hyperfine by @risu729 in [#5411](https://github.com/jdx/mise/pull/5411)

### New Contributors

- @paradox460 made their first contribution in [#5402](https://github.com/jdx/mise/pull/5402)
- @lelouvincx made their first contribution in [#5420](https://github.com/jdx/mise/pull/5420)

## [2025.6.7](https://github.com/jdx/mise/compare/v2025.6.6..v2025.6.7) - 2025-06-23

### 🐛 Bug Fixes

- **(aqua)** fix versions order by @risu729 in [#5406](https://github.com/jdx/mise/pull/5406)

### Ci

- use pinnable tag of taiki-e/install-action by @risu729 in [#5405](https://github.com/jdx/mise/pull/5405)

## [2025.6.6](https://github.com/jdx/mise/compare/v2025.6.5..v2025.6.6) - 2025-06-23

### 🚀 Features

- **(registry)** add wash by @jtakakura in [#5386](https://github.com/jdx/mise/pull/5386)

### 🐛 Bug Fixes

- **(aqua)** parse consecutive pipes in aqua templates by @risu729 in [#5385](https://github.com/jdx/mise/pull/5385)
- **(aqua)** use versions list to install correct version by @risu729 in [#5371](https://github.com/jdx/mise/pull/5371)
- **(registry)** talosctl use aqua by @mangkoran in [#5348](https://github.com/jdx/mise/pull/5348)
- **(registry)** use aqua backend for watchexec by @risu729 in [#5390](https://github.com/jdx/mise/pull/5390)
- **(shim)** improve resolve_symlink for Windows by @qianlongzt in [#5361](https://github.com/jdx/mise/pull/5361)
- add compression-zip-deflate feature on self_update crate for windows target by @roele in [#5391](https://github.com/jdx/mise/pull/5391)
- suppress hint on 'cargo search mise' command by @roele in [#5400](https://github.com/jdx/mise/pull/5400)

### 📚 Documentation

- Fix typo in README.md - Install mise by @cytsai1008 in [#5366](https://github.com/jdx/mise/pull/5366)
- Document trivial task syntax by @JayBazuzi in [#5352](https://github.com/jdx/mise/pull/5352)

### 🧪 Testing

- **(registry)** fix vultr test by @risu729 in [#5372](https://github.com/jdx/mise/pull/5372)

### 📦️ Dependency Updates

- update autofix-ci/action action to v1.3.2 by @renovate[bot] in [#5377](https://github.com/jdx/mise/pull/5377)
- update docker/setup-buildx-action digest to e468171 by @renovate[bot] in [#5376](https://github.com/jdx/mise/pull/5376)

### Chore

- update expr-lang crate to v0.3.2 by @risu729 in [#5364](https://github.com/jdx/mise/pull/5364)
- show curl error by @jdx in [729aa4a](https://github.com/jdx/mise/commit/729aa4a6279cbb8dd8b1d81e8726d252ad2ad2bc)
- fix latest version fetch by @jdx in [729aadc](https://github.com/jdx/mise/commit/729aadc83e042b276e3ebd3ae378a7e647a54bc0)
- update vfox.rs crate to v1.0.3 by @risu729 in [#5393](https://github.com/jdx/mise/pull/5393)
- updated deps by @jdx in [#5403](https://github.com/jdx/mise/pull/5403)

### Ci

- use cargo info to retrieve latest mise version by @risu729 in [#5401](https://github.com/jdx/mise/pull/5401)

### New Contributors

- @jtakakura made their first contribution in [#5386](https://github.com/jdx/mise/pull/5386)
- @JayBazuzi made their first contribution in [#5352](https://github.com/jdx/mise/pull/5352)
- @cytsai1008 made their first contribution in [#5366](https://github.com/jdx/mise/pull/5366)

## [2025.6.5](https://github.com/jdx/mise/compare/v2025.6.4..v2025.6.5) - 2025-06-16

### 🚀 Features

- **(registry)** add diffoci by @mangkoran in [#5350](https://github.com/jdx/mise/pull/5350)

### 🐛 Bug Fixes

- **(registry)** use mintoolkit/mint for docker-slim by @risu729 in [#5351](https://github.com/jdx/mise/pull/5351)
- **(schema)** add missing tool options to schema by @risu729 in [#5356](https://github.com/jdx/mise/pull/5356)
- only show deprecation if not using 'tools-version' by @timfallmk in [#5290](https://github.com/jdx/mise/pull/5290)

### New Contributors

- @timfallmk made their first contribution in [#5290](https://github.com/jdx/mise/pull/5290)

## [2025.6.4](https://github.com/jdx/mise/compare/v2025.6.3..v2025.6.4) - 2025-06-13

### 🐛 Bug Fixes

- **(registry)** use aqua for checkov by @risu729 in [#5343](https://github.com/jdx/mise/pull/5343)

### ◀️ Revert

- fix(aqua): parse templates in version_filter by @risu729 in [#5345](https://github.com/jdx/mise/pull/5345)

## [2025.6.3](https://github.com/jdx/mise/compare/v2025.6.2..v2025.6.3) - 2025-06-13

### 🚀 Features

- support matching_regex from the ubi backend by @yjoer in [#5320](https://github.com/jdx/mise/pull/5320)

### 🐛 Bug Fixes

- **(aqua)** parse templates in version_filter by @risu729 in [#5341](https://github.com/jdx/mise/pull/5341)
- **(registry)** use extract_all for docker-slim by @risu729 in [#5342](https://github.com/jdx/mise/pull/5342)

### 🚜 Refactor

- **(getting-started)** update powershell profile instructions by @Armaldio in [#5340](https://github.com/jdx/mise/pull/5340)

### 📦️ Dependency Updates

- update docker/build-push-action digest to 2634353 by @renovate[bot] in [#5338](https://github.com/jdx/mise/pull/5338)
- update jdx/mise-action digest to 13abe50 by @renovate[bot] in [#5339](https://github.com/jdx/mise/pull/5339)

### New Contributors

- @yjoer made their first contribution in [#5320](https://github.com/jdx/mise/pull/5320)

## [2025.6.2](https://github.com/jdx/mise/compare/v2025.6.1..v2025.6.2) - 2025-06-12

### 🚀 Features

- **(aqua)** support cosign bundle option by @risu729 in [#5314](https://github.com/jdx/mise/pull/5314)
- **(registry)** add xcodes by @MontakOleg in [#5321](https://github.com/jdx/mise/pull/5321)
- **(registry)** add typstyle by @3w36zj6 in [#5319](https://github.com/jdx/mise/pull/5319)

### 🐛 Bug Fixes

- **(cli/doctor)** reduce severity of new version to warnings by @risu729 in [#5317](https://github.com/jdx/mise/pull/5317)
- **(doctor)** ignored config roots not displaying by @jdx in [#5336](https://github.com/jdx/mise/pull/5336)
- ls command does not respect MISE_COLOR value by @roele in [#5322](https://github.com/jdx/mise/pull/5322)

### 📚 Documentation

- Update contributing.md by @GitToby in [#5332](https://github.com/jdx/mise/pull/5332)
- add instructions to create/open pwsh profile file by @Armaldio in [#5316](https://github.com/jdx/mise/pull/5316)

### New Contributors

- @Armaldio made their first contribution in [#5316](https://github.com/jdx/mise/pull/5316)
- @GitToby made their first contribution in [#5332](https://github.com/jdx/mise/pull/5332)

## [2025.6.1](https://github.com/jdx/mise/compare/v2025.6.0..v2025.6.1) - 2025-06-09

### 🚀 Features

- **(aqua)** support no_asset and error_message by @risu729 in [#5303](https://github.com/jdx/mise/pull/5303)
- **(registry)** use ubi backend for func-e by @risu729 in [#5273](https://github.com/jdx/mise/pull/5273)

### 🐛 Bug Fixes

- **(task)** use empty string for the default value of option by @risu729 in [#5309](https://github.com/jdx/mise/pull/5309)

### 📚 Documentation

- **(registry)** fix links of registry by @risu729 in [#5266](https://github.com/jdx/mise/pull/5266)
- **(registry)** fix links to tools by @risu729 in [#5272](https://github.com/jdx/mise/pull/5272)
- update example with `pnpm` by @mrazauskas in [#5306](https://github.com/jdx/mise/pull/5306)

### 🧪 Testing

- **(registry)** fix test typos by @risu729 in [#5269](https://github.com/jdx/mise/pull/5269)

### 🛡️ Security

- **(security)** prevent untarring outside expected path by @jdx in [#5279](https://github.com/jdx/mise/pull/5279)

### New Contributors

- @mrazauskas made their first contribution in [#5306](https://github.com/jdx/mise/pull/5306)

## [2025.6.0](https://github.com/jdx/mise/compare/v2025.5.17..v2025.6.0) - 2025-06-02

### 🐛 Bug Fixes

- race condition with uv_venv by @jdx in [#5262](https://github.com/jdx/mise/pull/5262)
- disable victoria-metrics test by @jdx in [11bda4b](https://github.com/jdx/mise/commit/11bda4bda97bd02f6a8cae2c7f345846769ff776)

## [2025.5.17](https://github.com/jdx/mise/compare/v2025.5.16..v2025.5.17) - 2025-05-31

### 🚀 Features

- add railway cli by @jahands in [#5083](https://github.com/jdx/mise/pull/5083)

### 🐛 Bug Fixes

- **(zig)** exclude mach version from version list by @mangkoran in [#5240](https://github.com/jdx/mise/pull/5240)
- refresh settings by @jdx in [#5252](https://github.com/jdx/mise/pull/5252)

### ⚡ Performance

- re-enable parallelism for `mise up` by @jdx in [#5249](https://github.com/jdx/mise/pull/5249)

## [2025.5.16](https://github.com/jdx/mise/compare/v2025.5.15..v2025.5.16) - 2025-05-29

### 🐛 Bug Fixes

- ensure config is always wrapped in Result by @jdx in [#5223](https://github.com/jdx/mise/pull/5223)

### ⚡ Performance

- improve init performance by @jdx in [#5231](https://github.com/jdx/mise/pull/5231)

### Chore

- remove hyperfine from main builds by @jdx in [#5226](https://github.com/jdx/mise/pull/5226)

## [2025.5.15](https://github.com/jdx/mise/compare/v2025.5.14..v2025.5.15) - 2025-05-28

### 🚀 Features

- **(registry)** add aqua backend for maven by @ZeroAurora in [#5219](https://github.com/jdx/mise/pull/5219)

### 🐛 Bug Fixes

- **(zig)** **breaking** get tarball url from download index by @mangkoran in [#5182](https://github.com/jdx/mise/pull/5182)
- **(zig)** get version list from download index by @mangkoran in [#5217](https://github.com/jdx/mise/pull/5217)
- use a better completion dir for more compatibility by @ken-kuro in [#5207](https://github.com/jdx/mise/pull/5207)
- set handler for ctrlc on windows shell by @L0RD-ZER0 in [#5209](https://github.com/jdx/mise/pull/5209)
- prevent go installation failure on go.mod version mismatch by @roele in [#5212](https://github.com/jdx/mise/pull/5212)
- mise run --cd <dir> not working with latest mise by @roele in [#5221](https://github.com/jdx/mise/pull/5221)

### 📚 Documentation

- update dependencies section in contributing.md by @LuckyWindsck in [#5200](https://github.com/jdx/mise/pull/5200)

### Chore

- disable auto cargo up by @jdx in [3306f6e](https://github.com/jdx/mise/commit/3306f6ef726fe85d71163121497e1d5dd5cd73ca)

### New Contributors

- @L0RD-ZER0 made their first contribution in [#5209](https://github.com/jdx/mise/pull/5209)

## [2025.5.14](https://github.com/jdx/mise/compare/v2025.5.13..v2025.5.14) - 2025-05-26

### 🐛 Bug Fixes

- installing tools with postinstall hooks fails by @roele in [#5191](https://github.com/jdx/mise/pull/5191)
- prefer offline when executing shims by @jdx in [#5195](https://github.com/jdx/mise/pull/5195)
- multi-line task output is shown in bold by @roele in [#5197](https://github.com/jdx/mise/pull/5197)

### ⚡ Performance

- improve tool loading performance in async code by @jdx in [#5198](https://github.com/jdx/mise/pull/5198)

## [2025.5.13](https://github.com/jdx/mise/compare/v2025.5.12..v2025.5.13) - 2025-05-26

### 🐛 Bug Fixes

- output was silenced on task fail with keep-order by @artemisart in [#5175](https://github.com/jdx/mise/pull/5175)
- avoid mapfile to run e2e tests on macOS (bash 3.2) by @artemisart in [#5170](https://github.com/jdx/mise/pull/5170)
- flaky keep-order e2e test by @artemisart in [#5178](https://github.com/jdx/mise/pull/5178)
- watch mise.lock for changes by @jdx in [#5184](https://github.com/jdx/mise/pull/5184)
- remote task dependency does not work by @roele in [#5183](https://github.com/jdx/mise/pull/5183)
- rayon -> tokio by @jdx in [#5172](https://github.com/jdx/mise/pull/5172)
- cache results from version host by @jdx in [#5187](https://github.com/jdx/mise/pull/5187)
- cache results from version host for aqua packages by @jdx in [#5188](https://github.com/jdx/mise/pull/5188)

### 📚 Documentation

- standardize subcommand format to 'u|use' for consistency by @LuckyWindsck in [#5167](https://github.com/jdx/mise/pull/5167)
- clarify how to enable ideomatic version file reading for ruby by @amkisko in [#5163](https://github.com/jdx/mise/pull/5163)

### 🧪 Testing

- added perf test by @jdx in [#5179](https://github.com/jdx/mise/pull/5179)
- skip benchmark errors for now by @jdx in [#5186](https://github.com/jdx/mise/pull/5186)

### Chore

- fix clippy issue in xonsh by @jdx in [#5180](https://github.com/jdx/mise/pull/5180)
- improve shfmt linter by @jdx in [#5181](https://github.com/jdx/mise/pull/5181)
- cargo up by @jdx in [3ece604](https://github.com/jdx/mise/commit/3ece60479bd8b8e6a00a02b83c0afdd544d95034)
- fix hyperfine step summary by @jdx in [36ab4a1](https://github.com/jdx/mise/commit/36ab4a12ffed85f07ce918d1a21a6da9f7ebef2c)
- adjust perf thresholds by @jdx in [4113a3b](https://github.com/jdx/mise/commit/4113a3b82c3fca4eae0dbe7845ec2d513f5b6c8b)

### New Contributors

- @amkisko made their first contribution in [#5163](https://github.com/jdx/mise/pull/5163)
- @LuckyWindsck made their first contribution in [#5167](https://github.com/jdx/mise/pull/5167)

## [2025.5.12](https://github.com/jdx/mise/compare/v2025.5.11..v2025.5.12) - 2025-05-25

### 🐛 Bug Fixes

- read global/system config file tasks properly by @jdx in [#5169](https://github.com/jdx/mise/pull/5169)
- typo in time! parallelize_tasks by @artemisart in [#5171](https://github.com/jdx/mise/pull/5171)

### 🧪 Testing

- disable non-working zig test by @jdx in [2ffb7ea](https://github.com/jdx/mise/commit/2ffb7eaa22e3623363dd153d581bb1a17da78483)

### New Contributors

- @artemisart made their first contribution in [#5171](https://github.com/jdx/mise/pull/5171)

## [2025.5.11](https://github.com/jdx/mise/compare/v2025.5.10..v2025.5.11) - 2025-05-23

### 🚀 Features

- **(registry)** add victoriametrics by @shikharbhardwaj in [#5161](https://github.com/jdx/mise/pull/5161)
- added dotslash by @jdx in [#5165](https://github.com/jdx/mise/pull/5165)

### 🐛 Bug Fixes

- **(registry)** remove full from taplo by @risu729 in [#5160](https://github.com/jdx/mise/pull/5160)
- mise registry links for ubi with exe selector by @mnm364 in [#5156](https://github.com/jdx/mise/pull/5156)
- mise settings add idiomatic_version_file_enable_tools stores duplicates in config by @roele in [#5162](https://github.com/jdx/mise/pull/5162)
- infinite sourcing loop on bash-completion by @ken-kuro in [#5150](https://github.com/jdx/mise/pull/5150)

### 🧪 Testing

- disable mockolo since linux does not work anymore by @jdx in [5387d70](https://github.com/jdx/mise/commit/5387d7012d65b3da3dde12cd0a0eb07288b2d8f6)

### New Contributors

- @ken-kuro made their first contribution in [#5150](https://github.com/jdx/mise/pull/5150)
- @shikharbhardwaj made their first contribution in [#5161](https://github.com/jdx/mise/pull/5161)

## [2025.5.10](https://github.com/jdx/mise/compare/v2025.5.9..v2025.5.10) - 2025-05-22

### 🚀 Features

- **(registry)** add process-compose by @evanleck in [#4788](https://github.com/jdx/mise/pull/4788)
- **(registry)** add tailpipe by @pdecat in [#4858](https://github.com/jdx/mise/pull/4858)
- mise search by @roele in [#5153](https://github.com/jdx/mise/pull/5153)

### 🐛 Bug Fixes

- **(aqua)** windows exe fix by @jdx in [#5154](https://github.com/jdx/mise/pull/5154)

### 🧪 Testing

- disable failing edit test by @jdx in [8698bce](https://github.com/jdx/mise/commit/8698bce774eafa86afa9d5b56a225fa6cdbe6ea1)

### Chore

- disable failing docker dev build by @jdx in [496c1c9](https://github.com/jdx/mise/commit/496c1c91545ed7f013726cd48e746835bdf570d8)
- temporarily disable cargo up to fix build by @jdx in [90c66b7](https://github.com/jdx/mise/commit/90c66b7b561e81efe7d951a0ce9574c11e7b91a7)

### New Contributors

- @evanleck made their first contribution in [#4788](https://github.com/jdx/mise/pull/4788)

## [2025.5.9](https://github.com/jdx/mise/compare/v2025.5.8..v2025.5.9) - 2025-05-21

### 🚀 Features

- **(registry)** add microsoft `edit` by @garysassano in [#5145](https://github.com/jdx/mise/pull/5145)
- added buildifier by @jdx in [#5142](https://github.com/jdx/mise/pull/5142)
- add shims in REMOTE ENV by @acesyde in [#5139](https://github.com/jdx/mise/pull/5139)

### 🐛 Bug Fixes

- **(aqua)** use complete_windows_ext by @jdx in [#5146](https://github.com/jdx/mise/pull/5146)
- **(registry)** support editorconfig-checker in windows by @risu729 in [#5125](https://github.com/jdx/mise/pull/5125)
- SSH remote tasks do not support organizations in repository path by @roele in [#5124](https://github.com/jdx/mise/pull/5124)
- SSH remote tasks do not support organizations in repository path by @roele in [#5132](https://github.com/jdx/mise/pull/5132)

### 📚 Documentation

- squeeze spaces when migrating from asdf by @maximd in [#5131](https://github.com/jdx/mise/pull/5131)

### Chore

- pin github actions by @jdx in [bf18644](https://github.com/jdx/mise/commit/bf1864472c3ed587fbdb497722849cf6cfacca5c)
- use renovate to pin github actions by @jdx in [b80d8e3](https://github.com/jdx/mise/commit/b80d8e3ffe73d315c4214f77dedcf4cce7a54032)
- disable mold in ci by @jdx in [#5128](https://github.com/jdx/mise/pull/5128)
- fix buildifier test by @jdx in [232a4c6](https://github.com/jdx/mise/commit/232a4c641fedc9dfb83ce048ad5b47253b139854)

### New Contributors

- @maximd made their first contribution in [#5131](https://github.com/jdx/mise/pull/5131)

## [2025.5.8](https://github.com/jdx/mise/compare/v2025.5.7..v2025.5.8) - 2025-05-18

### 🚀 Features

- **(registry)** added astro by @mnm364 in [#5106](https://github.com/jdx/mise/pull/5106)

### 🐛 Bug Fixes

- **(registry)** use aqua for delta by @risu729 in [#5116](https://github.com/jdx/mise/pull/5116)
- elixir bin name on windows by @arilence in [#5107](https://github.com/jdx/mise/pull/5107)

### Chore

- create a detached signature when signing the source tarball by @digital-wonderland in [#5108](https://github.com/jdx/mise/pull/5108)

### New Contributors

- @arilence made their first contribution in [#5107](https://github.com/jdx/mise/pull/5107)

## [2025.5.7](https://github.com/jdx/mise/compare/v2025.5.6..v2025.5.7) - 2025-05-18

### 🐛 Bug Fixes

- using custom port with SSH based remote tasks by @roele in [#5110](https://github.com/jdx/mise/pull/5110)
- update rabbitmq backend by @SerhiiFesenko in [#5115](https://github.com/jdx/mise/pull/5115)
- maven-mvnd does not install with aqua by @roele in [#5117](https://github.com/jdx/mise/pull/5117)

### New Contributors

- @SerhiiFesenko made their first contribution in [#5115](https://github.com/jdx/mise/pull/5115)

## [2025.5.6](https://github.com/jdx/mise/compare/v2025.5.5..v2025.5.6) - 2025-05-17

### 🚀 Features

- **(registry)** add oauth2c by @kklee998 in [#5056](https://github.com/jdx/mise/pull/5056)
- use new Java metadata source by @roele in [#5089](https://github.com/jdx/mise/pull/5089)

### 🐛 Bug Fixes

- **(config)** project root for files in .config/ or mise/ by @scop in [#5102](https://github.com/jdx/mise/pull/5102)
- Clarify some of the filters and fix the config_root filter example by @afranchuk in [#5086](https://github.com/jdx/mise/pull/5086)

### 🚜 Refactor

- **(registry)** use aqua for rclone by @scop in [#5096](https://github.com/jdx/mise/pull/5096)

### 📚 Documentation

- **(tasks)** point to `dir` config for task default cwd by @scop in [#5103](https://github.com/jdx/mise/pull/5103)
- remove go.mod from idiomatic version files by @Gandem in [#5090](https://github.com/jdx/mise/pull/5090)
- remove stray backquote from toml-tasks by @scop in [#5097](https://github.com/jdx/mise/pull/5097)
- add some missing vue interpolation escapes by @scop in [#5099](https://github.com/jdx/mise/pull/5099)
- remove some references to rtx by @jdx in [#5105](https://github.com/jdx/mise/pull/5105)

### 📦️ Dependency Updates

- update dependency node to v22 by @renovate[bot] in [#5093](https://github.com/jdx/mise/pull/5093)

### Chore

- sign source tarball by @digital-wonderland in [#5087](https://github.com/jdx/mise/pull/5087)

### New Contributors

- @digital-wonderland made their first contribution in [#5087](https://github.com/jdx/mise/pull/5087)
- @kklee998 made their first contribution in [#5056](https://github.com/jdx/mise/pull/5056)
- @afranchuk made their first contribution in [#5086](https://github.com/jdx/mise/pull/5086)
- @Gandem made their first contribution in [#5090](https://github.com/jdx/mise/pull/5090)

## [2025.5.5](https://github.com/jdx/mise/compare/v2025.5.4..v2025.5.5) - 2025-05-15

### 🚀 Features

- **(registry)** add pinact by @3w36zj6 in [#5061](https://github.com/jdx/mise/pull/5061)
- **(registry)** add ghalint by @risu729 in [#5063](https://github.com/jdx/mise/pull/5063)
- new "enable-tools" option by @zeitlinger in [#4784](https://github.com/jdx/mise/pull/4784)

### 📚 Documentation

- hide `ls --offline` flag that is a no-op by @jdx in [#5068](https://github.com/jdx/mise/pull/5068)

### Chore

- add pr comment for new tools by @jdx in [#5067](https://github.com/jdx/mise/pull/5067)
- set comment-tag for registry pr comment by @jdx in [#5069](https://github.com/jdx/mise/pull/5069)
- run multiple test-tool jobs by @jdx in [#5070](https://github.com/jdx/mise/pull/5070)
- fix typo in registry comment by @jdx in [#5071](https://github.com/jdx/mise/pull/5071)
- bump zip-rs version by @hkoosha in [#5073](https://github.com/jdx/mise/pull/5073)

### New Contributors

- @3w36zj6 made their first contribution in [#5061](https://github.com/jdx/mise/pull/5061)

## [2025.5.4](https://github.com/jdx/mise/compare/v2025.5.3..v2025.5.4) - 2025-05-14

### 🚀 Features

- **(registry)** add sshi by @scop in [#5048](https://github.com/jdx/mise/pull/5048)
- **(registry)** added Neon CLI by @joehorsnell in [#4994](https://github.com/jdx/mise/pull/4994)

### 🐛 Bug Fixes

- **(registry)** update glab ubi provider by @StingRayZA in [#5052](https://github.com/jdx/mise/pull/5052)
- mise panics if CI env var isn't a boolean by @roele in [#5059](https://github.com/jdx/mise/pull/5059)
- `aqua` version test by @joehorsnell in [#5038](https://github.com/jdx/mise/pull/5038)
- run hook-env after trusting config file by @jdx in [#5062](https://github.com/jdx/mise/pull/5062)

### 🚜 Refactor

- **(hooks)** remove duplicated code by @risu729 in [#5036](https://github.com/jdx/mise/pull/5036)

### 📚 Documentation

- fix add_predicate handler in neovim cookbook by @okuuva in [#5044](https://github.com/jdx/mise/pull/5044)
- improve treesitter queries in neovim cookbook by @okuuva in [#5045](https://github.com/jdx/mise/pull/5045)

### New Contributors

- @okuuva made their first contribution in [#5045](https://github.com/jdx/mise/pull/5045)

## [2025.5.3](https://github.com/jdx/mise/compare/v2025.5.2..v2025.5.3) - 2025-05-09

### 🚀 Features

- **(registry)** add coreutils by @kit494way in [#5033](https://github.com/jdx/mise/pull/5033)

### 🐛 Bug Fixes

- unuse command does not support env, global and path options by @roele in [#5021](https://github.com/jdx/mise/pull/5021)

### 🧪 Testing

- disable aqua for now due to bad version output by @jdx in [fa3daa2](https://github.com/jdx/mise/commit/fa3daa2cab09ba7e0140fcf2112375eef8427a85)
- fix python poetry test by @jdx in [c46a190](https://github.com/jdx/mise/commit/c46a190cb699b7700aa636a2bc888222ed7e9dbc)

### ◀️ Revert

- Revert "fix(dotenv): properly escape values in generated dotenv " by @jdx in [358c3da](https://github.com/jdx/mise/commit/358c3dab2dba7129ac115fc3414657dc39b2bd79)
- Revert "fix(env): fix dotenv files cascading (fix #4688) " by @jdx in [b1ca323](https://github.com/jdx/mise/commit/b1ca3235ffc9635f17dac0896c3c07b975d65819)

### 📦️ Dependency Updates

- update rust crate nix to 0.30 by @renovate[bot] in [#5032](https://github.com/jdx/mise/pull/5032)
- update rust crate built to 0.8 by @renovate[bot] in [#5031](https://github.com/jdx/mise/pull/5031)

## [2025.5.2](https://github.com/jdx/mise/compare/v2025.5.1..v2025.5.2) - 2025-05-07

### 🐛 Bug Fixes

- **(dotenv)** properly escape values in generated dotenv by @noirbizarre in [#5010](https://github.com/jdx/mise/pull/5010)
- **(registry)** use full version of taplo by @risu729 in [#5017](https://github.com/jdx/mise/pull/5017)

### 📚 Documentation

- hide rtx docs by @jdx in [90ae2ce](https://github.com/jdx/mise/commit/90ae2ce5abf4faa65ef2414385e587d97ff0ca2c)
- describe cache auto-prune by @jdx in [#5013](https://github.com/jdx/mise/pull/5013)
- mark idiomatic_version_file_disable_tools as deprecated by @jdx in [9bb80f3](https://github.com/jdx/mise/commit/9bb80f301e29fcc668f51de8e0a168a32c9ac8db)

### Chore

- remove homebrew bump step by @jdx in [1625608](https://github.com/jdx/mise/commit/1625608c0025ec21a49eedcc85533facde52a8a7)
- simplify git logs by @jdx in [#5012](https://github.com/jdx/mise/pull/5012)

## [2025.5.1](https://github.com/jdx/mise/compare/v2025.5.0..v2025.5.1) - 2025-05-05

### 🚀 Features

- **(registry)** use aqua for taplo by @risu729 in [#4991](https://github.com/jdx/mise/pull/4991)
- add mise_env tera variable for templates by @auxesis in [#5002](https://github.com/jdx/mise/pull/5002)

### 🐛 Bug Fixes

- **(env)** fix dotenv files cascading (fix #4688) by @noirbizarre in [#4996](https://github.com/jdx/mise/pull/4996)

### Ci

- **(registry)** increaset timeout to 30 mins by @risu729 in [#5006](https://github.com/jdx/mise/pull/5006)

## [2025.5.0](https://github.com/jdx/mise/compare/v2025.4.12..v2025.5.0) - 2025-05-03

### 🚀 Features

- **(registry)** add luau by @rhanneken in [#4993](https://github.com/jdx/mise/pull/4993)
- **(registry)** add numbat by @risu729 in [#4980](https://github.com/jdx/mise/pull/4980)
- **(status)** add setting to control status message truncation by @rarescosma in [#4986](https://github.com/jdx/mise/pull/4986)
- add check flag for the fmt command by @roele in [#4972](https://github.com/jdx/mise/pull/4972)
- use aqua for btop by @jdx in [#4979](https://github.com/jdx/mise/pull/4979)

### 🐛 Bug Fixes

- **(java)** filter out JetBrains releases with features by @roele in [#4970](https://github.com/jdx/mise/pull/4970)
- fix deadlocks caused by uv_venv_auto by @risu729 in [#4900](https://github.com/jdx/mise/pull/4900)

### 📚 Documentation

- Put dot in dotfile example by @ryanbrainard in [#4965](https://github.com/jdx/mise/pull/4965)

### Chore

- only use mold when available by @jdx in [#4978](https://github.com/jdx/mise/pull/4978)
- enable clearing screen for confirm and dialog by @roele in [#4990](https://github.com/jdx/mise/pull/4990)

### New Contributors

- @rarescosma made their first contribution in [#4986](https://github.com/jdx/mise/pull/4986)
- @rhanneken made their first contribution in [#4993](https://github.com/jdx/mise/pull/4993)
- @ryanbrainard made their first contribution in [#4965](https://github.com/jdx/mise/pull/4965)

## [2025.4.12](https://github.com/jdx/mise/compare/v2025.4.11..v2025.4.12) - 2025-04-29

### 🐛 Bug Fixes

- **(aqua)** fix bin_path of tools in monorepo by @risu729 in [#4954](https://github.com/jdx/mise/pull/4954)
- **(schema)** allow array of objects for hooks by @risu729 in [#4955](https://github.com/jdx/mise/pull/4955)
- store tool version opts in .mise.backend by @roele in [#4960](https://github.com/jdx/mise/pull/4960)

### 📚 Documentation

- add information about the DNF repository by @acesyde in [#4956](https://github.com/jdx/mise/pull/4956)

### 🧪 Testing

- fix registry tools by @jdx in [#4959](https://github.com/jdx/mise/pull/4959)

### Chore

- **(deny)** added CDLA-Permissive-2.0 by @jdx in [#4961](https://github.com/jdx/mise/pull/4961)

## [2025.4.11](https://github.com/jdx/mise/compare/v2025.4.10..v2025.4.11) - 2025-04-27

### 🚀 Features

- **(cargo)** allow customizable registry by @acesyde in [#4948](https://github.com/jdx/mise/pull/4948)
- **(doctor)** show error if tool not installed by @jdx in [#4952](https://github.com/jdx/mise/pull/4952)
- added sd by @jdx in [#4950](https://github.com/jdx/mise/pull/4950)
- MISE_LOG_HTTP by @jdx in [#4951](https://github.com/jdx/mise/pull/4951)

### 🐛 Bug Fixes

- set prune age to 10y in dockerfile by @jdx in [9a521dc](https://github.com/jdx/mise/commit/9a521dc1e93e57567dcb262482a6a8d382fbebe8)

### Chore

- brew update by @jdx in [641f3b3](https://github.com/jdx/mise/commit/641f3b3ef1c8c7b2e4931c5012c2b8dc94533070)
- brew sync repos by @jdx in [3318e98](https://github.com/jdx/mise/commit/3318e98d78af8a11e36f13574abe4f1cce181a92)
- bump usage by @jdx in [#4949](https://github.com/jdx/mise/pull/4949)

## [2025.4.10](https://github.com/jdx/mise/compare/v2025.4.9..v2025.4.10) - 2025-04-26

### 🚀 Features

- **(registry)** add `cli53` backend by @garysassano in [#4937](https://github.com/jdx/mise/pull/4937)
- pipx custom repository url by @acesyde in [#4945](https://github.com/jdx/mise/pull/4945)

### 🐛 Bug Fixes

- **(hook-env)** path order by @jdx in [#4946](https://github.com/jdx/mise/pull/4946)
- **(unuse)** allow unusing any version if version not specified by @jdx in [#4944](https://github.com/jdx/mise/pull/4944)
- Always use env::MISE_BIN when calling mise from itself by @hverlin in [#4943](https://github.com/jdx/mise/pull/4943)

### 📚 Documentation

- remove outdated note about automatic shim activation with Scoop by @jgutierrezre in [#4941](https://github.com/jdx/mise/pull/4941)

### Chore

- checkout for homebrew bump by @jdx in [6d7b0f6](https://github.com/jdx/mise/commit/6d7b0f6fdf83ee9d7be29a61b5b5be202ac0526a)
- mise.lock by @jdx in [05c9a24](https://github.com/jdx/mise/commit/05c9a241744fa330677402a365344b8430a4984c)
- updated deps by @jdx in [ac5cf5d](https://github.com/jdx/mise/commit/ac5cf5d840dc3a997dce0b1d3a1af963ef456ac2)
- brew developer by @jdx in [445e313](https://github.com/jdx/mise/commit/445e313985cb948cf2a7cb57d896055b898a0f67)

### New Contributors

- @garysassano made their first contribution in [#4937](https://github.com/jdx/mise/pull/4937)
- @jgutierrezre made their first contribution in [#4941](https://github.com/jdx/mise/pull/4941)

## [2025.4.9](https://github.com/jdx/mise/compare/v2025.4.8..v2025.4.9) - 2025-04-25

### 🚀 Features

- **(registry)** added tusd by @mnm364 in [#4928](https://github.com/jdx/mise/pull/4928)
- **(registry)** added fastfetch by @sassdavid in [#4932](https://github.com/jdx/mise/pull/4932)

### 🐛 Bug Fixes

- remove missing symlinks on unuse when pruning by @roele in [#4930](https://github.com/jdx/mise/pull/4930)

### 📚 Documentation

- typo by @jdx in [314657f](https://github.com/jdx/mise/commit/314657fb6ee69646464c35ed4d8b72f0f2d551da)

### ⚡ Performance

- turn several of the list functions into parallel iters by @lespea in [#4924](https://github.com/jdx/mise/pull/4924)

### 🧪 Testing

- fix kwok by @jdx in [4516335](https://github.com/jdx/mise/commit/451633512b67d26f2b3263094826da7c7406c1da)
- increase windows-e2e timeout by @jdx in [ce4f734](https://github.com/jdx/mise/commit/ce4f73462b10979f3721400393c4d3ba782c3bb4)

### 📦️ Dependency Updates

- update apple-actions/import-codesign-certs action to v5 by @renovate[bot] in [#4936](https://github.com/jdx/mise/pull/4936)
- update rust crate tabled to 0.19 by @renovate[bot] in [#4935](https://github.com/jdx/mise/pull/4935)

### Chore

- use macos-latest in GHA by @jdx in [05b5d49](https://github.com/jdx/mise/commit/05b5d49eaa3c4e78f1102dd2d9cfbca63c276ec0)
- attempt to fix brew bump by @jdx in [043f97f](https://github.com/jdx/mise/commit/043f97f23e9af914772474ee0379b5a7d9399f3e)
- mise up by @jdx in [ee7436d](https://github.com/jdx/mise/commit/ee7436d65c89416ee39ee424e296ae329f747323)

### New Contributors

- @lespea made their first contribution in [#4924](https://github.com/jdx/mise/pull/4924)

## [2025.4.8](https://github.com/jdx/mise/compare/v2025.4.7..v2025.4.8) - 2025-04-23

### 🐛 Bug Fixes

- hide idiomatic warning if no versions in idiomatic file by @jdx in [#4922](https://github.com/jdx/mise/pull/4922)

### 📚 Documentation

- clean up idiomatic deprecation message by @jdx in [c31aa2c](https://github.com/jdx/mise/commit/c31aa2cbd07a1f74049a0c6b72dfb91632ff5816)
- punctuation improvements to idiomatic deprecation message by @glasser in [#4915](https://github.com/jdx/mise/pull/4915)

## [2025.4.7](https://github.com/jdx/mise/compare/v2025.4.6..v2025.4.7) - 2025-04-23

### 🚀 Features

- **(registry)** added oxipng by @ldrouard in [#4452](https://github.com/jdx/mise/pull/4452)
- `mise tasks --local|--global` by @jdx in [#4907](https://github.com/jdx/mise/pull/4907)

### 🐛 Bug Fixes

- added lockfile for pyenv by @jdx in [#4906](https://github.com/jdx/mise/pull/4906)
- move idiomatic version breaking change from 2026.1.1 to 2025.10.0 by @jdx in [#4909](https://github.com/jdx/mise/pull/4909)
- allow setting lists to be empty by @jdx in [#4912](https://github.com/jdx/mise/pull/4912)

### 🧪 Testing

- test registry changes by themselves by @jdx in [#4910](https://github.com/jdx/mise/pull/4910)
- test registry changes by themselves by @jdx in [#4911](https://github.com/jdx/mise/pull/4911)

### 📦️ Dependency Updates

- update rust crate tabled to 0.18 by @renovate[bot] in [#4873](https://github.com/jdx/mise/pull/4873)

### Chore

- use hk for linting by @jdx in [#4908](https://github.com/jdx/mise/pull/4908)
- prefer ubi for shellcheck by @jdx in [c805f39](https://github.com/jdx/mise/commit/c805f399a0987db2ce812f2bd6ff66beb53de989)

## [2025.4.6](https://github.com/jdx/mise/compare/v2025.4.5..v2025.4.6) - 2025-04-22

### 🚀 Features

- **(aqua)** support github_release minisign type by @risu729 in [#4897](https://github.com/jdx/mise/pull/4897)
- **(go)** support build tags by @bamorim in [#4863](https://github.com/jdx/mise/pull/4863)
- **(registry)** added Signadot by @joehorsnell in [#4868](https://github.com/jdx/mise/pull/4868)
- added `idiomatic_version_file_enable_tools` and deprecated `idiomatic_version_file_disable_tools` by @jdx in [#4902](https://github.com/jdx/mise/pull/4902)

### 🐛 Bug Fixes

- **(doctor)** redact gitlab/enterprise tokens by @risu729 in [#4888](https://github.com/jdx/mise/pull/4888)
- **(task)** enable templates in shell and tools of tasks by @risu729 in [#4887](https://github.com/jdx/mise/pull/4887)
- allow interactive upgrade to select nothing by @risu729 in [#4891](https://github.com/jdx/mise/pull/4891)
- enable templates for shell of hooks by @risu729 in [#4893](https://github.com/jdx/mise/pull/4893)

### 📚 Documentation

- fix typo in go backend tags option title by @bamorim in [#4884](https://github.com/jdx/mise/pull/4884)
- update link to faq in use_versions_host by @risu729 in [#4890](https://github.com/jdx/mise/pull/4890)

### 🧪 Testing

- remove flaky bazel-watcher by @jdx in [9e95e6a](https://github.com/jdx/mise/commit/9e95e6afd04a43cc7d43e2f2280c7880bb481507)

### New Contributors

- @joehorsnell made their first contribution in [#4868](https://github.com/jdx/mise/pull/4868)
- @bamorim made their first contribution in [#4884](https://github.com/jdx/mise/pull/4884)

## [2025.4.5](https://github.com/jdx/mise/compare/v2025.4.4..v2025.4.5) - 2025-04-18

### 🐛 Bug Fixes

- **(ubi)** API URL for GitHub should not have /repos segement by @roele in [#4848](https://github.com/jdx/mise/pull/4848)
- **(ubi)** URL syntax fails by @roele in [#4859](https://github.com/jdx/mise/pull/4859)
- allow to install non-numeric elixir versions by @roele in [#4850](https://github.com/jdx/mise/pull/4850)
- removed possible single-point-of-failure while running `mise upgrade` by @hitblast in [#4847](https://github.com/jdx/mise/pull/4847)
- `#MISE tools=` in task header by @jdx in [#4860](https://github.com/jdx/mise/pull/4860)

### 🧪 Testing

- fix aqua tool test by @jdx in [4f2c050](https://github.com/jdx/mise/commit/4f2c0505502c1e3c7bf3478d61a2c352591f281c)

### New Contributors

- @hitblast made their first contribution in [#4847](https://github.com/jdx/mise/pull/4847)

## [2025.4.4](https://github.com/jdx/mise/compare/v2025.4.3..v2025.4.4) - 2025-04-15

### 🧪 Testing

- remove kpt test by @jdx in [b9d35ac](https://github.com/jdx/mise/commit/b9d35ac57936291a0a4629f9c200dfdb500a7efb)

## [2025.4.3](https://github.com/jdx/mise/compare/v2025.4.2..v2025.4.3) - 2025-04-15

### 🚀 Features

- **(aqua)** support SLSA source_uri setting by @scop in [#4833](https://github.com/jdx/mise/pull/4833)
- **(aqua)** use source tag in SLSA verification by @scop in [#4836](https://github.com/jdx/mise/pull/4836)
- **(ubi)** add support for self-hosted GitHub/GitLab by @roele in [#4765](https://github.com/jdx/mise/pull/4765)

### 📚 Documentation

- Update configuration.md by @jdx in [#4829](https://github.com/jdx/mise/pull/4829)
- correct `mise use` paths by @jdx in [c8374c0](https://github.com/jdx/mise/commit/c8374c00ca68e5722c28f9abfd2425b9722bdd83)

## [2025.4.2](https://github.com/jdx/mise/compare/v2025.4.1..v2025.4.2) - 2025-04-11

### 🚀 Features

- **(registry)** update aws-nuke backend by @StingRayZA in [#4815](https://github.com/jdx/mise/pull/4815)

### 🐛 Bug Fixes

- do not default to writing to mise.$MISE_ENV.toml by @jdx in [#4817](https://github.com/jdx/mise/pull/4817)
- mise watch forward --exts and --filter to watchexec by @cmhms in [#4826](https://github.com/jdx/mise/pull/4826)

### 📚 Documentation

- Fixing typo in code for flags in toml-tasks.md by @arafays in [#4820](https://github.com/jdx/mise/pull/4820)
- branding by @jdx in [9ad2c17](https://github.com/jdx/mise/commit/9ad2c17ec75b7460ebea09a9f0601a561349cc7f)
- remove references to not-working docker: tasks by @jdx in [2c2fd27](https://github.com/jdx/mise/commit/2c2fd272e3d76329a7c67e4070bfb122ae1e1120)
- document some dependencies by @jdx in [6e8bd51](https://github.com/jdx/mise/commit/6e8bd518757c5e49624fc2bef5777a2f2339c304)
- simplify mise.toml example by @jdx in [66d927b](https://github.com/jdx/mise/commit/66d927ba4db81ba70de261cd76e399e9f4fe35da)

### 📦️ Dependency Updates

- update dependency vitepress-plugin-tabs to ^0.7.0 by @renovate[bot] in [#4822](https://github.com/jdx/mise/pull/4822)
- update rust crate petgraph to 0.8 by @renovate[bot] in [#4823](https://github.com/jdx/mise/pull/4823)
- update rust crate strum to 0.27 by @renovate[bot] in [#4780](https://github.com/jdx/mise/pull/4780)

### New Contributors

- @cmhms made their first contribution in [#4826](https://github.com/jdx/mise/pull/4826)
- @StingRayZA made their first contribution in [#4815](https://github.com/jdx/mise/pull/4815)

## [2025.4.1](https://github.com/jdx/mise/compare/v2025.4.0..v2025.4.1) - 2025-04-09

### 🚀 Features

- **(registry)** added localstack by @mnm364 in [#4785](https://github.com/jdx/mise/pull/4785)
- **(registry)** added skeema by @mnm364 in [#4786](https://github.com/jdx/mise/pull/4786)
- **(registry)** add television by @mangkoran in [#4778](https://github.com/jdx/mise/pull/4778)

### 🐛 Bug Fixes

- show gh rate limit reset time in local time by @someoneinjd in [#4799](https://github.com/jdx/mise/pull/4799)

### 📚 Documentation

- all experimental note for lockfile by @zeitlinger in [#4781](https://github.com/jdx/mise/pull/4781)
- Include post about Mise secrets in the context of Swift app dev by @pepicrft in [#4809](https://github.com/jdx/mise/pull/4809)

### Chore

- update deps to fix deny check by @jdx in [432023b](https://github.com/jdx/mise/commit/432023b2cd04d2ea7f590d7b338054944512abd0)
- pin zip to avoid issue with ubi by @jdx in [315deb4](https://github.com/jdx/mise/commit/315deb4e24177408c598d22951adb95f3e841683)

### New Contributors

- @someoneinjd made their first contribution in [#4799](https://github.com/jdx/mise/pull/4799)
- @mnm364 made their first contribution in [#4786](https://github.com/jdx/mise/pull/4786)
- @zeitlinger made their first contribution in [#4781](https://github.com/jdx/mise/pull/4781)

## [2025.4.0](https://github.com/jdx/mise/compare/v2025.3.11..v2025.4.0) - 2025-04-02

### 🐛 Bug Fixes

- s/runtimes/tools by @jdx in [#4754](https://github.com/jdx/mise/pull/4754)
- add clarification on RUSTUP_HOME and CARGO_HOME by @lachieh in [#4759](https://github.com/jdx/mise/pull/4759)
- enhance confirmation logic to respect SETTINGS.yes by @roele in [#4764](https://github.com/jdx/mise/pull/4764)

### 🚜 Refactor

- **(registry)** use aqua for ubi by @scop in [#4745](https://github.com/jdx/mise/pull/4745)
- **(registry)** use aqua for ksops by @scop in [#4746](https://github.com/jdx/mise/pull/4746)

### 📚 Documentation

- mark code block for dnf5 install as shell code by @sina-hide in [#4747](https://github.com/jdx/mise/pull/4747)
- update demo by @hverlin in [#4350](https://github.com/jdx/mise/pull/4350)
- move demo to top-level by @jdx in [2b6f45a](https://github.com/jdx/mise/commit/2b6f45ac73d6f59542f9c7b401042ad5c75e37e2)
- Update config.ts by @jdx in [05ad4bc](https://github.com/jdx/mise/commit/05ad4bc9b2243737c0551fd36de1e37dc57ea578)
- Update walkthrough.md by @jdx in [89904b4](https://github.com/jdx/mise/commit/89904b46d8649a66bf960b1e5c7c0364dad8f94f)
- Update index.md by @jdx in [#4750](https://github.com/jdx/mise/pull/4750)
- Update walkthrough.md by @jdx in [#4751](https://github.com/jdx/mise/pull/4751)
- Update README.md by @jdx in [4f38142](https://github.com/jdx/mise/commit/4f38142bd3d822c3eafd78a74aa7a8d31791d2e3)

### New Contributors

- @lachieh made their first contribution in [#4759](https://github.com/jdx/mise/pull/4759)
- @sina-hide made their first contribution in [#4747](https://github.com/jdx/mise/pull/4747)

## [2025.3.11](https://github.com/jdx/mise/compare/v2025.3.10..v2025.3.11) - 2025-03-28

### 🚀 Features

- **(registry)** add protoc-gen-validate by @akanter in [#4703](https://github.com/jdx/mise/pull/4703)

### 🚜 Refactor

- **(registry)** use aqua for swiftlint by @scop in [#4726](https://github.com/jdx/mise/pull/4726)
- **(registry)** use ubi for opensearch-cli by @scop in [#4725](https://github.com/jdx/mise/pull/4725)
- **(registry)** use ubi for mdbook-linkcheck by @scop in [#4724](https://github.com/jdx/mise/pull/4724)
- **(registry)** use ubi for velad by @scop in [#4727](https://github.com/jdx/mise/pull/4727)

## [2025.3.10](https://github.com/jdx/mise/compare/v2025.3.9..v2025.3.10) - 2025-03-26

### ◀️ Revert

- Revert "chore: make awscli compatible with R2" by @jdx in [83e8c16](https://github.com/jdx/mise/commit/83e8c164ec78cab4325b4489d9cc5d1fa466ec3f)

## [2025.3.9](https://github.com/jdx/mise/compare/v2025.3.8..v2025.3.9) - 2025-03-26

### 🚀 Features

- Set usage arguments and flag as environment variables before running the command by @gturi in [#4700](https://github.com/jdx/mise/pull/4700)

### 🚜 Refactor

- **(registry)** use ubi for assh by @scop in [#4713](https://github.com/jdx/mise/pull/4713)
- **(registry)** use ubi for opsgenie-lamp by @scop in [#4712](https://github.com/jdx/mise/pull/4712)
- **(registry)** use ubi for auto-doc by @scop in [#4714](https://github.com/jdx/mise/pull/4714)
- **(registry)** use ubi for getenvoy by @scop in [#4715](https://github.com/jdx/mise/pull/4715)
- **(registry)** use ubi for mockolo by @scop in [#4705](https://github.com/jdx/mise/pull/4705)
- **(registry)** use ubi for haxe by @scop in [#4716](https://github.com/jdx/mise/pull/4716)
- **(registry)** use ubi for helm-diff by @scop in [#4717](https://github.com/jdx/mise/pull/4717)
- **(registry)** use ubi for grain by @scop in [#4718](https://github.com/jdx/mise/pull/4718)

## [2025.3.8](https://github.com/jdx/mise/compare/v2025.3.7..v2025.3.8) - 2025-03-24

### 🚀 Features

- **(registry)** add aichat by @kit494way in [#4691](https://github.com/jdx/mise/pull/4691)

### 🐛 Bug Fixes

- Update flake to fix nix build by @akanter in [#4686](https://github.com/jdx/mise/pull/4686)

### 📚 Documentation

- fix bash completion setup instructions by @bestagi in [#3920](https://github.com/jdx/mise/pull/3920)
- small tidy of shims docs by @AlecRust in [#4693](https://github.com/jdx/mise/pull/4693)

### Chore

- remove broken ripsecrets test by @jdx in [bb382aa](https://github.com/jdx/mise/commit/bb382aa783a2a1bfc44f02a5bb34f9397efb2e57)
- make awscli compatible with R2 by @jdx in [cad7fa2](https://github.com/jdx/mise/commit/cad7fa285e96483ba8d6aeb22f83de10e92700b2)
- enable workflow_dispatch for docs task by @jdx in [b0578db](https://github.com/jdx/mise/commit/b0578db141decc63992ebb0f74e29a53238611ba)

### New Contributors

- @akanter made their first contribution in [#4686](https://github.com/jdx/mise/pull/4686)
- @bestagi made their first contribution in [#3920](https://github.com/jdx/mise/pull/3920)

## [2025.3.7](https://github.com/jdx/mise/compare/v2025.3.6..v2025.3.7) - 2025-03-21

### 🐛 Bug Fixes

- **(node)** skip gpg verification of sig file not found by @jdx in [#4663](https://github.com/jdx/mise/pull/4663)
- **(task)** allow args to be used with tera tests by @risu729 in [#4605](https://github.com/jdx/mise/pull/4605)
- Fix syntax error on `activate nu` when PATH contains shims by @atty303 in [#4349](https://github.com/jdx/mise/pull/4349)

### 🚜 Refactor

- **(registry)** use ubi for yamlscript by @scop in [#4670](https://github.com/jdx/mise/pull/4670)

### 📚 Documentation

- Fix typo in java.md by @hverlin in [#4672](https://github.com/jdx/mise/pull/4672)

### ◀️ Revert

- "chore: temporarily disable bootstrap test" by @jdx in [#4658](https://github.com/jdx/mise/pull/4658)

### 📦️ Dependency Updates

- update rust crate ctor to 0.4 by @renovate[bot] in [#4553](https://github.com/jdx/mise/pull/4553)

### Chore

- **(registry)** declare copier by @looztra in [#4669](https://github.com/jdx/mise/pull/4669)
- Update to the latest version of ubi by @autarch in [#4648](https://github.com/jdx/mise/pull/4648)
- bump expr by @jdx in [#4666](https://github.com/jdx/mise/pull/4666)
- added android-sdk by @jdx in [#4668](https://github.com/jdx/mise/pull/4668)
- rename mise-php to asdf-php by @jdx in [#4674](https://github.com/jdx/mise/pull/4674)

### New Contributors

- @atty303 made their first contribution in [#4349](https://github.com/jdx/mise/pull/4349)
- @looztra made their first contribution in [#4669](https://github.com/jdx/mise/pull/4669)

## [2025.3.6](https://github.com/jdx/mise/compare/v2025.3.5..v2025.3.6) - 2025-03-18

### Chore

- unpin aws-cli by @jdx in [7fabed5](https://github.com/jdx/mise/commit/7fabed5c70fccfe095647c7b2220965ca2f1c07d)
- temporarily disable bootstrap test by @jdx in [599258a](https://github.com/jdx/mise/commit/599258aa4f5c0ab0b5581740b0c9eec17f1c7318)

## [2025.3.5](https://github.com/jdx/mise/compare/v2025.3.4..v2025.3.5) - 2025-03-18

### 🚀 Features

- **(registry)** use ubi for glab by @scop in [#4643](https://github.com/jdx/mise/pull/4643)
- ubi forge option support by @scop in [#4642](https://github.com/jdx/mise/pull/4642)

### 🐛 Bug Fixes

- **(tera)** use default inline shell to parse exec template by @risu729 in [#4645](https://github.com/jdx/mise/pull/4645)

## [2025.3.4](https://github.com/jdx/mise/compare/v2025.3.3..v2025.3.4) - 2025-03-18

### 🐛 Bug Fixes

- Failed to create venv at the same time by multiple uv processes by @NavyD in [#4640](https://github.com/jdx/mise/pull/4640)

## [2025.3.3](https://github.com/jdx/mise/compare/v2025.3.2..v2025.3.3) - 2025-03-14

### 🚀 Features

- **(env)** support env files in toml by @risu729 in [#4618](https://github.com/jdx/mise/pull/4618)
- **(registry)** add harper-ls and harper-cli by @kit494way in [#4615](https://github.com/jdx/mise/pull/4615)
- **(registry)** add curlie by @reitzig in [#4599](https://github.com/jdx/mise/pull/4599)
- cleanup the mutex use. by @boris-smidt-klarrio in [#4540](https://github.com/jdx/mise/pull/4540)
- Add flag to fmt command to read from stdin by @erickgnavar in [#4594](https://github.com/jdx/mise/pull/4594)

### 🐛 Bug Fixes

- **(uv)** avoid deadlocks while initializing UV_VENV by @risu729 in [#4609](https://github.com/jdx/mise/pull/4609)
- handle error when getting modified duration in file::modified_duration by @roele in [#4624](https://github.com/jdx/mise/pull/4624)
- SwiftPM backend not working with the Swift 6 toolchain by @pepicrft in [#4632](https://github.com/jdx/mise/pull/4632)
- quiet in file task not working by @roele in [#4588](https://github.com/jdx/mise/pull/4588)
- Unable to find uv when first creating py venv by @NavyD in [#4591](https://github.com/jdx/mise/pull/4591)

### 🚜 Refactor

- migrate humantime to jiff by @risu729 in [#4616](https://github.com/jdx/mise/pull/4616)
- use method to get the default inline shell instead of accessing the fields by @risu729 in [#4621](https://github.com/jdx/mise/pull/4621)

### 📚 Documentation

- **(settings)** clarify the usage of disable_default_registry by @gbloquel in [#4589](https://github.com/jdx/mise/pull/4589)

### ⚡ Performance

- speed up self-update by calling /releases/latest api instead of /releases by @vemoo in [#4619](https://github.com/jdx/mise/pull/4619)

### 🧪 Testing

- **(registry)** fix test of lazyjournal by @risu729 in [#4610](https://github.com/jdx/mise/pull/4610)

### Chore

- deny fixes by @jdx in [17d7c6e](https://github.com/jdx/mise/commit/17d7c6ee5e035272a8dc1b93c8fc7ac9cffb7f80)
- ignore humantime unmaintained advisory by @risu729 in [#4612](https://github.com/jdx/mise/pull/4612)
- remove rustup update in github actions by @risu729 in [#4617](https://github.com/jdx/mise/pull/4617)

### New Contributors

- @erickgnavar made their first contribution in [#4594](https://github.com/jdx/mise/pull/4594)
- @vemoo made their first contribution in [#4619](https://github.com/jdx/mise/pull/4619)
- @gbloquel made their first contribution in [#4589](https://github.com/jdx/mise/pull/4589)

## [2025.3.1](https://github.com/jdx/mise/compare/v2025.3.0..v2025.3.1) - 2025-03-06

### 🚀 Features

- **(registry)** added sampler by @tony-sol in [#4577](https://github.com/jdx/mise/pull/4577)
- **(registry)** added lazyjournal by @tony-sol in [#4584](https://github.com/jdx/mise/pull/4584)
- add support for components property in rust-toolchain.toml by @roele in [#4579](https://github.com/jdx/mise/pull/4579)
- add --local flag for ls by @tony-sol in [#4565](https://github.com/jdx/mise/pull/4565)

### 🐛 Bug Fixes

- favor aqua backend over asdf by @dud225 in [#4558](https://github.com/jdx/mise/pull/4558)

### 📚 Documentation

- continuous-integration.md: fix gitlab caching example by @nafg in [#4576](https://github.com/jdx/mise/pull/4576)

### Chore

- edition 2024 by @jdx in [#4541](https://github.com/jdx/mise/pull/4541)

### New Contributors

- @nafg made their first contribution in [#4576](https://github.com/jdx/mise/pull/4576)
- @dud225 made their first contribution in [#4558](https://github.com/jdx/mise/pull/4558)

## [2025.3.0](https://github.com/jdx/mise/compare/v2025.2.9..v2025.3.0) - 2025-03-01

### 🚀 Features

- **(registry)** added helmwave by @tony-sol in [#4542](https://github.com/jdx/mise/pull/4542)
- **(registry)** added doggo by @tony-sol in [#4545](https://github.com/jdx/mise/pull/4545)
- **(registry)** Add Boilerplate by @ZachGoldberg in [#4530](https://github.com/jdx/mise/pull/4530)
- **(registry)** added htmlq by @tony-sol in [#4548](https://github.com/jdx/mise/pull/4548)
- **(registry)** added gokey by @tony-sol in [#4546](https://github.com/jdx/mise/pull/4546)
- **(registry)** added octosql by @tony-sol in [#4549](https://github.com/jdx/mise/pull/4549)
- **(registry)** added hexyl by @tony-sol in [#4547](https://github.com/jdx/mise/pull/4547)
- **(registry)** added kubeone by @tony-sol in [#4550](https://github.com/jdx/mise/pull/4550)
- task confirmation by @roele in [#4328](https://github.com/jdx/mise/pull/4328)

### 🐛 Bug Fixes

- remote tasks and devcontainer by @acesyde in [#4557](https://github.com/jdx/mise/pull/4557)

### 📚 Documentation

- **(shim)** add faq for vscode windows spawn EINVAL & format value to list by @qianlongzt in [#4544](https://github.com/jdx/mise/pull/4544)

### New Contributors

- @ZachGoldberg made their first contribution in [#4530](https://github.com/jdx/mise/pull/4530)

## [2025.2.9](https://github.com/jdx/mise/compare/v2025.2.8..v2025.2.9) - 2025-02-26

### 🚀 Features

- **(registry)** add cocogitto by @reitzig in [#4513](https://github.com/jdx/mise/pull/4513)
- **(registry)** Added foundry by @suicide in [#4455](https://github.com/jdx/mise/pull/4455)
- **(registry)** added ast-grep by @tony-sol in [#4519](https://github.com/jdx/mise/pull/4519)

### 🐛 Bug Fixes

- non-utf8 external process handling by @jdx in [#4538](https://github.com/jdx/mise/pull/4538)

### 📚 Documentation

- **(cookbook)** add shell powerline-go config env recipe by @scop in [#4532](https://github.com/jdx/mise/pull/4532)
- update mise.el repo link by @tecoholic in [#4534](https://github.com/jdx/mise/pull/4534)

### Chore

- bump rust version for releases by @jdx in [f4e5970](https://github.com/jdx/mise/commit/f4e5970f00bf56d9be16a7e7e83289085c0e5cce)
- bump rust version for releases by @jdx in [52cff1c](https://github.com/jdx/mise/commit/52cff1c00b452b93b3ca1e4fc01fd21de73569e5)
- bump rust version for releases by @jdx in [9121c5e](https://github.com/jdx/mise/commit/9121c5e9270fae59ce753226ecbbe2939c4661e4)
- bump msrv for edition compatibility by @jdx in [3a222dd](https://github.com/jdx/mise/commit/3a222ddf272eef655b50796f34634fcedc3f1288)
- remove unused deny rule by @jdx in [053f5c1](https://github.com/jdx/mise/commit/053f5c1c0746e363c24b19577b958621ea91c40c)

### New Contributors

- @tony-sol made their first contribution in [#4519](https://github.com/jdx/mise/pull/4519)
- @tecoholic made their first contribution in [#4534](https://github.com/jdx/mise/pull/4534)
- @suicide made their first contribution in [#4455](https://github.com/jdx/mise/pull/4455)
- @reitzig made their first contribution in [#4513](https://github.com/jdx/mise/pull/4513)

## [2025.2.8](https://github.com/jdx/mise/compare/v2025.2.7..v2025.2.8) - 2025-02-25

### 🚀 Features

- **(registry)** add checkmake to registry by @eread in [#4466](https://github.com/jdx/mise/pull/4466)
- **(registry)** added sops from aqua registry by @ldrouard in [#4457](https://github.com/jdx/mise/pull/4457)
- **(registry)** added k9s from aqua registry by @ldrouard in [#4460](https://github.com/jdx/mise/pull/4460)
- **(registry)** added hadolint from aqua registry by @ldrouard in [#4456](https://github.com/jdx/mise/pull/4456)
- **(shim)** Windows shim add hardlink & symlink mode by @qianlongzt in [#4409](https://github.com/jdx/mise/pull/4409)
- **(ubi)** add option `rename_exe` by @wlmitch in [#4512](https://github.com/jdx/mise/pull/4512)
- use aqua for hk by @jdx in [f68de38](https://github.com/jdx/mise/commit/f68de3849c5ceb20475f2f30224abaa5f3f7441d)
- add bazel-watcher to registry by @betaboon in [#4296](https://github.com/jdx/mise/pull/4296)

### 🐛 Bug Fixes

- behavior of .disable-self-update by @ZeroAurora in [#4476](https://github.com/jdx/mise/pull/4476)
- devcontainer by @acesyde in [#4483](https://github.com/jdx/mise/pull/4483)
- mise outdated --json does not return json if all tools are up-to-date by @roele in [#4493](https://github.com/jdx/mise/pull/4493)
- bug when using mise use -g when MISE_ENV is filled by @roele in [#4494](https://github.com/jdx/mise/pull/4494)
- config of symlink tracked on windows is not respected by @NavyD in [#4501](https://github.com/jdx/mise/pull/4501)
- pruning unused tool leaves broken symlinks by @roele in [#4507](https://github.com/jdx/mise/pull/4507)

### 📚 Documentation

- Fixes typo in lang/zig by @carldaws in [#4497](https://github.com/jdx/mise/pull/4497)
- Fix activation on PowerShell by @kit494way in [#4498](https://github.com/jdx/mise/pull/4498)

### Chore

- remove aur job by @jdx in [fe5a71d](https://github.com/jdx/mise/commit/fe5a71dc486e6e585167d9d97018f2b467bc43fe)
- remove reference to aur in release script by @jdx in [0824490](https://github.com/jdx/mise/commit/0824490c14d17cd93c7d68930b514eb11635c451)
- deny ring sec by @jdx in [08e334c](https://github.com/jdx/mise/commit/08e334cb1209471d9c18b289473925ff0931053f)

### New Contributors

- @betaboon made their first contribution in [#4296](https://github.com/jdx/mise/pull/4296)
- @ldrouard made their first contribution in [#4456](https://github.com/jdx/mise/pull/4456)
- @qianlongzt made their first contribution in [#4409](https://github.com/jdx/mise/pull/4409)
- @wlmitch made their first contribution in [#4512](https://github.com/jdx/mise/pull/4512)
- @carldaws made their first contribution in [#4497](https://github.com/jdx/mise/pull/4497)
- @ZeroAurora made their first contribution in [#4476](https://github.com/jdx/mise/pull/4476)

## [2025.2.7](https://github.com/jdx/mise/compare/v2025.2.6..v2025.2.7) - 2025-02-19

### 🚀 Features

- **(registry)** add lychee to registry by @eread in [#4181](https://github.com/jdx/mise/pull/4181)
- Install latest nominated zig from https://machengine.org/zig/index.json by @tamadamas in [#4451](https://github.com/jdx/mise/pull/4451)

### 🐛 Bug Fixes

- **(cli/run)** inherit stdio by --raw even when redactions are enabled by @risu729 in [#4446](https://github.com/jdx/mise/pull/4446)
- **(task)** Running programs on windows without cmd.exe by @NavyD in [#4459](https://github.com/jdx/mise/pull/4459)
- bugs with grep in tar_supports_zstd in mise.run script by @glasser in [#4453](https://github.com/jdx/mise/pull/4453)

### 📚 Documentation

- fix watch files hook example by @rsyring in [#4427](https://github.com/jdx/mise/pull/4427)
- Fix run-on sentence by @henrebotha in [#4429](https://github.com/jdx/mise/pull/4429)
- mention hk by @jdx in [1a58e86](https://github.com/jdx/mise/commit/1a58e86ce2ce16d848755df8feccf514000053fd)
- discord link by @jdx in [b586085](https://github.com/jdx/mise/commit/b58608521cccee812adaa642145f061ccbcbac43)
- Add a section on how to use environment variables by @hverlin in [#4435](https://github.com/jdx/mise/pull/4435)
- Update installation for archLinux by @Nicknamely in [#4449](https://github.com/jdx/mise/pull/4449)
- Fix typo in getting-started by @alefteris in [#4448](https://github.com/jdx/mise/pull/4448)

### 🧪 Testing

- always set experimental = true in tests by @jdx in [#4443](https://github.com/jdx/mise/pull/4443)

### Chore

- fixed new clippy lints by @jdx in [#4463](https://github.com/jdx/mise/pull/4463)

### New Contributors

- @alefteris made their first contribution in [#4448](https://github.com/jdx/mise/pull/4448)
- @tamadamas made their first contribution in [#4451](https://github.com/jdx/mise/pull/4451)
- @Nicknamely made their first contribution in [#4449](https://github.com/jdx/mise/pull/4449)
- @eread made their first contribution in [#4181](https://github.com/jdx/mise/pull/4181)
- @rsyring made their first contribution in [#4427](https://github.com/jdx/mise/pull/4427)

## [2025.2.6](https://github.com/jdx/mise/compare/v2025.2.5..v2025.2.6) - 2025-02-16

### 🚀 Features

- add devcontainer generator by @acesyde in [#4355](https://github.com/jdx/mise/pull/4355)
- added hk by @jdx in [#4422](https://github.com/jdx/mise/pull/4422)

### 🐛 Bug Fixes

- short flag with value and var=#true bug by @jdx in [#4419](https://github.com/jdx/mise/pull/4419)
- regression with env overriding by @jdx in [#4421](https://github.com/jdx/mise/pull/4421)

### 📚 Documentation

- **(shims)** clarify `activate` only removes shims from `PATH` by @risu729 in [#4418](https://github.com/jdx/mise/pull/4418)
- Update shims page by @hverlin in [#4414](https://github.com/jdx/mise/pull/4414)

## [2025.2.5](https://github.com/jdx/mise/compare/v2025.2.4..v2025.2.5) - 2025-02-16

### 🐛 Bug Fixes

- properly replace non set flags with "false" by @IxDay in [#4410](https://github.com/jdx/mise/pull/4410)
- path env order with subdirs by @jdx in [#4412](https://github.com/jdx/mise/pull/4412)

### ◀️ Revert

- "feat: set usage arguments and flags as environment variables for toml tasks" by @jdx in [#4413](https://github.com/jdx/mise/pull/4413)

## [2025.2.4](https://github.com/jdx/mise/compare/v2025.2.3..v2025.2.4) - 2025-02-14

### 🚀 Features

- **(registry)** add e1s by @kiwamizamurai in [#4363](https://github.com/jdx/mise/pull/4363)
- **(registry)** add 'marksman' via 'aqua:artempyanykh/marksman' backend by @iamoeg in [#4357](https://github.com/jdx/mise/pull/4357)
- use `machengine.org` for downloading nominated zig versions by @hadronomy in [#4356](https://github.com/jdx/mise/pull/4356)

### 🐛 Bug Fixes

- **(aqua)** apply override of version_prefix by @risu729 in [#4338](https://github.com/jdx/mise/pull/4338)
- **(env_directive)** apply redactions only to env with redact by @risu729 in [#4388](https://github.com/jdx/mise/pull/4388)
- **(hook_env)** don't exit early if watching files are deleted by @risu729 in [#4390](https://github.com/jdx/mise/pull/4390)
- **(rubygems_plugin)** Replace which ruby check for Windows compatibility by @genskyff in [#4358](https://github.com/jdx/mise/pull/4358)
- lowercase desired shim names by @KevSlashNull in [#4333](https://github.com/jdx/mise/pull/4333)
- allow cosign opts to be empty in aqua by @IxDay in [#4396](https://github.com/jdx/mise/pull/4396)

### 📚 Documentation

- update Fedora install for dnf5 by @rkben in [#4387](https://github.com/jdx/mise/pull/4387)
- fix links to idiomatic version file option by @pietrodn in [#4382](https://github.com/jdx/mise/pull/4382)
- add mise bootstrap example in CI docs by @hverlin in [#4351](https://github.com/jdx/mise/pull/4351)
- Update link in comparison-to-asdf.md by @hverlin in [#4401](https://github.com/jdx/mise/pull/4401)

### 📦️ Dependency Updates

- update rust crate bzip2 to v0.5.1 by @renovate[bot] in [#4392](https://github.com/jdx/mise/pull/4392)
- update rust crate built to v0.7.6 by @renovate[bot] in [#4391](https://github.com/jdx/mise/pull/4391)

### Chore

- issue closer by @jdx in [bee1f55](https://github.com/jdx/mise/commit/bee1f5557b829b9a637a28af90b519fdfa74b8dd)

### New Contributors

- @iamoeg made their first contribution in [#4357](https://github.com/jdx/mise/pull/4357)
- @hadronomy made their first contribution in [#4356](https://github.com/jdx/mise/pull/4356)
- @pietrodn made their first contribution in [#4382](https://github.com/jdx/mise/pull/4382)
- @genskyff made their first contribution in [#4358](https://github.com/jdx/mise/pull/4358)
- @kiwamizamurai made their first contribution in [#4363](https://github.com/jdx/mise/pull/4363)
- @rkben made their first contribution in [#4387](https://github.com/jdx/mise/pull/4387)
- @IxDay made their first contribution in [#4396](https://github.com/jdx/mise/pull/4396)
- @KevSlashNull made their first contribution in [#4333](https://github.com/jdx/mise/pull/4333)

## [2025.2.3](https://github.com/jdx/mise/compare/v2025.2.2..v2025.2.3) - 2025-02-09

### ◀️ Revert

- Revert "feat: add support for idiomatic go.mod file " by @jdx in [7fc9beb](https://github.com/jdx/mise/commit/7fc9bebd02abfee4b622a211b86c516df9bd4f6d)

## [2025.2.2](https://github.com/jdx/mise/compare/v2025.2.1..v2025.2.2) - 2025-02-08

### 🚀 Features

- **(registry)** add jd by @risu729 in [#4318](https://github.com/jdx/mise/pull/4318)
- **(registry)** add jc by @risu729 in [#4317](https://github.com/jdx/mise/pull/4317)
- **(registry)** Add qsv cli by @vjda in [#4334](https://github.com/jdx/mise/pull/4334)
- add support for idiomatic go.mod file by @roele in [#4312](https://github.com/jdx/mise/pull/4312)
- add -g short version for unuse cmd by @kimle in [#4330](https://github.com/jdx/mise/pull/4330)
- add git remote task provider by @acesyde in [#4233](https://github.com/jdx/mise/pull/4233)
- set usage arguments and flags as environment variables for toml tasks by @gturi in [#4159](https://github.com/jdx/mise/pull/4159)

### 🐛 Bug Fixes

- **(aqua)** trim prefix before comparing versions by @risu729 in [#4340](https://github.com/jdx/mise/pull/4340)
- wrong config file type for rust-toolchain.toml files by @roele in [#4321](https://github.com/jdx/mise/pull/4321)

### 🚜 Refactor

- **(registry)** use aqua for yq by @scop in [#4326](https://github.com/jdx/mise/pull/4326)

### 📚 Documentation

- **(schema)** fix description of task.dir default by @risu729 in [#4324](https://github.com/jdx/mise/pull/4324)
- Add PowerShell example by @jahanson in [#3857](https://github.com/jdx/mise/pull/3857)
- Include "A Mise guide for Swift developers" by @pepicrft in [#4329](https://github.com/jdx/mise/pull/4329)
- Update documentation for core tools by @hverlin in [#4341](https://github.com/jdx/mise/pull/4341)
- Update vitepress to fix search by @hverlin in [#4342](https://github.com/jdx/mise/pull/4342)

### Chore

- **(bun.lock)** migrate bun lockfiles to text-based by @risu729 in [#4319](https://github.com/jdx/mise/pull/4319)

### New Contributors

- @vjda made their first contribution in [#4334](https://github.com/jdx/mise/pull/4334)
- @kimle made their first contribution in [#4330](https://github.com/jdx/mise/pull/4330)
- @pepicrft made their first contribution in [#4329](https://github.com/jdx/mise/pull/4329)
- @jahanson made their first contribution in [#3857](https://github.com/jdx/mise/pull/3857)

## [2025.2.1](https://github.com/jdx/mise/compare/v2025.2.0..v2025.2.1) - 2025-02-03

### Chore

- fix winget releaser job by @jdx in [e67c653](https://github.com/jdx/mise/commit/e67c653de35ff83d4ee280bf5cb2381741a2108e)

## [2025.2.0](https://github.com/jdx/mise/compare/v2025.1.17..v2025.2.0) - 2025-02-02

### 🚀 Features

- **(registry)** add kwokctl by @mangkoran in [#4282](https://github.com/jdx/mise/pull/4282)
- add biome to registry by @kit494way in [#4283](https://github.com/jdx/mise/pull/4283)
- add gittool/gitversion by @acesyde in [#4289](https://github.com/jdx/mise/pull/4289)

### 📚 Documentation

- add filtering support to registry docs page by @roele in [#4285](https://github.com/jdx/mise/pull/4285)
- improve registry filtering performance by @roele in [#4287](https://github.com/jdx/mise/pull/4287)
- fix registry table rendering for mobile by @roele in [#4288](https://github.com/jdx/mise/pull/4288)

### Chore

- updated deps by @jdx in [#4290](https://github.com/jdx/mise/pull/4290)
- do not run autofix on renovate PRs by @jdx in [41c5ce4](https://github.com/jdx/mise/commit/41c5ce4c6581f856bf0d756e3fe99ec2fae2e7bd)

### New Contributors

- @ELLIOTTCABLE made their first contribution in [#4280](https://github.com/jdx/mise/pull/4280)

## [2025.1.17](https://github.com/jdx/mise/compare/v2025.1.16..v2025.1.17) - 2025-01-31

### 🚀 Features

- **(registry)** use aqua for duckdb by @mangkoran in [#4270](https://github.com/jdx/mise/pull/4270)

### 🐛 Bug Fixes

- mise does not operate well under Git Bash on Windows by @roele in [#4048](https://github.com/jdx/mise/pull/4048)
- mise rm removes/reports wrong version of tool by @roele in [#4272](https://github.com/jdx/mise/pull/4272)

### 📚 Documentation

- Update python documentation by @hverlin in [#4260](https://github.com/jdx/mise/pull/4260)
- fix postinstall typo in nodejs cookbook by @arafays in [#4251](https://github.com/jdx/mise/pull/4251)
- Fix typo by @henrebotha in [#4277](https://github.com/jdx/mise/pull/4277)

### Hooks.md

- MISE_PROJECT_DIR -> MISE_PROJECT_ROOT by @jubr in [#4269](https://github.com/jdx/mise/pull/4269)

### New Contributors

- @mangkoran made their first contribution in [#4270](https://github.com/jdx/mise/pull/4270)
- @jubr made their first contribution in [#4269](https://github.com/jdx/mise/pull/4269)
- @arafays made their first contribution in [#4251](https://github.com/jdx/mise/pull/4251)

## [2025.1.16](https://github.com/jdx/mise/compare/v2025.1.15..v2025.1.16) - 2025-01-29

### 🚀 Features

- **(registry)** add duckdb by @swfz in [#4248](https://github.com/jdx/mise/pull/4248)

### 🐛 Bug Fixes

- Swift on Ubuntu 24.04 arm64 generates the incorrect download URL by @spyder-ian in [#4235](https://github.com/jdx/mise/pull/4235)
- Do not attempt to parse directories by @adamcohen2 in [#4256](https://github.com/jdx/mise/pull/4256)
- path option should take precedence over global configuration by @roele in [#4249](https://github.com/jdx/mise/pull/4249)

### 📚 Documentation

- Add devtools.fm episode about mise to external-resources.md by @CanRau in [#4253](https://github.com/jdx/mise/pull/4253)
- Update sections about idiomatic version files by @hverlin in [#4252](https://github.com/jdx/mise/pull/4252)

### Chore

- make self_update optional by @jdx in [#4230](https://github.com/jdx/mise/pull/4230)
- added some defaul reqwest features by @jdx in [#4232](https://github.com/jdx/mise/pull/4232)

### New Contributors

- @adamcohen2 made their first contribution in [#4256](https://github.com/jdx/mise/pull/4256)
- @CanRau made their first contribution in [#4253](https://github.com/jdx/mise/pull/4253)
- @spyder-ian made their first contribution in [#4235](https://github.com/jdx/mise/pull/4235)

## [2025.1.15](https://github.com/jdx/mise/compare/v2025.1.14..v2025.1.15) - 2025-01-26

### 🚀 Features

- add http cache by @acesyde in [#4160](https://github.com/jdx/mise/pull/4160)
- expose `test-tool` command by @jdx in [#4224](https://github.com/jdx/mise/pull/4224)

### 🐛 Bug Fixes

- elixir installation failed by @roele in [#4144](https://github.com/jdx/mise/pull/4144)
- re-run tasks when files removed or permissions change by @jdx in [#4223](https://github.com/jdx/mise/pull/4223)

### 🚜 Refactor

- use builder pattern by @acesyde in [#4220](https://github.com/jdx/mise/pull/4220)

### 📚 Documentation

- **(how-i-use-mise)** switch to discussion by @risu729 in [#4225](https://github.com/jdx/mise/pull/4225)
- add hint about environment variable parsing by @roele in [#4219](https://github.com/jdx/mise/pull/4219)

### Chore

- added vscode workspace by @jdx in [a0d181f](https://github.com/jdx/mise/commit/a0d181f8d60270d09d06156ebc500a2fa85f74db)
- switch from git2 to gix by @jdx in [#4226](https://github.com/jdx/mise/pull/4226)
- remove git2 from built by @jdx in [#4227](https://github.com/jdx/mise/pull/4227)
- use mise-plugins/mise-jib by @jdx in [#4228](https://github.com/jdx/mise/pull/4228)

### New Contributors

- @vgnh made their first contribution in [#4216](https://github.com/jdx/mise/pull/4216)

## [2025.1.14](https://github.com/jdx/mise/compare/v2025.1.13..v2025.1.14) - 2025-01-24

### 🚀 Features

- **(registry)** add gron by @MontakOleg in [#4204](https://github.com/jdx/mise/pull/4204)

### 🐛 Bug Fixes

- spurious semver warning on `mise outdated` by @jdx in [#4199](https://github.com/jdx/mise/pull/4199)

### Chore

- lint issue in Dockerfile by @jdx in [47ad5d6](https://github.com/jdx/mise/commit/47ad5d67890188478cf8c8f2e6796b6752546e6c)
- fix some typos in markdown file by @chuangjinglu in [#4198](https://github.com/jdx/mise/pull/4198)
- pin aws-cli by @jdx in [f7311fd](https://github.com/jdx/mise/commit/f7311fd8fc85b6920c5a484862865adc9ef7261d)
- use arm64 runners for docker by @jdx in [#4200](https://github.com/jdx/mise/pull/4200)

### New Contributors

- @chuangjinglu made their first contribution in [#4198](https://github.com/jdx/mise/pull/4198)

## [2025.1.13](https://github.com/jdx/mise/compare/v2025.1.12..v2025.1.13) - 2025-01-24

### Chore

- fixing aws-cli in release.sh by @jdx in [5b4a65a](https://github.com/jdx/mise/commit/5b4a65a84e07141de9ed69798921b4b0ef69aa02)
- fixing aws-cli in release.sh by @jdx in [4c67db5](https://github.com/jdx/mise/commit/4c67db59ecfb55eb724dc05bca7eb7281a625929)

## [2025.1.12](https://github.com/jdx/mise/compare/v2025.1.11..v2025.1.12) - 2025-01-24

### Chore

- setup mise for release task by @jdx in [78d3dfb](https://github.com/jdx/mise/commit/78d3dfb164776cfb39a1920485c21fcd6ecd3ebe)

## [2025.1.11](https://github.com/jdx/mise/compare/v2025.1.10..v2025.1.11) - 2025-01-23

### Chore

- pin aws-cli by @jdx in [ca16daf](https://github.com/jdx/mise/commit/ca16daf5e5dbb9159d853570528087b24f63500b)

## [2025.1.10](https://github.com/jdx/mise/compare/v2025.1.9..v2025.1.10) - 2025-01-23

### 🚀 Features

- **(registry)** use aqua for periphery by @MontakOleg in [#4157](https://github.com/jdx/mise/pull/4157)
- split remote task by @acesyde in [#4156](https://github.com/jdx/mise/pull/4156)

### 🐛 Bug Fixes

- **(docs)** environment variable MISE_OVERRIDE_TOOL_VERSIONS_FILENAME should be plural by @roele in [#4183](https://github.com/jdx/mise/pull/4183)
- completions were missing non-asdf tools by @jdx in [55b31a4](https://github.com/jdx/mise/commit/55b31a452b807ada4e2ba40c8b5588b77b79642e)
- broken link for `/tasks/task-configuration` by @134130 in [#4155](https://github.com/jdx/mise/pull/4155)
- whitespace in mise.run script by @jdx in [#4153](https://github.com/jdx/mise/pull/4153)
- confusing error in fish_command_not_found by @MrGreenTea in [#4162](https://github.com/jdx/mise/pull/4162)
- use correct python path for venv creation in windows by @tisoft in [#4164](https://github.com/jdx/mise/pull/4164)

### 📚 Documentation

- neovim cookbook by @EricDriussi in [#4161](https://github.com/jdx/mise/pull/4161)

### 🧪 Testing

- fix a couple of tool tests by @jdx in [#4186](https://github.com/jdx/mise/pull/4186)

### Chore

- added issue auto-closer by @jdx in [3c831c1](https://github.com/jdx/mise/commit/3c831c19a644fbb2f393f969ebaa5137f9415793)

### New Contributors

- @tisoft made their first contribution in [#4164](https://github.com/jdx/mise/pull/4164)
- @MrGreenTea made their first contribution in [#4162](https://github.com/jdx/mise/pull/4162)
- @EricDriussi made their first contribution in [#4161](https://github.com/jdx/mise/pull/4161)
- @134130 made their first contribution in [#4155](https://github.com/jdx/mise/pull/4155)

## [2025.1.9](https://github.com/jdx/mise/compare/v2025.1.8..v2025.1.9) - 2025-01-17

### 🚀 Features

- **(aqua)** pass --verbose flag down to cosign and added aqua.cosign_extra_args setting by @jdx in [#4148](https://github.com/jdx/mise/pull/4148)
- **(doctor)** display redacted github token by @jdx in [#4149](https://github.com/jdx/mise/pull/4149)

### 🐛 Bug Fixes

- **(ruby)** remove ruby/gem tests by @jdx in [#4130](https://github.com/jdx/mise/pull/4130)
- Fixes fish_command_not_found glob error by @halostatue in [#4133](https://github.com/jdx/mise/pull/4133)
- completions for `mise use` by @jdx in [#4147](https://github.com/jdx/mise/pull/4147)

### 📦️ Dependency Updates

- update dependency bun to v1.1.44 by @renovate[bot] in [#4134](https://github.com/jdx/mise/pull/4134)

### Chore

- add install.sh.sig to releases by @jdx in [1b6ea86](https://github.com/jdx/mise/commit/1b6ea8644edcf3a6ff68fc6d511622c44f1f1f9a)

### New Contributors

- @halostatue made their first contribution in [#4133](https://github.com/jdx/mise/pull/4133)

## [2025.1.8](https://github.com/jdx/mise/compare/v2025.1.7..v2025.1.8) - 2025-01-17

### 🚀 Features

- upgrade ubi by @jdx in [#4078](https://github.com/jdx/mise/pull/4078)
- enable erlang for Windows by @roele in [#4128](https://github.com/jdx/mise/pull/4128)
- use aqua for opentofu by @jdx in [#4129](https://github.com/jdx/mise/pull/4129)

### 🐛 Bug Fixes

- **(spm)** install from annotated tag by @MontakOleg in [#4120](https://github.com/jdx/mise/pull/4120)
- Fixes infinite loop in auto install not found bash function by @bnorick in [#4094](https://github.com/jdx/mise/pull/4094)
- installing with empty version fails by @roele in [#4123](https://github.com/jdx/mise/pull/4123)

### 📚 Documentation

- correct link to gem.rs source by @petrblaho in [#4119](https://github.com/jdx/mise/pull/4119)
- fix {{config_root}} got interpolated by vitepress by @peter50216 in [#4122](https://github.com/jdx/mise/pull/4122)

### Chore

- remove minisign from mise.toml by @jdx in [b115ba9](https://github.com/jdx/mise/commit/b115ba962fce4e63e0d6ce85f41704f302ef3e9a)

### New Contributors

- @peter50216 made their first contribution in [#4122](https://github.com/jdx/mise/pull/4122)
- @petrblaho made their first contribution in [#4119](https://github.com/jdx/mise/pull/4119)

## [2025.1.7](https://github.com/jdx/mise/compare/v2025.1.6..v2025.1.7) - 2025-01-15

### 🚀 Features

- **(registry)** add gup by @scop in [#4107](https://github.com/jdx/mise/pull/4107)
- **(registry)** add aqua and cmdx by @scop in [#4106](https://github.com/jdx/mise/pull/4106)
- use aqua for eza on linux by @jdx in [#4075](https://github.com/jdx/mise/pull/4075)
- allow to specify Rust profile by @roele in [#4101](https://github.com/jdx/mise/pull/4101)

### 🐛 Bug Fixes

- use vars in [env] templates by @hverlin in [#4100](https://github.com/jdx/mise/pull/4100)
- panic when directory name contains japanese characters by @roele in [#4104](https://github.com/jdx/mise/pull/4104)
- incorrect config_root for project/.mise/config.toml by @roele in [#4108](https://github.com/jdx/mise/pull/4108)

### 🚜 Refactor

- **(registry)** alias protobuf to protoc by @scop in [#4087](https://github.com/jdx/mise/pull/4087)
- **(registry)** use aqua for go-getter and kcl by @scop in [#4088](https://github.com/jdx/mise/pull/4088)
- **(registry)** use aqua for powerline-go by @scop in [#4105](https://github.com/jdx/mise/pull/4105)

### 📚 Documentation

- clean up activation instructions by @jdx in [e235c74](https://github.com/jdx/mise/commit/e235c74daa8f5e5f9e1bb89c70a6cff96c08956e)
- correct urls for crawler by @jdx in [21cb77b](https://github.com/jdx/mise/commit/21cb77b1f79a57e6ebd3fec367bd5b223239a3ed)
- added sitemap meta tag by @jdx in [033aa14](https://github.com/jdx/mise/commit/033aa149e8b7a45ea750c09c31438709420214c8)

## [2025.1.6](https://github.com/jdx/mise/compare/v2025.1.5..v2025.1.6) - 2025-01-12

### 🐛 Bug Fixes

- Panic when run without arguments with bootstrapped script by @jdx in [#4065](https://github.com/jdx/mise/pull/4065)

### 🚜 Refactor

- use better rust syntax by @jdx in [#4072](https://github.com/jdx/mise/pull/4072)

### 📚 Documentation

- fix TOML-based Tasks usage spec example by @gturi in [#4067](https://github.com/jdx/mise/pull/4067)
- eza by @jdx in [5a80cbf](https://github.com/jdx/mise/commit/5a80cbf9e0b37be800bc6f6f0404bcf86cbe3bd9)
- removed bit about verifying with asdf by @jdx in [d505486](https://github.com/jdx/mise/commit/d505486fbbe49af0f7bf6029569812441c1e3fdc)
- added more getting started installers by @jdx in [b310e11](https://github.com/jdx/mise/commit/b310e118b00d2b0a64cf2d423d20ece6dc9692f6)
- clean up activation instructions by @jdx in [3df60dd](https://github.com/jdx/mise/commit/3df60dd9cbecf3086b1755d4e397159379d27b27)
- clean up activation instructions by @jdx in [8ab4bce](https://github.com/jdx/mise/commit/8ab4bcef77c4bc1e07951dbb8b5787df4a4b15bf)
- clean up activation instructions by @jdx in [d4a67e8](https://github.com/jdx/mise/commit/d4a67e8ec72fed064cc776ab643f41da1ae01caa)
- clean up activation instructions by @jdx in [d208418](https://github.com/jdx/mise/commit/d208418a5f63803185c4aa5f06afecd9e8832496)
- clean up activation instructions by @jdx in [b9f581d](https://github.com/jdx/mise/commit/b9f581d644295f372eb0cd026560e9c97dcb8091)

### New Contributors

- @gturi made their first contribution in [#4067](https://github.com/jdx/mise/pull/4067)

## [2025.1.5](https://github.com/jdx/mise/compare/v2025.1.4..v2025.1.5) - 2025-01-11

### 🚀 Features

- added gdu and dua to registry by @sassdavid in [#4052](https://github.com/jdx/mise/pull/4052)
- added prefix-dev/pixi by @jdx in [#4056](https://github.com/jdx/mise/pull/4056)
- added `mise cfg --tracked-configs` by @jdx in [#4059](https://github.com/jdx/mise/pull/4059)
- added `mise version --json` flag by @jdx in [#4061](https://github.com/jdx/mise/pull/4061)
- added `mise ls --prunable` flag by @jdx in [#4062](https://github.com/jdx/mise/pull/4062)

### 🐛 Bug Fixes

- switch jib back to asdf by @jdx in [#4055](https://github.com/jdx/mise/pull/4055)
- `mise unuse` bug not pruning if not in config file by @jdx in [#4058](https://github.com/jdx/mise/pull/4058)

### 📚 Documentation

- explain pipx better by @jdx in [42dcb3b](https://github.com/jdx/mise/commit/42dcb3bc5a6547d3d148c391ceccfd9228e34669)

### 🧪 Testing

- added test case for `mise rm` by @jdx in [f7511b6](https://github.com/jdx/mise/commit/f7511b696c2ada7af878074e89b0dfc1edb73197)

### New Contributors

- @sassdavid made their first contribution in [#4052](https://github.com/jdx/mise/pull/4052)

## [2025.1.4](https://github.com/jdx/mise/compare/v2025.1.3..v2025.1.4) - 2025-01-10

### 🚀 Features

- update JSON output for task info/ls by @hverlin in [#4034](https://github.com/jdx/mise/pull/4034)
- **breaking** bump usage to 2.x by @jdx in [#4049](https://github.com/jdx/mise/pull/4049)

### 🐛 Bug Fixes

- ignore github releases marked as draft by @jdx in [#4030](https://github.com/jdx/mise/pull/4030)
- `mise run` shorthand with tasks that have an extension by @jdx in [#4029](https://github.com/jdx/mise/pull/4029)
- use consistent casing by @jdx in [a4d4133](https://github.com/jdx/mise/commit/a4d41338139355b0dd86a068fd89790eb7e34584)
- support latest ansible packages by @jdx in [#4045](https://github.com/jdx/mise/pull/4045)
- use go backend for goconvey/ginkgo by @jdx in [#4047](https://github.com/jdx/mise/pull/4047)
- Improve fig spec with better generators by @miguelmig in [#3762](https://github.com/jdx/mise/pull/3762)

### 📚 Documentation

- set prose-wrap with prettier by @jdx in [#4038](https://github.com/jdx/mise/pull/4038)
- Fix "Example of a NodeJS file task with arguments" by @highb in [#4046](https://github.com/jdx/mise/pull/4046)

### 🧪 Testing

- disable some non-working plugins by @jdx in [106ee40](https://github.com/jdx/mise/commit/106ee40b463923bb5c6444e0c0127dabc502d9ee)
- remove test for flarectl by @jdx in [a63b449](https://github.com/jdx/mise/commit/a63b44910d55ad2cdc801a472f0c196c605cce25)

### ◀️ Revert

- Revert "docs: set prose-wrap with prettier " by @jdx in [065dd8f](https://github.com/jdx/mise/commit/065dd8fa917b6097fb168b631b506455af3e1d28)

### Chore

- added `cargo check` to pre-commit by @jdx in [73eb25a](https://github.com/jdx/mise/commit/73eb25a88bbfe1b979bb5483ca3c81a689be184f)
- fix release-plz pr creation by @jdx in [8299c6b](https://github.com/jdx/mise/commit/8299c6b943119ffda94d18445c5b789948b6f9c0)
- use -q in pre-commit:check by @jdx in [099b2d8](https://github.com/jdx/mise/commit/099b2d88d3ed31ace30c67be816170dc50f87b6d)
- fix release-plz pr creation by @jdx in [c2accc5](https://github.com/jdx/mise/commit/c2accc5f7192202d0a8249ae7f3ab0ea7f100e1b)
- make prettier/pre-commit much faster by @jdx in [#4036](https://github.com/jdx/mise/pull/4036)
- fix release-plz edit command by @jdx in [86b5816](https://github.com/jdx/mise/commit/86b5816660f5a13d45c1795132a29e881645e271)

## [2025.1.3](https://github.com/jdx/mise/compare/v2025.1.2..v2025.1.3) - 2025-01-09

### 🐛 Bug Fixes

- **(rust)** respect RUSTUP_HOME/CARGO_HOME by @jdx in [#4026](https://github.com/jdx/mise/pull/4026)
- mise fails to install kubectl on windows from aqua registry by @roele in [#4006](https://github.com/jdx/mise/pull/4006)
- aliases with aqua by @jdx in [#4007](https://github.com/jdx/mise/pull/4007)
- issue with enter hook and subdirs by @jdx in [#4008](https://github.com/jdx/mise/pull/4008)
- allow using depends and depends_post on separate tasks by @jdx in [#4010](https://github.com/jdx/mise/pull/4010)
- mise fails to install kubectl on windows from aqua registry by @roele in [#4024](https://github.com/jdx/mise/pull/4024)

### 📚 Documentation

- Add default description to github token link by @hverlin in [#4019](https://github.com/jdx/mise/pull/4019)
- fix source code links by @jdx in [#4025](https://github.com/jdx/mise/pull/4025)

### Chore

- make pre-commit faster by @jdx in [70dfdd0](https://github.com/jdx/mise/commit/70dfdd0b874a5292b4b20fa72c9c341a13900bde)
- added commented out paths config by @jdx in [c1f25ac](https://github.com/jdx/mise/commit/c1f25ac4cdaf74219d700fcaf37d3341971a3120)

## [2025.1.2](https://github.com/jdx/mise/compare/v2025.1.1..v2025.1.2) - 2025-01-08

### 🚀 Features

- migrate asdf plugins to aqua/ubi by @jdx in [#3962](https://github.com/jdx/mise/pull/3962)
- migrate asdf plugins to aqua/ubi by @jdx in [#3978](https://github.com/jdx/mise/pull/3978)
- migrate asdf plugins to aqua/ubi by @jdx in [#3991](https://github.com/jdx/mise/pull/3991)
- replace asdf-spark plugin with mise-spark plugin by @benberryallwood in [#3994](https://github.com/jdx/mise/pull/3994)
- add kubectx/kubens to registry by @roele in [#3992](https://github.com/jdx/mise/pull/3992)
- added ktlint from aqua by @jdx in [#4004](https://github.com/jdx/mise/pull/4004)

### 🐛 Bug Fixes

- **(schema)** fix task sources and outputs schema by @risu729 in [#3988](https://github.com/jdx/mise/pull/3988)
- **(schema)** update task schema by @risu729 in [#3999](https://github.com/jdx/mise/pull/3999)
- correct age keyname by @jdx in [e28c293](https://github.com/jdx/mise/commit/e28c293bc5a241b043d0b72ec9aa0559e888f97b)
- mise install rust failed on windows by @roele in [#3969](https://github.com/jdx/mise/pull/3969)
- maven-mvnd does not install with aqua by @roele in [#3982](https://github.com/jdx/mise/pull/3982)
- maven-mvnd does not install with aqua by @roele in [#3993](https://github.com/jdx/mise/pull/3993)
- use friendly error in `mise run` by @jdx in [#3998](https://github.com/jdx/mise/pull/3998)
- use task display_name in more places by @hverlin in [#3997](https://github.com/jdx/mise/pull/3997)
- aqua:apache/spark doesn't work by @roele in [#3995](https://github.com/jdx/mise/pull/3995)

### 📚 Documentation

- style on rustup settings by @jdx in [da91716](https://github.com/jdx/mise/commit/da91716c856b0bb1e8bdf70f9f97f74fe09f15ac)
- Escape template examples by @henrebotha in [#3987](https://github.com/jdx/mise/pull/3987)
- update SECURITY.md by @jdx in [6372f10](https://github.com/jdx/mise/commit/6372f101639386e94cd8df400c78962eab1dbdd5)

### 🧪 Testing

- fix test-plugins CI job for ubuntu-24 by @jdx in [492f6ac](https://github.com/jdx/mise/commit/492f6acc99014cb70f97efdd12700ee365a418ea)
- remove postgres test-plugins test by @jdx in [e93bc80](https://github.com/jdx/mise/commit/e93bc80a780fd0f7b4619af37c3f646dd622bed4)

### Chore

- remove deprecated tar syntax by @jdx in [322735a](https://github.com/jdx/mise/commit/322735a75bef9c602ffcec4d81914662cac00647)
- fix tar/gzip syntax by @jdx in [cd0a049](https://github.com/jdx/mise/commit/cd0a049ecace47354a931cd364ac2f5915812658)
- fork remaining asdf plugins to mise-plugins by @jdx in [#3996](https://github.com/jdx/mise/pull/3996)

### New Contributors

- @henrebotha made their first contribution in [#3987](https://github.com/jdx/mise/pull/3987)

## [2025.1.1](https://github.com/jdx/mise/compare/v2025.1.0..v2025.1.1) - 2025-01-06

### 🚀 Features

- add databricks-cli to registry by @benberryallwood in [#3937](https://github.com/jdx/mise/pull/3937)
- add navi to registry by @kit494way in [#3943](https://github.com/jdx/mise/pull/3943)
- added allurectl to registry by @MontakOleg in [#3918](https://github.com/jdx/mise/pull/3918)
- Add setting description to mise settings --json-extended output by @hverlin in [#3919](https://github.com/jdx/mise/pull/3919)

### 🐛 Bug Fixes

- improve mise generate bootstrap by @hverlin in [#3939](https://github.com/jdx/mise/pull/3939)
- update year in copyright to dynamic with current year by @nexckycort in [#3957](https://github.com/jdx/mise/pull/3957)

### 📚 Documentation

- Fix broken link to environment variables doc by @xcapaldi in [#3938](https://github.com/jdx/mise/pull/3938)
- Add usage property to mise schema by @hverlin in [#3942](https://github.com/jdx/mise/pull/3942)
- clarity on relative paths vs config_root in _.path by @glasser in [#3923](https://github.com/jdx/mise/pull/3923)

### 📦️ Dependency Updates

- update rust crate itertools to 0.14 by @renovate[bot] in [#3926](https://github.com/jdx/mise/pull/3926)
- update rust crate petgraph to 0.7 by @renovate[bot] in [#3927](https://github.com/jdx/mise/pull/3927)
- update rust crate self_update to 0.42 by @renovate[bot] in [#3931](https://github.com/jdx/mise/pull/3931)

### Chore

- upgrade expr by @jdx in [c06a415](https://github.com/jdx/mise/commit/c06a41544e2cb09912244efe6a8f5bcc03eb24d7)
- mise up by @jdx in [678f648](https://github.com/jdx/mise/commit/678f6489a9501b32bf3c36771977771d933f2466)
- cargo-show by @jdx in [69d44fd](https://github.com/jdx/mise/commit/69d44fd064d2fdaae08ff9ea3300a42e560630cd)
- remove cargo-show dependency by @jdx in [ab8e9e9](https://github.com/jdx/mise/commit/ab8e9e9e429beeb23731c356537525f64bc59b28)
- remove cargo-show dependency by @jdx in [ca2f89c](https://github.com/jdx/mise/commit/ca2f89c6cd36d828a9eab2884a3f8c9cc1fe2c19)
- remove cargo-show dependency by @jdx in [82e3390](https://github.com/jdx/mise/commit/82e3390c5fc9a97c942dc407b2073edfcb3974bc)
- fix release-plz by @jdx in [52ac62a](https://github.com/jdx/mise/commit/52ac62a7d7e8439d32b84c4247ee366c28901863)
- fix release-plz by @jdx in [dba7044](https://github.com/jdx/mise/commit/dba7044b4dcce808fd4734e9a284ab2174758be0)

### New Contributors

- @nexckycort made their first contribution in [#3957](https://github.com/jdx/mise/pull/3957)
- @MontakOleg made their first contribution in [#3918](https://github.com/jdx/mise/pull/3918)
- @kit494way made their first contribution in [#3943](https://github.com/jdx/mise/pull/3943)
- @benberryallwood made their first contribution in [#3937](https://github.com/jdx/mise/pull/3937)
- @xcapaldi made their first contribution in [#3938](https://github.com/jdx/mise/pull/3938)
- @auxesis made their first contribution in [#3914](https://github.com/jdx/mise/pull/3914)

## [2025.1.0](https://github.com/jdx/mise/compare/v2024.12.24..v2025.1.0) - 2025-01-01

### 🚀 Features

- use aqua for gradle by @jdx in [#3903](https://github.com/jdx/mise/pull/3903)
- added completions to more commands by @jdx in [#3910](https://github.com/jdx/mise/pull/3910)

### 🐛 Bug Fixes

- panic when setting config value by @roele in [#3823](https://github.com/jdx/mise/pull/3823)
- add hidden settings/task --complete option by @jdx in [#3902](https://github.com/jdx/mise/pull/3902)
- handle panic when task contains invalid template by @jdx in [#3904](https://github.com/jdx/mise/pull/3904)
- missing checksums in mise.run script by @jdx in [#3906](https://github.com/jdx/mise/pull/3906)
- active flag for symlinked tools in `mise ls --json` by @jdx in [#3907](https://github.com/jdx/mise/pull/3907)

### 📚 Documentation

- Update LICENSE by @jdx in [156db11](https://github.com/jdx/mise/commit/156db1130c2757aaaf6e53686148d8b9b0791ae7)
- updated roadmap by @jdx in [f8916d4](https://github.com/jdx/mise/commit/f8916d4cbd09fbbc8142bf25b4d586e146d19a21)

## [2024.12.24](https://github.com/jdx/mise/compare/v2024.12.23..v2024.12.24) - 2024-12-31

### 🐛 Bug Fixes

- switch back to asdf for gradle by @jdx in [cc88dca](https://github.com/jdx/mise/commit/cc88dca50e8e0dac94dbb83d0ce1ebcfc38a1ec4)

### Chore

- add commented out cleanup of old CLIs by @jdx in [bb7e022](https://github.com/jdx/mise/commit/bb7e022240c0e7019a595d093a33b414119e975f)

## [2024.12.23](https://github.com/jdx/mise/compare/v2024.12.22..v2024.12.23) - 2024-12-30

### 🐛 Bug Fixes

- winget release PRs by @jdx in [9dec542](https://github.com/jdx/mise/commit/9dec542188e731ef357fd74339dd08ac005cb9e3)
- mise settings unset does not seem to work by @roele in [#3867](https://github.com/jdx/mise/pull/3867)
- gradle aqua package by @jdx in [#3880](https://github.com/jdx/mise/pull/3880)
- **breaking** remove `root` env var in tasks by @jdx in [#3884](https://github.com/jdx/mise/pull/3884)

### 📚 Documentation

- syntax in `mise watch` by @jdx in [beab480](https://github.com/jdx/mise/commit/beab48029b3e7a91047012b655f3efe4fd722acf)
- Update registry link by @bmulholland in [#3864](https://github.com/jdx/mise/pull/3864)
- clarify shims behaviour by @syhol in [#3881](https://github.com/jdx/mise/pull/3881)

### Chore

- remove unused versioned tarballs from mise.jdx.dev by @jdx in [48f1021](https://github.com/jdx/mise/commit/48f1021048646061e7cd85d9f9969946b00962a6)
- trim newline in banner by @jdx in [c8f2c90](https://github.com/jdx/mise/commit/c8f2c90111c5d20fe4586d59eb66f3bb2f8cfd9a)

### New Contributors

- @bmulholland made their first contribution in [#3864](https://github.com/jdx/mise/pull/3864)

## [2024.12.22](https://github.com/jdx/mise/compare/v2024.12.21..v2024.12.22) - 2024-12-30

### 🚀 Features

- colorize banner by @jdx in [ad3a5f0](https://github.com/jdx/mise/commit/ad3a5f040013bad046f2ca3abb9eebc941301368)

### 🐛 Bug Fixes

- add `:` escaping for tasks with multiple colons by @eitamal in [#3853](https://github.com/jdx/mise/pull/3853)
- type issue in docs/JSON schema for python_create_args and uv_create_args by @roele in [#3855](https://github.com/jdx/mise/pull/3855)

### 📚 Documentation

- **(settings)** fix link to precompiled python binaries by @scop in [#3851](https://github.com/jdx/mise/pull/3851)
- Fix cargo install examples by @orf in [#3862](https://github.com/jdx/mise/pull/3862)

### New Contributors

- @orf made their first contribution in [#3862](https://github.com/jdx/mise/pull/3862)
- @eitamal made their first contribution in [#3853](https://github.com/jdx/mise/pull/3853)

## [2024.12.21](https://github.com/jdx/mise/compare/v2024.12.20..v2024.12.21) - 2024-12-27

### 🐛 Bug Fixes

- **(python)** force precompiled setting warning message syntax by @scop in [#3850](https://github.com/jdx/mise/pull/3850)
- zstd detection false positive on MacOS by @roele in [#3845](https://github.com/jdx/mise/pull/3845)

### 📚 Documentation

- fix incorrect examples that were causing 'expected a sequence' error by @ssbarnea in [#3839](https://github.com/jdx/mise/pull/3839)

### 📦️ Dependency Updates

- update rust crate ubi to 0.3 by @renovate[bot] in [#3836](https://github.com/jdx/mise/pull/3836)

## [2024.12.20](https://github.com/jdx/mise/compare/v2024.12.19..v2024.12.20) - 2024-12-25

### 🚀 Features

- **(hugo)** add extended registry from aqua and keep only one registry with all aliases by @kilianpaquier in [#3813](https://github.com/jdx/mise/pull/3813)
- build erlang with all cores by @jdx in [#3802](https://github.com/jdx/mise/pull/3802)
- Modify install_rubygems_hook to place plugin in site_ruby directory by @zkhadikov in [#3812](https://github.com/jdx/mise/pull/3812)

### 🐛 Bug Fixes

- do not require "v" prefix in mise.run by @jdx in [#3800](https://github.com/jdx/mise/pull/3800)
- add checksum for macos-x86 by @jdx in [#3815](https://github.com/jdx/mise/pull/3815)

### 📚 Documentation

- Correct link to aqua registry by @jesse-c in [#3803](https://github.com/jdx/mise/pull/3803)

### 🧪 Testing

- skip dotnet if not installed by @jdx in [1a663dd](https://github.com/jdx/mise/commit/1a663dd63e17cc08a961b86b5b0b6a1d7e9b2a1f)

### New Contributors

- @zkhadikov made their first contribution in [#3812](https://github.com/jdx/mise/pull/3812)
- @kilianpaquier made their first contribution in [#3813](https://github.com/jdx/mise/pull/3813)
- @jesse-c made their first contribution in [#3803](https://github.com/jdx/mise/pull/3803)

## [2024.12.19](https://github.com/jdx/mise/compare/v2024.12.18..v2024.12.19) - 2024-12-23

### 🚀 Features

- use zstd in mise.run by @jdx in [#3798](https://github.com/jdx/mise/pull/3798)
- verify zig with minisign by @jdx in [#3793](https://github.com/jdx/mise/pull/3793)

### Chore

- increase tarball compression by @jdx in [a899155](https://github.com/jdx/mise/commit/a8991551bd7c61d1f75a800906d2f718b4bdf7c0)
- use max threads for zstd compression by @jdx in [a3f792a](https://github.com/jdx/mise/commit/a3f792a1eb0a395c7a82a063b96d30282b6343de)
- print all tarball sizes by @jdx in [29fbc04](https://github.com/jdx/mise/commit/29fbc04e52c76b16c9a72385ead4edbfaff984fb)

## [2024.12.18](https://github.com/jdx/mise/compare/v2024.12.17..v2024.12.18) - 2024-12-23

### 🚀 Features

- allow dotnet prerelease by @acesyde in [#3753](https://github.com/jdx/mise/pull/3753)
- added minisign to registry by @jdx in [#3788](https://github.com/jdx/mise/pull/3788)
- `mise g bootstrap` by @jdx in [#3792](https://github.com/jdx/mise/pull/3792)
- `mise g bootstrap` by @jdx in [f79ce71](https://github.com/jdx/mise/commit/f79ce719f9121eb6e0e821cf271af306f2a9d6c8)

### 🐛 Bug Fixes

- hide task file extension in completions by @jdx in [#3772](https://github.com/jdx/mise/pull/3772)
- settings completions by @jdx in [#3787](https://github.com/jdx/mise/pull/3787)

### 📚 Documentation

- update IDE integration page by @hverlin in [#3765](https://github.com/jdx/mise/pull/3765)
- add powershell sample by @acesyde in [#3771](https://github.com/jdx/mise/pull/3771)
- add missing dotnet left menu by @acesyde in [#3770](https://github.com/jdx/mise/pull/3770)

### 🧪 Testing

- added stubbed test for https://github.com/jdx/mise/discussions/3783 by @jdx in [f79a3a4](https://github.com/jdx/mise/commit/f79a3a41ebf833d2c49bdc91ae4026c46498d9f7)

### ◀️ Revert

- Revert "fix: Use arguments for to pass staged filenames to pre-commit task (#…" by @jdx in [#3791](https://github.com/jdx/mise/pull/3791)

### Chore

- add shell to user-agent by @jdx in [#3786](https://github.com/jdx/mise/pull/3786)
- sign releases with minisign by @jdx in [#3789](https://github.com/jdx/mise/pull/3789)
- create minisign secret key by @jdx in [dea4676](https://github.com/jdx/mise/commit/dea4676f53ee4d1a905ae17b004131c6dee3b385)
- create minisign secret key by @jdx in [ecebebe](https://github.com/jdx/mise/commit/ecebebee13cc20773eaefda706bad4e5ac8cc25f)
- fix minisign signing by @jdx in [6401ff8](https://github.com/jdx/mise/commit/6401ff84e0dcbdb890dd037aff6fbcf3edc51af5)
- added install.sh to releases by @jdx in [2946d58](https://github.com/jdx/mise/commit/2946d5864cffb65a1ee1260f3c38070531743854)
- install minisign by @jdx in [f22272c](https://github.com/jdx/mise/commit/f22272c3838fcb8de0365a4022f8aefc00c46f4c)
- use ubuntu-24 for release by @jdx in [40a13f8](https://github.com/jdx/mise/commit/40a13f8e7088ba13762178eccc5eb8438bc9ce6b)
- set minisign pub key by @jdx in [fd6aa1e](https://github.com/jdx/mise/commit/fd6aa1eccf23f97e82ff166ff8950721c236239b)
- age encrypt minisign key by @jdx in [02c30e2](https://github.com/jdx/mise/commit/02c30e2c9167d3f4bf5ac05a82a43bc82b703123)
- apt install age by @jdx in [769a088](https://github.com/jdx/mise/commit/769a08875b3651c3edd63fd4387497ce6b16cd4b)
- switch back to MINISIGN_KEY by @jdx in [66dc8cf](https://github.com/jdx/mise/commit/66dc8cf199adb57c22ac398b3333ba12abaaf106)
- fix minisign signing by @jdx in [a3f8173](https://github.com/jdx/mise/commit/a3f81738bb4ab0827eb6bfae4a1639c29f29da36)
- add zst tarballs by @jdx in [85a1192](https://github.com/jdx/mise/commit/85a1192091b7f37ab7c3712e4100c8b43d587857)
- add zst tarballs by @jdx in [5238124](https://github.com/jdx/mise/commit/5238124dbda89fe32380beab9b64d31cb2cb4ddb)
- add zst tarballs by @jdx in [2a4d0bf](https://github.com/jdx/mise/commit/2a4d0bf0ee78dfe672d97bc763643300516d5a9b)
- add zst tarballs by @jdx in [285d777](https://github.com/jdx/mise/commit/285d777b3f33bfa587070b3d15cd904fc83e111f)
- extract artifact with zstd by @jdx in [ba66d46](https://github.com/jdx/mise/commit/ba66d4659c6d8f3ffa589dacfe402d6988e46d9a)

## [2024.12.17](https://github.com/jdx/mise/compare/v2024.12.16..v2024.12.17) - 2024-12-21

### 🚀 Features

- added a banner to `mise --version` by @jdx in [#3748](https://github.com/jdx/mise/pull/3748)
- add usage field to tasks by @jdx in [#3746](https://github.com/jdx/mise/pull/3746)
- added keep-order task output type by @jdx in [#3763](https://github.com/jdx/mise/pull/3763)
- `replacing` task output type by @jdx in [#3764](https://github.com/jdx/mise/pull/3764)
- added timed task output type by @jdx in [#3766](https://github.com/jdx/mise/pull/3766)

### 🐛 Bug Fixes

- dotnet backend doc by @acesyde in [#3752](https://github.com/jdx/mise/pull/3752)
- include full env in toolset tera_ctx by @risu729 in [#3751](https://github.com/jdx/mise/pull/3751)
- set env vars in task templates by @jdx in [#3758](https://github.com/jdx/mise/pull/3758)

### 📚 Documentation

- update mise-action version in tips and tricks by @scop in [#3749](https://github.com/jdx/mise/pull/3749)
- Small cookbooks fixes by @hverlin in [#3754](https://github.com/jdx/mise/pull/3754)

### 🧪 Testing

- fix elixir release test by @jdx in [b4f11da](https://github.com/jdx/mise/commit/b4f11dabf7a16a875f9d7ab3ded6a516b481f6f8)
- add some test cases for env var templates by @jdx in [c938977](https://github.com/jdx/mise/commit/c938977ccc265c9530200e0b19bb0cce5f73ddbb)

### Chore

- updated usage by @jdx in [dad7857](https://github.com/jdx/mise/commit/dad785727c80efeb4bf498995ed5237f6cd94d79)

## [2024.12.16](https://github.com/jdx/mise/compare/v2024.12.15..v2024.12.16) - 2024-12-20

### 🚀 Features

- add dotnet backend by @acesyde in [#3737](https://github.com/jdx/mise/pull/3737)
- added ignored_config_paths to `mise dr` by @jdx in [#3742](https://github.com/jdx/mise/pull/3742)

### 🐛 Bug Fixes

- **(ruby)** fix Ruby plugin to use `ruby_install` option correctly by @yuhr in [#3732](https://github.com/jdx/mise/pull/3732)
- `mise run` shorthand with options by @jdx in [#3719](https://github.com/jdx/mise/pull/3719)
- zig on windows by @jdx in [#3739](https://github.com/jdx/mise/pull/3739)
- allow using previously defined vars by @jdx in [#3741](https://github.com/jdx/mise/pull/3741)
- make --help consistent with `mise run` and `mise <task>` by @jdx in [#3723](https://github.com/jdx/mise/pull/3723)
- use implicit keys for `mise config set` by @jdx in [#3744](https://github.com/jdx/mise/pull/3744)

### 📚 Documentation

- update cookbook by @hverlin in [#3718](https://github.com/jdx/mise/pull/3718)
- remove reference to deprecated asdf_compat functionality by @jdx in [03a2afb](https://github.com/jdx/mise/commit/03a2afb4f8c738e3b172d0f5e1ca1465bf1d6a5c)
- describe behavior of `run --output` better by @jdx in [#3740](https://github.com/jdx/mise/pull/3740)

### 📦️ Dependency Updates

- update dependency bun to v1.1.40 by @renovate[bot] in [#3729](https://github.com/jdx/mise/pull/3729)

### Chore

- lint fix by @jdx in [118b8de](https://github.com/jdx/mise/commit/118b8de645712ff1d78c33b9a2c094a1f92c5b20)
- switch from home -> homedir crate by @jdx in [#3743](https://github.com/jdx/mise/pull/3743)

### New Contributors

- @acesyde made their first contribution in [#3737](https://github.com/jdx/mise/pull/3737)
- @ssbarnea made their first contribution in [#3735](https://github.com/jdx/mise/pull/3735)
- @yuhr made their first contribution in [#3732](https://github.com/jdx/mise/pull/3732)

## [2024.12.15](https://github.com/jdx/mise/compare/v2024.12.14..v2024.12.15) - 2024-12-19

### 🚀 Features

- unnest output when `mise run` is nested by @jdx in [#3686](https://github.com/jdx/mise/pull/3686)
- `mise rm` by @jdx in [#3627](https://github.com/jdx/mise/pull/3627)
- added *:_default task name by @jdx in [#3690](https://github.com/jdx/mise/pull/3690)
- `mise run --continue-on-error by @jdx in [#3692](https://github.com/jdx/mise/pull/3692)
- added .tool-versions -> mise.toml converter by @jdx in [#3693](https://github.com/jdx/mise/pull/3693)
- get mise sync python --uv to work by @jdx in [#3706](https://github.com/jdx/mise/pull/3706)
- `mise install-into` by @jdx in [#3711](https://github.com/jdx/mise/pull/3711)
- added `mise dr --json` by @jdx in [#3715](https://github.com/jdx/mise/pull/3715)

### 🐛 Bug Fixes

- retain "os" options in `mise up --bump` by @jdx in [#3688](https://github.com/jdx/mise/pull/3688)
- unnest task cmd output by @jdx in [#3691](https://github.com/jdx/mise/pull/3691)
- ensure MISE_PROJECT_ROOT is set with no mise.toml by @jdx in [#3695](https://github.com/jdx/mise/pull/3695)
- create venv uses absolute tool paths by @syhol in [#3698](https://github.com/jdx/mise/pull/3698)
- jj repository moved to an organization by @phyrog in [#3703](https://github.com/jdx/mise/pull/3703)
- disable reverse uv syncing by @jdx in [#3704](https://github.com/jdx/mise/pull/3704)
- add full tera context to tasks by @jdx in [#3708](https://github.com/jdx/mise/pull/3708)
- powershell warning by @jdx in [#3713](https://github.com/jdx/mise/pull/3713)

### 🚜 Refactor

- **(registry)** use aqua for more tools by @scop in [#3614](https://github.com/jdx/mise/pull/3614)
- **(registry)** use aqua:skaji/relocatable-perl for perl by @scop in [#3716](https://github.com/jdx/mise/pull/3716)
- switch to std::sync::LazyLock by @jdx in [#3707](https://github.com/jdx/mise/pull/3707)

### 📚 Documentation

- fix some broken anchor links by @hverlin in [#3694](https://github.com/jdx/mise/pull/3694)
- note hooks require `mise activate` by @jdx in [211d3d3](https://github.com/jdx/mise/commit/211d3d3b91c52e418a3e25af4a021da93c64ed4d)

### 🧪 Testing

- fix conduit test for new structure by @jdx in [8691331](https://github.com/jdx/mise/commit/86913318f7705e6cabb999970475c958605219d1)

### Chore

- hide non-functioning docker tasks by @jdx in [40fd3f6](https://github.com/jdx/mise/commit/40fd3f60ebde1d549503a6d9927b79b37622b1b0)

### New Contributors

- @highb made their first contribution in [#3696](https://github.com/jdx/mise/pull/3696)

## [2024.12.14](https://github.com/jdx/mise/compare/v2024.12.13..v2024.12.14) - 2024-12-18

### 🚀 Features

- **(registry)** Add lazydocker by @hverlin in [#3655](https://github.com/jdx/mise/pull/3655)
- **(registry)** Add btop by @hverlin in [#3667](https://github.com/jdx/mise/pull/3667)
- Allows control of config_root for global config by @bnorick in [#3670](https://github.com/jdx/mise/pull/3670)
- allow inserting PATH in env._.source by @jdx in [#3685](https://github.com/jdx/mise/pull/3685)

### 🐛 Bug Fixes

- Can not find the bin files when using python venv on windows by @NavyD in [#3664](https://github.com/jdx/mise/pull/3664)
- render tasks in task files by @risu729 in [#3666](https://github.com/jdx/mise/pull/3666)
- dont require run script for `task add` by @jdx in [#3675](https://github.com/jdx/mise/pull/3675)
- auto-trust on `task add` by @jdx in [#3676](https://github.com/jdx/mise/pull/3676)
- completions getting wrapped in quotes by @jdx in [#3679](https://github.com/jdx/mise/pull/3679)
- pass pristine env to tera in final_env by @risu729 in [#3682](https://github.com/jdx/mise/pull/3682)
- trap panics in task resolving by @jdx in [#3677](https://github.com/jdx/mise/pull/3677)

### 📚 Documentation

- mark new features as experimental by @syhol in [#3659](https://github.com/jdx/mise/pull/3659)

### 🧪 Testing

- add test cases for venv templates by @jdx in [#3683](https://github.com/jdx/mise/pull/3683)

### New Contributors

- @NavyD made their first contribution in [#3664](https://github.com/jdx/mise/pull/3664)

## [2024.12.13](https://github.com/jdx/mise/compare/v2024.12.12..v2024.12.13) - 2024-12-17

### 🚀 Features

- `mise task add` by @jdx in [#3616](https://github.com/jdx/mise/pull/3616)
- elixir core tool by @jdx in [#3620](https://github.com/jdx/mise/pull/3620)
- elixir on windows by @jdx in [#3623](https://github.com/jdx/mise/pull/3623)
- added install_env tool option by @jdx in [#3622](https://github.com/jdx/mise/pull/3622)
- Add Powershell support by @fgilcc in [#3506](https://github.com/jdx/mise/pull/3506)
- improve redactions by @jdx in [#3647](https://github.com/jdx/mise/pull/3647)

### 🐛 Bug Fixes

- run venv after tools are loaded by @jdx in [#3612](https://github.com/jdx/mise/pull/3612)
- some improvements to `mise fmt` by @jdx in [#3615](https://github.com/jdx/mise/pull/3615)
- always run postinstall hook by @jdx in [#3618](https://github.com/jdx/mise/pull/3618)
- move bat from aqua to ubi by @jdx in [60d0c79](https://github.com/jdx/mise/commit/60d0c798f695199bdc81f8beec737f0e2a8589e0)
- do not require version for `mise sh --unset` by @jdx in [#3628](https://github.com/jdx/mise/pull/3628)
- back nomad with nomad, not levant by @rliebz in [#3633](https://github.com/jdx/mise/pull/3633)
- correct python precompiled urls for freebsd by @jdx in [#3637](https://github.com/jdx/mise/pull/3637)
- bug fixes with tools=true in env by @jdx in [#3639](https://github.com/jdx/mise/pull/3639)
- sort keys in `__MISE_DIFF` to make the serialised value deterministic by @joshbode in [#3640](https://github.com/jdx/mise/pull/3640)
- resolve config_root for dir tasks option by @risu729 in [#3649](https://github.com/jdx/mise/pull/3649)

### 📚 Documentation

- add getting-started carousel by @hverlin in [#3613](https://github.com/jdx/mise/pull/3613)
- Fix Sops URL by @matthew-snyder in [#3619](https://github.com/jdx/mise/pull/3619)
- add elixir to sidebar by @risu729 in [#3650](https://github.com/jdx/mise/pull/3650)
- update task documentation by @hverlin in [#3651](https://github.com/jdx/mise/pull/3651)

### Chore

- format toml with taplo by @jdx in [#3625](https://github.com/jdx/mise/pull/3625)
- add platform field to registry backends by @jdx in [#3626](https://github.com/jdx/mise/pull/3626)

### New Contributors

- @fgilcc made their first contribution in [#3506](https://github.com/jdx/mise/pull/3506)
- @rliebz made their first contribution in [#3633](https://github.com/jdx/mise/pull/3633)
- @matthew-snyder made their first contribution in [#3619](https://github.com/jdx/mise/pull/3619)

## [2024.12.12](https://github.com/jdx/mise/compare/v2024.12.11..v2024.12.12) - 2024-12-16

### 🚀 Features

- Add upx,actionlint and correct ripsecret error by @boris-smidt-klarrio in [#3601](https://github.com/jdx/mise/pull/3601)
- aqua:argo-cd by @boris-smidt-klarrio in [#3600](https://github.com/jdx/mise/pull/3600)
- task tools by @jdx in [#3599](https://github.com/jdx/mise/pull/3599)
- lazy env eval by @jdx in [#3598](https://github.com/jdx/mise/pull/3598)
- added cache feature to templates by @jdx in [#3608](https://github.com/jdx/mise/pull/3608)

### 🐛 Bug Fixes

- added MISE_SOPS_ROPS setting by @jdx in [#3603](https://github.com/jdx/mise/pull/3603)
- respect CLICOLOR_FORCE by @jdx in [#3607](https://github.com/jdx/mise/pull/3607)
- only create 1 venv by @jdx in [#3610](https://github.com/jdx/mise/pull/3610)
- set bash --noprofile for env._.source by @jdx in [#3611](https://github.com/jdx/mise/pull/3611)

### 📚 Documentation

- improve settings a bit by @jdx in [d53d011](https://github.com/jdx/mise/commit/d53d01195e88e82d9a88a410e8feb991c1e8179d)
- Install on Windows - Update doc on install on Windows with Scoop and WinGet + fix NOTE section by @o-l-a-v in [#3604](https://github.com/jdx/mise/pull/3604)
- remove note about winget by @jdx in [9c0c1ce](https://github.com/jdx/mise/commit/9c0c1ce943c6fb54ca049d6cdfb81c1122987d05)

### Chore

- disable automatic cargo up on release by @jdx in [3f0d91a](https://github.com/jdx/mise/commit/3f0d91a40928df8ed10cef1837730d8c3a15efea)

### New Contributors

- @o-l-a-v made their first contribution in [#3604](https://github.com/jdx/mise/pull/3604)

## [2024.12.11](https://github.com/jdx/mise/compare/v2024.12.10..v2024.12.11) - 2024-12-15

### 🚀 Features

- added selector for `mise use` with no args by @jdx in [#3570](https://github.com/jdx/mise/pull/3570)
- added tool descriptions by @jdx in [#3571](https://github.com/jdx/mise/pull/3571)
- added `mise sync python --uv` by @jdx in [#3575](https://github.com/jdx/mise/pull/3575)
- `sync ruby --brew` by @jdx in [#3577](https://github.com/jdx/mise/pull/3577)
- encrypted configs by @jdx in [#3584](https://github.com/jdx/mise/pull/3584)
- added `mise --no-config` by @jdx in [#3590](https://github.com/jdx/mise/pull/3590)
- allow _.file in vars by @jdx in [#3593](https://github.com/jdx/mise/pull/3593)

### 🐛 Bug Fixes

- **(python)** reduce network usage for python precompiled manifests by @jdx in [#3568](https://github.com/jdx/mise/pull/3568)
- **(python)** check only if first or specified python is installed for _.venv by @jdx in [#3576](https://github.com/jdx/mise/pull/3576)
- **(swift)** prevent swift from using linux platforms that are not available by @jdx in [#3583](https://github.com/jdx/mise/pull/3583)
- correct headers on `mise ls` by @jdx in [5af3b17](https://github.com/jdx/mise/commit/5af3b17a41decd2d7368f5985f2cb5d3e3b341e8)
- correct message truncation in `mise run` by @jdx in [c668857](https://github.com/jdx/mise/commit/c6688571cfb0eca70a55377b70ec6b9cd0cb6a68)
- include uv in path for hook-env by @jdx in [#3572](https://github.com/jdx/mise/pull/3572)
- correct subtitle in `mise use` selector by @jdx in [4be6d79](https://github.com/jdx/mise/commit/4be6d798f9398f9e072d4067a56e134463e71b41)
- some bugs with status.show_tools and status.show_env by @jdx in [#3586](https://github.com/jdx/mise/pull/3586)
- use task.display_name for `mise run` by @jdx in [a009de1](https://github.com/jdx/mise/commit/a009de13ffa4319de89b0fcaf1ba54ae2524a9b6)
- path is treated differently in nushell by @samuelallan72 in [#3592](https://github.com/jdx/mise/pull/3592)
- allow number/bool in .env.json by @jdx in [#3594](https://github.com/jdx/mise/pull/3594)

### 🚜 Refactor

- break up env_directive by @jdx in [#3587](https://github.com/jdx/mise/pull/3587)

### 📚 Documentation

- better warning when venv auto create is skipped by @syhol in [#3573](https://github.com/jdx/mise/pull/3573)
- added rendered go settings by @jdx in [b41c3dd](https://github.com/jdx/mise/commit/b41c3dd8cfd97f97352900a9d856194185347e8d)

### New Contributors

- @fhalim made their first contribution in [#3595](https://github.com/jdx/mise/pull/3595)

## [2024.12.10](https://github.com/jdx/mise/compare/v2024.12.9..v2024.12.10) - 2024-12-14

### 🚀 Features

- **(python)** add other indygreg flavors by @jdx in [#3565](https://github.com/jdx/mise/pull/3565)
- redactions by @jdx in [#3529](https://github.com/jdx/mise/pull/3529)
- show unload messages/run leave hook by @jdx in [#3532](https://github.com/jdx/mise/pull/3532)
- update demand and default `mise run` to filtering by @jdx in [48c366d](https://github.com/jdx/mise/commit/48c366d4d2256f6b12aabcbe82abe429622b120e)

### 🐛 Bug Fixes

- **(go)** only use "v" prefix if version is semver-like by @jdx in [#3556](https://github.com/jdx/mise/pull/3556)
- **(go)** fix non-v installs by @jdx in [36e7631](https://github.com/jdx/mise/commit/36e7631e26445f9f2bc34fd09a93ba9a15363c98)
- disable libgit2 for updating plugin repos for now by @jdx in [#3533](https://github.com/jdx/mise/pull/3533)
- rename kubelogin to azure-kubelogin and add replace it with more popular kubelogin cli by @jdx in [#3534](https://github.com/jdx/mise/pull/3534)
- add backend to lockfile by @jdx in [#3535](https://github.com/jdx/mise/pull/3535)
- parse task env vars as templates by @jdx in [#3536](https://github.com/jdx/mise/pull/3536)
- do not add ignore file if not tty by @jdx in [#3558](https://github.com/jdx/mise/pull/3558)
- improve output of `mise tasks` by @jdx in [#3562](https://github.com/jdx/mise/pull/3562)

### 📚 Documentation

- add installation via zinit by @Finkregh in [#3563](https://github.com/jdx/mise/pull/3563)

### Chore

- added comfy-table by @jdx in [#3561](https://github.com/jdx/mise/pull/3561)
- pitchfork by @jdx in [2c47f72](https://github.com/jdx/mise/commit/2c47f721c03e8fed57a8ae5ed2f63a0649ffaa9b)
- updated usage by @jdx in [#3564](https://github.com/jdx/mise/pull/3564)
- added install-dev task by @jdx in [0c351a8](https://github.com/jdx/mise/commit/0c351a83d952cff8b953fd5c244698a14d74c305)

### New Contributors

- @Finkregh made their first contribution in [#3563](https://github.com/jdx/mise/pull/3563)

## [2024.12.9](https://github.com/jdx/mise/compare/v2024.12.8..v2024.12.9) - 2024-12-14

### 🚀 Features

- **(tasks)** optional automatic outputs by @jdx in [#3528](https://github.com/jdx/mise/pull/3528)
- added quiet field to tasks by @jdx in [#3514](https://github.com/jdx/mise/pull/3514)
- show instructions for updating when min_version does not match by @jdx in [#3520](https://github.com/jdx/mise/pull/3520)
- several enhancements to tasks by @jdx in [#3526](https://github.com/jdx/mise/pull/3526)

### 🐛 Bug Fixes

- make bash_completions lib optional by @jdx in [#3516](https://github.com/jdx/mise/pull/3516)
- make plugin update work with libgit2 by @jdx in [#3519](https://github.com/jdx/mise/pull/3519)
- bug with `mise task edit` and new tasks by @jdx in [#3521](https://github.com/jdx/mise/pull/3521)
- correct self-update message by @jdx in [eff0cff](https://github.com/jdx/mise/commit/eff0cffca079ee58fc2297396604b96e0253c324)
- task source bug fixes by @jdx in [#3522](https://github.com/jdx/mise/pull/3522)

### 📚 Documentation

- add explanation about shebang by @hverlin in [#3501](https://github.com/jdx/mise/pull/3501)
- add vitepress-plugin-group-icons by @hverlin in [#3527](https://github.com/jdx/mise/pull/3527)

### 🧪 Testing

- pin swift version by @jdx in [2b966a4](https://github.com/jdx/mise/commit/2b966a4945851b35be593182527bd40a80279fe4)
- skip firebase by @jdx in [e5714bc](https://github.com/jdx/mise/commit/e5714bcfe9cd45f173aecefcbd3c95fbeab83417)

### 📦️ Dependency Updates

- update rust crate bzip2 to 0.5 by @renovate[bot] in [#3511](https://github.com/jdx/mise/pull/3511)

## [2024.12.8](https://github.com/jdx/mise/compare/v2024.12.7..v2024.12.8) - 2024-12-12

### 🚀 Features

- **(registry)** use pipx for pdm by @risu729 in [#3504](https://github.com/jdx/mise/pull/3504)
- added pitchfork by @jdx in [bac731e](https://github.com/jdx/mise/commit/bac731e47f00245ce13e7eec5716509704519d71)

### 🐛 Bug Fixes

- Adds support for multi-use args by @bnorick in [#3505](https://github.com/jdx/mise/pull/3505)
- make task completion script POSIX by @jdx in [b92b560](https://github.com/jdx/mise/commit/b92b5603bb23d55b58e7ee8effe8d6293036c5a9)

### 📚 Documentation

- Add more examples for toml tasks by @hverlin in [#3491](https://github.com/jdx/mise/pull/3491)

### Chore

- use main branch for winget by @jdx in [b4036cf](https://github.com/jdx/mise/commit/b4036cf0d10f6ccd8758b0bebc341963c8777d2e)

### New Contributors

- @bnorick made their first contribution in [#3505](https://github.com/jdx/mise/pull/3505)
- @elchocarrero made their first contribution in [#3502](https://github.com/jdx/mise/pull/3502)

## [2024.12.7](https://github.com/jdx/mise/compare/v2024.12.6..v2024.12.7) - 2024-12-12

### 🚀 Features

- add the users PATH to `mise doctor` by @syhol in [#3474](https://github.com/jdx/mise/pull/3474)
- feat : Add superfile with aqua backend to registery by @yodatak in [#3479](https://github.com/jdx/mise/pull/3479)
- added `task_auto_install` setting by @jdx in [#3481](https://github.com/jdx/mise/pull/3481)
- Add yazi with aqua backend to registery by @yodatak in [#3485](https://github.com/jdx/mise/pull/3485)
- Migrating Terragrunt asdf plugin over to gruntwork-io by @yhakbar in [#3486](https://github.com/jdx/mise/pull/3486)
- add settings for python venv creation by @jdx in [#3489](https://github.com/jdx/mise/pull/3489)
- added MISE_ARCH setting by @jdx in [#3490](https://github.com/jdx/mise/pull/3490)
- add jj to registry by @phyrog in [#3495](https://github.com/jdx/mise/pull/3495)
- add task descriptions to completions by @jdx in [#3497](https://github.com/jdx/mise/pull/3497)

### 🐛 Bug Fixes

- mise upgrade with rust by @jdx in [#3475](https://github.com/jdx/mise/pull/3475)
- improve arg parsing for mise watch by @jdx in [#3478](https://github.com/jdx/mise/pull/3478)
- skip reading ignored config dirs by @jdx in [#3480](https://github.com/jdx/mise/pull/3480)
- deprecated attribute in json schema by @jdx in [#3482](https://github.com/jdx/mise/pull/3482)
- simplify auto_install settings by @jdx in [#3483](https://github.com/jdx/mise/pull/3483)
- use config_root for env._.source by @jdx in [#3484](https://github.com/jdx/mise/pull/3484)
- allow directories as task source by @jdx in [#3488](https://github.com/jdx/mise/pull/3488)
- Use arguments for to pass staged filenames to pre-commit task by @joshbode in [#3492](https://github.com/jdx/mise/pull/3492)

### 📚 Documentation

- updated `mise watch` docs to drop the `-t` by @jdx in [8ea6226](https://github.com/jdx/mise/commit/8ea622688cb01a0a0a2805692b38a4a7f1340ce5)

### Chore

- move debug log to trace by @jdx in [5c6c884](https://github.com/jdx/mise/commit/5c6c884cf51e704d1c8c347790ec30b30b0f401e)

### New Contributors

- @yhakbar made their first contribution in [#3486](https://github.com/jdx/mise/pull/3486)

## [2024.12.6](https://github.com/jdx/mise/compare/v2024.12.5..v2024.12.6) - 2024-12-11

### 🚀 Features

- added descriptions to `mise run` by @jdx in [#3460](https://github.com/jdx/mise/pull/3460)
- `mise format` by @jdx in [#3461](https://github.com/jdx/mise/pull/3461)
- `mise fmt` (renamed from `mise format`) by @jdx in [#3465](https://github.com/jdx/mise/pull/3465)
- `mise format` by @jdx in [d18b040](https://github.com/jdx/mise/commit/d18b040b8ae8eea16ed98b7f7b884a6f52797edc)

### 🐛 Bug Fixes

- **(swift)** remove clang bins by @jdx in [#3468](https://github.com/jdx/mise/pull/3468)
- use 7zip for windows zip by @jdx in [475ae62](https://github.com/jdx/mise/commit/475ae62d209795cf8fe9cc846f258755e1092918)
- disable filtering by default on `mise run` by @jdx in [507ee27](https://github.com/jdx/mise/commit/507ee27a736b8cd57714a8365fc88855edf62507)
- deprecate direnv integration by @jdx in [#3464](https://github.com/jdx/mise/pull/3464)
- remove hidden commands from docs by @jdx in [42a9a05](https://github.com/jdx/mise/commit/42a9a0567fbd8ef61550cf2bfe956074777c7d76)
- improve hook-env by @jdx in [#3466](https://github.com/jdx/mise/pull/3466)
- deprecate @system versions by @jdx in [#3467](https://github.com/jdx/mise/pull/3467)
- do not reuse local tool options for `mise use -g` by @jdx in [#3469](https://github.com/jdx/mise/pull/3469)
- allow "~" in python.default_packages_file by @jdx in [#3472](https://github.com/jdx/mise/pull/3472)
- read all config files for `mise set` by @jdx in [#3473](https://github.com/jdx/mise/pull/3473)

### 📚 Documentation

- fixing elvish install instructions by @ejrichards in [#3459](https://github.com/jdx/mise/pull/3459)
- remove bad formatting in setting by @jdx in [f33813b](https://github.com/jdx/mise/commit/f33813bde40cf65e946a3c1773a4275fce3cb0ef)
- added external links by @jdx in [8271e7b](https://github.com/jdx/mise/commit/8271e7ba0fa8628279cff0460715ec9c80a1c6bd)

### Chore

- fix windows zip structure by @jdx in [195039f](https://github.com/jdx/mise/commit/195039ff2bbe702c7e80ace3fcaeb95cb02d018b)

### New Contributors

- @ejrichards made their first contribution in [#3459](https://github.com/jdx/mise/pull/3459)

## [2024.12.5](https://github.com/jdx/mise/compare/v2024.12.4..v2024.12.5) - 2024-12-10

### 🚀 Features

- make `mise trust` act on directories instead of files by @jdx in [#3454](https://github.com/jdx/mise/pull/3454)

### 🐛 Bug Fixes

- correctly lowercase "zsh" for shell hooks by @jdx in [035ae59](https://github.com/jdx/mise/commit/035ae59bd898a16be4fcd55b708ae8ba620c60fe)
- read MISE_CONFIG_DIR/conf.d/*.toml configs by @jdx in [#3439](https://github.com/jdx/mise/pull/3439)
- retains spm artifacts by @jdx in [#3441](https://github.com/jdx/mise/pull/3441)
- add env var for MISE_NPM_BUN setting by @jdx in [b3c57e2](https://github.com/jdx/mise/commit/b3c57e29bd26d772e2f708351a3c61bf04ee3d65)
- hide hidden tasks in `mise run` selector UI by @jdx in [#3449](https://github.com/jdx/mise/pull/3449)
- trim run scripts whitespace by @jdx in [#3450](https://github.com/jdx/mise/pull/3450)
- shell-escape arg() in tasks by @jdx in [#3453](https://github.com/jdx/mise/pull/3453)
- use shebang in run script to determine how arg escaping should work by @jdx in [#3455](https://github.com/jdx/mise/pull/3455)

### 📚 Documentation

- example with required version by @felixhummel in [#3448](https://github.com/jdx/mise/pull/3448)
- document new windows installers by @jdx in [#3452](https://github.com/jdx/mise/pull/3452)

### Chore

- added winget workflow by @jdx in [901e048](https://github.com/jdx/mise/commit/901e04865842f765188dd687584f9120ad4e5519)

### New Contributors

- @felixhummel made their first contribution in [#3448](https://github.com/jdx/mise/pull/3448)

## [2024.12.4](https://github.com/jdx/mise/compare/v2024.12.3..v2024.12.4) - 2024-12-09

### 🚀 Features

- add staged files to `mise generate git-pre-commit` by @jdx in [#3410](https://github.com/jdx/mise/pull/3410)
- shell hooks by @jdx in [#3414](https://github.com/jdx/mise/pull/3414)
- added cowsay by @jdx in [#3420](https://github.com/jdx/mise/pull/3420)
- add openbao by @phyrog in [#3426](https://github.com/jdx/mise/pull/3426)
- add gocryptfs by @phyrog in [#3427](https://github.com/jdx/mise/pull/3427)
- use aqua for flyctl by @jdx in [f7ed363](https://github.com/jdx/mise/commit/f7ed363b3eebb82e6242061e78f9ebfdf050d154)

### 🐛 Bug Fixes

- do not set debug mode when calling `mise -v` by @jdx in [#3418](https://github.com/jdx/mise/pull/3418)
- issue with usage and arg completions by @jdx in [#3433](https://github.com/jdx/mise/pull/3433)

### 📚 Documentation

- Small documentation improvements by @hverlin in [#3413](https://github.com/jdx/mise/pull/3413)
- updated demo.gif by @jdx in [#3419](https://github.com/jdx/mise/pull/3419)

### Build

- update default.nix by @minhtrancccp in [#3430](https://github.com/jdx/mise/pull/3430)

### New Contributors

- @will-ockmore made their first contribution in [#3435](https://github.com/jdx/mise/pull/3435)
- @minhtrancccp made their first contribution in [#3430](https://github.com/jdx/mise/pull/3430)
- @phyrog made their first contribution in [#3427](https://github.com/jdx/mise/pull/3427)

## [2024.12.3](https://github.com/jdx/mise/compare/v2024.12.2..v2024.12.3) - 2024-12-08

### 🚀 Features

- add danger-swift by @msnazarow in [#3406](https://github.com/jdx/mise/pull/3406)

### 📚 Documentation

- **(backend)** fix git url syntax example by @risu729 in [#3404](https://github.com/jdx/mise/pull/3404)
- update dev-tools overview documentation by @hverlin in [#3400](https://github.com/jdx/mise/pull/3400)

### ⚡ Performance

- increase performance of watch_files by @jdx in [#3407](https://github.com/jdx/mise/pull/3407)
- make `ls --offline` default behavior by @jdx in [#3409](https://github.com/jdx/mise/pull/3409)

### New Contributors

- @msnazarow made their first contribution in [#3406](https://github.com/jdx/mise/pull/3406)

## [2024.12.2](https://github.com/jdx/mise/compare/v2024.12.1..v2024.12.2) - 2024-12-07

### 🚀 Features

- **(registry)** add zls to registry by @hverlin in [#3392](https://github.com/jdx/mise/pull/3392)
- Add --json-extended option to mise env by @hverlin in [#3389](https://github.com/jdx/mise/pull/3389)

### 🐛 Bug Fixes

- **(config)** set config_root for tasks defined in included toml files by @risu729 in [#3388](https://github.com/jdx/mise/pull/3388)
- global hooks by @jdx in [#3393](https://github.com/jdx/mise/pull/3393)
- only run watch_file hook when it has changed file by @jdx in [#3394](https://github.com/jdx/mise/pull/3394)
- bug with aliasing core tools by @jdx in [#3395](https://github.com/jdx/mise/pull/3395)
- remove shims directory before activating by @jdx in [#3396](https://github.com/jdx/mise/pull/3396)

### 🚜 Refactor

- use github crate to list zig releases by @risu729 in [#3386](https://github.com/jdx/mise/pull/3386)

### 📚 Documentation

- add zig to core tools by @risu729 in [#3385](https://github.com/jdx/mise/pull/3385)

### Chore

- debug log by @jdx in [0075db0](https://github.com/jdx/mise/commit/0075db05a24a9bc2e3015b8a48bcfe730fe80d07)

## [2024.12.1](https://github.com/jdx/mise/compare/v2024.12.0..v2024.12.1) - 2024-12-06

### 🚀 Features

- **(registry)** use aqua for some tools by @risu729 in [#3375](https://github.com/jdx/mise/pull/3375)
- allow filtering `mise bin-paths` on tools by @jdx in [#3367](https://github.com/jdx/mise/pull/3367)
- added aws-cli from aqua by @jdx in [#3370](https://github.com/jdx/mise/pull/3370)
- multiple MISE_ENV environments by @jdx in [#3371](https://github.com/jdx/mise/pull/3371)
- add mise-task.json schema by @hverlin in [#3374](https://github.com/jdx/mise/pull/3374)
- automatically call `hook-env` by @jdx in [#3373](https://github.com/jdx/mise/pull/3373)

### 🐛 Bug Fixes

- **(docs)** correct syntax error in IDE integration examples by @EricGusmao in [#3360](https://github.com/jdx/mise/pull/3360)
- ensure version check message is displayed by @jdx in [#3358](https://github.com/jdx/mise/pull/3358)
- show warning if no precompiled pythons found by @jdx in [#3359](https://github.com/jdx/mise/pull/3359)
- allow compilation not on macOS, Linux, or Windows by @avysk in [#3363](https://github.com/jdx/mise/pull/3363)
- make hook-env compatible with zsh auto_name_dirs by @jdx in [#3366](https://github.com/jdx/mise/pull/3366)
- skip optional env._.file files by @jdx in [#3381](https://github.com/jdx/mise/pull/3381)
- .terraform-version by @jdx in [#3380](https://github.com/jdx/mise/pull/3380)

### 📚 Documentation

- update auto-completion docs by @hverlin in [#3355](https://github.com/jdx/mise/pull/3355)
- fix `Environment variables passed to tasks` section by @hverlin in [#3378](https://github.com/jdx/mise/pull/3378)

### 🧪 Testing

- try to fix coverage rate limits by @jdx in [#3384](https://github.com/jdx/mise/pull/3384)

### New Contributors

- @avysk made their first contribution in [#3363](https://github.com/jdx/mise/pull/3363)
- @EricGusmao made their first contribution in [#3360](https://github.com/jdx/mise/pull/3360)

## [2024.12.0](https://github.com/jdx/mise/compare/v2024.11.37..v2024.12.0) - 2024-12-04

### 🚀 Features

- **(erlang)** use precompiled binaries for macos by @jdx in [#3353](https://github.com/jdx/mise/pull/3353)
- add upctl by @scop in [#3309](https://github.com/jdx/mise/pull/3309)
- Add `json-with-sources` option to settings ls by @hverlin in [#3307](https://github.com/jdx/mise/pull/3307)
- add ripsecrets to registry.toml by @boris-smidt-klarrio in [#3334](https://github.com/jdx/mise/pull/3334)
- Add kyverno-cli by @boris-smidt-klarrio in [#3336](https://github.com/jdx/mise/pull/3336)

### 🐛 Bug Fixes

- add exec to `mise g git-pre-commit` by @jdx in [27a3aef](https://github.com/jdx/mise/commit/27a3aefa767c8ef142009dd54c4d7dcc19c235b2)
- bake gpg keys in by @jdx in [#3318](https://github.com/jdx/mise/pull/3318)
- deprecate `mise local|global` by @jdx in [#3350](https://github.com/jdx/mise/pull/3350)

### 🚜 Refactor

- use aqua for ruff by @scop in [#3316](https://github.com/jdx/mise/pull/3316)

### 📚 Documentation

- add terraform recipe to the cookbook by @AliSajid in [#3305](https://github.com/jdx/mise/pull/3305)
- fix git examples for cargo backend by @tmeijn in [#3335](https://github.com/jdx/mise/pull/3335)

### 🧪 Testing

- remove non-working maven test by @jdx in [5a3ed16](https://github.com/jdx/mise/commit/5a3ed16efb29dbf80f5ac251eec39e3a462d2219)
- remove gleam by @jdx in [fdfe20b](https://github.com/jdx/mise/commit/fdfe20b32b16b835655551d3f12b5d6e90856b2e)
- use latest golang in e2e test by @jdx in [#3349](https://github.com/jdx/mise/pull/3349)

### Chore

- upgrade usage-lib by @jdx in [554d533](https://github.com/jdx/mise/commit/554d533a253a137c27c5cdac6da2ae09629029dc)
- use asdf:mise-plugins/mise-nim by @jdx in [#3352](https://github.com/jdx/mise/pull/3352)

### New Contributors

- @leogurja made their first contribution in [#3341](https://github.com/jdx/mise/pull/3341)
- @tmeijn made their first contribution in [#3335](https://github.com/jdx/mise/pull/3335)
- @boris-smidt-klarrio made their first contribution in [#3336](https://github.com/jdx/mise/pull/3336)
- @AliSajid made their first contribution in [#3305](https://github.com/jdx/mise/pull/3305)

## [2024.11.37](https://github.com/jdx/mise/compare/v2024.11.36..v2024.11.37) - 2024-11-30

### 🚀 Features

- add black by @scop in [#3292](https://github.com/jdx/mise/pull/3292)
- migrate more tools away from asdf by @jdx in [40f92c6](https://github.com/jdx/mise/commit/40f92c6b0e1fefd171dd44ee9f62f1f597ee352c)

### 🐛 Bug Fixes

- handle General/Complex Versioning in --bump by @liskin in [#2889](https://github.com/jdx/mise/pull/2889)
- broken path example by @minddust in [#3296](https://github.com/jdx/mise/pull/3296)
- swift path on macos by @jdx in [#3299](https://github.com/jdx/mise/pull/3299)
- do not auto-install on `mise x` if some tools are passed by @jdx in [35d31a1](https://github.com/jdx/mise/commit/35d31a1baf96fe6f0e764e26228c1b03ba24ddce)
- fix: also make certain we are not auto installing inside shims by checking by @jdx in [b0c4a74](https://github.com/jdx/mise/commit/b0c4a749309064825852041d8d72c7eac9fb116c)
- cache github release information for 24 hours by @jdx in [#3300](https://github.com/jdx/mise/pull/3300)

### 🚜 Refactor

- use aqua for snyk by @scop in [#3290](https://github.com/jdx/mise/pull/3290)

### ◀️ Revert

- Revert "fix: always prefer glibc to musl in mise run " by @jdx in [#3298](https://github.com/jdx/mise/pull/3298)

### Chore

- bump expr-lang by @jdx in [#3297](https://github.com/jdx/mise/pull/3297)
- mise up --bump by @jdx in [6872b54](https://github.com/jdx/mise/commit/6872b5469622140335a12131dfa4acf310fc0c2a)
- update mise.lock by @jdx in [4c12502](https://github.com/jdx/mise/commit/4c12502c459ba2e214689c3f55d964b8f75966af)
- disable tool tests until I can sort out gh rate limit issues by @jdx in [f42f010](https://github.com/jdx/mise/commit/f42f010f03a57cab128290c0b9d936fd7a90c785)

### New Contributors

- @minddust made their first contribution in [#3296](https://github.com/jdx/mise/pull/3296)

## [2024.11.36](https://github.com/jdx/mise/compare/v2024.11.35..v2024.11.36) - 2024-11-29

### Chore

- mise i by @jdx in [8150732](https://github.com/jdx/mise/commit/81507327e7f1c9f2137b3dadcf35a8245d43a8ba)

## [2024.11.35](https://github.com/jdx/mise/compare/v2024.11.34..v2024.11.35) - 2024-11-29

### 🚀 Features

- migrate more tools away from asdf by @jdx in [#3279](https://github.com/jdx/mise/pull/3279)

### 🐛 Bug Fixes

- remove conflicting MISE_SHELL setting by @jdx in [#3284](https://github.com/jdx/mise/pull/3284)

### 🚜 Refactor

- simplify __MISE_WATCH variable to only contain the most recent timestamp by @jdx in [#3282](https://github.com/jdx/mise/pull/3282)

### 🧪 Testing

- remove unnecessary cargo-binstall test by @jdx in [0a4da7a](https://github.com/jdx/mise/commit/0a4da7a023b1cb969b732afd3ad4b3cf02c42530)

### Chore

- dont require build-windows before unit-windows by @jdx in [c85e2ec](https://github.com/jdx/mise/commit/c85e2ec77193d73ff20d4ce8fb7e3787a6db223d)

## [2024.11.34](https://github.com/jdx/mise/compare/v2024.11.33..v2024.11.34) - 2024-11-29

### 🚀 Features

- fragmented configs by @jdx in [#3273](https://github.com/jdx/mise/pull/3273)
- hooks by @jdx in [#3256](https://github.com/jdx/mise/pull/3256)
- added MISE_TASK_DISABLE_PATHS setting by @jdx in [9c2e6e4](https://github.com/jdx/mise/commit/9c2e6e40f3a98f352fbf03107e1901dec445a7f5)
- gpg verification for node by @jdx in [#3277](https://github.com/jdx/mise/pull/3277)

### 🐛 Bug Fixes

- make _.file and _.source optional if the file is missing by @jdx in [#3275](https://github.com/jdx/mise/pull/3275)
- prevent deadlock when resetting by @jdx in [8e6d093](https://github.com/jdx/mise/commit/8e6d09377de81c65203684725fa9dfc2140db520)
- prevent deadlock when resetting by @jdx in [201ba90](https://github.com/jdx/mise/commit/201ba904052379595e399672d1657ed0e3c3a138)
- prevent deadlock when resetting by @jdx in [169338a](https://github.com/jdx/mise/commit/169338a2debb99ee4dd885376c4123740237af23)

### 🚜 Refactor

- clean up arcs by @jdx in [f49d330](https://github.com/jdx/mise/commit/f49d330b6f97b08e72b1a448af0021708b2a2417)

### 📚 Documentation

- added hooks to sidebar by @jdx in [4bbc340](https://github.com/jdx/mise/commit/4bbc3403e46aa817450e6936f37b5d4c983b43d4)
- added swift to sidebar by @jdx in [bc06cbf](https://github.com/jdx/mise/commit/bc06cbf240cc7aae2173575cfa83289ae526dad1)

### Chore

- skip checkov test by @jdx in [2ae18a3](https://github.com/jdx/mise/commit/2ae18a3e8329eb9913dc43ae94432f8f75b36a94)
- added timeout for release-plz by @jdx in [dae4bc3](https://github.com/jdx/mise/commit/dae4bc32bbb7de7873e3fa047a785c70f02a5c05)
- remove coverage by @jdx in [#3278](https://github.com/jdx/mise/pull/3278)

## [2024.11.33](https://github.com/jdx/mise/compare/v2024.11.32..v2024.11.33) - 2024-11-28

### 🚀 Features

- respect --quiet in `mise run` by @jdx in [#3257](https://github.com/jdx/mise/pull/3257)
- added special "_" portion of mise.toml for custom data by @jdx in [#3259](https://github.com/jdx/mise/pull/3259)
- **breaking** added MISE_OVERRIDE_CONFIG_FILENAMES config by @jdx in [#3266](https://github.com/jdx/mise/pull/3266)
- added swift by @jdx in [#3271](https://github.com/jdx/mise/pull/3271)

### 🐛 Bug Fixes

- **(spm)** git proxy config by @jdx in [#3264](https://github.com/jdx/mise/pull/3264)
- clean up some windows error cases by @jdx in [#3255](https://github.com/jdx/mise/pull/3255)
- run `hook-env` on directory change by @jdx in [#3258](https://github.com/jdx/mise/pull/3258)
- always prefer glibc to musl in mise run by @jdx in [#3261](https://github.com/jdx/mise/pull/3261)
- issue with non-default backends not getting tool options by @jdx in [#3265](https://github.com/jdx/mise/pull/3265)
- explicitly stop progress bars when exiting by @jdx in [#3272](https://github.com/jdx/mise/pull/3272)

### 🚜 Refactor

- use aqua for shellcheck by @scop in [#3270](https://github.com/jdx/mise/pull/3270)
- use aqua for goreleaser by @scop in [#3269](https://github.com/jdx/mise/pull/3269)
- use aqua for golangci-lint by @scop in [#3268](https://github.com/jdx/mise/pull/3268)

### 📚 Documentation

- describe mise behavior when mise version is lower than min_version by @erickguan in [#2994](https://github.com/jdx/mise/pull/2994)

### Chore

- wait for gh rate limit if expended by @jdx in [#3251](https://github.com/jdx/mise/pull/3251)
- set github token for docs job by @jdx in [908dd18](https://github.com/jdx/mise/commit/908dd18fe3ddf19d1531c93695ee3ff98d0995c5)
- skip hyperfine unless on release pr by @jdx in [#3253](https://github.com/jdx/mise/pull/3253)
- move tasks dir so it doesnt show up in unrelated projects by @jdx in [#3254](https://github.com/jdx/mise/pull/3254)

## [2024.11.32](https://github.com/jdx/mise/compare/v2024.11.31..v2024.11.32) - 2024-11-27

### 🚀 Features

- allow running tasks without `mise run`, e.g.: `mise test` as shorthand for `mise run test` by @jdx in [#3235](https://github.com/jdx/mise/pull/3235)
- default task directory config by @jdx in [#3238](https://github.com/jdx/mise/pull/3238)
- standalone tasks by @jdx in [#3240](https://github.com/jdx/mise/pull/3240)
- automatic uv venv activation by @jdx in [#3239](https://github.com/jdx/mise/pull/3239)
- migrate more tools away from asdf by @jdx in [#3242](https://github.com/jdx/mise/pull/3242)
- add committed by @scop in [#3247](https://github.com/jdx/mise/pull/3247)
- use ubi for figma-export by @jdx in [19dbeac](https://github.com/jdx/mise/commit/19dbeac16a68248bb780a2de1056d16409714204)
- add vacuum by @scop in [#3249](https://github.com/jdx/mise/pull/3249)

### 🐛 Bug Fixes

- skip _.source files if not present by @jdx in [#3236](https://github.com/jdx/mise/pull/3236)
- rust idiomatic file parsing by @jdx in [#3241](https://github.com/jdx/mise/pull/3241)
- automatic reinstall of uvx tools during python upgrades by @jdx in [#3243](https://github.com/jdx/mise/pull/3243)

### 🚜 Refactor

- use aqua for shfmt by @scop in [#3244](https://github.com/jdx/mise/pull/3244)
- use aqua for lefthook by @scop in [#3246](https://github.com/jdx/mise/pull/3246)
- use aqua for nfpm by @scop in [#3248](https://github.com/jdx/mise/pull/3248)

### 📚 Documentation

- correction in aqua by @jdx in [b7de2f3](https://github.com/jdx/mise/commit/b7de2f32e6a23458bbd3573372f9c49733b80e62)
- typo by @jdx in [98aa6bd](https://github.com/jdx/mise/commit/98aa6bd7b2631a5904243cbf9aeb2eaf218c9c64)

### Chore

- bump tabled by @jdx in [#3245](https://github.com/jdx/mise/pull/3245)
- fix tools tests on release branch by @jdx in [675a2b0](https://github.com/jdx/mise/commit/675a2b086116f0afb431189c51136255b6f6c434)
- fix tools tests on release branch by @jdx in [130c3a4](https://github.com/jdx/mise/commit/130c3a4de60edfbed98642bc6dc71e67ba9b6ce1)
- fix tools tests on release branch by @jdx in [9feb3b6](https://github.com/jdx/mise/commit/9feb3b638ef634d320f576921b3e366f6cd73075)

### New Contributors

- @rmacklin made their first contribution in [#2295](https://github.com/jdx/mise/pull/2295)

## [2024.11.31](https://github.com/jdx/mise/compare/v2024.11.30..v2024.11.31) - 2024-11-27

### 🚀 Features

- rust in core by @jdx in [#3219](https://github.com/jdx/mise/pull/3219)

### 🐛 Bug Fixes

- use tv.pathname() in `mise ls` by @jdx in [#3217](https://github.com/jdx/mise/pull/3217)
- show gh rate limit reset time by @jdx in [#3221](https://github.com/jdx/mise/pull/3221)
- add @version back into show_tools by @jdx in [fd7d8d1](https://github.com/jdx/mise/commit/fd7d8d10395f8c80a80c60c0de89bf78e31fd762)
- use pipx for yamllint by @jdx in [#3227](https://github.com/jdx/mise/pull/3227)
- remove shims directory in `mise activate` by @jdx in [#3232](https://github.com/jdx/mise/pull/3232)

### 🚜 Refactor

- remove duplicate remote_versions_caches by @jdx in [#3220](https://github.com/jdx/mise/pull/3220)

### 📚 Documentation

- rename legacy version files to idiomatic version files by @jdx in [#3216](https://github.com/jdx/mise/pull/3216)
- document aqua better by @jdx in [#3234](https://github.com/jdx/mise/pull/3234)

### 🎨 Styling

- spelling and grammar fixes by @scop in [#3225](https://github.com/jdx/mise/pull/3225)

### 🧪 Testing

- move some unit tests to e2e by @jdx in [#3218](https://github.com/jdx/mise/pull/3218)
- migrate tests from unit to e2e by @jdx in [#3231](https://github.com/jdx/mise/pull/3231)

## [2024.11.30](https://github.com/jdx/mise/compare/v2024.11.29..v2024.11.30) - 2024-11-26

### 🚀 Features

- migrate wren-cli to ubi by @jdx in [#3193](https://github.com/jdx/mise/pull/3193)
- migrate more tools away from asdf by @jdx in [#3202](https://github.com/jdx/mise/pull/3202)
- automatically set `set -e` in toml tasks by @jdx in [#3215](https://github.com/jdx/mise/pull/3215)
- added MISE_ORIGINAL_CWD to tasks by @jdx in [#3214](https://github.com/jdx/mise/pull/3214)
- add ruby backend by @andrewthauer in [#1657](https://github.com/jdx/mise/pull/1657)
- migrate more tools away from asdf by @jdx in [#3205](https://github.com/jdx/mise/pull/3205)

### 🐛 Bug Fixes

- Make Rebar backend depend on Erlang by @eproxus in [#3197](https://github.com/jdx/mise/pull/3197)
- trust system/global config by default by @jdx in [#3201](https://github.com/jdx/mise/pull/3201)
- use tv.short in show_tools by @jdx in [#3213](https://github.com/jdx/mise/pull/3213)

### 📚 Documentation

- flatten tools in sidebar by @jdx in [0556024](https://github.com/jdx/mise/commit/0556024b5abdb2d5f1cb025d105494c71aa79647)

### 🧪 Testing

- remove flaky maven test by @jdx in [65f6eb4](https://github.com/jdx/mise/commit/65f6eb48880b6322439c33b3cd53eab7b8b97439)
- added test for vault by @jdx in [#3194](https://github.com/jdx/mise/pull/3194)

### Chore

- bump expr-lang by @jdx in [#3199](https://github.com/jdx/mise/pull/3199)
- add aqua-registry as submodule by @jdx in [#3204](https://github.com/jdx/mise/pull/3204)

### New Contributors

- @eproxus made their first contribution in [#3197](https://github.com/jdx/mise/pull/3197)

## [2024.11.29](https://github.com/jdx/mise/compare/v2024.11.28..v2024.11.29) - 2024-11-25

### 🚀 Features

- **(env)** Allow exporting env vars as dotenv format by @miguelmig in [#3185](https://github.com/jdx/mise/pull/3185)
- move more tools away from asdf by @jdx in [#3184](https://github.com/jdx/mise/pull/3184)
- use aqua for cargo-binstall by @jdx in [#3182](https://github.com/jdx/mise/pull/3182)

### 🐛 Bug Fixes

- use shift_remove by @jdx in [#3188](https://github.com/jdx/mise/pull/3188)
- pass boolean tool options as strings by @jdx in [#3191](https://github.com/jdx/mise/pull/3191)
- move semver cmp errors to debug by @jdx in [ab4e638](https://github.com/jdx/mise/commit/ab4e638cdeda9845f3b7421a22a0d3bf71d81eae)
- show more accurate error if no tasks are available by @jdx in [e1b1b48](https://github.com/jdx/mise/commit/e1b1b48840b8c96e45a567a47922138544ab9f59)
- move semver cmp errors to debug by @jdx in [#3172](https://github.com/jdx/mise/pull/3172)
- use aqua for terraform by @jdx in [#3192](https://github.com/jdx/mise/pull/3192)

### 🧪 Testing

- disable cargo-binstall test by @jdx in [8fee82e](https://github.com/jdx/mise/commit/8fee82e652031a1c9a31dbb05437478c961b6107)

### Chore

- include aqua-registry yaml files in crate by @jdx in [#3186](https://github.com/jdx/mise/pull/3186)
- gitignore aqua-registry by @jdx in [1c38bca](https://github.com/jdx/mise/commit/1c38bca434cfc17792eb3053be2f4271a9e92fdd)
- gitignore aqua-registry by @jdx in [644cb6d](https://github.com/jdx/mise/commit/644cb6dfa762d6360b5aaa7fce0502fe61ac1067)

## [2024.11.28] - 2024-11-24

### 🚀 Features

- migrate more tools away from asdf by @jdx in [#3170](https://github.com/jdx/mise/pull/3170)
- auto-install tools on `mise run` by @jdx in [#3181](https://github.com/jdx/mise/pull/3181)
- move more tools away from asdf by @jdx in [#3179](https://github.com/jdx/mise/pull/3179)

### 🐛 Bug Fixes

- allow passing integers to task env by @jdx in [#3177](https://github.com/jdx/mise/pull/3177)
- remove __MISE_WATCH,__MISE_DIFF env vars on `mise deactivate` by @jdx in [#3178](https://github.com/jdx/mise/pull/3178)

### 📚 Documentation

- **(security)** added information about checksums/cosign/slsa verification by @jdx in [1faef6e](https://github.com/jdx/mise/commit/1faef6ecbb48692955f4ce424d77d03472aa4617)
- **(security)** added release gpg key by @jdx in [8f5dfd6](https://github.com/jdx/mise/commit/8f5dfd6dd2903c55fd792aeecd8ec97ef9f7f7ba)
- typos by @jdx in [#3173](https://github.com/jdx/mise/pull/3173)

### Chore

- clean up CHANGELOG by @jdx in [8ec0ca2](https://github.com/jdx/mise/commit/8ec0ca20fce57d07d769209fd9043a129daa86f1)

<!-- generated by git-cliff -->
