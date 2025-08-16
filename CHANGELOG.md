# Changelog

## [2025.8.11](https://github.com/jdx/mise/compare/v2025.8.10..v2025.8.11) - 2025-08-16

### 📦 Registry

- add container-use ([aqua:dagger/container-use](https://github.com/dagger/container-use)) by [@TyceHerrman](https://github.com/TyceHerrman) in [#6029](https://github.com/jdx/mise/pull/6029)
- add prek ([aqua:j178/prek](https://github.com/j178/prek)) by [@HenryZhang-ZHY](https://github.com/HenryZhang-ZHY) in [#6023](https://github.com/jdx/mise/pull/6023)

### 🚀 Features

- **(task)** allow more #MISE comments patterns by [@risu729](https://github.com/risu729) in [#6011](https://github.com/jdx/mise/pull/6011)

### 📚 Documentation

- **(settings)** use php as an example for `disable_default_registry` by [@risu729](https://github.com/risu729) in [#6025](https://github.com/jdx/mise/pull/6025)
- Update ide-integration.md by [@jdx](https://github.com/jdx) in [#6035](https://github.com/jdx/mise/pull/6035)
- Update ide-integration.md by [@jdx](https://github.com/jdx) in [#6040](https://github.com/jdx/mise/pull/6040)
- added openSUSE zypper install instructions by [@lfromanini](https://github.com/lfromanini) in [#6037](https://github.com/jdx/mise/pull/6037)

### Chore

- fix warnings by [@jdx](https://github.com/jdx) in [#6043](https://github.com/jdx/mise/pull/6043)
- remove unused permissions in registry test by [@risu729](https://github.com/risu729) in [#6044](https://github.com/jdx/mise/pull/6044)

### New Contributors

- @HenryZhang-ZHY made their first contribution in [#6023](https://github.com/jdx/mise/pull/6023)
- @lfromanini made their first contribution in [#6037](https://github.com/jdx/mise/pull/6037)

## [2025.8.10](https://github.com/jdx/mise/compare/v2025.8.9..v2025.8.10) - 2025-08-14

### 📦 Registry

- enable kubecolor test by [@risu729](https://github.com/risu729) in [#6008](https://github.com/jdx/mise/pull/6008)
- fix os specific backends for usage by [@risu729](https://github.com/risu729) in [#6007](https://github.com/jdx/mise/pull/6007)
- use aqua backend for restish by [@risu729](https://github.com/risu729) in [#5986](https://github.com/jdx/mise/pull/5986)
- add cfssljson ([aqua:cloudflare/cfssl/cfssljson](https://github.com/cloudflare/cfssl/cfssljson)) by [@disintegrator](https://github.com/disintegrator) in [#6013](https://github.com/jdx/mise/pull/6013)
- add claude-squad ([aqua:smtg-ai/claude-squad](https://github.com/smtg-ai/claude-squad)) by [@TyceHerrman](https://github.com/TyceHerrman) in [#5894](https://github.com/jdx/mise/pull/5894)

### 🚀 Features

- **(aqua)** make bin paths executable by [@risu729](https://github.com/risu729) in [#6010](https://github.com/jdx/mise/pull/6010)
- added header bar during `mise install` by [@jdx](https://github.com/jdx) in [#6022](https://github.com/jdx/mise/pull/6022)

### 🐛 Bug Fixes

- **(aqua)** improve warnings for packages without repo_owner and repo_name  (2nd attempt) by [@risu729](https://github.com/risu729) in [#6009](https://github.com/jdx/mise/pull/6009)
- version prefix detection by [@risu729](https://github.com/risu729) in [#5943](https://github.com/jdx/mise/pull/5943)
- respect MISE_DEFAULT_CONFIG_FILENAME by [@risu729](https://github.com/risu729) in [#5899](https://github.com/jdx/mise/pull/5899)

### New Contributors

- @disintegrator made their first contribution in [#6013](https://github.com/jdx/mise/pull/6013)

## [2025.8.9](https://github.com/jdx/mise/compare/v2025.8.8..v2025.8.9) - 2025-08-13

### 📦 Registry

- add vfox-yarn as primary yarn backend by [@jdx](https://github.com/jdx) in [#5982](https://github.com/jdx/mise/pull/5982)
- add missing description field for a lot of tools by [@jylenhof](https://github.com/jylenhof) in [#5966](https://github.com/jdx/mise/pull/5966)
- rename benthos to redpanda-connect by [@risu729](https://github.com/risu729) in [#5984](https://github.com/jdx/mise/pull/5984)
- rename coq to rocq by [@risu729](https://github.com/risu729) in [#5985](https://github.com/jdx/mise/pull/5985)

### 🚀 Features

- **(timeout)** show duration, URL, and config hint on timeouts; increase fetch timeout default to 10s by [@jdx](https://github.com/jdx) in [#5991](https://github.com/jdx/mise/pull/5991)

### 🐛 Bug Fixes

- **(aqua)** add executable permissions for zip-extracted binaries by [@itochan](https://github.com/itochan) in [#5998](https://github.com/jdx/mise/pull/5998)
- **(core)** auto-repair corrupted pyenv cache by recloning on update failure by [@jdx](https://github.com/jdx) in [#6003](https://github.com/jdx/mise/pull/6003)
- **(uv_venv)** fixes PATH ordering with `mise x` by [@jdx](https://github.com/jdx) in [#6005](https://github.com/jdx/mise/pull/6005)
- duplicate versions and validation in `mise tool` by [@jdx](https://github.com/jdx) in [#6001](https://github.com/jdx/mise/pull/6001)

### 📚 Documentation

- **(tools)** document per-tool postinstall option in [tools] by [@jdx](https://github.com/jdx) in [#5993](https://github.com/jdx/mise/pull/5993)
- Update install instructions for nushell by [@Joniator](https://github.com/Joniator) in [#5981](https://github.com/jdx/mise/pull/5981)
- README.md typo by [@jdx](https://github.com/jdx) in [#5990](https://github.com/jdx/mise/pull/5990)

### ◀️ Revert

- Revert "docs: Update install instructions for nushell" by [@jdx](https://github.com/jdx) in [#5983](https://github.com/jdx/mise/pull/5983)
- Revert "fix(aqua): add executable permissions for zip-extracted binaries" by [@jdx](https://github.com/jdx) in [#6004](https://github.com/jdx/mise/pull/6004)

### 📦️ Dependency Updates

- update taiki-e/install-action digest to 2c73a74 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5962](https://github.com/jdx/mise/pull/5962)
- update docker/metadata-action digest to c1e5197 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5961](https://github.com/jdx/mise/pull/5961)
- update docker/login-action digest to 184bdaa by [@renovate[bot]](https://github.com/renovate[bot]) in [#5958](https://github.com/jdx/mise/pull/5958)

### Chore

- cargo up by [@jdx](https://github.com/jdx) in [#5992](https://github.com/jdx/mise/pull/5992)

### New Contributors

- @Joniator made their first contribution in [#5981](https://github.com/jdx/mise/pull/5981)
- @jylenhof made their first contribution in [#5966](https://github.com/jdx/mise/pull/5966)

## [2025.8.8](https://github.com/jdx/mise/compare/v2025.8.7..v2025.8.8) - 2025-08-11

### 📦 Registry

- add bob ([aqua:MordechaiHadad/bob](https://github.com/MordechaiHadad/bob)) by [@TyceHerrman](https://github.com/TyceHerrman) in [#5914](https://github.com/jdx/mise/pull/5914)
- support usage on FreeBSD by [@risu729](https://github.com/risu729) in [#5973](https://github.com/jdx/mise/pull/5973)
- filter out installer for podman by [@risu729](https://github.com/risu729) in [#5974](https://github.com/jdx/mise/pull/5974)
- use pipx aqua backend by [@itochan](https://github.com/itochan) in [#5971](https://github.com/jdx/mise/pull/5971)
- only use aqua backend for yarn on windows by [@jdx](https://github.com/jdx) in [#5978](https://github.com/jdx/mise/pull/5978)

### 📚 Documentation

- add documentation for os field in tool configuration by [@jdx](https://github.com/jdx) in [#5947](https://github.com/jdx/mise/pull/5947)

### Chore

- **(ci)** accept @ in regular expressions for new registry PR titles by [@mst-mkt](https://github.com/mst-mkt) in [#5969](https://github.com/jdx/mise/pull/5969)
- fix registry test filter by [@risu729](https://github.com/risu729) in [#5942](https://github.com/jdx/mise/pull/5942)
- fix registry test by [@risu729](https://github.com/risu729) in [#5953](https://github.com/jdx/mise/pull/5953)

### New Contributors

- @itochan made their first contribution in [#5971](https://github.com/jdx/mise/pull/5971)
- @mst-mkt made their first contribution in [#5969](https://github.com/jdx/mise/pull/5969)

## [2025.8.7](https://github.com/jdx/mise/compare/v2025.8.6..v2025.8.7) - 2025-08-06

### 📦 Registry

- add python to gcloud dependencies by [@risu729](https://github.com/risu729) in [#5936](https://github.com/jdx/mise/pull/5936)

### 🐛 Bug Fixes

- **(lockfile)** fix multiple lockfile issues with version management by [@jdx](https://github.com/jdx) in [#5907](https://github.com/jdx/mise/pull/5907)
- **(toolset)** properly handle MISE_ADD_PATH from plugins by [@jdx](https://github.com/jdx) in [#5937](https://github.com/jdx/mise/pull/5937)

## [2025.8.6](https://github.com/jdx/mise/compare/v2025.8.5..v2025.8.6) - 2025-08-06

### 📦 Registry

- add resvg ([aqua:linebender/resvg](https://github.com/linebender/resvg)) by [@TyceHerrman](https://github.com/TyceHerrman) in [#5926](https://github.com/jdx/mise/pull/5926)
- add specstory ([aqua:specstoryai/getspecstory](https://github.com/specstoryai/getspecstory)) by [@TyceHerrman](https://github.com/TyceHerrman) in [#5927](https://github.com/jdx/mise/pull/5927)
- add oxker ([aqua:mrjackwills/oxker](https://github.com/mrjackwills/oxker)) by [@TyceHerrman](https://github.com/TyceHerrman) in [#5929](https://github.com/jdx/mise/pull/5929)
- add tssh ([aqua:trzsz/trzsz-ssh](https://github.com/trzsz/trzsz-ssh)) by [@TyceHerrman](https://github.com/TyceHerrman) in [#5928](https://github.com/jdx/mise/pull/5928)

### 🚀 Features

- **(tool-stub)** improve stub generation with bin inference, error handling, and fetch mode by [@jdx](https://github.com/jdx) in [#5932](https://github.com/jdx/mise/pull/5932)

## [2025.8.5](https://github.com/jdx/mise/compare/v2025.8.4..v2025.8.5) - 2025-08-05

### 📦 Registry

- add tlrc ([aqua:tldr-pages/tlrc](https://github.com/tldr-pages/tlrc)) by [@TyceHerrman](https://github.com/TyceHerrman) in [#5895](https://github.com/jdx/mise/pull/5895)
- support `.terragrunt-version` by [@risu729](https://github.com/risu729) in [#5903](https://github.com/jdx/mise/pull/5903)
- add lnav ([aqua:tstack/lnav](https://github.com/tstack/lnav)) by [@TyceHerrman](https://github.com/TyceHerrman) in [#5896](https://github.com/jdx/mise/pull/5896)
- use aqua backend for yarn by [@risu729](https://github.com/risu729) in [#5902](https://github.com/jdx/mise/pull/5902)
- add dotenvx ([aqua:dotenvx/dotenvx](https://github.com/dotenvx/dotenvx)) by [@TyceHerrman](https://github.com/TyceHerrman) in [#5915](https://github.com/jdx/mise/pull/5915)
- update kubecolor ([aqua:kubecolor/kubecolor](https://github.com/kubecolor/kubecolor)) by [@Darwiner](https://github.com/Darwiner) in [#5887](https://github.com/jdx/mise/pull/5887)
- add oxlint ([aqua:oxc-project/oxc/oxlint](https://github.com/oxc-project/oxc/oxlint)) by [@TyceHerrman](https://github.com/TyceHerrman) in [#5919](https://github.com/jdx/mise/pull/5919)
- add container ([aqua:apple/container](https://github.com/apple/container)) by [@TyceHerrman](https://github.com/TyceHerrman) in [#5917](https://github.com/jdx/mise/pull/5917)
- support `.packer-version` by [@risu729](https://github.com/risu729) in [#5900](https://github.com/jdx/mise/pull/5900)

### 🚀 Features

- **(ci)** enhance registry PR validation with strict format checking by [@jdx](https://github.com/jdx) in [#5897](https://github.com/jdx/mise/pull/5897)
- add Model Context Protocol (MCP) server command by [@jdx](https://github.com/jdx) in [#5920](https://github.com/jdx/mise/pull/5920)

### 🐛 Bug Fixes

- **(elixir)** support `.exenv-version` by [@risu729](https://github.com/risu729) in [#5901](https://github.com/jdx/mise/pull/5901)
- **(env)** improve PATH handling for env._.path directives by [@jdx](https://github.com/jdx) in [#5922](https://github.com/jdx/mise/pull/5922)
- allow devcontainer creation without a git repository by [@acesyde](https://github.com/acesyde) in [#5891](https://github.com/jdx/mise/pull/5891)

### Chore

- add synchronize to registry_comment gha by [@jdx](https://github.com/jdx) in [cbb1429](https://github.com/jdx/mise/commit/cbb14294072e9cbd3b0b9f21b2cb0a993a71d5ff)
- fix registry_comment gha by [@jdx](https://github.com/jdx) in [7ce513b](https://github.com/jdx/mise/commit/7ce513be3efe60372f667f76570e16ce0d4a013f)
- run registry test only for changed tools by [@risu729](https://github.com/risu729) in [#5905](https://github.com/jdx/mise/pull/5905)

### New Contributors

- @Darwiner made their first contribution in [#5887](https://github.com/jdx/mise/pull/5887)
- @zekefast made their first contribution in [#5912](https://github.com/jdx/mise/pull/5912)

## [2025.8.4](https://github.com/jdx/mise/compare/v2025.8.3..v2025.8.4) - 2025-08-03

### 🚀 Features

- **(tasks)** **breaking** Add environment variable directives for mise tasks by [@jdx](https://github.com/jdx) in [#5638](https://github.com/jdx/mise/pull/5638)

## [2025.8.3](https://github.com/jdx/mise/compare/v2025.8.2..v2025.8.3) - 2025-08-03

### 📦 Registry

- add git-lfs ([aqua:git-lfs/git-lfs](https://github.com/git-lfs/git-lfs)) by [@TyceHerrman](https://github.com/TyceHerrman) in [#5885](https://github.com/jdx/mise/pull/5885)

### 🚀 Features

- **(registry)** add atuin package to registry by [@TyceHerrman](https://github.com/TyceHerrman) in [#5883](https://github.com/jdx/mise/pull/5883)
- introduce registry commit type for new tool additions by [@jdx](https://github.com/jdx) in [#5884](https://github.com/jdx/mise/pull/5884)

### 🐛 Bug Fixes

- **(aqua,github)** make asset name matching case-insensitive by [@jdx](https://github.com/jdx) in [#5886](https://github.com/jdx/mise/pull/5886)

### 🚜 Refactor

- **(ci)** separate Alpine release into its own workflow by [@jdx](https://github.com/jdx) in [#5868](https://github.com/jdx/mise/pull/5868)

### 📚 Documentation

- **(changelog)** automate backend links in changelog by [@jdx](https://github.com/jdx) in [#5889](https://github.com/jdx/mise/pull/5889)

### ⚡ Performance

- reduce render env task calls by [@jdx](https://github.com/jdx) in [#5888](https://github.com/jdx/mise/pull/5888)

## [2025.8.2](https://github.com/jdx/mise/compare/v2025.8.1..v2025.8.2) - 2025-08-02

### 🚀 Features

- **(registry)** add jjui by [@TyceHerrman](https://github.com/TyceHerrman) in [#5877](https://github.com/jdx/mise/pull/5877)
- **(registry)** add trunk metalinter by [@daveio](https://github.com/daveio) in [#5875](https://github.com/jdx/mise/pull/5875)

### 🐛 Bug Fixes

- **(python)** Windows OS no longer suffixed with `-shared` by [@malept](https://github.com/malept) in [#5879](https://github.com/jdx/mise/pull/5879)

### New Contributors

- @daveio made their first contribution in [#5875](https://github.com/jdx/mise/pull/5875)
- @TyceHerrman made their first contribution in [#5877](https://github.com/jdx/mise/pull/5877)

## [2025.8.1](https://github.com/jdx/mise/compare/v2025.8.0..v2025.8.1) - 2025-08-01

### 🐛 Bug Fixes

- node gpg keys by [@jdx](https://github.com/jdx) in [#5866](https://github.com/jdx/mise/pull/5866)

## [2025.8.0](https://github.com/jdx/mise/compare/v2025.7.32..v2025.8.0) - 2025-08-01

### 🚀 Features

- **(registry)** use npm backend for yarn by [@mrazauskas](https://github.com/mrazauskas) in [#5745](https://github.com/jdx/mise/pull/5745)
- **(registry)** add codebuff tool by [@zacheryph](https://github.com/zacheryph) in [#5856](https://github.com/jdx/mise/pull/5856)

### 🐛 Bug Fixes

- **(go)** implement heuristic-based go module find logic by [@risu729](https://github.com/risu729) in [#5851](https://github.com/jdx/mise/pull/5851)
- **(node)** Add NodeJS maintainer Antoine du Hamel's new GPG key by [@chadlwilson](https://github.com/chadlwilson) in [#5862](https://github.com/jdx/mise/pull/5862)
- **(pipx)** align HTML backend with PEP 503 registry URL assignment by [@acesyde](https://github.com/acesyde) in [#5853](https://github.com/jdx/mise/pull/5853)
- **(registry)** fix balena ubi backend options by [@risu729](https://github.com/risu729) in [#5861](https://github.com/jdx/mise/pull/5861)
- **(registry)** add aqua backends to tools by [@risu729](https://github.com/risu729) in [#5863](https://github.com/jdx/mise/pull/5863)

### 📚 Documentation

- fix uv_venv_create_args reference for python by [@jasonraimondi](https://github.com/jasonraimondi) in [#5854](https://github.com/jdx/mise/pull/5854)
- expand on env directive examples and formats by [@syhol](https://github.com/syhol) in [#5857](https://github.com/jdx/mise/pull/5857)

### ◀️ Revert

- Revert "docs: fix uv_venv_create_args reference for python" by [@jdx](https://github.com/jdx) in [#5859](https://github.com/jdx/mise/pull/5859)

### New Contributors

- @zacheryph made their first contribution in [#5856](https://github.com/jdx/mise/pull/5856)
- @chadlwilson made their first contribution in [#5862](https://github.com/jdx/mise/pull/5862)
- @jasonraimondi made their first contribution in [#5854](https://github.com/jdx/mise/pull/5854)

## [2025.7.32](https://github.com/jdx/mise/compare/v2025.7.31..v2025.7.32) - 2025-07-31

### 🚀 Features

- **(tool-stubs)** Add human readable comments to stub sizes by [@jdx](https://github.com/jdx) in [#5845](https://github.com/jdx/mise/pull/5845)
- **(tool-stubs)** improve binary path detection in tool stub generator by [@jdx](https://github.com/jdx) in [#5847](https://github.com/jdx/mise/pull/5847)

### 🐛 Bug Fixes

- **(aqua)** support `AND` operator in semver by [@risu729](https://github.com/risu729) in [#5838](https://github.com/jdx/mise/pull/5838)
- **(cli)** remove empty [platforms] section from generated tool stubs by [@jdx](https://github.com/jdx) in [#5844](https://github.com/jdx/mise/pull/5844)
- **(tool-stubs)** remove comment line from tool stub generator by [@jdx](https://github.com/jdx) in [#5843](https://github.com/jdx/mise/pull/5843)
- **(tool-stubs)** Remove latest version from tool stubs by [@jdx](https://github.com/jdx) in [#5846](https://github.com/jdx/mise/pull/5846)
- **(tool-stubs)** allow -v flag to be passed through to tool stubs by [@jdx](https://github.com/jdx) in [#5848](https://github.com/jdx/mise/pull/5848)

## [2025.7.31](https://github.com/jdx/mise/compare/v2025.7.30..v2025.7.31) - 2025-07-29

### 🚀 Features

- **(tool-stubs)** append to existing tool-stub files instead of overwriting by [@jdx](https://github.com/jdx) in [#5835](https://github.com/jdx/mise/pull/5835)
- **(tool-stubs)** add auto-platform detection from URLs by [@jdx](https://github.com/jdx) in [#5836](https://github.com/jdx/mise/pull/5836)
- Add sops.strict setting for non-strict decryption mode by [@pepicrft](https://github.com/pepicrft) in [#5378](https://github.com/jdx/mise/pull/5378)

### 🐛 Bug Fixes

- **(tool-stub)** use URL hash as version for HTTP backend with "latest" by [@jdx](https://github.com/jdx) in [#5828](https://github.com/jdx/mise/pull/5828)
- **(tool-stubs)** fix -v and --help flags by [@jdx](https://github.com/jdx) in [#5829](https://github.com/jdx/mise/pull/5829)
- **(tool-stubs)** use 'checksum' field instead of 'blake3' in generated stubs by [@jdx](https://github.com/jdx) in [#5834](https://github.com/jdx/mise/pull/5834)
- dotnet SearchQueryService fallback by [@acesyde](https://github.com/acesyde) in [#5824](https://github.com/jdx/mise/pull/5824)
- registry.toml - Specify sbt dependency on java by [@jatcwang](https://github.com/jatcwang) in [#5827](https://github.com/jdx/mise/pull/5827)

### 🧪 Testing

- remove has test which is failing by [@jdx](https://github.com/jdx) in [4aa9cc9](https://github.com/jdx/mise/commit/4aa9cc973acb1bc34df51f27333a226df3256b69)

### New Contributors

- @jatcwang made their first contribution in [#5827](https://github.com/jdx/mise/pull/5827)

## [2025.7.30](https://github.com/jdx/mise/compare/v2025.7.29..v2025.7.30) - 2025-07-29

### 🚀 Features

- **(registry)** add amp by [@jahands](https://github.com/jahands) in [#5814](https://github.com/jdx/mise/pull/5814)

### 🐛 Bug Fixes

- **(tool-stubs)** fix error messages when it can't find the bin by [@jdx](https://github.com/jdx) in [#5817](https://github.com/jdx/mise/pull/5817)
- misidentifying built-in backend as a plugin backend by [@syhol](https://github.com/syhol) in [#5822](https://github.com/jdx/mise/pull/5822)

### 📚 Documentation

- **(troubleshooting)** path limits on Windows by [@W1M0R](https://github.com/W1M0R) in [#5815](https://github.com/jdx/mise/pull/5815)

## [2025.7.29](https://github.com/jdx/mise/compare/v2025.7.28..v2025.7.29) - 2025-07-28

### 🐛 Bug Fixes

- **(cli)** stable path env for exec on windows by [@W1M0R](https://github.com/W1M0R) in [#5790](https://github.com/jdx/mise/pull/5790)
- **(tool-stubs)** platform-specific bin fields by [@jdx](https://github.com/jdx) in [#5812](https://github.com/jdx/mise/pull/5812)
- tool-stub generation with archive downloads by [@jdx](https://github.com/jdx) in [#5811](https://github.com/jdx/mise/pull/5811)

### 📦️ Dependency Updates

- update jdx/mise-action digest to c37c932 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5784](https://github.com/jdx/mise/pull/5784)

### New Contributors

- @W1M0R made their first contribution in [#5790](https://github.com/jdx/mise/pull/5790)

## [2025.7.28](https://github.com/jdx/mise/compare/v2025.7.27..v2025.7.28) - 2025-07-27

### 🚀 Features

- **(http)** show retry after for github rate limit by [@risu729](https://github.com/risu729) in [#5803](https://github.com/jdx/mise/pull/5803)
- **(registry)** add carapace by [@jahands](https://github.com/jahands) in [#5804](https://github.com/jdx/mise/pull/5804)
- **(registry)** add `hatch` by [@hasansezertasan](https://github.com/hasansezertasan) in [#5788](https://github.com/jdx/mise/pull/5788)
- tool-stubs by [@jdx](https://github.com/jdx) in [#5795](https://github.com/jdx/mise/pull/5795)
- used shared cache for http backend by [@jdx](https://github.com/jdx) in [#5808](https://github.com/jdx/mise/pull/5808)

### 🐛 Bug Fixes

- **(aqua)** avoid unnecessary head requests in version resolution by [@risu729](https://github.com/risu729) in [#5800](https://github.com/jdx/mise/pull/5800)
- **(toolset)** use join_paths for MISE_ADD_PATH by [@risu729](https://github.com/risu729) in [#5785](https://github.com/jdx/mise/pull/5785)
- check lib64 directories for .disable-self-update file by [@jdx](https://github.com/jdx) in [#5809](https://github.com/jdx/mise/pull/5809)

### 🚜 Refactor

- **(aqua)** move alternative backend suggestions into validate by [@risu729](https://github.com/risu729) in [#5794](https://github.com/jdx/mise/pull/5794)

### 📚 Documentation

- **(tool-stubs)** added shebangs by [@jdx](https://github.com/jdx) in [2d37500](https://github.com/jdx/mise/commit/2d37500e309a61062fc0e821a38be98626176d5d)
- **(tool-stubs)** corrected url syntax by [@jdx](https://github.com/jdx) in [32627be](https://github.com/jdx/mise/commit/32627bec8b3df5060ea9f93dc50003126585e572)
- fix plugin-lua-modules docs to match the vfox lua_mod functions by [@syhol](https://github.com/syhol) in [#5792](https://github.com/jdx/mise/pull/5792)
- fix http backend tool options example by [@roele](https://github.com/roele) in [#5802](https://github.com/jdx/mise/pull/5802)

### 📦️ Dependency Updates

- update taiki-e/install-action digest to 7fbb30f by [@renovate[bot]](https://github.com/renovate[bot]) in [#5786](https://github.com/jdx/mise/pull/5786)
- pin actions/checkout action to 11bd719 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5783](https://github.com/jdx/mise/pull/5783)

### New Contributors

- @hasansezertasan made their first contribution in [#5788](https://github.com/jdx/mise/pull/5788)

## [2025.7.27](https://github.com/jdx/mise/compare/v2025.7.26..v2025.7.27) - 2025-07-24

### 🐛 Bug Fixes

- **(copr)** disable self-update by [@jdx](https://github.com/jdx) in [#5780](https://github.com/jdx/mise/pull/5780)
- **(link.md)** correct example comment in mise link documentation by [@mmurdockk](https://github.com/mmurdockk) in [#5760](https://github.com/jdx/mise/pull/5760)
- use github releases in install.sh for non-current version by [@jdx](https://github.com/jdx) in [c2b1ef1](https://github.com/jdx/mise/commit/c2b1ef1c53d736e14fb64365aa1339dc955d6c59)

### New Contributors

- @mmurdockk made their first contribution in [#5760](https://github.com/jdx/mise/pull/5760)

## [2025.7.26](https://github.com/jdx/mise/compare/v2025.7.25..v2025.7.26) - 2025-07-24

### Chore

- use correct release dirname by [@jdx](https://github.com/jdx) in [c8e0b5b](https://github.com/jdx/mise/commit/c8e0b5b42f3d258ec977b68326461d2fc81c4724)

## [2025.7.25](https://github.com/jdx/mise/compare/v2025.7.24..v2025.7.25) - 2025-07-24

### Chore

- updated deps by [@jdx](https://github.com/jdx) in [#5771](https://github.com/jdx/mise/pull/5771)

## [2025.7.24](https://github.com/jdx/mise/compare/v2025.7.23..v2025.7.24) - 2025-07-24

### Chore

- add MISE_INSTALL_FROM_GITHUB option for mise.run by [@jdx](https://github.com/jdx) in [#5772](https://github.com/jdx/mise/pull/5772)

## [2025.7.22](https://github.com/jdx/mise/compare/v2025.7.21..v2025.7.22) - 2025-07-24

### 🚀 Features

- **(doctor)** display # of baked-in aqua registry tools by [@jdx](https://github.com/jdx) in [#5756](https://github.com/jdx/mise/pull/5756)
- **(lock)** `mise lock` enhancements by [@jdx](https://github.com/jdx) in [#5765](https://github.com/jdx/mise/pull/5765)
- registry.toml: add SST by [@juxuanu](https://github.com/juxuanu) in [#5758](https://github.com/jdx/mise/pull/5758)

### 🐛 Bug Fixes

- **(copr)** fix remaining issues by [@jdx](https://github.com/jdx) in [#5755](https://github.com/jdx/mise/pull/5755)

### 📚 Documentation

- add descriptions for all the tasks by [@jdx](https://github.com/jdx) in [#5764](https://github.com/jdx/mise/pull/5764)

### 📦️ Dependency Updates

- update fedora docker tag to v43 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5159](https://github.com/jdx/mise/pull/5159)

### Chore

- **(copr)** chmod +x by [@jdx](https://github.com/jdx) in [71cf6ee](https://github.com/jdx/mise/commit/71cf6eee0d1766bbc214c6cf307b3d7ae300cd33)
- **(hyperfine)** temporarily remove uncached benchmarks since they are not reporting right by [@jdx](https://github.com/jdx) in [#5769](https://github.com/jdx/mise/pull/5769)
- added `mise` shim for devcontainer by [@jdx](https://github.com/jdx) in [#5768](https://github.com/jdx/mise/pull/5768)

### Task-configuration.md

- typo by [@mustafa0x](https://github.com/mustafa0x) in [#5216](https://github.com/jdx/mise/pull/5216)

### New Contributors

- @mustafa0x made their first contribution in [#5216](https://github.com/jdx/mise/pull/5216)
- @juxuanu made their first contribution in [#5758](https://github.com/jdx/mise/pull/5758)

## [2025.7.21](https://github.com/jdx/mise/compare/v2025.7.20..v2025.7.21) - 2025-07-23

### 🚀 Features

- **(packaging)** add COPR publishing workflow and documentation by [@jdx](https://github.com/jdx) in [#5719](https://github.com/jdx/mise/pull/5719)

### 🐛 Bug Fixes

- **(pwsh)** resolve issue caused by previous #5732 patch (hardcoded path) by [@IMXEren](https://github.com/IMXEren) in [#5753](https://github.com/jdx/mise/pull/5753)
- copr docker building by [@jdx](https://github.com/jdx) in [#5748](https://github.com/jdx/mise/pull/5748)

### 📚 Documentation

- **(README)** mention project alexandria by [@jdx](https://github.com/jdx) in [681bc75](https://github.com/jdx/mise/commit/681bc751025a848411b7dff322cd14d9487dd59f)
- Removes invalid array in redaction example by [@EverlastingBugstopper](https://github.com/EverlastingBugstopper) in [#5752](https://github.com/jdx/mise/pull/5752)
- document mise-versions app by [@jdx](https://github.com/jdx) in [785ef24](https://github.com/jdx/mise/commit/785ef24e65259b95f56ecccebe9463a8a0c37519)

### 🧪 Testing

- fix asset detector test on musl by [@jdx](https://github.com/jdx) in [#5744](https://github.com/jdx/mise/pull/5744)

### Chore

- use 302 redirects for curl installs by [@jdx](https://github.com/jdx) in [#5747](https://github.com/jdx/mise/pull/5747)

### New Contributors

- @EverlastingBugstopper made their first contribution in [#5752](https://github.com/jdx/mise/pull/5752)

## [2025.7.20](https://github.com/jdx/mise/compare/v2025.7.19..v2025.7.20) - 2025-07-22

### 🚀 Features

- use mise.run for rosetta tip by [@jdx](https://github.com/jdx) in [#5739](https://github.com/jdx/mise/pull/5739)

### 🐛 Bug Fixes

- **(npm)** use bin/ as bin_paths when installed with bun on windows by [@risu729](https://github.com/risu729) in [#5725](https://github.com/jdx/mise/pull/5725)

### 📚 Documentation

- remove curl instructions by [@jdx](https://github.com/jdx) in [785d2f2](https://github.com/jdx/mise/commit/785d2f2fe4795b23cb196a70a0b7956707d40437)
- add back in supported os/arch combinations by [@jdx](https://github.com/jdx) in [87b86b0](https://github.com/jdx/mise/commit/87b86b0f4f756dd6b7116192214c25e2995e9939)

### Chore

- set redirect for curl installs by [@jdx](https://github.com/jdx) in [#5740](https://github.com/jdx/mise/pull/5740)
- reduce binary size for linux by [@jdx](https://github.com/jdx) in [#5741](https://github.com/jdx/mise/pull/5741)

## [2025.7.19](https://github.com/jdx/mise/compare/v2025.7.18..v2025.7.19) - 2025-07-22

### 🐛 Bug Fixes

- **(pwsh)** set console encoding to UTF-8 to prevent Unicode garbling by [@IMXEren](https://github.com/IMXEren) in [#5732](https://github.com/jdx/mise/pull/5732)
- **(registry)** set matching_regex for glab on Windows to pick the correct asset by [@risu729](https://github.com/risu729) in [#5727](https://github.com/jdx/mise/pull/5727)

### 📚 Documentation

- **(config)** fix alias section name by [@malept](https://github.com/malept) in [#5736](https://github.com/jdx/mise/pull/5736)
- fix typo in contributing commit message prefixes by [@malept](https://github.com/malept) in [#5737](https://github.com/jdx/mise/pull/5737)

### Chore

- **(ppa)** wait for gh rate limit by [@jdx](https://github.com/jdx) in [#5721](https://github.com/jdx/mise/pull/5721)
- **(vfox-test)** set GITHUB_TOKEN by [@jdx](https://github.com/jdx) in [cdbb62b](https://github.com/jdx/mise/commit/cdbb62b0f63bcb0a3b650c1d49aefb8c9798c6aa)

### New Contributors

- @malept made their first contribution in [#5736](https://github.com/jdx/mise/pull/5736)

## [2025.7.18](https://github.com/jdx/mise/compare/v2025.7.17..v2025.7.18) - 2025-07-21

### 🚀 Features

- **(registry)** add `jsonschema` CLI tool by [@mrazauskas](https://github.com/mrazauskas) in [#5714](https://github.com/jdx/mise/pull/5714)

### 🐛 Bug Fixes

- mise up parallel execution by [@jdx](https://github.com/jdx) in [#5591](https://github.com/jdx/mise/pull/5591)
- ppa releases by [@jdx](https://github.com/jdx) in [#5717](https://github.com/jdx/mise/pull/5717)

### 📚 Documentation

- add comprehensive CLAUDE.md for Claude Code guidance by [@jdx](https://github.com/jdx) in [#5718](https://github.com/jdx/mise/pull/5718)

### Chore

- ubuntu ppa by [@jdx](https://github.com/jdx) in [#5715](https://github.com/jdx/mise/pull/5715)

## [2025.7.17](https://github.com/jdx/mise/compare/v2025.7.16..v2025.7.17) - 2025-07-19

### 🚀 Features

- consolidate lockfile assets and add URL tracking by [@jdx](https://github.com/jdx) in [#5629](https://github.com/jdx/mise/pull/5629)

### 🐛 Bug Fixes

- **(registry)** use aqua backend for available tools by [@risu729](https://github.com/risu729) in [#5707](https://github.com/jdx/mise/pull/5707)

### 📚 Documentation

- document auto_install behavior by [@jdx](https://github.com/jdx) in [#5697](https://github.com/jdx/mise/pull/5697)

### 🧪 Testing

- **(registry)** enable disabled tests by [@risu729](https://github.com/risu729) in [#5708](https://github.com/jdx/mise/pull/5708)
- **(registry)** comment out failing maven test in configuration by [@jdx](https://github.com/jdx) in [ae3e62b](https://github.com/jdx/mise/commit/ae3e62b232ab974058cf7b7c7a05d05086f48e48)

## [2025.7.16](https://github.com/jdx/mise/compare/v2025.7.15..v2025.7.16) - 2025-07-18

### 🐛 Bug Fixes

- mise.run cloudflare worker publish by [@jdx](https://github.com/jdx) in [#5704](https://github.com/jdx/mise/pull/5704)

### Chore

- **(release)** increase timeout for macos tarballs by [@jdx](https://github.com/jdx) in [05e3a45](https://github.com/jdx/mise/commit/05e3a459982745f365d958501492430effab1fc0)
- disable tests for 2025.7.16 by [@jdx](https://github.com/jdx) in [30d3b97](https://github.com/jdx/mise/commit/30d3b974dc3893158c10bfac500ac671407214b3)

## [2025.7.15](https://github.com/jdx/mise/compare/v2025.7.14..v2025.7.15) - 2025-07-18

### 🧪 Testing

- added .release-skip-e2e functionality by [@jdx](https://github.com/jdx) in [#5698](https://github.com/jdx/mise/pull/5698)

## [2025.7.14](https://github.com/jdx/mise/compare/v2025.7.13..v2025.7.14) - 2025-07-18

### 🐛 Bug Fixes

- mise.run cloudflare worker syntax by [@jdx](https://github.com/jdx) in [#5693](https://github.com/jdx/mise/pull/5693)

### 📦️ Dependency Updates

- update rust crate tabled to 0.20 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5688](https://github.com/jdx/mise/pull/5688)
- update rust crate indicatif to 0.18 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5687](https://github.com/jdx/mise/pull/5687)

## [2025.7.13](https://github.com/jdx/mise/compare/v2025.7.12..v2025.7.13) - 2025-07-18

### 🚀 Features

- https://mise.run/{bash,zsh,fish} by [@jdx](https://github.com/jdx) in [#5677](https://github.com/jdx/mise/pull/5677)
- add opencode tool with description, backends, and test command by [@nipuna-perera](https://github.com/nipuna-perera) in [#5679](https://github.com/jdx/mise/pull/5679)

### 🐛 Bug Fixes

- don't follow symlink to ignore symlinks from deletion by [@risu729](https://github.com/risu729) in [#5672](https://github.com/jdx/mise/pull/5672)
- update completions by [@risu729](https://github.com/risu729) in [#5682](https://github.com/jdx/mise/pull/5682)
- NoMethodError with Bundler::Installer by [@hsbt](https://github.com/hsbt) in [#5678](https://github.com/jdx/mise/pull/5678)

### 📚 Documentation

- fix typo in RUSTUP_TOOLCHAIN env variable name by [@anderso](https://github.com/anderso) in [#5673](https://github.com/jdx/mise/pull/5673)

### 📦️ Dependency Updates

- update jdx/mise-action digest to bfb9fa0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5681](https://github.com/jdx/mise/pull/5681)
- pin dependencies by [@renovate[bot]](https://github.com/renovate[bot]) in [#5680](https://github.com/jdx/mise/pull/5680)
- update rust crate console to 0.16 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5685](https://github.com/jdx/mise/pull/5685)
- update taiki-e/install-action digest to 4fd6bde by [@renovate[bot]](https://github.com/renovate[bot]) in [#5684](https://github.com/jdx/mise/pull/5684)

### New Contributors

- @nipuna-perera made their first contribution in [#5679](https://github.com/jdx/mise/pull/5679)
- @hsbt made their first contribution in [#5678](https://github.com/jdx/mise/pull/5678)
- @anderso made their first contribution in [#5673](https://github.com/jdx/mise/pull/5673)

## [2025.7.12](https://github.com/jdx/mise/compare/v2025.7.11..v2025.7.12) - 2025-07-17

### 🐛 Bug Fixes

- **(file)** remove top level directories in strip_archive_path_components by [@risu729](https://github.com/risu729) in [#5662](https://github.com/jdx/mise/pull/5662)
- **(npm)** run bun in install_path instead of using --cwd flag of bun by [@risu729](https://github.com/risu729) in [#5656](https://github.com/jdx/mise/pull/5656)
- **(nushell)** fix `get -i` deprecation by [@JoaquinTrinanes](https://github.com/JoaquinTrinanes) in [#5666](https://github.com/jdx/mise/pull/5666)

### ◀️ Revert

- Revert "fix(aqua): improve warnings for packages without repo_owner and repo_name " by [@jdx](https://github.com/jdx) in [#5668](https://github.com/jdx/mise/pull/5668)

### Chore

- update deps by [@risu729](https://github.com/risu729) in [#5657](https://github.com/jdx/mise/pull/5657)
- update usage by [@risu729](https://github.com/risu729) in [#5661](https://github.com/jdx/mise/pull/5661)

### New Contributors

- @JoaquinTrinanes made their first contribution in [#5666](https://github.com/jdx/mise/pull/5666)

## [2025.7.11](https://github.com/jdx/mise/compare/v2025.7.10..v2025.7.11) - 2025-07-16

### 🚀 Features

- support extracting 7z archives for static backends by [@yjoer](https://github.com/yjoer) in [#5632](https://github.com/jdx/mise/pull/5632)

### 🐛 Bug Fixes

- **(aqua)** improve warnings for packages without repo_owner and repo_name by [@risu729](https://github.com/risu729) in [#5644](https://github.com/jdx/mise/pull/5644)
- **(generate)** fix task docs inject by [@risu729](https://github.com/risu729) in [#5651](https://github.com/jdx/mise/pull/5651)
- **(static)** support `strip_components` for zip files by [@risu729](https://github.com/risu729) in [#5631](https://github.com/jdx/mise/pull/5631)
- private forges by [@hamnis](https://github.com/hamnis) in [#5650](https://github.com/jdx/mise/pull/5650)

### 🚜 Refactor

- **(aqua)** move no_aset and error_message checks into validate by [@risu729](https://github.com/risu729) in [#5649](https://github.com/jdx/mise/pull/5649)

### 📚 Documentation

- **(vfox)** replace deprecated asdf and vfox settings with disable_backends by [@risu729](https://github.com/risu729) in [#5652](https://github.com/jdx/mise/pull/5652)
- tweak static backend docs by [@jdx](https://github.com/jdx) in [#5627](https://github.com/jdx/mise/pull/5627)

### 🧪 Testing

- **(e2e)** move test_github_auto_detect to correct directory by [@risu729](https://github.com/risu729) in [#5640](https://github.com/jdx/mise/pull/5640)

### New Contributors

- @hamnis made their first contribution in [#5650](https://github.com/jdx/mise/pull/5650)

## [2025.7.10](https://github.com/jdx/mise/compare/v2025.7.9..v2025.7.10) - 2025-07-14

### 🐛 Bug Fixes

- **(backend)** avoid double untar by [@jdx](https://github.com/jdx) in [#5626](https://github.com/jdx/mise/pull/5626)
- **(github)** handle missing "v" prefix by [@jdx](https://github.com/jdx) in [#5625](https://github.com/jdx/mise/pull/5625)

### 📚 Documentation

- add asset autodetection documentation to GitHub/GitLab backends by [@jdx](https://github.com/jdx) in [#5623](https://github.com/jdx/mise/pull/5623)

## [2025.7.9](https://github.com/jdx/mise/compare/v2025.7.8..v2025.7.9) - 2025-07-14

### 🚀 Features

- **(shim)** prevent mise-specific flags from interfering with shim execution by [@jdx](https://github.com/jdx) in [#5616](https://github.com/jdx/mise/pull/5616)
- github asset auto-detection by [@jdx](https://github.com/jdx) in [#5622](https://github.com/jdx/mise/pull/5622)

### 🐛 Bug Fixes

- resolve GitHub alias tool name parsing and add platform-specific asset support by [@jdx](https://github.com/jdx) in [#5621](https://github.com/jdx/mise/pull/5621)

## [2025.7.8](https://github.com/jdx/mise/compare/v2025.7.7..v2025.7.8) - 2025-07-13

### 🚀 Features

- custom backends through plugins by [@jdx](https://github.com/jdx) in [#5579](https://github.com/jdx/mise/pull/5579)
- nested tool options by [@jdx](https://github.com/jdx) in [#5614](https://github.com/jdx/mise/pull/5614)

### 🐛 Bug Fixes

- accept platform_ or platforms_ in http/github backends by [@jdx](https://github.com/jdx) in [#5608](https://github.com/jdx/mise/pull/5608)

### 📚 Documentation

- correct toml syntax by [@jdx](https://github.com/jdx) in [#5609](https://github.com/jdx/mise/pull/5609)
- removed some markdownlint rules by [@jdx](https://github.com/jdx) in [#5615](https://github.com/jdx/mise/pull/5615)

## [2025.7.7](https://github.com/jdx/mise/compare/v2025.7.4..v2025.7.7) - 2025-07-13

### 🚀 Features

- add static backends (Github, GitLab, and HTTP) by [@jdx](https://github.com/jdx) in [#5602](https://github.com/jdx/mise/pull/5602)
- blake3 support by [@jdx](https://github.com/jdx) in [#5605](https://github.com/jdx/mise/pull/5605)

### 🐛 Bug Fixes

- **(e2e)** simplify test path handling logic by [@jdx](https://github.com/jdx) in [#5600](https://github.com/jdx/mise/pull/5600)
- skip gh release edit on dry run in release workflow by [@jdx](https://github.com/jdx) in [#5603](https://github.com/jdx/mise/pull/5603)

### 📚 Documentation

- **(cursor)** fix conventional commits rule formatting by [@jdx](https://github.com/jdx) in [#5597](https://github.com/jdx/mise/pull/5597)
- **(cursor)** add testing rule for mise codebase by [@jdx](https://github.com/jdx) in [#5598](https://github.com/jdx/mise/pull/5598)

### 🧪 Testing

- disable cmake test for now by [@jdx](https://github.com/jdx) in [d521c31](https://github.com/jdx/mise/commit/d521c31eff1675cd18333c5c258b5d41110fc81a)

### 📦️ Dependency Updates

- pin dependencies by [@renovate[bot]](https://github.com/renovate[bot]) in [#5511](https://github.com/jdx/mise/pull/5511)

### Chore

- **(release)** mark a release as draft until assets are added by [@risu729](https://github.com/risu729) in [#5584](https://github.com/jdx/mise/pull/5584)
- added reverts to git-cliff by [@jdx](https://github.com/jdx) in [#5577](https://github.com/jdx/mise/pull/5577)
- reduce binary size for linux by [@jdx](https://github.com/jdx) in [#5587](https://github.com/jdx/mise/pull/5587)
- `cargo check` fixes by [@jdx](https://github.com/jdx) in [#5589](https://github.com/jdx/mise/pull/5589)
- Merge vfox.rs into jdx/mise monorepo by [@jdx](https://github.com/jdx) in [#5590](https://github.com/jdx/mise/pull/5590)
- Add cursor rule for conventional commits by [@jdx](https://github.com/jdx) in [#5592](https://github.com/jdx/mise/pull/5592)
- Create GitHub action for vfox.rs tests by [@jdx](https://github.com/jdx) in [#5593](https://github.com/jdx/mise/pull/5593)
- tweak paths for test-vfox workflow by [@jdx](https://github.com/jdx) in [0189372](https://github.com/jdx/mise/commit/0189372aadad456cdac459317bb96ae3987cfd15)
- set workspace resolver by [@jdx](https://github.com/jdx) in [#5606](https://github.com/jdx/mise/pull/5606)
- add workspace resolver = 3 by [@jdx](https://github.com/jdx) in [304547a](https://github.com/jdx/mise/commit/304547a0b9a324b5d925c45e2841cadc3f6e938b)
- fix release-plz with workspace by [@jdx](https://github.com/jdx) in [5b3be6e](https://github.com/jdx/mise/commit/5b3be6eb8f06c509964a2b030eccb2f6e006f398)
- only bump mise version for release-plz by [@jdx](https://github.com/jdx) in [8f14d10](https://github.com/jdx/mise/commit/8f14d1014d217c91c36a96beaad4565a3aaf567e)
- add cargo-release by [@jdx](https://github.com/jdx) in [f657db5](https://github.com/jdx/mise/commit/f657db512fdb7ea4f58ac98af729ac6495e61100)
- mise up by [@jdx](https://github.com/jdx) in [4872ae6](https://github.com/jdx/mise/commit/4872ae6b4d63de54de4ac93e72e9a3cd51e20c2e)
- fix release-plz with workspace by [@jdx](https://github.com/jdx) in [bdb7119](https://github.com/jdx/mise/commit/bdb71196d6930091c68a6198d445fa16e108f75e)
- set-version by [@jdx](https://github.com/jdx) in [82fcd4f](https://github.com/jdx/mise/commit/82fcd4f22116bb92e1e615d9f1c03723d02aaaba)
- set-version by [@jdx](https://github.com/jdx) in [54388a4](https://github.com/jdx/mise/commit/54388a419427c664e557aa4ea034e13a2443bb8e)
- set-version by [@jdx](https://github.com/jdx) in [fe0a0a9](https://github.com/jdx/mise/commit/fe0a0a93b27219bd132b39f1f0b522bed1ad2b51)
- set-version by [@jdx](https://github.com/jdx) in [d9f24e2](https://github.com/jdx/mise/commit/d9f24e2b45fb7a9f5c2b795b490ba64a8d9eb207)
- set-version by [@jdx](https://github.com/jdx) in [97f6f4f](https://github.com/jdx/mise/commit/97f6f4febaf03f7c0d6d754701308edeb2287b53)
- set-version by [@jdx](https://github.com/jdx) in [13296e1](https://github.com/jdx/mise/commit/13296e10947ea5a96768e07bd95d009e95bace32)
- set-version by [@jdx](https://github.com/jdx) in [587a707](https://github.com/jdx/mise/commit/587a70744c4127f92cfe9381e7e273ac101c4a4f)
- set-version by [@jdx](https://github.com/jdx) in [1e80d52](https://github.com/jdx/mise/commit/1e80d52144144aaebc804aeef17010980f3a0caf)

## [2025.7.4](https://github.com/jdx/mise/compare/v2025.7.3..v2025.7.4) - 2025-07-11

### 🐛 Bug Fixes

- **(aqua)** align version resolution logic in list_bin_paths by [@risu729](https://github.com/risu729) in [#5562](https://github.com/jdx/mise/pull/5562)
- Xonsh integration by [@jfmontanaro](https://github.com/jfmontanaro) in [#5557](https://github.com/jdx/mise/pull/5557)

### 📚 Documentation

- create comprehensive architecture documentation suite and enhance development guides by [@jdx](https://github.com/jdx) in [d2b4a05](https://github.com/jdx/mise/commit/d2b4a050261b685279c502009f55a3e260b72ff9)

### ◀️ Revert

- Revert "fix(aqua): align version resolution logic in list_bin_paths" by [@jdx](https://github.com/jdx) in [#5574](https://github.com/jdx/mise/pull/5574)

### 📦️ Dependency Updates

- update rust crate bzip2 to 0.6 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5568](https://github.com/jdx/mise/pull/5568)
- update rust crate clap_mangen to v0.2.28 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5566](https://github.com/jdx/mise/pull/5566)
- update rust crate clap to v4.5.41 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5565](https://github.com/jdx/mise/pull/5565)
- update rust crate taplo to 0.14 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5158](https://github.com/jdx/mise/pull/5158)

### Chore

- added xonsh for release builds by [@jdx](https://github.com/jdx) in [#5561](https://github.com/jdx/mise/pull/5561)
- enable backtrace lines on panic by [@jdx](https://github.com/jdx) in [#5571](https://github.com/jdx/mise/pull/5571)
- shfmt update by [@jdx](https://github.com/jdx) in [67ee245](https://github.com/jdx/mise/commit/67ee24556f1533c508e422513399ae04ecf6bdaa)

### New Contributors

- @jfmontanaro made their first contribution in [#5557](https://github.com/jdx/mise/pull/5557)

## [2025.7.3](https://github.com/jdx/mise/compare/v2025.7.2..v2025.7.3) - 2025-07-10

### 🚀 Features

- **(registry)** add vfox by [@risu729](https://github.com/risu729) in [#5551](https://github.com/jdx/mise/pull/5551)

### 🐛 Bug Fixes

- **(aqua)** show other backends suggestion for unsupported package types by [@risu729](https://github.com/risu729) in [#5547](https://github.com/jdx/mise/pull/5547)
- **(registry)** use aqua and fix ubi options for yamlscript by [@risu729](https://github.com/risu729) in [#5538](https://github.com/jdx/mise/pull/5538)
- **(registry)** add java and yq to android-sdk dependencies by [@risu729](https://github.com/risu729) in [#5545](https://github.com/jdx/mise/pull/5545)
- **(schema)** broken $schema ref by [@tpansino](https://github.com/tpansino) in [#5540](https://github.com/jdx/mise/pull/5540)
- auto_install_disable_tools env var by [@jdx](https://github.com/jdx) in [#5543](https://github.com/jdx/mise/pull/5543)
- do not overwrite github tokens environment variables by [@risu729](https://github.com/risu729) in [#5546](https://github.com/jdx/mise/pull/5546)

### Chore

- update Cargo.lock by [@risu729](https://github.com/risu729) in [#5549](https://github.com/jdx/mise/pull/5549)

### New Contributors

- @tpansino made their first contribution in [#5540](https://github.com/jdx/mise/pull/5540)

## [2025.7.2](https://github.com/jdx/mise/compare/v2025.7.1..v2025.7.2) - 2025-07-09

### 🚀 Features

- **(registry)** add zizmor by [@risu729](https://github.com/risu729) in [#5519](https://github.com/jdx/mise/pull/5519)
- Add `self_update_available` to `mise doctor` output by [@joehorsnell](https://github.com/joehorsnell) in [#5534](https://github.com/jdx/mise/pull/5534)

### 🐛 Bug Fixes

- **(aqua)** use the version in url to verify and install by [@risu729](https://github.com/risu729) in [#5537](https://github.com/jdx/mise/pull/5537)
- **(registry)** use aqua for numbat, gokey, golines by [@risu729](https://github.com/risu729) in [#5518](https://github.com/jdx/mise/pull/5518)
- `self-update` on MITM firewall (attempt #2) by [@joehorsnell](https://github.com/joehorsnell) in [#5459](https://github.com/jdx/mise/pull/5459)
- mise panic in removed directory by [@roele](https://github.com/roele) in [#5532](https://github.com/jdx/mise/pull/5532)

### 📚 Documentation

- update ubi tag_regex syntax by [@grimm26](https://github.com/grimm26) in [#5529](https://github.com/jdx/mise/pull/5529)

### 🧪 Testing

- disable yamlscript test by [@jdx](https://github.com/jdx) in [#5536](https://github.com/jdx/mise/pull/5536)

### New Contributors

- @grimm26 made their first contribution in [#5529](https://github.com/jdx/mise/pull/5529)

## [2025.7.1](https://github.com/jdx/mise/compare/v2025.7.0..v2025.7.1) - 2025-07-06

### 🚀 Features

- **(aqua)** add support for zst compressed assets by [@andreabedini](https://github.com/andreabedini) in [#5495](https://github.com/jdx/mise/pull/5495)
- **(registry)** import package descriptions from aqua and add os specifier for tuist by [@matracey](https://github.com/matracey) in [#5487](https://github.com/jdx/mise/pull/5487)

### 🐛 Bug Fixes

- **(aqua)** handle hard links in aqua packages (attempt #2) by [@risu729](https://github.com/risu729) in [#5486](https://github.com/jdx/mise/pull/5486)
- **(aqua)** apply correct `version_override` by [@risu729](https://github.com/risu729) in [#5474](https://github.com/jdx/mise/pull/5474)
- **(erlang)** fix install_precompiled method signature for unsupported os by [@roele](https://github.com/roele) in [#5503](https://github.com/jdx/mise/pull/5503)
- **(java)** relax version filter regex for JetBrains builds by [@roele](https://github.com/roele) in [#5508](https://github.com/jdx/mise/pull/5508)
- **(registry)** use aqua backend for bat by [@risu729](https://github.com/risu729) in [#5490](https://github.com/jdx/mise/pull/5490)
- **(registry)** use pipx backend for aws-sam on windows by [@risu729](https://github.com/risu729) in [#5491](https://github.com/jdx/mise/pull/5491)
- enhance self-update for musl targets by [@roele](https://github.com/roele) in [#5502](https://github.com/jdx/mise/pull/5502)
- include arch and os settings in cache keys by [@risu729](https://github.com/risu729) in [#5504](https://github.com/jdx/mise/pull/5504)

### 🧪 Testing

- **(registry)** enable youtube-dl test by [@risu729](https://github.com/risu729) in [#5492](https://github.com/jdx/mise/pull/5492)

### 📦️ Dependency Updates

- update swatinem/rust-cache digest to 98c8021 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5512](https://github.com/jdx/mise/pull/5512)

### New Contributors

- @matracey made their first contribution in [#5487](https://github.com/jdx/mise/pull/5487)
- @andreabedini made their first contribution in [#5495](https://github.com/jdx/mise/pull/5495)

## [2025.7.0](https://github.com/jdx/mise/compare/v2025.6.8..v2025.7.0) - 2025-07-01

### 🚀 Features

- **(registry)** adds gemini-cli by [@risu729](https://github.com/risu729) in [#5447](https://github.com/jdx/mise/pull/5447)
- **(registry)** adds npm backended tools by [@risu729](https://github.com/risu729) in [#5446](https://github.com/jdx/mise/pull/5446)
- **(registry)** add powershell alias by [@risu729](https://github.com/risu729) in [#5449](https://github.com/jdx/mise/pull/5449)
- **(registry)** add dagu by [@yottahmd](https://github.com/yottahmd) in [#5476](https://github.com/jdx/mise/pull/5476)
- **(registry)** update aws-sam backends to include aqua source by [@yashikota](https://github.com/yashikota) in [#5461](https://github.com/jdx/mise/pull/5461)
- **(registry)** use ubi backend for youtube-dl nightly releases by [@risu729](https://github.com/risu729) in [#5466](https://github.com/jdx/mise/pull/5466)

### 🐛 Bug Fixes

- **(aqua)** update victoria-metrics package name casing by [@shikharbhardwaj](https://github.com/shikharbhardwaj) in [#5483](https://github.com/jdx/mise/pull/5483)
- **(aqua)** handle hard links in aqua packages by [@risu729](https://github.com/risu729) in [#5463](https://github.com/jdx/mise/pull/5463)
- **(bun)** enhance architecture detection for musl targets by [@roele](https://github.com/roele) in [#5450](https://github.com/jdx/mise/pull/5450)
- **(erlang)** use precompiled ubuntu binaries on GHA by [@paradox460](https://github.com/paradox460) in [#5439](https://github.com/jdx/mise/pull/5439)
- **(erlang)** add `install_precompiled` for unsupported os by [@risu729](https://github.com/risu729) in [#5479](https://github.com/jdx/mise/pull/5479)
- **(registry)** use aqua backend for cargo-make by [@risu729](https://github.com/risu729) in [#5465](https://github.com/jdx/mise/pull/5465)
- **(registry)** use aqua backends for all available tools by [@risu729](https://github.com/risu729) in [#5467](https://github.com/jdx/mise/pull/5467)
- `parse_command` passing `-c` flag to cmd.exe by [@IMXEren](https://github.com/IMXEren) in [#5441](https://github.com/jdx/mise/pull/5441)

### 🧪 Testing

- **(registry)** disable bitwarden test by [@risu729](https://github.com/risu729) in [#5468](https://github.com/jdx/mise/pull/5468)

### ◀️ Revert

- Revert "chore(deps): pin dependencies" by [@jdx](https://github.com/jdx) in [#5453](https://github.com/jdx/mise/pull/5453)
- Revert "fix(aqua): handle hard links in aqua packages" by [@jdx](https://github.com/jdx) in [#5485](https://github.com/jdx/mise/pull/5485)

### 📦️ Dependency Updates

- pin dependencies by [@renovate[bot]](https://github.com/renovate[bot]) in [#5443](https://github.com/jdx/mise/pull/5443)
- update jdx/mise-action digest to 5cb1df6 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5444](https://github.com/jdx/mise/pull/5444)

### Chore

- disable automatic cargo up due to windows build failure in homedir crate by [@jdx](https://github.com/jdx) in [7570d0a](https://github.com/jdx/mise/commit/7570d0a95498d7b5626645fe3065429e19d0b26e)

### Ci

- **(test)** run `apt-get update` before `apt-get install` by [@risu729](https://github.com/risu729) in [#5448](https://github.com/jdx/mise/pull/5448)

### New Contributors

- @yashikota made their first contribution in [#5461](https://github.com/jdx/mise/pull/5461)
- @yottahmd made their first contribution in [#5476](https://github.com/jdx/mise/pull/5476)
- @IMXEren made their first contribution in [#5441](https://github.com/jdx/mise/pull/5441)

## [2025.6.8](https://github.com/jdx/mise/compare/v2025.6.7..v2025.6.8) - 2025-06-26

### 🚀 Features

- **(java)** add support for tar.xz in Java core plugin to support RedHat JDKs by [@roele](https://github.com/roele) in [#5354](https://github.com/jdx/mise/pull/5354)
- **(registry)** add osv-scanner by [@scop](https://github.com/scop) in [#5413](https://github.com/jdx/mise/pull/5413)
- **(registry)** add scorecard by [@scop](https://github.com/scop) in [#5410](https://github.com/jdx/mise/pull/5410)
- **(registry)** add docker cli by [@acesyde](https://github.com/acesyde) in [#5344](https://github.com/jdx/mise/pull/5344)
- **(registry)** add claude code by [@lelouvincx](https://github.com/lelouvincx) in [#5420](https://github.com/jdx/mise/pull/5420)
- **(registry)** add aws `cfn-lint` by [@garysassano](https://github.com/garysassano) in [#5434](https://github.com/jdx/mise/pull/5434)
- added graphite by [@jdx](https://github.com/jdx) in [#5429](https://github.com/jdx/mise/pull/5429)

### 🐛 Bug Fixes

- **(erlang)** use precompiled binaries for linux ubuntu by [@paradox460](https://github.com/paradox460) in [#5402](https://github.com/jdx/mise/pull/5402)
- **(ubi)** checksum generation might fail if extract_all option is used by [@roele](https://github.com/roele) in [#5394](https://github.com/jdx/mise/pull/5394)
- `self-update` on MITM firewall by [@joehorsnell](https://github.com/joehorsnell) in [#5387](https://github.com/jdx/mise/pull/5387)
- lint warning by [@jdx](https://github.com/jdx) in [#5425](https://github.com/jdx/mise/pull/5425)
- only warn on toolset resolve errors by [@jdx](https://github.com/jdx) in [#5435](https://github.com/jdx/mise/pull/5435)

### 🚜 Refactor

- **(registry)** use pipx for semgrep by [@scop](https://github.com/scop) in [#5423](https://github.com/jdx/mise/pull/5423)
- **(registry)** add backends and tests by [@risu729](https://github.com/risu729) in [#5388](https://github.com/jdx/mise/pull/5388)

### ◀️ Revert

- Revert "fix: `self-update` on MITM firewall" by [@jdx](https://github.com/jdx) in [#5427](https://github.com/jdx/mise/pull/5427)

### Ci

- unpin hyperfine by [@risu729](https://github.com/risu729) in [#5411](https://github.com/jdx/mise/pull/5411)

### New Contributors

- @paradox460 made their first contribution in [#5402](https://github.com/jdx/mise/pull/5402)
- @lelouvincx made their first contribution in [#5420](https://github.com/jdx/mise/pull/5420)

## [2025.6.7](https://github.com/jdx/mise/compare/v2025.6.6..v2025.6.7) - 2025-06-23

### 🐛 Bug Fixes

- **(aqua)** fix versions order by [@risu729](https://github.com/risu729) in [#5406](https://github.com/jdx/mise/pull/5406)

### Ci

- use pinnable tag of taiki-e/install-action by [@risu729](https://github.com/risu729) in [#5405](https://github.com/jdx/mise/pull/5405)

## [2025.6.6](https://github.com/jdx/mise/compare/v2025.6.5..v2025.6.6) - 2025-06-23

### 🚀 Features

- **(registry)** add wash by [@jtakakura](https://github.com/jtakakura) in [#5386](https://github.com/jdx/mise/pull/5386)

### 🐛 Bug Fixes

- **(aqua)** parse consecutive pipes in aqua templates by [@risu729](https://github.com/risu729) in [#5385](https://github.com/jdx/mise/pull/5385)
- **(aqua)** use versions list to install correct version by [@risu729](https://github.com/risu729) in [#5371](https://github.com/jdx/mise/pull/5371)
- **(registry)** talosctl use aqua by [@mangkoran](https://github.com/mangkoran) in [#5348](https://github.com/jdx/mise/pull/5348)
- **(registry)** use aqua backend for watchexec by [@risu729](https://github.com/risu729) in [#5390](https://github.com/jdx/mise/pull/5390)
- **(shim)** improve resolve_symlink for Windows by [@qianlongzt](https://github.com/qianlongzt) in [#5361](https://github.com/jdx/mise/pull/5361)
- add compression-zip-deflate feature on self_update crate for windows target by [@roele](https://github.com/roele) in [#5391](https://github.com/jdx/mise/pull/5391)
- suppress hint on 'cargo search mise' command by [@roele](https://github.com/roele) in [#5400](https://github.com/jdx/mise/pull/5400)

### 📚 Documentation

- Fix typo in README.md - Install mise by [@cytsai1008](https://github.com/cytsai1008) in [#5366](https://github.com/jdx/mise/pull/5366)
- Document trivial task syntax by [@JayBazuzi](https://github.com/JayBazuzi) in [#5352](https://github.com/jdx/mise/pull/5352)

### 🧪 Testing

- **(registry)** fix vultr test by [@risu729](https://github.com/risu729) in [#5372](https://github.com/jdx/mise/pull/5372)

### 📦️ Dependency Updates

- update autofix-ci/action action to v1.3.2 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5377](https://github.com/jdx/mise/pull/5377)
- update docker/setup-buildx-action digest to e468171 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5376](https://github.com/jdx/mise/pull/5376)

### Chore

- update expr-lang crate to v0.3.2 by [@risu729](https://github.com/risu729) in [#5364](https://github.com/jdx/mise/pull/5364)
- show curl error by [@jdx](https://github.com/jdx) in [729aa4a](https://github.com/jdx/mise/commit/729aa4a6279cbb8dd8b1d81e8726d252ad2ad2bc)
- fix latest version fetch by [@jdx](https://github.com/jdx) in [729aadc](https://github.com/jdx/mise/commit/729aadc83e042b276e3ebd3ae378a7e647a54bc0)
- update vfox.rs crate to v1.0.3 by [@risu729](https://github.com/risu729) in [#5393](https://github.com/jdx/mise/pull/5393)
- updated deps by [@jdx](https://github.com/jdx) in [#5403](https://github.com/jdx/mise/pull/5403)

### Ci

- use cargo info to retrieve latest mise version by [@risu729](https://github.com/risu729) in [#5401](https://github.com/jdx/mise/pull/5401)

### New Contributors

- @jtakakura made their first contribution in [#5386](https://github.com/jdx/mise/pull/5386)
- @JayBazuzi made their first contribution in [#5352](https://github.com/jdx/mise/pull/5352)
- @cytsai1008 made their first contribution in [#5366](https://github.com/jdx/mise/pull/5366)

## [2025.6.5](https://github.com/jdx/mise/compare/v2025.6.4..v2025.6.5) - 2025-06-16

### 🚀 Features

- **(registry)** add diffoci by [@mangkoran](https://github.com/mangkoran) in [#5350](https://github.com/jdx/mise/pull/5350)

### 🐛 Bug Fixes

- **(registry)** use mintoolkit/mint for docker-slim by [@risu729](https://github.com/risu729) in [#5351](https://github.com/jdx/mise/pull/5351)
- **(schema)** add missing tool options to schema by [@risu729](https://github.com/risu729) in [#5356](https://github.com/jdx/mise/pull/5356)
- only show deprecation if not using 'tools-version' by [@timfallmk](https://github.com/timfallmk) in [#5290](https://github.com/jdx/mise/pull/5290)

### New Contributors

- @timfallmk made their first contribution in [#5290](https://github.com/jdx/mise/pull/5290)

## [2025.6.4](https://github.com/jdx/mise/compare/v2025.6.3..v2025.6.4) - 2025-06-13

### 🐛 Bug Fixes

- **(registry)** use aqua for checkov by [@risu729](https://github.com/risu729) in [#5343](https://github.com/jdx/mise/pull/5343)

### ◀️ Revert

- fix(aqua): parse templates in version_filter by [@risu729](https://github.com/risu729) in [#5345](https://github.com/jdx/mise/pull/5345)

## [2025.6.3](https://github.com/jdx/mise/compare/v2025.6.2..v2025.6.3) - 2025-06-13

### 🚀 Features

- support matching_regex from the ubi backend by [@yjoer](https://github.com/yjoer) in [#5320](https://github.com/jdx/mise/pull/5320)

### 🐛 Bug Fixes

- **(aqua)** parse templates in version_filter by [@risu729](https://github.com/risu729) in [#5341](https://github.com/jdx/mise/pull/5341)
- **(registry)** use extract_all for docker-slim by [@risu729](https://github.com/risu729) in [#5342](https://github.com/jdx/mise/pull/5342)

### 🚜 Refactor

- **(getting-started)** update powershell profile instructions by [@Armaldio](https://github.com/Armaldio) in [#5340](https://github.com/jdx/mise/pull/5340)

### 📦️ Dependency Updates

- update docker/build-push-action digest to 2634353 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5338](https://github.com/jdx/mise/pull/5338)
- update jdx/mise-action digest to 13abe50 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5339](https://github.com/jdx/mise/pull/5339)

### New Contributors

- @yjoer made their first contribution in [#5320](https://github.com/jdx/mise/pull/5320)

## [2025.6.2](https://github.com/jdx/mise/compare/v2025.6.1..v2025.6.2) - 2025-06-12

### 🚀 Features

- **(aqua)** support cosign bundle option by [@risu729](https://github.com/risu729) in [#5314](https://github.com/jdx/mise/pull/5314)
- **(registry)** add xcodes by [@MontakOleg](https://github.com/MontakOleg) in [#5321](https://github.com/jdx/mise/pull/5321)
- **(registry)** add typstyle by [@3w36zj6](https://github.com/3w36zj6) in [#5319](https://github.com/jdx/mise/pull/5319)

### 🐛 Bug Fixes

- **(cli/doctor)** reduce severity of new version to warnings by [@risu729](https://github.com/risu729) in [#5317](https://github.com/jdx/mise/pull/5317)
- **(doctor)** ignored config roots not displaying by [@jdx](https://github.com/jdx) in [#5336](https://github.com/jdx/mise/pull/5336)
- ls command does not respect MISE_COLOR value by [@roele](https://github.com/roele) in [#5322](https://github.com/jdx/mise/pull/5322)

### 📚 Documentation

- Update contributing.md by [@GitToby](https://github.com/GitToby) in [#5332](https://github.com/jdx/mise/pull/5332)
- add instructions to create/open pwsh profile file by [@Armaldio](https://github.com/Armaldio) in [#5316](https://github.com/jdx/mise/pull/5316)

### New Contributors

- @Armaldio made their first contribution in [#5316](https://github.com/jdx/mise/pull/5316)
- @GitToby made their first contribution in [#5332](https://github.com/jdx/mise/pull/5332)

## [2025.6.1](https://github.com/jdx/mise/compare/v2025.6.0..v2025.6.1) - 2025-06-09

### 🚀 Features

- **(aqua)** support no_asset and error_message by [@risu729](https://github.com/risu729) in [#5303](https://github.com/jdx/mise/pull/5303)
- **(registry)** use ubi backend for func-e by [@risu729](https://github.com/risu729) in [#5273](https://github.com/jdx/mise/pull/5273)

### 🐛 Bug Fixes

- **(task)** use empty string for the default value of option by [@risu729](https://github.com/risu729) in [#5309](https://github.com/jdx/mise/pull/5309)

### 📚 Documentation

- **(registry)** fix links of registry by [@risu729](https://github.com/risu729) in [#5266](https://github.com/jdx/mise/pull/5266)
- **(registry)** fix links to tools by [@risu729](https://github.com/risu729) in [#5272](https://github.com/jdx/mise/pull/5272)
- update example with `pnpm` by [@mrazauskas](https://github.com/mrazauskas) in [#5306](https://github.com/jdx/mise/pull/5306)

### 🧪 Testing

- **(registry)** fix test typos by [@risu729](https://github.com/risu729) in [#5269](https://github.com/jdx/mise/pull/5269)

### 🛡️ Security

- **(security)** prevent untarring outside expected path by [@jdx](https://github.com/jdx) in [#5279](https://github.com/jdx/mise/pull/5279)

### New Contributors

- @mrazauskas made their first contribution in [#5306](https://github.com/jdx/mise/pull/5306)

## [2025.6.0](https://github.com/jdx/mise/compare/v2025.5.17..v2025.6.0) - 2025-06-02

### 🐛 Bug Fixes

- race condition with uv_venv by [@jdx](https://github.com/jdx) in [#5262](https://github.com/jdx/mise/pull/5262)
- disable victoria-metrics test by [@jdx](https://github.com/jdx) in [11bda4b](https://github.com/jdx/mise/commit/11bda4bda97bd02f6a8cae2c7f345846769ff776)

## [2025.5.17](https://github.com/jdx/mise/compare/v2025.5.16..v2025.5.17) - 2025-05-31

### 🚀 Features

- add railway cli by [@jahands](https://github.com/jahands) in [#5083](https://github.com/jdx/mise/pull/5083)

### 🐛 Bug Fixes

- **(zig)** exclude mach version from version list by [@mangkoran](https://github.com/mangkoran) in [#5240](https://github.com/jdx/mise/pull/5240)
- refresh settings by [@jdx](https://github.com/jdx) in [#5252](https://github.com/jdx/mise/pull/5252)

### ⚡ Performance

- re-enable parallelism for `mise up` by [@jdx](https://github.com/jdx) in [#5249](https://github.com/jdx/mise/pull/5249)

## [2025.5.16](https://github.com/jdx/mise/compare/v2025.5.15..v2025.5.16) - 2025-05-29

### 🐛 Bug Fixes

- ensure config is always wrapped in Result by [@jdx](https://github.com/jdx) in [#5223](https://github.com/jdx/mise/pull/5223)

### ⚡ Performance

- improve init performance by [@jdx](https://github.com/jdx) in [#5231](https://github.com/jdx/mise/pull/5231)

### Chore

- remove hyperfine from main builds by [@jdx](https://github.com/jdx) in [#5226](https://github.com/jdx/mise/pull/5226)

## [2025.5.15](https://github.com/jdx/mise/compare/v2025.5.14..v2025.5.15) - 2025-05-28

### 🚀 Features

- **(registry)** add aqua backend for maven by [@ZeroAurora](https://github.com/ZeroAurora) in [#5219](https://github.com/jdx/mise/pull/5219)

### 🐛 Bug Fixes

- **(zig)** **breaking** get tarball url from download index by [@mangkoran](https://github.com/mangkoran) in [#5182](https://github.com/jdx/mise/pull/5182)
- **(zig)** get version list from download index by [@mangkoran](https://github.com/mangkoran) in [#5217](https://github.com/jdx/mise/pull/5217)
- use a better completion dir for more compatibility by [@ken-kuro](https://github.com/ken-kuro) in [#5207](https://github.com/jdx/mise/pull/5207)
- set handler for ctrlc on windows shell by [@L0RD-ZER0](https://github.com/L0RD-ZER0) in [#5209](https://github.com/jdx/mise/pull/5209)
- prevent go installation failure on go.mod version mismatch by [@roele](https://github.com/roele) in [#5212](https://github.com/jdx/mise/pull/5212)
- mise run --cd <dir> not working with latest mise by [@roele](https://github.com/roele) in [#5221](https://github.com/jdx/mise/pull/5221)

### 📚 Documentation

- update dependencies section in contributing.md by [@LuckyWindsck](https://github.com/LuckyWindsck) in [#5200](https://github.com/jdx/mise/pull/5200)

### Chore

- disable auto cargo up by [@jdx](https://github.com/jdx) in [3306f6e](https://github.com/jdx/mise/commit/3306f6ef726fe85d71163121497e1d5dd5cd73ca)

### New Contributors

- @L0RD-ZER0 made their first contribution in [#5209](https://github.com/jdx/mise/pull/5209)

## [2025.5.14](https://github.com/jdx/mise/compare/v2025.5.13..v2025.5.14) - 2025-05-26

### 🐛 Bug Fixes

- installing tools with postinstall hooks fails by [@roele](https://github.com/roele) in [#5191](https://github.com/jdx/mise/pull/5191)
- prefer offline when executing shims by [@jdx](https://github.com/jdx) in [#5195](https://github.com/jdx/mise/pull/5195)
- multi-line task output is shown in bold by [@roele](https://github.com/roele) in [#5197](https://github.com/jdx/mise/pull/5197)

### ⚡ Performance

- improve tool loading performance in async code by [@jdx](https://github.com/jdx) in [#5198](https://github.com/jdx/mise/pull/5198)

## [2025.5.13](https://github.com/jdx/mise/compare/v2025.5.12..v2025.5.13) - 2025-05-26

### 🐛 Bug Fixes

- output was silenced on task fail with keep-order by [@artemisart](https://github.com/artemisart) in [#5175](https://github.com/jdx/mise/pull/5175)
- avoid mapfile to run e2e tests on macOS (bash 3.2) by [@artemisart](https://github.com/artemisart) in [#5170](https://github.com/jdx/mise/pull/5170)
- flaky keep-order e2e test by [@artemisart](https://github.com/artemisart) in [#5178](https://github.com/jdx/mise/pull/5178)
- watch mise.lock for changes by [@jdx](https://github.com/jdx) in [#5184](https://github.com/jdx/mise/pull/5184)
- remote task dependency does not work by [@roele](https://github.com/roele) in [#5183](https://github.com/jdx/mise/pull/5183)
- rayon -> tokio by [@jdx](https://github.com/jdx) in [#5172](https://github.com/jdx/mise/pull/5172)
- cache results from version host by [@jdx](https://github.com/jdx) in [#5187](https://github.com/jdx/mise/pull/5187)
- cache results from version host for aqua packages by [@jdx](https://github.com/jdx) in [#5188](https://github.com/jdx/mise/pull/5188)

### 📚 Documentation

- standardize subcommand format to 'u|use' for consistency by [@LuckyWindsck](https://github.com/LuckyWindsck) in [#5167](https://github.com/jdx/mise/pull/5167)
- clarify how to enable ideomatic version file reading for ruby by [@amkisko](https://github.com/amkisko) in [#5163](https://github.com/jdx/mise/pull/5163)

### 🧪 Testing

- added perf test by [@jdx](https://github.com/jdx) in [#5179](https://github.com/jdx/mise/pull/5179)
- skip benchmark errors for now by [@jdx](https://github.com/jdx) in [#5186](https://github.com/jdx/mise/pull/5186)

### Chore

- fix clippy issue in xonsh by [@jdx](https://github.com/jdx) in [#5180](https://github.com/jdx/mise/pull/5180)
- improve shfmt linter by [@jdx](https://github.com/jdx) in [#5181](https://github.com/jdx/mise/pull/5181)
- cargo up by [@jdx](https://github.com/jdx) in [3ece604](https://github.com/jdx/mise/commit/3ece60479bd8b8e6a00a02b83c0afdd544d95034)
- fix hyperfine step summary by [@jdx](https://github.com/jdx) in [36ab4a1](https://github.com/jdx/mise/commit/36ab4a12ffed85f07ce918d1a21a6da9f7ebef2c)
- adjust perf thresholds by [@jdx](https://github.com/jdx) in [4113a3b](https://github.com/jdx/mise/commit/4113a3b82c3fca4eae0dbe7845ec2d513f5b6c8b)

### New Contributors

- @amkisko made their first contribution in [#5163](https://github.com/jdx/mise/pull/5163)
- @LuckyWindsck made their first contribution in [#5167](https://github.com/jdx/mise/pull/5167)

## [2025.5.12](https://github.com/jdx/mise/compare/v2025.5.11..v2025.5.12) - 2025-05-25

### 🐛 Bug Fixes

- read global/system config file tasks properly by [@jdx](https://github.com/jdx) in [#5169](https://github.com/jdx/mise/pull/5169)
- typo in time! parallelize_tasks by [@artemisart](https://github.com/artemisart) in [#5171](https://github.com/jdx/mise/pull/5171)

### 🧪 Testing

- disable non-working zig test by [@jdx](https://github.com/jdx) in [2ffb7ea](https://github.com/jdx/mise/commit/2ffb7eaa22e3623363dd153d581bb1a17da78483)

### New Contributors

- @artemisart made their first contribution in [#5171](https://github.com/jdx/mise/pull/5171)

## [2025.5.11](https://github.com/jdx/mise/compare/v2025.5.10..v2025.5.11) - 2025-05-23

### 🚀 Features

- **(registry)** add victoriametrics by [@shikharbhardwaj](https://github.com/shikharbhardwaj) in [#5161](https://github.com/jdx/mise/pull/5161)
- added dotslash by [@jdx](https://github.com/jdx) in [#5165](https://github.com/jdx/mise/pull/5165)

### 🐛 Bug Fixes

- **(registry)** remove full from taplo by [@risu729](https://github.com/risu729) in [#5160](https://github.com/jdx/mise/pull/5160)
- mise registry links for ubi with exe selector by [@mnm364](https://github.com/mnm364) in [#5156](https://github.com/jdx/mise/pull/5156)
- mise settings add idiomatic_version_file_enable_tools stores duplicates in config by [@roele](https://github.com/roele) in [#5162](https://github.com/jdx/mise/pull/5162)
- infinite sourcing loop on bash-completion by [@ken-kuro](https://github.com/ken-kuro) in [#5150](https://github.com/jdx/mise/pull/5150)

### 🧪 Testing

- disable mockolo since linux does not work anymore by [@jdx](https://github.com/jdx) in [5387d70](https://github.com/jdx/mise/commit/5387d7012d65b3da3dde12cd0a0eb07288b2d8f6)

### New Contributors

- @ken-kuro made their first contribution in [#5150](https://github.com/jdx/mise/pull/5150)
- @shikharbhardwaj made their first contribution in [#5161](https://github.com/jdx/mise/pull/5161)

## [2025.5.10](https://github.com/jdx/mise/compare/v2025.5.9..v2025.5.10) - 2025-05-22

### 🚀 Features

- **(registry)** add process-compose by [@evanleck](https://github.com/evanleck) in [#4788](https://github.com/jdx/mise/pull/4788)
- **(registry)** add tailpipe by [@pdecat](https://github.com/pdecat) in [#4858](https://github.com/jdx/mise/pull/4858)
- mise search by [@roele](https://github.com/roele) in [#5153](https://github.com/jdx/mise/pull/5153)

### 🐛 Bug Fixes

- **(aqua)** windows exe fix by [@jdx](https://github.com/jdx) in [#5154](https://github.com/jdx/mise/pull/5154)

### 🧪 Testing

- disable failing edit test by [@jdx](https://github.com/jdx) in [8698bce](https://github.com/jdx/mise/commit/8698bce774eafa86afa9d5b56a225fa6cdbe6ea1)

### Chore

- disable failing docker dev build by [@jdx](https://github.com/jdx) in [496c1c9](https://github.com/jdx/mise/commit/496c1c91545ed7f013726cd48e746835bdf570d8)
- temporarily disable cargo up to fix build by [@jdx](https://github.com/jdx) in [90c66b7](https://github.com/jdx/mise/commit/90c66b7b561e81efe7d951a0ce9574c11e7b91a7)

### New Contributors

- @evanleck made their first contribution in [#4788](https://github.com/jdx/mise/pull/4788)

## [2025.5.9](https://github.com/jdx/mise/compare/v2025.5.8..v2025.5.9) - 2025-05-21

### 🚀 Features

- **(registry)** add microsoft `edit` by [@garysassano](https://github.com/garysassano) in [#5145](https://github.com/jdx/mise/pull/5145)
- added buildifier by [@jdx](https://github.com/jdx) in [#5142](https://github.com/jdx/mise/pull/5142)
- add shims in REMOTE ENV by [@acesyde](https://github.com/acesyde) in [#5139](https://github.com/jdx/mise/pull/5139)

### 🐛 Bug Fixes

- **(aqua)** use complete_windows_ext by [@jdx](https://github.com/jdx) in [#5146](https://github.com/jdx/mise/pull/5146)
- **(registry)** support editorconfig-checker in windows by [@risu729](https://github.com/risu729) in [#5125](https://github.com/jdx/mise/pull/5125)
- SSH remote tasks do not support organizations in repository path by [@roele](https://github.com/roele) in [#5124](https://github.com/jdx/mise/pull/5124)
- SSH remote tasks do not support organizations in repository path by [@roele](https://github.com/roele) in [#5132](https://github.com/jdx/mise/pull/5132)

### 📚 Documentation

- squeeze spaces when migrating from asdf by [@maximd](https://github.com/maximd) in [#5131](https://github.com/jdx/mise/pull/5131)

### Chore

- pin github actions by [@jdx](https://github.com/jdx) in [bf18644](https://github.com/jdx/mise/commit/bf1864472c3ed587fbdb497722849cf6cfacca5c)
- use renovate to pin github actions by [@jdx](https://github.com/jdx) in [b80d8e3](https://github.com/jdx/mise/commit/b80d8e3ffe73d315c4214f77dedcf4cce7a54032)
- disable mold in ci by [@jdx](https://github.com/jdx) in [#5128](https://github.com/jdx/mise/pull/5128)
- fix buildifier test by [@jdx](https://github.com/jdx) in [232a4c6](https://github.com/jdx/mise/commit/232a4c641fedc9dfb83ce048ad5b47253b139854)

### New Contributors

- @maximd made their first contribution in [#5131](https://github.com/jdx/mise/pull/5131)

## [2025.5.8](https://github.com/jdx/mise/compare/v2025.5.7..v2025.5.8) - 2025-05-18

### 🚀 Features

- **(registry)** added astro by [@mnm364](https://github.com/mnm364) in [#5106](https://github.com/jdx/mise/pull/5106)

### 🐛 Bug Fixes

- **(registry)** use aqua for delta by [@risu729](https://github.com/risu729) in [#5116](https://github.com/jdx/mise/pull/5116)
- elixir bin name on windows by [@arilence](https://github.com/arilence) in [#5107](https://github.com/jdx/mise/pull/5107)

### Chore

- create a detached signature when signing the source tarball by [@digital-wonderland](https://github.com/digital-wonderland) in [#5108](https://github.com/jdx/mise/pull/5108)

### New Contributors

- @arilence made their first contribution in [#5107](https://github.com/jdx/mise/pull/5107)

## [2025.5.7](https://github.com/jdx/mise/compare/v2025.5.6..v2025.5.7) - 2025-05-18

### 🐛 Bug Fixes

- using custom port with SSH based remote tasks by [@roele](https://github.com/roele) in [#5110](https://github.com/jdx/mise/pull/5110)
- update rabbitmq backend by [@SerhiiFesenko](https://github.com/SerhiiFesenko) in [#5115](https://github.com/jdx/mise/pull/5115)
- maven-mvnd does not install with aqua by [@roele](https://github.com/roele) in [#5117](https://github.com/jdx/mise/pull/5117)

### New Contributors

- @SerhiiFesenko made their first contribution in [#5115](https://github.com/jdx/mise/pull/5115)

## [2025.5.6](https://github.com/jdx/mise/compare/v2025.5.5..v2025.5.6) - 2025-05-17

### 🚀 Features

- **(registry)** add oauth2c by [@kklee998](https://github.com/kklee998) in [#5056](https://github.com/jdx/mise/pull/5056)
- use new Java metadata source by [@roele](https://github.com/roele) in [#5089](https://github.com/jdx/mise/pull/5089)

### 🐛 Bug Fixes

- **(config)** project root for files in .config/ or mise/ by [@scop](https://github.com/scop) in [#5102](https://github.com/jdx/mise/pull/5102)
- Clarify some of the filters and fix the config_root filter example by [@afranchuk](https://github.com/afranchuk) in [#5086](https://github.com/jdx/mise/pull/5086)

### 🚜 Refactor

- **(registry)** use aqua for rclone by [@scop](https://github.com/scop) in [#5096](https://github.com/jdx/mise/pull/5096)

### 📚 Documentation

- **(tasks)** point to `dir` config for task default cwd by [@scop](https://github.com/scop) in [#5103](https://github.com/jdx/mise/pull/5103)
- remove go.mod from idiomatic version files by [@Gandem](https://github.com/Gandem) in [#5090](https://github.com/jdx/mise/pull/5090)
- remove stray backquote from toml-tasks by [@scop](https://github.com/scop) in [#5097](https://github.com/jdx/mise/pull/5097)
- add some missing vue interpolation escapes by [@scop](https://github.com/scop) in [#5099](https://github.com/jdx/mise/pull/5099)
- remove some references to rtx by [@jdx](https://github.com/jdx) in [#5105](https://github.com/jdx/mise/pull/5105)

### 📦️ Dependency Updates

- update dependency node to v22 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5093](https://github.com/jdx/mise/pull/5093)

### Chore

- sign source tarball by [@digital-wonderland](https://github.com/digital-wonderland) in [#5087](https://github.com/jdx/mise/pull/5087)

### New Contributors

- @digital-wonderland made their first contribution in [#5087](https://github.com/jdx/mise/pull/5087)
- @kklee998 made their first contribution in [#5056](https://github.com/jdx/mise/pull/5056)
- @afranchuk made their first contribution in [#5086](https://github.com/jdx/mise/pull/5086)
- @Gandem made their first contribution in [#5090](https://github.com/jdx/mise/pull/5090)

## [2025.5.5](https://github.com/jdx/mise/compare/v2025.5.4..v2025.5.5) - 2025-05-15

### 🚀 Features

- **(registry)** add pinact by [@3w36zj6](https://github.com/3w36zj6) in [#5061](https://github.com/jdx/mise/pull/5061)
- **(registry)** add ghalint by [@risu729](https://github.com/risu729) in [#5063](https://github.com/jdx/mise/pull/5063)
- new "enable-tools" option by [@zeitlinger](https://github.com/zeitlinger) in [#4784](https://github.com/jdx/mise/pull/4784)

### 📚 Documentation

- hide `ls --offline` flag that is a no-op by [@jdx](https://github.com/jdx) in [#5068](https://github.com/jdx/mise/pull/5068)

### Chore

- add pr comment for new tools by [@jdx](https://github.com/jdx) in [#5067](https://github.com/jdx/mise/pull/5067)
- set comment-tag for registry pr comment by [@jdx](https://github.com/jdx) in [#5069](https://github.com/jdx/mise/pull/5069)
- run multiple test-tool jobs by [@jdx](https://github.com/jdx) in [#5070](https://github.com/jdx/mise/pull/5070)
- fix typo in registry comment by [@jdx](https://github.com/jdx) in [#5071](https://github.com/jdx/mise/pull/5071)
- bump zip-rs version by [@hkoosha](https://github.com/hkoosha) in [#5073](https://github.com/jdx/mise/pull/5073)

### New Contributors

- @3w36zj6 made their first contribution in [#5061](https://github.com/jdx/mise/pull/5061)

## [2025.5.4](https://github.com/jdx/mise/compare/v2025.5.3..v2025.5.4) - 2025-05-14

### 🚀 Features

- **(registry)** add sshi by [@scop](https://github.com/scop) in [#5048](https://github.com/jdx/mise/pull/5048)
- **(registry)** added Neon CLI by [@joehorsnell](https://github.com/joehorsnell) in [#4994](https://github.com/jdx/mise/pull/4994)

### 🐛 Bug Fixes

- **(registry)** update glab ubi provider by [@StingRayZA](https://github.com/StingRayZA) in [#5052](https://github.com/jdx/mise/pull/5052)
- mise panics if CI env var isn't a boolean by [@roele](https://github.com/roele) in [#5059](https://github.com/jdx/mise/pull/5059)
- `aqua` version test by [@joehorsnell](https://github.com/joehorsnell) in [#5038](https://github.com/jdx/mise/pull/5038)
- run hook-env after trusting config file by [@jdx](https://github.com/jdx) in [#5062](https://github.com/jdx/mise/pull/5062)

### 🚜 Refactor

- **(hooks)** remove duplicated code by [@risu729](https://github.com/risu729) in [#5036](https://github.com/jdx/mise/pull/5036)

### 📚 Documentation

- fix add_predicate handler in neovim cookbook by [@okuuva](https://github.com/okuuva) in [#5044](https://github.com/jdx/mise/pull/5044)
- improve treesitter queries in neovim cookbook by [@okuuva](https://github.com/okuuva) in [#5045](https://github.com/jdx/mise/pull/5045)

### New Contributors

- @okuuva made their first contribution in [#5045](https://github.com/jdx/mise/pull/5045)

## [2025.5.3](https://github.com/jdx/mise/compare/v2025.5.2..v2025.5.3) - 2025-05-09

### 🚀 Features

- **(registry)** add coreutils by [@kit494way](https://github.com/kit494way) in [#5033](https://github.com/jdx/mise/pull/5033)

### 🐛 Bug Fixes

- unuse command does not support env, global and path options by [@roele](https://github.com/roele) in [#5021](https://github.com/jdx/mise/pull/5021)

### 🧪 Testing

- disable aqua for now due to bad version output by [@jdx](https://github.com/jdx) in [fa3daa2](https://github.com/jdx/mise/commit/fa3daa2cab09ba7e0140fcf2112375eef8427a85)
- fix python poetry test by [@jdx](https://github.com/jdx) in [c46a190](https://github.com/jdx/mise/commit/c46a190cb699b7700aa636a2bc888222ed7e9dbc)

### ◀️ Revert

- Revert "fix(dotenv): properly escape values in generated dotenv " by [@jdx](https://github.com/jdx) in [358c3da](https://github.com/jdx/mise/commit/358c3dab2dba7129ac115fc3414657dc39b2bd79)
- Revert "fix(env): fix dotenv files cascading (fix #4688) " by [@jdx](https://github.com/jdx) in [b1ca323](https://github.com/jdx/mise/commit/b1ca3235ffc9635f17dac0896c3c07b975d65819)

### 📦️ Dependency Updates

- update rust crate nix to 0.30 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5032](https://github.com/jdx/mise/pull/5032)
- update rust crate built to 0.8 by [@renovate[bot]](https://github.com/renovate[bot]) in [#5031](https://github.com/jdx/mise/pull/5031)

## [2025.5.2](https://github.com/jdx/mise/compare/v2025.5.1..v2025.5.2) - 2025-05-07

### 🐛 Bug Fixes

- **(dotenv)** properly escape values in generated dotenv by [@noirbizarre](https://github.com/noirbizarre) in [#5010](https://github.com/jdx/mise/pull/5010)
- **(registry)** use full version of taplo by [@risu729](https://github.com/risu729) in [#5017](https://github.com/jdx/mise/pull/5017)

### 📚 Documentation

- hide rtx docs by [@jdx](https://github.com/jdx) in [90ae2ce](https://github.com/jdx/mise/commit/90ae2ce5abf4faa65ef2414385e587d97ff0ca2c)
- describe cache auto-prune by [@jdx](https://github.com/jdx) in [#5013](https://github.com/jdx/mise/pull/5013)
- mark idiomatic_version_file_disable_tools as deprecated by [@jdx](https://github.com/jdx) in [9bb80f3](https://github.com/jdx/mise/commit/9bb80f301e29fcc668f51de8e0a168a32c9ac8db)

### Chore

- remove homebrew bump step by [@jdx](https://github.com/jdx) in [1625608](https://github.com/jdx/mise/commit/1625608c0025ec21a49eedcc85533facde52a8a7)
- simplify git logs by [@jdx](https://github.com/jdx) in [#5012](https://github.com/jdx/mise/pull/5012)

## [2025.5.1](https://github.com/jdx/mise/compare/v2025.5.0..v2025.5.1) - 2025-05-05

### 🚀 Features

- **(registry)** use aqua for taplo by [@risu729](https://github.com/risu729) in [#4991](https://github.com/jdx/mise/pull/4991)
- add mise_env tera variable for templates by [@auxesis](https://github.com/auxesis) in [#5002](https://github.com/jdx/mise/pull/5002)

### 🐛 Bug Fixes

- **(env)** fix dotenv files cascading (fix #4688) by [@noirbizarre](https://github.com/noirbizarre) in [#4996](https://github.com/jdx/mise/pull/4996)

### Ci

- **(registry)** increaset timeout to 30 mins by [@risu729](https://github.com/risu729) in [#5006](https://github.com/jdx/mise/pull/5006)

## [2025.5.0](https://github.com/jdx/mise/compare/v2025.4.12..v2025.5.0) - 2025-05-03

### 🚀 Features

- **(registry)** add luau by [@rhanneken](https://github.com/rhanneken) in [#4993](https://github.com/jdx/mise/pull/4993)
- **(registry)** add numbat by [@risu729](https://github.com/risu729) in [#4980](https://github.com/jdx/mise/pull/4980)
- **(status)** add setting to control status message truncation by [@rarescosma](https://github.com/rarescosma) in [#4986](https://github.com/jdx/mise/pull/4986)
- add check flag for the fmt command by [@roele](https://github.com/roele) in [#4972](https://github.com/jdx/mise/pull/4972)
- use aqua for btop by [@jdx](https://github.com/jdx) in [#4979](https://github.com/jdx/mise/pull/4979)

### 🐛 Bug Fixes

- **(java)** filter out JetBrains releases with features by [@roele](https://github.com/roele) in [#4970](https://github.com/jdx/mise/pull/4970)
- fix deadlocks caused by uv_venv_auto by [@risu729](https://github.com/risu729) in [#4900](https://github.com/jdx/mise/pull/4900)

### 📚 Documentation

- Put dot in dotfile example by [@ryanbrainard](https://github.com/ryanbrainard) in [#4965](https://github.com/jdx/mise/pull/4965)

### Chore

- only use mold when available by [@jdx](https://github.com/jdx) in [#4978](https://github.com/jdx/mise/pull/4978)
- enable clearing screen for confirm and dialog by [@roele](https://github.com/roele) in [#4990](https://github.com/jdx/mise/pull/4990)

### New Contributors

- @rarescosma made their first contribution in [#4986](https://github.com/jdx/mise/pull/4986)
- @rhanneken made their first contribution in [#4993](https://github.com/jdx/mise/pull/4993)
- @ryanbrainard made their first contribution in [#4965](https://github.com/jdx/mise/pull/4965)

## [2025.4.12](https://github.com/jdx/mise/compare/v2025.4.11..v2025.4.12) - 2025-04-29

### 🐛 Bug Fixes

- **(aqua)** fix bin_path of tools in monorepo by [@risu729](https://github.com/risu729) in [#4954](https://github.com/jdx/mise/pull/4954)
- **(schema)** allow array of objects for hooks by [@risu729](https://github.com/risu729) in [#4955](https://github.com/jdx/mise/pull/4955)
- store tool version opts in .mise.backend by [@roele](https://github.com/roele) in [#4960](https://github.com/jdx/mise/pull/4960)

### 📚 Documentation

- add information about the DNF repository by [@acesyde](https://github.com/acesyde) in [#4956](https://github.com/jdx/mise/pull/4956)

### 🧪 Testing

- fix registry tools by [@jdx](https://github.com/jdx) in [#4959](https://github.com/jdx/mise/pull/4959)

### Chore

- **(deny)** added CDLA-Permissive-2.0 by [@jdx](https://github.com/jdx) in [#4961](https://github.com/jdx/mise/pull/4961)

## [2025.4.11](https://github.com/jdx/mise/compare/v2025.4.10..v2025.4.11) - 2025-04-27

### 🚀 Features

- **(cargo)** allow customizable registry by [@acesyde](https://github.com/acesyde) in [#4948](https://github.com/jdx/mise/pull/4948)
- **(doctor)** show error if tool not installed by [@jdx](https://github.com/jdx) in [#4952](https://github.com/jdx/mise/pull/4952)
- added sd by [@jdx](https://github.com/jdx) in [#4950](https://github.com/jdx/mise/pull/4950)
- MISE_LOG_HTTP by [@jdx](https://github.com/jdx) in [#4951](https://github.com/jdx/mise/pull/4951)

### 🐛 Bug Fixes

- set prune age to 10y in dockerfile by [@jdx](https://github.com/jdx) in [9a521dc](https://github.com/jdx/mise/commit/9a521dc1e93e57567dcb262482a6a8d382fbebe8)

### Chore

- brew update by [@jdx](https://github.com/jdx) in [641f3b3](https://github.com/jdx/mise/commit/641f3b3ef1c8c7b2e4931c5012c2b8dc94533070)
- brew sync repos by [@jdx](https://github.com/jdx) in [3318e98](https://github.com/jdx/mise/commit/3318e98d78af8a11e36f13574abe4f1cce181a92)
- bump usage by [@jdx](https://github.com/jdx) in [#4949](https://github.com/jdx/mise/pull/4949)

## [2025.4.10](https://github.com/jdx/mise/compare/v2025.4.9..v2025.4.10) - 2025-04-26

### 🚀 Features

- **(registry)** add `cli53` backend by [@garysassano](https://github.com/garysassano) in [#4937](https://github.com/jdx/mise/pull/4937)
- pipx custom repository url by [@acesyde](https://github.com/acesyde) in [#4945](https://github.com/jdx/mise/pull/4945)

### 🐛 Bug Fixes

- **(hook-env)** path order by [@jdx](https://github.com/jdx) in [#4946](https://github.com/jdx/mise/pull/4946)
- **(unuse)** allow unusing any version if version not specified by [@jdx](https://github.com/jdx) in [#4944](https://github.com/jdx/mise/pull/4944)
- Always use env::MISE_BIN when calling mise from itself by [@hverlin](https://github.com/hverlin) in [#4943](https://github.com/jdx/mise/pull/4943)

### 📚 Documentation

- remove outdated note about automatic shim activation with Scoop by [@jgutierrezre](https://github.com/jgutierrezre) in [#4941](https://github.com/jdx/mise/pull/4941)

### Chore

- checkout for homebrew bump by [@jdx](https://github.com/jdx) in [6d7b0f6](https://github.com/jdx/mise/commit/6d7b0f6fdf83ee9d7be29a61b5b5be202ac0526a)
- mise.lock by [@jdx](https://github.com/jdx) in [05c9a24](https://github.com/jdx/mise/commit/05c9a241744fa330677402a365344b8430a4984c)
- updated deps by [@jdx](https://github.com/jdx) in [ac5cf5d](https://github.com/jdx/mise/commit/ac5cf5d840dc3a997dce0b1d3a1af963ef456ac2)
- brew developer by [@jdx](https://github.com/jdx) in [445e313](https://github.com/jdx/mise/commit/445e313985cb948cf2a7cb57d896055b898a0f67)

### New Contributors

- @garysassano made their first contribution in [#4937](https://github.com/jdx/mise/pull/4937)
- @jgutierrezre made their first contribution in [#4941](https://github.com/jdx/mise/pull/4941)

## [2025.4.9](https://github.com/jdx/mise/compare/v2025.4.8..v2025.4.9) - 2025-04-25

### 🚀 Features

- **(registry)** added tusd by [@mnm364](https://github.com/mnm364) in [#4928](https://github.com/jdx/mise/pull/4928)
- **(registry)** added fastfetch by [@sassdavid](https://github.com/sassdavid) in [#4932](https://github.com/jdx/mise/pull/4932)

### 🐛 Bug Fixes

- remove missing symlinks on unuse when pruning by [@roele](https://github.com/roele) in [#4930](https://github.com/jdx/mise/pull/4930)

### 📚 Documentation

- typo by [@jdx](https://github.com/jdx) in [314657f](https://github.com/jdx/mise/commit/314657fb6ee69646464c35ed4d8b72f0f2d551da)

### ⚡ Performance

- turn several of the list functions into parallel iters by [@lespea](https://github.com/lespea) in [#4924](https://github.com/jdx/mise/pull/4924)

### 🧪 Testing

- fix kwok by [@jdx](https://github.com/jdx) in [4516335](https://github.com/jdx/mise/commit/451633512b67d26f2b3263094826da7c7406c1da)
- increase windows-e2e timeout by [@jdx](https://github.com/jdx) in [ce4f734](https://github.com/jdx/mise/commit/ce4f73462b10979f3721400393c4d3ba782c3bb4)

### 📦️ Dependency Updates

- update apple-actions/import-codesign-certs action to v5 by [@renovate[bot]](https://github.com/renovate[bot]) in [#4936](https://github.com/jdx/mise/pull/4936)
- update rust crate tabled to 0.19 by [@renovate[bot]](https://github.com/renovate[bot]) in [#4935](https://github.com/jdx/mise/pull/4935)

### Chore

- use macos-latest in GHA by [@jdx](https://github.com/jdx) in [05b5d49](https://github.com/jdx/mise/commit/05b5d49eaa3c4e78f1102dd2d9cfbca63c276ec0)
- attempt to fix brew bump by [@jdx](https://github.com/jdx) in [043f97f](https://github.com/jdx/mise/commit/043f97f23e9af914772474ee0379b5a7d9399f3e)
- mise up by [@jdx](https://github.com/jdx) in [ee7436d](https://github.com/jdx/mise/commit/ee7436d65c89416ee39ee424e296ae329f747323)

### New Contributors

- @lespea made their first contribution in [#4924](https://github.com/jdx/mise/pull/4924)

## [2025.4.8](https://github.com/jdx/mise/compare/v2025.4.7..v2025.4.8) - 2025-04-23

### 🐛 Bug Fixes

- hide idiomatic warning if no versions in idiomatic file by [@jdx](https://github.com/jdx) in [#4922](https://github.com/jdx/mise/pull/4922)

### 📚 Documentation

- clean up idiomatic deprecation message by [@jdx](https://github.com/jdx) in [c31aa2c](https://github.com/jdx/mise/commit/c31aa2cbd07a1f74049a0c6b72dfb91632ff5816)
- punctuation improvements to idiomatic deprecation message by [@glasser](https://github.com/glasser) in [#4915](https://github.com/jdx/mise/pull/4915)

## [2025.4.7](https://github.com/jdx/mise/compare/v2025.4.6..v2025.4.7) - 2025-04-23

### 🚀 Features

- **(registry)** added oxipng by [@ldrouard](https://github.com/ldrouard) in [#4452](https://github.com/jdx/mise/pull/4452)
- `mise tasks --local|--global` by [@jdx](https://github.com/jdx) in [#4907](https://github.com/jdx/mise/pull/4907)

### 🐛 Bug Fixes

- added lockfile for pyenv by [@jdx](https://github.com/jdx) in [#4906](https://github.com/jdx/mise/pull/4906)
- move idiomatic version breaking change from 2026.1.1 to 2025.10.0 by [@jdx](https://github.com/jdx) in [#4909](https://github.com/jdx/mise/pull/4909)
- allow setting lists to be empty by [@jdx](https://github.com/jdx) in [#4912](https://github.com/jdx/mise/pull/4912)

### 🧪 Testing

- test registry changes by themselves by [@jdx](https://github.com/jdx) in [#4910](https://github.com/jdx/mise/pull/4910)
- test registry changes by themselves by [@jdx](https://github.com/jdx) in [#4911](https://github.com/jdx/mise/pull/4911)

### 📦️ Dependency Updates

- update rust crate tabled to 0.18 by [@renovate[bot]](https://github.com/renovate[bot]) in [#4873](https://github.com/jdx/mise/pull/4873)

### Chore

- use hk for linting by [@jdx](https://github.com/jdx) in [#4908](https://github.com/jdx/mise/pull/4908)
- prefer ubi for shellcheck by [@jdx](https://github.com/jdx) in [c805f39](https://github.com/jdx/mise/commit/c805f399a0987db2ce812f2bd6ff66beb53de989)

## [2025.4.6](https://github.com/jdx/mise/compare/v2025.4.5..v2025.4.6) - 2025-04-22

### 🚀 Features

- **(aqua)** support github_release minisign type by [@risu729](https://github.com/risu729) in [#4897](https://github.com/jdx/mise/pull/4897)
- **(go)** support build tags by [@bamorim](https://github.com/bamorim) in [#4863](https://github.com/jdx/mise/pull/4863)
- **(registry)** added Signadot by [@joehorsnell](https://github.com/joehorsnell) in [#4868](https://github.com/jdx/mise/pull/4868)
- added `idiomatic_version_file_enable_tools` and deprecated `idiomatic_version_file_disable_tools` by [@jdx](https://github.com/jdx) in [#4902](https://github.com/jdx/mise/pull/4902)

### 🐛 Bug Fixes

- **(doctor)** redact gitlab/enterprise tokens by [@risu729](https://github.com/risu729) in [#4888](https://github.com/jdx/mise/pull/4888)
- **(task)** enable templates in shell and tools of tasks by [@risu729](https://github.com/risu729) in [#4887](https://github.com/jdx/mise/pull/4887)
- allow interactive upgrade to select nothing by [@risu729](https://github.com/risu729) in [#4891](https://github.com/jdx/mise/pull/4891)
- enable templates for shell of hooks by [@risu729](https://github.com/risu729) in [#4893](https://github.com/jdx/mise/pull/4893)

### 📚 Documentation

- fix typo in go backend tags option title by [@bamorim](https://github.com/bamorim) in [#4884](https://github.com/jdx/mise/pull/4884)
- update link to faq in use_versions_host by [@risu729](https://github.com/risu729) in [#4890](https://github.com/jdx/mise/pull/4890)

### 🧪 Testing

- remove flaky bazel-watcher by [@jdx](https://github.com/jdx) in [9e95e6a](https://github.com/jdx/mise/commit/9e95e6afd04a43cc7d43e2f2280c7880bb481507)

### New Contributors

- @joehorsnell made their first contribution in [#4868](https://github.com/jdx/mise/pull/4868)
- @bamorim made their first contribution in [#4884](https://github.com/jdx/mise/pull/4884)

## [2025.4.5](https://github.com/jdx/mise/compare/v2025.4.4..v2025.4.5) - 2025-04-18

### 🐛 Bug Fixes

- **(ubi)** API URL for GitHub should not have /repos segement by [@roele](https://github.com/roele) in [#4848](https://github.com/jdx/mise/pull/4848)
- **(ubi)** URL syntax fails by [@roele](https://github.com/roele) in [#4859](https://github.com/jdx/mise/pull/4859)
- allow to install non-numeric elixir versions by [@roele](https://github.com/roele) in [#4850](https://github.com/jdx/mise/pull/4850)
- removed possible single-point-of-failure while running `mise upgrade` by [@hitblast](https://github.com/hitblast) in [#4847](https://github.com/jdx/mise/pull/4847)
- `#MISE tools=` in task header by [@jdx](https://github.com/jdx) in [#4860](https://github.com/jdx/mise/pull/4860)

### 🧪 Testing

- fix aqua tool test by [@jdx](https://github.com/jdx) in [4f2c050](https://github.com/jdx/mise/commit/4f2c0505502c1e3c7bf3478d61a2c352591f281c)

### New Contributors

- @hitblast made their first contribution in [#4847](https://github.com/jdx/mise/pull/4847)

## [2025.4.4](https://github.com/jdx/mise/compare/v2025.4.3..v2025.4.4) - 2025-04-15

### 🧪 Testing

- remove kpt test by [@jdx](https://github.com/jdx) in [b9d35ac](https://github.com/jdx/mise/commit/b9d35ac57936291a0a4629f9c200dfdb500a7efb)

## [2025.4.3](https://github.com/jdx/mise/compare/v2025.4.2..v2025.4.3) - 2025-04-15

### 🚀 Features

- **(aqua)** support SLSA source_uri setting by [@scop](https://github.com/scop) in [#4833](https://github.com/jdx/mise/pull/4833)
- **(aqua)** use source tag in SLSA verification by [@scop](https://github.com/scop) in [#4836](https://github.com/jdx/mise/pull/4836)
- **(ubi)** add support for self-hosted GitHub/GitLab by [@roele](https://github.com/roele) in [#4765](https://github.com/jdx/mise/pull/4765)

### 📚 Documentation

- Update configuration.md by [@jdx](https://github.com/jdx) in [#4829](https://github.com/jdx/mise/pull/4829)
- correct `mise use` paths by [@jdx](https://github.com/jdx) in [c8374c0](https://github.com/jdx/mise/commit/c8374c00ca68e5722c28f9abfd2425b9722bdd83)

## [2025.4.2](https://github.com/jdx/mise/compare/v2025.4.1..v2025.4.2) - 2025-04-11

### 🚀 Features

- **(registry)** update aws-nuke backend by [@StingRayZA](https://github.com/StingRayZA) in [#4815](https://github.com/jdx/mise/pull/4815)

### 🐛 Bug Fixes

- do not default to writing to mise.$MISE_ENV.toml by [@jdx](https://github.com/jdx) in [#4817](https://github.com/jdx/mise/pull/4817)
- mise watch forward --exts and --filter to watchexec by [@cmhms](https://github.com/cmhms) in [#4826](https://github.com/jdx/mise/pull/4826)

### 📚 Documentation

- Fixing typo in code for flags in toml-tasks.md by [@arafays](https://github.com/arafays) in [#4820](https://github.com/jdx/mise/pull/4820)
- branding by [@jdx](https://github.com/jdx) in [9ad2c17](https://github.com/jdx/mise/commit/9ad2c17ec75b7460ebea09a9f0601a561349cc7f)
- remove references to not-working docker: tasks by [@jdx](https://github.com/jdx) in [2c2fd27](https://github.com/jdx/mise/commit/2c2fd272e3d76329a7c67e4070bfb122ae1e1120)
- document some dependencies by [@jdx](https://github.com/jdx) in [6e8bd51](https://github.com/jdx/mise/commit/6e8bd518757c5e49624fc2bef5777a2f2339c304)
- simplify mise.toml example by [@jdx](https://github.com/jdx) in [66d927b](https://github.com/jdx/mise/commit/66d927ba4db81ba70de261cd76e399e9f4fe35da)

### 📦️ Dependency Updates

- update dependency vitepress-plugin-tabs to ^0.7.0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#4822](https://github.com/jdx/mise/pull/4822)
- update rust crate petgraph to 0.8 by [@renovate[bot]](https://github.com/renovate[bot]) in [#4823](https://github.com/jdx/mise/pull/4823)
- update rust crate strum to 0.27 by [@renovate[bot]](https://github.com/renovate[bot]) in [#4780](https://github.com/jdx/mise/pull/4780)

### New Contributors

- @cmhms made their first contribution in [#4826](https://github.com/jdx/mise/pull/4826)
- @StingRayZA made their first contribution in [#4815](https://github.com/jdx/mise/pull/4815)

## [2025.4.1](https://github.com/jdx/mise/compare/v2025.4.0..v2025.4.1) - 2025-04-09

### 🚀 Features

- **(registry)** added localstack by [@mnm364](https://github.com/mnm364) in [#4785](https://github.com/jdx/mise/pull/4785)
- **(registry)** added skeema by [@mnm364](https://github.com/mnm364) in [#4786](https://github.com/jdx/mise/pull/4786)
- **(registry)** add television by [@mangkoran](https://github.com/mangkoran) in [#4778](https://github.com/jdx/mise/pull/4778)

### 🐛 Bug Fixes

- show gh rate limit reset time in local time by [@someoneinjd](https://github.com/someoneinjd) in [#4799](https://github.com/jdx/mise/pull/4799)

### 📚 Documentation

- all experimental note for lockfile by [@zeitlinger](https://github.com/zeitlinger) in [#4781](https://github.com/jdx/mise/pull/4781)
- Include post about Mise secrets in the context of Swift app dev by [@pepicrft](https://github.com/pepicrft) in [#4809](https://github.com/jdx/mise/pull/4809)

### Chore

- update deps to fix deny check by [@jdx](https://github.com/jdx) in [432023b](https://github.com/jdx/mise/commit/432023b2cd04d2ea7f590d7b338054944512abd0)
- pin zip to avoid issue with ubi by [@jdx](https://github.com/jdx) in [315deb4](https://github.com/jdx/mise/commit/315deb4e24177408c598d22951adb95f3e841683)

### New Contributors

- @someoneinjd made their first contribution in [#4799](https://github.com/jdx/mise/pull/4799)
- @mnm364 made their first contribution in [#4786](https://github.com/jdx/mise/pull/4786)
- @zeitlinger made their first contribution in [#4781](https://github.com/jdx/mise/pull/4781)

## [2025.4.0](https://github.com/jdx/mise/compare/v2025.3.11..v2025.4.0) - 2025-04-02

### 🐛 Bug Fixes

- s/runtimes/tools by [@jdx](https://github.com/jdx) in [#4754](https://github.com/jdx/mise/pull/4754)
- add clarification on RUSTUP_HOME and CARGO_HOME by [@lachieh](https://github.com/lachieh) in [#4759](https://github.com/jdx/mise/pull/4759)
- enhance confirmation logic to respect SETTINGS.yes by [@roele](https://github.com/roele) in [#4764](https://github.com/jdx/mise/pull/4764)

### 🚜 Refactor

- **(registry)** use aqua for ubi by [@scop](https://github.com/scop) in [#4745](https://github.com/jdx/mise/pull/4745)
- **(registry)** use aqua for ksops by [@scop](https://github.com/scop) in [#4746](https://github.com/jdx/mise/pull/4746)

### 📚 Documentation

- mark code block for dnf5 install as shell code by [@sina-hide](https://github.com/sina-hide) in [#4747](https://github.com/jdx/mise/pull/4747)
- update demo by [@hverlin](https://github.com/hverlin) in [#4350](https://github.com/jdx/mise/pull/4350)
- move demo to top-level by [@jdx](https://github.com/jdx) in [2b6f45a](https://github.com/jdx/mise/commit/2b6f45ac73d6f59542f9c7b401042ad5c75e37e2)
- Update config.ts by [@jdx](https://github.com/jdx) in [05ad4bc](https://github.com/jdx/mise/commit/05ad4bc9b2243737c0551fd36de1e37dc57ea578)
- Update walkthrough.md by [@jdx](https://github.com/jdx) in [89904b4](https://github.com/jdx/mise/commit/89904b46d8649a66bf960b1e5c7c0364dad8f94f)
- Update index.md by [@jdx](https://github.com/jdx) in [#4750](https://github.com/jdx/mise/pull/4750)
- Update walkthrough.md by [@jdx](https://github.com/jdx) in [#4751](https://github.com/jdx/mise/pull/4751)
- Update README.md by [@jdx](https://github.com/jdx) in [4f38142](https://github.com/jdx/mise/commit/4f38142bd3d822c3eafd78a74aa7a8d31791d2e3)

### New Contributors

- @lachieh made their first contribution in [#4759](https://github.com/jdx/mise/pull/4759)
- @sina-hide made their first contribution in [#4747](https://github.com/jdx/mise/pull/4747)

## [2025.3.11](https://github.com/jdx/mise/compare/v2025.3.10..v2025.3.11) - 2025-03-28

### 🚀 Features

- **(registry)** add protoc-gen-validate by [@akanter](https://github.com/akanter) in [#4703](https://github.com/jdx/mise/pull/4703)

### 🚜 Refactor

- **(registry)** use aqua for swiftlint by [@scop](https://github.com/scop) in [#4726](https://github.com/jdx/mise/pull/4726)
- **(registry)** use ubi for opensearch-cli by [@scop](https://github.com/scop) in [#4725](https://github.com/jdx/mise/pull/4725)
- **(registry)** use ubi for mdbook-linkcheck by [@scop](https://github.com/scop) in [#4724](https://github.com/jdx/mise/pull/4724)
- **(registry)** use ubi for velad by [@scop](https://github.com/scop) in [#4727](https://github.com/jdx/mise/pull/4727)

## [2025.3.10](https://github.com/jdx/mise/compare/v2025.3.9..v2025.3.10) - 2025-03-26

### ◀️ Revert

- Revert "chore: make awscli compatible with R2" by [@jdx](https://github.com/jdx) in [83e8c16](https://github.com/jdx/mise/commit/83e8c164ec78cab4325b4489d9cc5d1fa466ec3f)

## [2025.3.9](https://github.com/jdx/mise/compare/v2025.3.8..v2025.3.9) - 2025-03-26

### 🚀 Features

- Set usage arguments and flag as environment variables before running the command by [@gturi](https://github.com/gturi) in [#4700](https://github.com/jdx/mise/pull/4700)

### 🚜 Refactor

- **(registry)** use ubi for assh by [@scop](https://github.com/scop) in [#4713](https://github.com/jdx/mise/pull/4713)
- **(registry)** use ubi for opsgenie-lamp by [@scop](https://github.com/scop) in [#4712](https://github.com/jdx/mise/pull/4712)
- **(registry)** use ubi for auto-doc by [@scop](https://github.com/scop) in [#4714](https://github.com/jdx/mise/pull/4714)
- **(registry)** use ubi for getenvoy by [@scop](https://github.com/scop) in [#4715](https://github.com/jdx/mise/pull/4715)
- **(registry)** use ubi for mockolo by [@scop](https://github.com/scop) in [#4705](https://github.com/jdx/mise/pull/4705)
- **(registry)** use ubi for haxe by [@scop](https://github.com/scop) in [#4716](https://github.com/jdx/mise/pull/4716)
- **(registry)** use ubi for helm-diff by [@scop](https://github.com/scop) in [#4717](https://github.com/jdx/mise/pull/4717)
- **(registry)** use ubi for grain by [@scop](https://github.com/scop) in [#4718](https://github.com/jdx/mise/pull/4718)

## [2025.3.8](https://github.com/jdx/mise/compare/v2025.3.7..v2025.3.8) - 2025-03-24

### 🚀 Features

- **(registry)** add aichat by [@kit494way](https://github.com/kit494way) in [#4691](https://github.com/jdx/mise/pull/4691)

### 🐛 Bug Fixes

- Update flake to fix nix build by [@akanter](https://github.com/akanter) in [#4686](https://github.com/jdx/mise/pull/4686)

### 📚 Documentation

- fix bash completion setup instructions by [@bestagi](https://github.com/bestagi) in [#3920](https://github.com/jdx/mise/pull/3920)
- small tidy of shims docs by [@AlecRust](https://github.com/AlecRust) in [#4693](https://github.com/jdx/mise/pull/4693)

### Chore

- remove broken ripsecrets test by [@jdx](https://github.com/jdx) in [bb382aa](https://github.com/jdx/mise/commit/bb382aa783a2a1bfc44f02a5bb34f9397efb2e57)
- make awscli compatible with R2 by [@jdx](https://github.com/jdx) in [cad7fa2](https://github.com/jdx/mise/commit/cad7fa285e96483ba8d6aeb22f83de10e92700b2)
- enable workflow_dispatch for docs task by [@jdx](https://github.com/jdx) in [b0578db](https://github.com/jdx/mise/commit/b0578db141decc63992ebb0f74e29a53238611ba)

### New Contributors

- @akanter made their first contribution in [#4686](https://github.com/jdx/mise/pull/4686)
- @bestagi made their first contribution in [#3920](https://github.com/jdx/mise/pull/3920)

## [2025.3.7](https://github.com/jdx/mise/compare/v2025.3.6..v2025.3.7) - 2025-03-21

### 🐛 Bug Fixes

- **(node)** skip gpg verification of sig file not found by [@jdx](https://github.com/jdx) in [#4663](https://github.com/jdx/mise/pull/4663)
- **(task)** allow args to be used with tera tests by [@risu729](https://github.com/risu729) in [#4605](https://github.com/jdx/mise/pull/4605)
- Fix syntax error on `activate nu` when PATH contains shims by [@atty303](https://github.com/atty303) in [#4349](https://github.com/jdx/mise/pull/4349)

### 🚜 Refactor

- **(registry)** use ubi for yamlscript by [@scop](https://github.com/scop) in [#4670](https://github.com/jdx/mise/pull/4670)

### 📚 Documentation

- Fix typo in java.md by [@hverlin](https://github.com/hverlin) in [#4672](https://github.com/jdx/mise/pull/4672)

### ◀️ Revert

- "chore: temporarily disable bootstrap test" by [@jdx](https://github.com/jdx) in [#4658](https://github.com/jdx/mise/pull/4658)

### 📦️ Dependency Updates

- update rust crate ctor to 0.4 by [@renovate[bot]](https://github.com/renovate[bot]) in [#4553](https://github.com/jdx/mise/pull/4553)

### Chore

- **(registry)** declare copier by [@looztra](https://github.com/looztra) in [#4669](https://github.com/jdx/mise/pull/4669)
- Update to the latest version of ubi by [@autarch](https://github.com/autarch) in [#4648](https://github.com/jdx/mise/pull/4648)
- bump expr by [@jdx](https://github.com/jdx) in [#4666](https://github.com/jdx/mise/pull/4666)
- added android-sdk by [@jdx](https://github.com/jdx) in [#4668](https://github.com/jdx/mise/pull/4668)
- rename mise-php to asdf-php by [@jdx](https://github.com/jdx) in [#4674](https://github.com/jdx/mise/pull/4674)

### New Contributors

- @atty303 made their first contribution in [#4349](https://github.com/jdx/mise/pull/4349)
- @looztra made their first contribution in [#4669](https://github.com/jdx/mise/pull/4669)

## [2025.3.6](https://github.com/jdx/mise/compare/v2025.3.5..v2025.3.6) - 2025-03-18

### Chore

- unpin aws-cli by [@jdx](https://github.com/jdx) in [7fabed5](https://github.com/jdx/mise/commit/7fabed5c70fccfe095647c7b2220965ca2f1c07d)
- temporarily disable bootstrap test by [@jdx](https://github.com/jdx) in [599258a](https://github.com/jdx/mise/commit/599258aa4f5c0ab0b5581740b0c9eec17f1c7318)

## [2025.3.5](https://github.com/jdx/mise/compare/v2025.3.4..v2025.3.5) - 2025-03-18

### 🚀 Features

- **(registry)** use ubi for glab by [@scop](https://github.com/scop) in [#4643](https://github.com/jdx/mise/pull/4643)
- ubi forge option support by [@scop](https://github.com/scop) in [#4642](https://github.com/jdx/mise/pull/4642)

### 🐛 Bug Fixes

- **(tera)** use default inline shell to parse exec template by [@risu729](https://github.com/risu729) in [#4645](https://github.com/jdx/mise/pull/4645)

## [2025.3.4](https://github.com/jdx/mise/compare/v2025.3.3..v2025.3.4) - 2025-03-18

### 🐛 Bug Fixes

- Failed to create venv at the same time by multiple uv processes by [@NavyD](https://github.com/NavyD) in [#4640](https://github.com/jdx/mise/pull/4640)

## [2025.3.3](https://github.com/jdx/mise/compare/v2025.3.2..v2025.3.3) - 2025-03-14

### 🚀 Features

- **(env)** support env files in toml by [@risu729](https://github.com/risu729) in [#4618](https://github.com/jdx/mise/pull/4618)
- **(registry)** add harper-ls and harper-cli by [@kit494way](https://github.com/kit494way) in [#4615](https://github.com/jdx/mise/pull/4615)
- **(registry)** add curlie by [@reitzig](https://github.com/reitzig) in [#4599](https://github.com/jdx/mise/pull/4599)
- cleanup the mutex use. by [@boris-smidt-klarrio](https://github.com/boris-smidt-klarrio) in [#4540](https://github.com/jdx/mise/pull/4540)
- Add flag to fmt command to read from stdin by [@erickgnavar](https://github.com/erickgnavar) in [#4594](https://github.com/jdx/mise/pull/4594)

### 🐛 Bug Fixes

- **(uv)** avoid deadlocks while initializing UV_VENV by [@risu729](https://github.com/risu729) in [#4609](https://github.com/jdx/mise/pull/4609)
- handle error when getting modified duration in file::modified_duration by [@roele](https://github.com/roele) in [#4624](https://github.com/jdx/mise/pull/4624)
- SwiftPM backend not working with the Swift 6 toolchain by [@pepicrft](https://github.com/pepicrft) in [#4632](https://github.com/jdx/mise/pull/4632)
- quiet in file task not working by [@roele](https://github.com/roele) in [#4588](https://github.com/jdx/mise/pull/4588)
- Unable to find uv when first creating py venv by [@NavyD](https://github.com/NavyD) in [#4591](https://github.com/jdx/mise/pull/4591)

### 🚜 Refactor

- migrate humantime to jiff by [@risu729](https://github.com/risu729) in [#4616](https://github.com/jdx/mise/pull/4616)
- use method to get the default inline shell instead of accessing the fields by [@risu729](https://github.com/risu729) in [#4621](https://github.com/jdx/mise/pull/4621)

### 📚 Documentation

- **(settings)** clarify the usage of disable_default_registry by [@gbloquel](https://github.com/gbloquel) in [#4589](https://github.com/jdx/mise/pull/4589)

### ⚡ Performance

- speed up self-update by calling /releases/latest api instead of /releases by [@vemoo](https://github.com/vemoo) in [#4619](https://github.com/jdx/mise/pull/4619)

### 🧪 Testing

- **(registry)** fix test of lazyjournal by [@risu729](https://github.com/risu729) in [#4610](https://github.com/jdx/mise/pull/4610)

### Chore

- deny fixes by [@jdx](https://github.com/jdx) in [17d7c6e](https://github.com/jdx/mise/commit/17d7c6ee5e035272a8dc1b93c8fc7ac9cffb7f80)
- ignore humantime unmaintained advisory by [@risu729](https://github.com/risu729) in [#4612](https://github.com/jdx/mise/pull/4612)
- remove rustup update in github actions by [@risu729](https://github.com/risu729) in [#4617](https://github.com/jdx/mise/pull/4617)

### New Contributors

- @erickgnavar made their first contribution in [#4594](https://github.com/jdx/mise/pull/4594)
- @vemoo made their first contribution in [#4619](https://github.com/jdx/mise/pull/4619)
- @gbloquel made their first contribution in [#4589](https://github.com/jdx/mise/pull/4589)

## [2025.3.1](https://github.com/jdx/mise/compare/v2025.3.0..v2025.3.1) - 2025-03-06

### 🚀 Features

- **(registry)** added sampler by [@tony-sol](https://github.com/tony-sol) in [#4577](https://github.com/jdx/mise/pull/4577)
- **(registry)** added lazyjournal by [@tony-sol](https://github.com/tony-sol) in [#4584](https://github.com/jdx/mise/pull/4584)
- add support for components property in rust-toolchain.toml by [@roele](https://github.com/roele) in [#4579](https://github.com/jdx/mise/pull/4579)
- add --local flag for ls by [@tony-sol](https://github.com/tony-sol) in [#4565](https://github.com/jdx/mise/pull/4565)

### 🐛 Bug Fixes

- favor aqua backend over asdf by [@dud225](https://github.com/dud225) in [#4558](https://github.com/jdx/mise/pull/4558)

### 📚 Documentation

- continuous-integration.md: fix gitlab caching example by [@nafg](https://github.com/nafg) in [#4576](https://github.com/jdx/mise/pull/4576)

### Chore

- edition 2024 by [@jdx](https://github.com/jdx) in [#4541](https://github.com/jdx/mise/pull/4541)

### New Contributors

- @nafg made their first contribution in [#4576](https://github.com/jdx/mise/pull/4576)
- @dud225 made their first contribution in [#4558](https://github.com/jdx/mise/pull/4558)

## [2025.3.0](https://github.com/jdx/mise/compare/v2025.2.9..v2025.3.0) - 2025-03-01

### 🚀 Features

- **(registry)** added helmwave by [@tony-sol](https://github.com/tony-sol) in [#4542](https://github.com/jdx/mise/pull/4542)
- **(registry)** added doggo by [@tony-sol](https://github.com/tony-sol) in [#4545](https://github.com/jdx/mise/pull/4545)
- **(registry)** Add Boilerplate by [@ZachGoldberg](https://github.com/ZachGoldberg) in [#4530](https://github.com/jdx/mise/pull/4530)
- **(registry)** added htmlq by [@tony-sol](https://github.com/tony-sol) in [#4548](https://github.com/jdx/mise/pull/4548)
- **(registry)** added gokey by [@tony-sol](https://github.com/tony-sol) in [#4546](https://github.com/jdx/mise/pull/4546)
- **(registry)** added octosql by [@tony-sol](https://github.com/tony-sol) in [#4549](https://github.com/jdx/mise/pull/4549)
- **(registry)** added hexyl by [@tony-sol](https://github.com/tony-sol) in [#4547](https://github.com/jdx/mise/pull/4547)
- **(registry)** added kubeone by [@tony-sol](https://github.com/tony-sol) in [#4550](https://github.com/jdx/mise/pull/4550)
- task confirmation by [@roele](https://github.com/roele) in [#4328](https://github.com/jdx/mise/pull/4328)

### 🐛 Bug Fixes

- remote tasks and devcontainer by [@acesyde](https://github.com/acesyde) in [#4557](https://github.com/jdx/mise/pull/4557)

### 📚 Documentation

- **(shim)** add faq for vscode windows spawn EINVAL & format value to list by [@qianlongzt](https://github.com/qianlongzt) in [#4544](https://github.com/jdx/mise/pull/4544)

### New Contributors

- @ZachGoldberg made their first contribution in [#4530](https://github.com/jdx/mise/pull/4530)

## [2025.2.9](https://github.com/jdx/mise/compare/v2025.2.8..v2025.2.9) - 2025-02-26

### 🚀 Features

- **(registry)** add cocogitto by [@reitzig](https://github.com/reitzig) in [#4513](https://github.com/jdx/mise/pull/4513)
- **(registry)** Added foundry by [@suicide](https://github.com/suicide) in [#4455](https://github.com/jdx/mise/pull/4455)
- **(registry)** added ast-grep by [@tony-sol](https://github.com/tony-sol) in [#4519](https://github.com/jdx/mise/pull/4519)

### 🐛 Bug Fixes

- non-utf8 external process handling by [@jdx](https://github.com/jdx) in [#4538](https://github.com/jdx/mise/pull/4538)

### 📚 Documentation

- **(cookbook)** add shell powerline-go config env recipe by [@scop](https://github.com/scop) in [#4532](https://github.com/jdx/mise/pull/4532)
- update mise.el repo link by [@tecoholic](https://github.com/tecoholic) in [#4534](https://github.com/jdx/mise/pull/4534)

### Chore

- bump rust version for releases by [@jdx](https://github.com/jdx) in [f4e5970](https://github.com/jdx/mise/commit/f4e5970f00bf56d9be16a7e7e83289085c0e5cce)
- bump rust version for releases by [@jdx](https://github.com/jdx) in [52cff1c](https://github.com/jdx/mise/commit/52cff1c00b452b93b3ca1e4fc01fd21de73569e5)
- bump rust version for releases by [@jdx](https://github.com/jdx) in [9121c5e](https://github.com/jdx/mise/commit/9121c5e9270fae59ce753226ecbbe2939c4661e4)
- bump msrv for edition compatibility by [@jdx](https://github.com/jdx) in [3a222dd](https://github.com/jdx/mise/commit/3a222ddf272eef655b50796f34634fcedc3f1288)
- remove unused deny rule by [@jdx](https://github.com/jdx) in [053f5c1](https://github.com/jdx/mise/commit/053f5c1c0746e363c24b19577b958621ea91c40c)

### New Contributors

- @tony-sol made their first contribution in [#4519](https://github.com/jdx/mise/pull/4519)
- @tecoholic made their first contribution in [#4534](https://github.com/jdx/mise/pull/4534)
- @suicide made their first contribution in [#4455](https://github.com/jdx/mise/pull/4455)
- @reitzig made their first contribution in [#4513](https://github.com/jdx/mise/pull/4513)

## [2025.2.8](https://github.com/jdx/mise/compare/v2025.2.7..v2025.2.8) - 2025-02-25

### 🚀 Features

- **(registry)** add checkmake to registry by [@eread](https://github.com/eread) in [#4466](https://github.com/jdx/mise/pull/4466)
- **(registry)** added sops from aqua registry by [@ldrouard](https://github.com/ldrouard) in [#4457](https://github.com/jdx/mise/pull/4457)
- **(registry)** added k9s from aqua registry by [@ldrouard](https://github.com/ldrouard) in [#4460](https://github.com/jdx/mise/pull/4460)
- **(registry)** added hadolint from aqua registry by [@ldrouard](https://github.com/ldrouard) in [#4456](https://github.com/jdx/mise/pull/4456)
- **(shim)** Windows shim add hardlink & symlink mode by [@qianlongzt](https://github.com/qianlongzt) in [#4409](https://github.com/jdx/mise/pull/4409)
- **(ubi)** add option `rename_exe` by [@wlmitch](https://github.com/wlmitch) in [#4512](https://github.com/jdx/mise/pull/4512)
- use aqua for hk by [@jdx](https://github.com/jdx) in [f68de38](https://github.com/jdx/mise/commit/f68de3849c5ceb20475f2f30224abaa5f3f7441d)
- add bazel-watcher to registry by [@betaboon](https://github.com/betaboon) in [#4296](https://github.com/jdx/mise/pull/4296)

### 🐛 Bug Fixes

- behavior of .disable-self-update by [@ZeroAurora](https://github.com/ZeroAurora) in [#4476](https://github.com/jdx/mise/pull/4476)
- devcontainer by [@acesyde](https://github.com/acesyde) in [#4483](https://github.com/jdx/mise/pull/4483)
- mise outdated --json does not return json if all tools are up-to-date by [@roele](https://github.com/roele) in [#4493](https://github.com/jdx/mise/pull/4493)
- bug when using mise use -g when MISE_ENV is filled by [@roele](https://github.com/roele) in [#4494](https://github.com/jdx/mise/pull/4494)
- config of symlink tracked on windows is not respected by [@NavyD](https://github.com/NavyD) in [#4501](https://github.com/jdx/mise/pull/4501)
- pruning unused tool leaves broken symlinks by [@roele](https://github.com/roele) in [#4507](https://github.com/jdx/mise/pull/4507)

### 📚 Documentation

- Fixes typo in lang/zig by [@carldaws](https://github.com/carldaws) in [#4497](https://github.com/jdx/mise/pull/4497)
- Fix activation on PowerShell by [@kit494way](https://github.com/kit494way) in [#4498](https://github.com/jdx/mise/pull/4498)

### Chore

- remove aur job by [@jdx](https://github.com/jdx) in [fe5a71d](https://github.com/jdx/mise/commit/fe5a71dc486e6e585167d9d97018f2b467bc43fe)
- remove reference to aur in release script by [@jdx](https://github.com/jdx) in [0824490](https://github.com/jdx/mise/commit/0824490c14d17cd93c7d68930b514eb11635c451)
- deny ring sec by [@jdx](https://github.com/jdx) in [08e334c](https://github.com/jdx/mise/commit/08e334cb1209471d9c18b289473925ff0931053f)

### New Contributors

- @betaboon made their first contribution in [#4296](https://github.com/jdx/mise/pull/4296)
- @ldrouard made their first contribution in [#4456](https://github.com/jdx/mise/pull/4456)
- @qianlongzt made their first contribution in [#4409](https://github.com/jdx/mise/pull/4409)
- @wlmitch made their first contribution in [#4512](https://github.com/jdx/mise/pull/4512)
- @carldaws made their first contribution in [#4497](https://github.com/jdx/mise/pull/4497)
- @ZeroAurora made their first contribution in [#4476](https://github.com/jdx/mise/pull/4476)

## [2025.2.7](https://github.com/jdx/mise/compare/v2025.2.6..v2025.2.7) - 2025-02-19

### 🚀 Features

- **(registry)** add lychee to registry by [@eread](https://github.com/eread) in [#4181](https://github.com/jdx/mise/pull/4181)
- Install latest nominated zig from https://machengine.org/zig/index.json by [@tamadamas](https://github.com/tamadamas) in [#4451](https://github.com/jdx/mise/pull/4451)

### 🐛 Bug Fixes

- **(cli/run)** inherit stdio by --raw even when redactions are enabled by [@risu729](https://github.com/risu729) in [#4446](https://github.com/jdx/mise/pull/4446)
- **(task)** Running programs on windows without cmd.exe by [@NavyD](https://github.com/NavyD) in [#4459](https://github.com/jdx/mise/pull/4459)
- bugs with grep in tar_supports_zstd in mise.run script by [@glasser](https://github.com/glasser) in [#4453](https://github.com/jdx/mise/pull/4453)

### 📚 Documentation

- fix watch files hook example by [@rsyring](https://github.com/rsyring) in [#4427](https://github.com/jdx/mise/pull/4427)
- Fix run-on sentence by [@henrebotha](https://github.com/henrebotha) in [#4429](https://github.com/jdx/mise/pull/4429)
- mention hk by [@jdx](https://github.com/jdx) in [1a58e86](https://github.com/jdx/mise/commit/1a58e86ce2ce16d848755df8feccf514000053fd)
- discord link by [@jdx](https://github.com/jdx) in [b586085](https://github.com/jdx/mise/commit/b58608521cccee812adaa642145f061ccbcbac43)
- Add a section on how to use environment variables by [@hverlin](https://github.com/hverlin) in [#4435](https://github.com/jdx/mise/pull/4435)
- Update installation for archLinux by [@Nicknamely](https://github.com/Nicknamely) in [#4449](https://github.com/jdx/mise/pull/4449)
- Fix typo in getting-started by [@alefteris](https://github.com/alefteris) in [#4448](https://github.com/jdx/mise/pull/4448)

### 🧪 Testing

- always set experimental = true in tests by [@jdx](https://github.com/jdx) in [#4443](https://github.com/jdx/mise/pull/4443)

### Chore

- fixed new clippy lints by [@jdx](https://github.com/jdx) in [#4463](https://github.com/jdx/mise/pull/4463)

### New Contributors

- @alefteris made their first contribution in [#4448](https://github.com/jdx/mise/pull/4448)
- @tamadamas made their first contribution in [#4451](https://github.com/jdx/mise/pull/4451)
- @Nicknamely made their first contribution in [#4449](https://github.com/jdx/mise/pull/4449)
- @eread made their first contribution in [#4181](https://github.com/jdx/mise/pull/4181)
- @rsyring made their first contribution in [#4427](https://github.com/jdx/mise/pull/4427)

## [2025.2.6](https://github.com/jdx/mise/compare/v2025.2.5..v2025.2.6) - 2025-02-16

### 🚀 Features

- add devcontainer generator by [@acesyde](https://github.com/acesyde) in [#4355](https://github.com/jdx/mise/pull/4355)
- added hk by [@jdx](https://github.com/jdx) in [#4422](https://github.com/jdx/mise/pull/4422)

### 🐛 Bug Fixes

- short flag with value and var=#true bug by [@jdx](https://github.com/jdx) in [#4419](https://github.com/jdx/mise/pull/4419)
- regression with env overriding by [@jdx](https://github.com/jdx) in [#4421](https://github.com/jdx/mise/pull/4421)

### 📚 Documentation

- **(shims)** clarify `activate` only removes shims from `PATH` by [@risu729](https://github.com/risu729) in [#4418](https://github.com/jdx/mise/pull/4418)
- Update shims page by [@hverlin](https://github.com/hverlin) in [#4414](https://github.com/jdx/mise/pull/4414)

## [2025.2.5](https://github.com/jdx/mise/compare/v2025.2.4..v2025.2.5) - 2025-02-16

### 🐛 Bug Fixes

- properly replace non set flags with "false" by [@IxDay](https://github.com/IxDay) in [#4410](https://github.com/jdx/mise/pull/4410)
- path env order with subdirs by [@jdx](https://github.com/jdx) in [#4412](https://github.com/jdx/mise/pull/4412)

### ◀️ Revert

- "feat: set usage arguments and flags as environment variables for toml tasks" by [@jdx](https://github.com/jdx) in [#4413](https://github.com/jdx/mise/pull/4413)

## [2025.2.4](https://github.com/jdx/mise/compare/v2025.2.3..v2025.2.4) - 2025-02-14

### 🚀 Features

- **(registry)** add e1s by [@kiwamizamurai](https://github.com/kiwamizamurai) in [#4363](https://github.com/jdx/mise/pull/4363)
- **(registry)** add 'marksman' via 'aqua:artempyanykh/marksman' backend by [@iamoeg](https://github.com/iamoeg) in [#4357](https://github.com/jdx/mise/pull/4357)
- use `machengine.org` for downloading nominated zig versions by [@hadronomy](https://github.com/hadronomy) in [#4356](https://github.com/jdx/mise/pull/4356)

### 🐛 Bug Fixes

- **(aqua)** apply override of version_prefix by [@risu729](https://github.com/risu729) in [#4338](https://github.com/jdx/mise/pull/4338)
- **(env_directive)** apply redactions only to env with redact by [@risu729](https://github.com/risu729) in [#4388](https://github.com/jdx/mise/pull/4388)
- **(hook_env)** don't exit early if watching files are deleted by [@risu729](https://github.com/risu729) in [#4390](https://github.com/jdx/mise/pull/4390)
- **(rubygems_plugin)** Replace which ruby check for Windows compatibility by [@genskyff](https://github.com/genskyff) in [#4358](https://github.com/jdx/mise/pull/4358)
- lowercase desired shim names by [@KevSlashNull](https://github.com/KevSlashNull) in [#4333](https://github.com/jdx/mise/pull/4333)
- allow cosign opts to be empty in aqua by [@IxDay](https://github.com/IxDay) in [#4396](https://github.com/jdx/mise/pull/4396)

### 📚 Documentation

- update Fedora install for dnf5 by [@rkben](https://github.com/rkben) in [#4387](https://github.com/jdx/mise/pull/4387)
- fix links to idiomatic version file option by [@pietrodn](https://github.com/pietrodn) in [#4382](https://github.com/jdx/mise/pull/4382)
- add mise bootstrap example in CI docs by [@hverlin](https://github.com/hverlin) in [#4351](https://github.com/jdx/mise/pull/4351)
- Update link in comparison-to-asdf.md by [@hverlin](https://github.com/hverlin) in [#4401](https://github.com/jdx/mise/pull/4401)

### 📦️ Dependency Updates

- update rust crate bzip2 to v0.5.1 by [@renovate[bot]](https://github.com/renovate[bot]) in [#4392](https://github.com/jdx/mise/pull/4392)
- update rust crate built to v0.7.6 by [@renovate[bot]](https://github.com/renovate[bot]) in [#4391](https://github.com/jdx/mise/pull/4391)

### Chore

- issue closer by [@jdx](https://github.com/jdx) in [bee1f55](https://github.com/jdx/mise/commit/bee1f5557b829b9a637a28af90b519fdfa74b8dd)

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

- Revert "feat: add support for idiomatic go.mod file " by [@jdx](https://github.com/jdx) in [7fc9beb](https://github.com/jdx/mise/commit/7fc9bebd02abfee4b622a211b86c516df9bd4f6d)

## [2025.2.2](https://github.com/jdx/mise/compare/v2025.2.1..v2025.2.2) - 2025-02-08

### 🚀 Features

- **(registry)** add jd by [@risu729](https://github.com/risu729) in [#4318](https://github.com/jdx/mise/pull/4318)
- **(registry)** add jc by [@risu729](https://github.com/risu729) in [#4317](https://github.com/jdx/mise/pull/4317)
- **(registry)** Add qsv cli by [@vjda](https://github.com/vjda) in [#4334](https://github.com/jdx/mise/pull/4334)
- add support for idiomatic go.mod file by [@roele](https://github.com/roele) in [#4312](https://github.com/jdx/mise/pull/4312)
- add -g short version for unuse cmd by [@kimle](https://github.com/kimle) in [#4330](https://github.com/jdx/mise/pull/4330)
- add git remote task provider by [@acesyde](https://github.com/acesyde) in [#4233](https://github.com/jdx/mise/pull/4233)
- set usage arguments and flags as environment variables for toml tasks by [@gturi](https://github.com/gturi) in [#4159](https://github.com/jdx/mise/pull/4159)

### 🐛 Bug Fixes

- **(aqua)** trim prefix before comparing versions by [@risu729](https://github.com/risu729) in [#4340](https://github.com/jdx/mise/pull/4340)
- wrong config file type for rust-toolchain.toml files by [@roele](https://github.com/roele) in [#4321](https://github.com/jdx/mise/pull/4321)

### 🚜 Refactor

- **(registry)** use aqua for yq by [@scop](https://github.com/scop) in [#4326](https://github.com/jdx/mise/pull/4326)

### 📚 Documentation

- **(schema)** fix description of task.dir default by [@risu729](https://github.com/risu729) in [#4324](https://github.com/jdx/mise/pull/4324)
- Add PowerShell example by [@jahanson](https://github.com/jahanson) in [#3857](https://github.com/jdx/mise/pull/3857)
- Include "A Mise guide for Swift developers" by [@pepicrft](https://github.com/pepicrft) in [#4329](https://github.com/jdx/mise/pull/4329)
- Update documentation for core tools by [@hverlin](https://github.com/hverlin) in [#4341](https://github.com/jdx/mise/pull/4341)
- Update vitepress to fix search by [@hverlin](https://github.com/hverlin) in [#4342](https://github.com/jdx/mise/pull/4342)

### Chore

- **(bun.lock)** migrate bun lockfiles to text-based by [@risu729](https://github.com/risu729) in [#4319](https://github.com/jdx/mise/pull/4319)

### New Contributors

- @vjda made their first contribution in [#4334](https://github.com/jdx/mise/pull/4334)
- @kimle made their first contribution in [#4330](https://github.com/jdx/mise/pull/4330)
- @pepicrft made their first contribution in [#4329](https://github.com/jdx/mise/pull/4329)
- @jahanson made their first contribution in [#3857](https://github.com/jdx/mise/pull/3857)

## [2025.2.1](https://github.com/jdx/mise/compare/v2025.2.0..v2025.2.1) - 2025-02-03

### Chore

- fix winget releaser job by [@jdx](https://github.com/jdx) in [e67c653](https://github.com/jdx/mise/commit/e67c653de35ff83d4ee280bf5cb2381741a2108e)

## [2025.2.0](https://github.com/jdx/mise/compare/v2025.1.17..v2025.2.0) - 2025-02-02

### 🚀 Features

- **(registry)** add kwokctl by [@mangkoran](https://github.com/mangkoran) in [#4282](https://github.com/jdx/mise/pull/4282)
- add biome to registry by [@kit494way](https://github.com/kit494way) in [#4283](https://github.com/jdx/mise/pull/4283)
- add gittool/gitversion by [@acesyde](https://github.com/acesyde) in [#4289](https://github.com/jdx/mise/pull/4289)

### 📚 Documentation

- add filtering support to registry docs page by [@roele](https://github.com/roele) in [#4285](https://github.com/jdx/mise/pull/4285)
- improve registry filtering performance by [@roele](https://github.com/roele) in [#4287](https://github.com/jdx/mise/pull/4287)
- fix registry table rendering for mobile by [@roele](https://github.com/roele) in [#4288](https://github.com/jdx/mise/pull/4288)

### Chore

- updated deps by [@jdx](https://github.com/jdx) in [#4290](https://github.com/jdx/mise/pull/4290)
- do not run autofix on renovate PRs by [@jdx](https://github.com/jdx) in [41c5ce4](https://github.com/jdx/mise/commit/41c5ce4c6581f856bf0d756e3fe99ec2fae2e7bd)

### New Contributors

- @ELLIOTTCABLE made their first contribution in [#4280](https://github.com/jdx/mise/pull/4280)

## [2025.1.17](https://github.com/jdx/mise/compare/v2025.1.16..v2025.1.17) - 2025-01-31

### 🚀 Features

- **(registry)** use aqua for duckdb by [@mangkoran](https://github.com/mangkoran) in [#4270](https://github.com/jdx/mise/pull/4270)

### 🐛 Bug Fixes

- mise does not operate well under Git Bash on Windows by [@roele](https://github.com/roele) in [#4048](https://github.com/jdx/mise/pull/4048)
- mise rm removes/reports wrong version of tool by [@roele](https://github.com/roele) in [#4272](https://github.com/jdx/mise/pull/4272)

### 📚 Documentation

- Update python documentation by [@hverlin](https://github.com/hverlin) in [#4260](https://github.com/jdx/mise/pull/4260)
- fix postinstall typo in nodejs cookbook by [@arafays](https://github.com/arafays) in [#4251](https://github.com/jdx/mise/pull/4251)
- Fix typo by [@henrebotha](https://github.com/henrebotha) in [#4277](https://github.com/jdx/mise/pull/4277)

### Hooks.md

- MISE_PROJECT_DIR -> MISE_PROJECT_ROOT by [@jubr](https://github.com/jubr) in [#4269](https://github.com/jdx/mise/pull/4269)

### New Contributors

- @mangkoran made their first contribution in [#4270](https://github.com/jdx/mise/pull/4270)
- @jubr made their first contribution in [#4269](https://github.com/jdx/mise/pull/4269)
- @arafays made their first contribution in [#4251](https://github.com/jdx/mise/pull/4251)

## [2025.1.16](https://github.com/jdx/mise/compare/v2025.1.15..v2025.1.16) - 2025-01-29

### 🚀 Features

- **(registry)** add duckdb by [@swfz](https://github.com/swfz) in [#4248](https://github.com/jdx/mise/pull/4248)

### 🐛 Bug Fixes

- Swift on Ubuntu 24.04 arm64 generates the incorrect download URL by [@spyder-ian](https://github.com/spyder-ian) in [#4235](https://github.com/jdx/mise/pull/4235)
- Do not attempt to parse directories by [@adamcohen2](https://github.com/adamcohen2) in [#4256](https://github.com/jdx/mise/pull/4256)
- path option should take precedence over global configuration by [@roele](https://github.com/roele) in [#4249](https://github.com/jdx/mise/pull/4249)

### 📚 Documentation

- Add devtools.fm episode about mise to external-resources.md by [@CanRau](https://github.com/CanRau) in [#4253](https://github.com/jdx/mise/pull/4253)
- Update sections about idiomatic version files by [@hverlin](https://github.com/hverlin) in [#4252](https://github.com/jdx/mise/pull/4252)

### Chore

- make self_update optional by [@jdx](https://github.com/jdx) in [#4230](https://github.com/jdx/mise/pull/4230)
- added some defaul reqwest features by [@jdx](https://github.com/jdx) in [#4232](https://github.com/jdx/mise/pull/4232)

### New Contributors

- @adamcohen2 made their first contribution in [#4256](https://github.com/jdx/mise/pull/4256)
- @CanRau made their first contribution in [#4253](https://github.com/jdx/mise/pull/4253)
- @spyder-ian made their first contribution in [#4235](https://github.com/jdx/mise/pull/4235)

## [2025.1.15](https://github.com/jdx/mise/compare/v2025.1.14..v2025.1.15) - 2025-01-26

### 🚀 Features

- add http cache by [@acesyde](https://github.com/acesyde) in [#4160](https://github.com/jdx/mise/pull/4160)
- expose `test-tool` command by [@jdx](https://github.com/jdx) in [#4224](https://github.com/jdx/mise/pull/4224)

### 🐛 Bug Fixes

- elixir installation failed by [@roele](https://github.com/roele) in [#4144](https://github.com/jdx/mise/pull/4144)
- re-run tasks when files removed or permissions change by [@jdx](https://github.com/jdx) in [#4223](https://github.com/jdx/mise/pull/4223)

### 🚜 Refactor

- use builder pattern by [@acesyde](https://github.com/acesyde) in [#4220](https://github.com/jdx/mise/pull/4220)

### 📚 Documentation

- **(how-i-use-mise)** switch to discussion by [@risu729](https://github.com/risu729) in [#4225](https://github.com/jdx/mise/pull/4225)
- add hint about environment variable parsing by [@roele](https://github.com/roele) in [#4219](https://github.com/jdx/mise/pull/4219)

### Chore

- added vscode workspace by [@jdx](https://github.com/jdx) in [a0d181f](https://github.com/jdx/mise/commit/a0d181f8d60270d09d06156ebc500a2fa85f74db)
- switch from git2 to gix by [@jdx](https://github.com/jdx) in [#4226](https://github.com/jdx/mise/pull/4226)
- remove git2 from built by [@jdx](https://github.com/jdx) in [#4227](https://github.com/jdx/mise/pull/4227)
- use mise-plugins/mise-jib by [@jdx](https://github.com/jdx) in [#4228](https://github.com/jdx/mise/pull/4228)

### New Contributors

- @vgnh made their first contribution in [#4216](https://github.com/jdx/mise/pull/4216)

## [2025.1.14](https://github.com/jdx/mise/compare/v2025.1.13..v2025.1.14) - 2025-01-24

### 🚀 Features

- **(registry)** add gron by [@MontakOleg](https://github.com/MontakOleg) in [#4204](https://github.com/jdx/mise/pull/4204)

### 🐛 Bug Fixes

- spurious semver warning on `mise outdated` by [@jdx](https://github.com/jdx) in [#4199](https://github.com/jdx/mise/pull/4199)

### Chore

- lint issue in Dockerfile by [@jdx](https://github.com/jdx) in [47ad5d6](https://github.com/jdx/mise/commit/47ad5d67890188478cf8c8f2e6796b6752546e6c)
- fix some typos in markdown file by [@chuangjinglu](https://github.com/chuangjinglu) in [#4198](https://github.com/jdx/mise/pull/4198)
- pin aws-cli by [@jdx](https://github.com/jdx) in [f7311fd](https://github.com/jdx/mise/commit/f7311fd8fc85b6920c5a484862865adc9ef7261d)
- use arm64 runners for docker by [@jdx](https://github.com/jdx) in [#4200](https://github.com/jdx/mise/pull/4200)

### New Contributors

- @chuangjinglu made their first contribution in [#4198](https://github.com/jdx/mise/pull/4198)

## [2025.1.13](https://github.com/jdx/mise/compare/v2025.1.12..v2025.1.13) - 2025-01-24

### Chore

- fixing aws-cli in release.sh by [@jdx](https://github.com/jdx) in [5b4a65a](https://github.com/jdx/mise/commit/5b4a65a84e07141de9ed69798921b4b0ef69aa02)
- fixing aws-cli in release.sh by [@jdx](https://github.com/jdx) in [4c67db5](https://github.com/jdx/mise/commit/4c67db59ecfb55eb724dc05bca7eb7281a625929)

## [2025.1.12](https://github.com/jdx/mise/compare/v2025.1.11..v2025.1.12) - 2025-01-24

### Chore

- setup mise for release task by [@jdx](https://github.com/jdx) in [78d3dfb](https://github.com/jdx/mise/commit/78d3dfb164776cfb39a1920485c21fcd6ecd3ebe)

## [2025.1.11](https://github.com/jdx/mise/compare/v2025.1.10..v2025.1.11) - 2025-01-23

### Chore

- pin aws-cli by [@jdx](https://github.com/jdx) in [ca16daf](https://github.com/jdx/mise/commit/ca16daf5e5dbb9159d853570528087b24f63500b)

## [2025.1.10](https://github.com/jdx/mise/compare/v2025.1.9..v2025.1.10) - 2025-01-23

### 🚀 Features

- **(registry)** use aqua for periphery by [@MontakOleg](https://github.com/MontakOleg) in [#4157](https://github.com/jdx/mise/pull/4157)
- split remote task by [@acesyde](https://github.com/acesyde) in [#4156](https://github.com/jdx/mise/pull/4156)

### 🐛 Bug Fixes

- **(docs)** environment variable MISE_OVERRIDE_TOOL_VERSIONS_FILENAME should be plural by [@roele](https://github.com/roele) in [#4183](https://github.com/jdx/mise/pull/4183)
- completions were missing non-asdf tools by [@jdx](https://github.com/jdx) in [55b31a4](https://github.com/jdx/mise/commit/55b31a452b807ada4e2ba40c8b5588b77b79642e)
- broken link for `/tasks/task-configuration` by [@134130](https://github.com/134130) in [#4155](https://github.com/jdx/mise/pull/4155)
- whitespace in mise.run script by [@jdx](https://github.com/jdx) in [#4153](https://github.com/jdx/mise/pull/4153)
- confusing error in fish_command_not_found by [@MrGreenTea](https://github.com/MrGreenTea) in [#4162](https://github.com/jdx/mise/pull/4162)
- use correct python path for venv creation in windows by [@tisoft](https://github.com/tisoft) in [#4164](https://github.com/jdx/mise/pull/4164)

### 📚 Documentation

- neovim cookbook by [@EricDriussi](https://github.com/EricDriussi) in [#4161](https://github.com/jdx/mise/pull/4161)

### 🧪 Testing

- fix a couple of tool tests by [@jdx](https://github.com/jdx) in [#4186](https://github.com/jdx/mise/pull/4186)

### Chore

- added issue auto-closer by [@jdx](https://github.com/jdx) in [3c831c1](https://github.com/jdx/mise/commit/3c831c19a644fbb2f393f969ebaa5137f9415793)

### New Contributors

- @tisoft made their first contribution in [#4164](https://github.com/jdx/mise/pull/4164)
- @MrGreenTea made their first contribution in [#4162](https://github.com/jdx/mise/pull/4162)
- @EricDriussi made their first contribution in [#4161](https://github.com/jdx/mise/pull/4161)
- @134130 made their first contribution in [#4155](https://github.com/jdx/mise/pull/4155)

## [2025.1.9](https://github.com/jdx/mise/compare/v2025.1.8..v2025.1.9) - 2025-01-17

### 🚀 Features

- **(aqua)** pass --verbose flag down to cosign and added aqua.cosign_extra_args setting by [@jdx](https://github.com/jdx) in [#4148](https://github.com/jdx/mise/pull/4148)
- **(doctor)** display redacted github token by [@jdx](https://github.com/jdx) in [#4149](https://github.com/jdx/mise/pull/4149)

### 🐛 Bug Fixes

- **(ruby)** remove ruby/gem tests by [@jdx](https://github.com/jdx) in [#4130](https://github.com/jdx/mise/pull/4130)
- Fixes fish_command_not_found glob error by [@halostatue](https://github.com/halostatue) in [#4133](https://github.com/jdx/mise/pull/4133)
- completions for `mise use` by [@jdx](https://github.com/jdx) in [#4147](https://github.com/jdx/mise/pull/4147)

### 📦️ Dependency Updates

- update dependency bun to v1.1.44 by [@renovate[bot]](https://github.com/renovate[bot]) in [#4134](https://github.com/jdx/mise/pull/4134)

### Chore

- add install.sh.sig to releases by [@jdx](https://github.com/jdx) in [1b6ea86](https://github.com/jdx/mise/commit/1b6ea8644edcf3a6ff68fc6d511622c44f1f1f9a)

### New Contributors

- @halostatue made their first contribution in [#4133](https://github.com/jdx/mise/pull/4133)

## [2025.1.8](https://github.com/jdx/mise/compare/v2025.1.7..v2025.1.8) - 2025-01-17

### 🚀 Features

- upgrade ubi by [@jdx](https://github.com/jdx) in [#4078](https://github.com/jdx/mise/pull/4078)
- enable erlang for Windows by [@roele](https://github.com/roele) in [#4128](https://github.com/jdx/mise/pull/4128)
- use aqua for opentofu by [@jdx](https://github.com/jdx) in [#4129](https://github.com/jdx/mise/pull/4129)

### 🐛 Bug Fixes

- **(spm)** install from annotated tag by [@MontakOleg](https://github.com/MontakOleg) in [#4120](https://github.com/jdx/mise/pull/4120)
- Fixes infinite loop in auto install not found bash function by [@bnorick](https://github.com/bnorick) in [#4094](https://github.com/jdx/mise/pull/4094)
- installing with empty version fails by [@roele](https://github.com/roele) in [#4123](https://github.com/jdx/mise/pull/4123)

### 📚 Documentation

- correct link to gem.rs source by [@petrblaho](https://github.com/petrblaho) in [#4119](https://github.com/jdx/mise/pull/4119)
- fix {{config_root}} got interpolated by vitepress by [@peter50216](https://github.com/peter50216) in [#4122](https://github.com/jdx/mise/pull/4122)

### Chore

- remove minisign from mise.toml by [@jdx](https://github.com/jdx) in [b115ba9](https://github.com/jdx/mise/commit/b115ba962fce4e63e0d6ce85f41704f302ef3e9a)

### New Contributors

- @peter50216 made their first contribution in [#4122](https://github.com/jdx/mise/pull/4122)
- @petrblaho made their first contribution in [#4119](https://github.com/jdx/mise/pull/4119)

## [2025.1.7](https://github.com/jdx/mise/compare/v2025.1.6..v2025.1.7) - 2025-01-15

### 🚀 Features

- **(registry)** add gup by [@scop](https://github.com/scop) in [#4107](https://github.com/jdx/mise/pull/4107)
- **(registry)** add aqua and cmdx by [@scop](https://github.com/scop) in [#4106](https://github.com/jdx/mise/pull/4106)
- use aqua for eza on linux by [@jdx](https://github.com/jdx) in [#4075](https://github.com/jdx/mise/pull/4075)
- allow to specify Rust profile by [@roele](https://github.com/roele) in [#4101](https://github.com/jdx/mise/pull/4101)

### 🐛 Bug Fixes

- use vars in [env] templates by [@hverlin](https://github.com/hverlin) in [#4100](https://github.com/jdx/mise/pull/4100)
- panic when directory name contains japanese characters by [@roele](https://github.com/roele) in [#4104](https://github.com/jdx/mise/pull/4104)
- incorrect config_root for project/.mise/config.toml by [@roele](https://github.com/roele) in [#4108](https://github.com/jdx/mise/pull/4108)

### 🚜 Refactor

- **(registry)** alias protobuf to protoc by [@scop](https://github.com/scop) in [#4087](https://github.com/jdx/mise/pull/4087)
- **(registry)** use aqua for go-getter and kcl by [@scop](https://github.com/scop) in [#4088](https://github.com/jdx/mise/pull/4088)
- **(registry)** use aqua for powerline-go by [@scop](https://github.com/scop) in [#4105](https://github.com/jdx/mise/pull/4105)

### 📚 Documentation

- clean up activation instructions by [@jdx](https://github.com/jdx) in [e235c74](https://github.com/jdx/mise/commit/e235c74daa8f5e5f9e1bb89c70a6cff96c08956e)
- correct urls for crawler by [@jdx](https://github.com/jdx) in [21cb77b](https://github.com/jdx/mise/commit/21cb77b1f79a57e6ebd3fec367bd5b223239a3ed)
- added sitemap meta tag by [@jdx](https://github.com/jdx) in [033aa14](https://github.com/jdx/mise/commit/033aa149e8b7a45ea750c09c31438709420214c8)

## [2025.1.6](https://github.com/jdx/mise/compare/v2025.1.5..v2025.1.6) - 2025-01-12

### 🐛 Bug Fixes

- Panic when run without arguments with bootstrapped script by [@jdx](https://github.com/jdx) in [#4065](https://github.com/jdx/mise/pull/4065)

### 🚜 Refactor

- use better rust syntax by [@jdx](https://github.com/jdx) in [#4072](https://github.com/jdx/mise/pull/4072)

### 📚 Documentation

- fix TOML-based Tasks usage spec example by [@gturi](https://github.com/gturi) in [#4067](https://github.com/jdx/mise/pull/4067)
- eza by [@jdx](https://github.com/jdx) in [5a80cbf](https://github.com/jdx/mise/commit/5a80cbf9e0b37be800bc6f6f0404bcf86cbe3bd9)
- removed bit about verifying with asdf by [@jdx](https://github.com/jdx) in [d505486](https://github.com/jdx/mise/commit/d505486fbbe49af0f7bf6029569812441c1e3fdc)
- added more getting started installers by [@jdx](https://github.com/jdx) in [b310e11](https://github.com/jdx/mise/commit/b310e118b00d2b0a64cf2d423d20ece6dc9692f6)
- clean up activation instructions by [@jdx](https://github.com/jdx) in [3df60dd](https://github.com/jdx/mise/commit/3df60dd9cbecf3086b1755d4e397159379d27b27)
- clean up activation instructions by [@jdx](https://github.com/jdx) in [8ab4bce](https://github.com/jdx/mise/commit/8ab4bcef77c4bc1e07951dbb8b5787df4a4b15bf)
- clean up activation instructions by [@jdx](https://github.com/jdx) in [d4a67e8](https://github.com/jdx/mise/commit/d4a67e8ec72fed064cc776ab643f41da1ae01caa)
- clean up activation instructions by [@jdx](https://github.com/jdx) in [d208418](https://github.com/jdx/mise/commit/d208418a5f63803185c4aa5f06afecd9e8832496)
- clean up activation instructions by [@jdx](https://github.com/jdx) in [b9f581d](https://github.com/jdx/mise/commit/b9f581d644295f372eb0cd026560e9c97dcb8091)

### New Contributors

- @gturi made their first contribution in [#4067](https://github.com/jdx/mise/pull/4067)

## [2025.1.5](https://github.com/jdx/mise/compare/v2025.1.4..v2025.1.5) - 2025-01-11

### 🚀 Features

- added gdu and dua to registry by [@sassdavid](https://github.com/sassdavid) in [#4052](https://github.com/jdx/mise/pull/4052)
- added prefix-dev/pixi by [@jdx](https://github.com/jdx) in [#4056](https://github.com/jdx/mise/pull/4056)
- added `mise cfg --tracked-configs` by [@jdx](https://github.com/jdx) in [#4059](https://github.com/jdx/mise/pull/4059)
- added `mise version --json` flag by [@jdx](https://github.com/jdx) in [#4061](https://github.com/jdx/mise/pull/4061)
- added `mise ls --prunable` flag by [@jdx](https://github.com/jdx) in [#4062](https://github.com/jdx/mise/pull/4062)

### 🐛 Bug Fixes

- switch jib back to asdf by [@jdx](https://github.com/jdx) in [#4055](https://github.com/jdx/mise/pull/4055)
- `mise unuse` bug not pruning if not in config file by [@jdx](https://github.com/jdx) in [#4058](https://github.com/jdx/mise/pull/4058)

### 📚 Documentation

- explain pipx better by [@jdx](https://github.com/jdx) in [42dcb3b](https://github.com/jdx/mise/commit/42dcb3bc5a6547d3d148c391ceccfd9228e34669)

### 🧪 Testing

- added test case for `mise rm` by [@jdx](https://github.com/jdx) in [f7511b6](https://github.com/jdx/mise/commit/f7511b696c2ada7af878074e89b0dfc1edb73197)

### New Contributors

- @sassdavid made their first contribution in [#4052](https://github.com/jdx/mise/pull/4052)

## [2025.1.4](https://github.com/jdx/mise/compare/v2025.1.3..v2025.1.4) - 2025-01-10

### 🚀 Features

- update JSON output for task info/ls by [@hverlin](https://github.com/hverlin) in [#4034](https://github.com/jdx/mise/pull/4034)
- **breaking** bump usage to 2.x by [@jdx](https://github.com/jdx) in [#4049](https://github.com/jdx/mise/pull/4049)

### 🐛 Bug Fixes

- ignore github releases marked as draft by [@jdx](https://github.com/jdx) in [#4030](https://github.com/jdx/mise/pull/4030)
- `mise run` shorthand with tasks that have an extension by [@jdx](https://github.com/jdx) in [#4029](https://github.com/jdx/mise/pull/4029)
- use consistent casing by [@jdx](https://github.com/jdx) in [a4d4133](https://github.com/jdx/mise/commit/a4d41338139355b0dd86a068fd89790eb7e34584)
- support latest ansible packages by [@jdx](https://github.com/jdx) in [#4045](https://github.com/jdx/mise/pull/4045)
- use go backend for goconvey/ginkgo by [@jdx](https://github.com/jdx) in [#4047](https://github.com/jdx/mise/pull/4047)
- Improve fig spec with better generators by [@miguelmig](https://github.com/miguelmig) in [#3762](https://github.com/jdx/mise/pull/3762)

### 📚 Documentation

- set prose-wrap with prettier by [@jdx](https://github.com/jdx) in [#4038](https://github.com/jdx/mise/pull/4038)
- Fix "Example of a NodeJS file task with arguments" by [@highb](https://github.com/highb) in [#4046](https://github.com/jdx/mise/pull/4046)

### 🧪 Testing

- disable some non-working plugins by [@jdx](https://github.com/jdx) in [106ee40](https://github.com/jdx/mise/commit/106ee40b463923bb5c6444e0c0127dabc502d9ee)
- remove test for flarectl by [@jdx](https://github.com/jdx) in [a63b449](https://github.com/jdx/mise/commit/a63b44910d55ad2cdc801a472f0c196c605cce25)

### ◀️ Revert

- Revert "docs: set prose-wrap with prettier " by [@jdx](https://github.com/jdx) in [065dd8f](https://github.com/jdx/mise/commit/065dd8fa917b6097fb168b631b506455af3e1d28)

### Chore

- added `cargo check` to pre-commit by [@jdx](https://github.com/jdx) in [73eb25a](https://github.com/jdx/mise/commit/73eb25a88bbfe1b979bb5483ca3c81a689be184f)
- fix release-plz pr creation by [@jdx](https://github.com/jdx) in [8299c6b](https://github.com/jdx/mise/commit/8299c6b943119ffda94d18445c5b789948b6f9c0)
- use -q in pre-commit:check by [@jdx](https://github.com/jdx) in [099b2d8](https://github.com/jdx/mise/commit/099b2d88d3ed31ace30c67be816170dc50f87b6d)
- fix release-plz pr creation by [@jdx](https://github.com/jdx) in [c2accc5](https://github.com/jdx/mise/commit/c2accc5f7192202d0a8249ae7f3ab0ea7f100e1b)
- make prettier/pre-commit much faster by [@jdx](https://github.com/jdx) in [#4036](https://github.com/jdx/mise/pull/4036)
- fix release-plz edit command by [@jdx](https://github.com/jdx) in [86b5816](https://github.com/jdx/mise/commit/86b5816660f5a13d45c1795132a29e881645e271)

## [2025.1.3](https://github.com/jdx/mise/compare/v2025.1.2..v2025.1.3) - 2025-01-09

### 🐛 Bug Fixes

- **(rust)** respect RUSTUP_HOME/CARGO_HOME by [@jdx](https://github.com/jdx) in [#4026](https://github.com/jdx/mise/pull/4026)
- mise fails to install kubectl on windows from aqua registry by [@roele](https://github.com/roele) in [#4006](https://github.com/jdx/mise/pull/4006)
- aliases with aqua by [@jdx](https://github.com/jdx) in [#4007](https://github.com/jdx/mise/pull/4007)
- issue with enter hook and subdirs by [@jdx](https://github.com/jdx) in [#4008](https://github.com/jdx/mise/pull/4008)
- allow using depends and depends_post on separate tasks by [@jdx](https://github.com/jdx) in [#4010](https://github.com/jdx/mise/pull/4010)
- mise fails to install kubectl on windows from aqua registry by [@roele](https://github.com/roele) in [#4024](https://github.com/jdx/mise/pull/4024)

### 📚 Documentation

- Add default description to github token link by [@hverlin](https://github.com/hverlin) in [#4019](https://github.com/jdx/mise/pull/4019)
- fix source code links by [@jdx](https://github.com/jdx) in [#4025](https://github.com/jdx/mise/pull/4025)

### Chore

- make pre-commit faster by [@jdx](https://github.com/jdx) in [70dfdd0](https://github.com/jdx/mise/commit/70dfdd0b874a5292b4b20fa72c9c341a13900bde)
- added commented out paths config by [@jdx](https://github.com/jdx) in [c1f25ac](https://github.com/jdx/mise/commit/c1f25ac4cdaf74219d700fcaf37d3341971a3120)

## [2025.1.2](https://github.com/jdx/mise/compare/v2025.1.1..v2025.1.2) - 2025-01-08

### 🚀 Features

- migrate asdf plugins to aqua/ubi by [@jdx](https://github.com/jdx) in [#3962](https://github.com/jdx/mise/pull/3962)
- migrate asdf plugins to aqua/ubi by [@jdx](https://github.com/jdx) in [#3978](https://github.com/jdx/mise/pull/3978)
- migrate asdf plugins to aqua/ubi by [@jdx](https://github.com/jdx) in [#3991](https://github.com/jdx/mise/pull/3991)
- replace asdf-spark plugin with mise-spark plugin by [@benberryallwood](https://github.com/benberryallwood) in [#3994](https://github.com/jdx/mise/pull/3994)
- add kubectx/kubens to registry by [@roele](https://github.com/roele) in [#3992](https://github.com/jdx/mise/pull/3992)
- added ktlint from aqua by [@jdx](https://github.com/jdx) in [#4004](https://github.com/jdx/mise/pull/4004)

### 🐛 Bug Fixes

- **(schema)** fix task sources and outputs schema by [@risu729](https://github.com/risu729) in [#3988](https://github.com/jdx/mise/pull/3988)
- **(schema)** update task schema by [@risu729](https://github.com/risu729) in [#3999](https://github.com/jdx/mise/pull/3999)
- correct age keyname by [@jdx](https://github.com/jdx) in [e28c293](https://github.com/jdx/mise/commit/e28c293bc5a241b043d0b72ec9aa0559e888f97b)
- mise install rust failed on windows by [@roele](https://github.com/roele) in [#3969](https://github.com/jdx/mise/pull/3969)
- maven-mvnd does not install with aqua by [@roele](https://github.com/roele) in [#3982](https://github.com/jdx/mise/pull/3982)
- maven-mvnd does not install with aqua by [@roele](https://github.com/roele) in [#3993](https://github.com/jdx/mise/pull/3993)
- use friendly error in `mise run` by [@jdx](https://github.com/jdx) in [#3998](https://github.com/jdx/mise/pull/3998)
- use task display_name in more places by [@hverlin](https://github.com/hverlin) in [#3997](https://github.com/jdx/mise/pull/3997)
- aqua:apache/spark doesn't work by [@roele](https://github.com/roele) in [#3995](https://github.com/jdx/mise/pull/3995)

### 📚 Documentation

- style on rustup settings by [@jdx](https://github.com/jdx) in [da91716](https://github.com/jdx/mise/commit/da91716c856b0bb1e8bdf70f9f97f74fe09f15ac)
- Escape template examples by [@henrebotha](https://github.com/henrebotha) in [#3987](https://github.com/jdx/mise/pull/3987)
- update SECURITY.md by [@jdx](https://github.com/jdx) in [6372f10](https://github.com/jdx/mise/commit/6372f101639386e94cd8df400c78962eab1dbdd5)

### 🧪 Testing

- fix test-plugins CI job for ubuntu-24 by [@jdx](https://github.com/jdx) in [492f6ac](https://github.com/jdx/mise/commit/492f6acc99014cb70f97efdd12700ee365a418ea)
- remove postgres test-plugins test by [@jdx](https://github.com/jdx) in [e93bc80](https://github.com/jdx/mise/commit/e93bc80a780fd0f7b4619af37c3f646dd622bed4)

### Chore

- remove deprecated tar syntax by [@jdx](https://github.com/jdx) in [322735a](https://github.com/jdx/mise/commit/322735a75bef9c602ffcec4d81914662cac00647)
- fix tar/gzip syntax by [@jdx](https://github.com/jdx) in [cd0a049](https://github.com/jdx/mise/commit/cd0a049ecace47354a931cd364ac2f5915812658)
- fork remaining asdf plugins to mise-plugins by [@jdx](https://github.com/jdx) in [#3996](https://github.com/jdx/mise/pull/3996)

### New Contributors

- @henrebotha made their first contribution in [#3987](https://github.com/jdx/mise/pull/3987)

## [2025.1.1](https://github.com/jdx/mise/compare/v2025.1.0..v2025.1.1) - 2025-01-06

### 🚀 Features

- add databricks-cli to registry by [@benberryallwood](https://github.com/benberryallwood) in [#3937](https://github.com/jdx/mise/pull/3937)
- add navi to registry by [@kit494way](https://github.com/kit494way) in [#3943](https://github.com/jdx/mise/pull/3943)
- added allurectl to registry by [@MontakOleg](https://github.com/MontakOleg) in [#3918](https://github.com/jdx/mise/pull/3918)
- Add setting description to mise settings --json-extended output by [@hverlin](https://github.com/hverlin) in [#3919](https://github.com/jdx/mise/pull/3919)

### 🐛 Bug Fixes

- improve mise generate bootstrap by [@hverlin](https://github.com/hverlin) in [#3939](https://github.com/jdx/mise/pull/3939)
- update year in copyright to dynamic with current year by [@nexckycort](https://github.com/nexckycort) in [#3957](https://github.com/jdx/mise/pull/3957)

### 📚 Documentation

- Fix broken link to environment variables doc by [@xcapaldi](https://github.com/xcapaldi) in [#3938](https://github.com/jdx/mise/pull/3938)
- Add usage property to mise schema by [@hverlin](https://github.com/hverlin) in [#3942](https://github.com/jdx/mise/pull/3942)
- clarity on relative paths vs config_root in _.path by [@glasser](https://github.com/glasser) in [#3923](https://github.com/jdx/mise/pull/3923)

### 📦️ Dependency Updates

- update rust crate itertools to 0.14 by [@renovate[bot]](https://github.com/renovate[bot]) in [#3926](https://github.com/jdx/mise/pull/3926)
- update rust crate petgraph to 0.7 by [@renovate[bot]](https://github.com/renovate[bot]) in [#3927](https://github.com/jdx/mise/pull/3927)
- update rust crate self_update to 0.42 by [@renovate[bot]](https://github.com/renovate[bot]) in [#3931](https://github.com/jdx/mise/pull/3931)

### Chore

- upgrade expr by [@jdx](https://github.com/jdx) in [c06a415](https://github.com/jdx/mise/commit/c06a41544e2cb09912244efe6a8f5bcc03eb24d7)
- mise up by [@jdx](https://github.com/jdx) in [678f648](https://github.com/jdx/mise/commit/678f6489a9501b32bf3c36771977771d933f2466)
- cargo-show by [@jdx](https://github.com/jdx) in [69d44fd](https://github.com/jdx/mise/commit/69d44fd064d2fdaae08ff9ea3300a42e560630cd)
- remove cargo-show dependency by [@jdx](https://github.com/jdx) in [ab8e9e9](https://github.com/jdx/mise/commit/ab8e9e9e429beeb23731c356537525f64bc59b28)
- remove cargo-show dependency by [@jdx](https://github.com/jdx) in [ca2f89c](https://github.com/jdx/mise/commit/ca2f89c6cd36d828a9eab2884a3f8c9cc1fe2c19)
- remove cargo-show dependency by [@jdx](https://github.com/jdx) in [82e3390](https://github.com/jdx/mise/commit/82e3390c5fc9a97c942dc407b2073edfcb3974bc)
- fix release-plz by [@jdx](https://github.com/jdx) in [52ac62a](https://github.com/jdx/mise/commit/52ac62a7d7e8439d32b84c4247ee366c28901863)
- fix release-plz by [@jdx](https://github.com/jdx) in [dba7044](https://github.com/jdx/mise/commit/dba7044b4dcce808fd4734e9a284ab2174758be0)

### New Contributors

- @nexckycort made their first contribution in [#3957](https://github.com/jdx/mise/pull/3957)
- @MontakOleg made their first contribution in [#3918](https://github.com/jdx/mise/pull/3918)
- @kit494way made their first contribution in [#3943](https://github.com/jdx/mise/pull/3943)
- @benberryallwood made their first contribution in [#3937](https://github.com/jdx/mise/pull/3937)
- @xcapaldi made their first contribution in [#3938](https://github.com/jdx/mise/pull/3938)
- @auxesis made their first contribution in [#3914](https://github.com/jdx/mise/pull/3914)

## [2025.1.0](https://github.com/jdx/mise/compare/v2024.12.24..v2025.1.0) - 2025-01-01

### 🚀 Features

- use aqua for gradle by [@jdx](https://github.com/jdx) in [#3903](https://github.com/jdx/mise/pull/3903)
- added completions to more commands by [@jdx](https://github.com/jdx) in [#3910](https://github.com/jdx/mise/pull/3910)

### 🐛 Bug Fixes

- panic when setting config value by [@roele](https://github.com/roele) in [#3823](https://github.com/jdx/mise/pull/3823)
- add hidden settings/task --complete option by [@jdx](https://github.com/jdx) in [#3902](https://github.com/jdx/mise/pull/3902)
- handle panic when task contains invalid template by [@jdx](https://github.com/jdx) in [#3904](https://github.com/jdx/mise/pull/3904)
- missing checksums in mise.run script by [@jdx](https://github.com/jdx) in [#3906](https://github.com/jdx/mise/pull/3906)
- active flag for symlinked tools in `mise ls --json` by [@jdx](https://github.com/jdx) in [#3907](https://github.com/jdx/mise/pull/3907)

### 📚 Documentation

- Update LICENSE by [@jdx](https://github.com/jdx) in [156db11](https://github.com/jdx/mise/commit/156db1130c2757aaaf6e53686148d8b9b0791ae7)
- updated roadmap by [@jdx](https://github.com/jdx) in [f8916d4](https://github.com/jdx/mise/commit/f8916d4cbd09fbbc8142bf25b4d586e146d19a21)

## [2024.12.24](https://github.com/jdx/mise/compare/v2024.12.23..v2024.12.24) - 2024-12-31

### 🐛 Bug Fixes

- switch back to asdf for gradle by [@jdx](https://github.com/jdx) in [cc88dca](https://github.com/jdx/mise/commit/cc88dca50e8e0dac94dbb83d0ce1ebcfc38a1ec4)

### Chore

- add commented out cleanup of old CLIs by [@jdx](https://github.com/jdx) in [bb7e022](https://github.com/jdx/mise/commit/bb7e022240c0e7019a595d093a33b414119e975f)

## [2024.12.23](https://github.com/jdx/mise/compare/v2024.12.22..v2024.12.23) - 2024-12-30

### 🐛 Bug Fixes

- winget release PRs by [@jdx](https://github.com/jdx) in [9dec542](https://github.com/jdx/mise/commit/9dec542188e731ef357fd74339dd08ac005cb9e3)
- mise settings unset does not seem to work by [@roele](https://github.com/roele) in [#3867](https://github.com/jdx/mise/pull/3867)
- gradle aqua package by [@jdx](https://github.com/jdx) in [#3880](https://github.com/jdx/mise/pull/3880)
- **breaking** remove `root` env var in tasks by [@jdx](https://github.com/jdx) in [#3884](https://github.com/jdx/mise/pull/3884)

### 📚 Documentation

- syntax in `mise watch` by [@jdx](https://github.com/jdx) in [beab480](https://github.com/jdx/mise/commit/beab48029b3e7a91047012b655f3efe4fd722acf)
- Update registry link by [@bmulholland](https://github.com/bmulholland) in [#3864](https://github.com/jdx/mise/pull/3864)
- clarify shims behaviour by [@syhol](https://github.com/syhol) in [#3881](https://github.com/jdx/mise/pull/3881)

### Chore

- remove unused versioned tarballs from mise.jdx.dev by [@jdx](https://github.com/jdx) in [48f1021](https://github.com/jdx/mise/commit/48f1021048646061e7cd85d9f9969946b00962a6)
- trim newline in banner by [@jdx](https://github.com/jdx) in [c8f2c90](https://github.com/jdx/mise/commit/c8f2c90111c5d20fe4586d59eb66f3bb2f8cfd9a)

### New Contributors

- @bmulholland made their first contribution in [#3864](https://github.com/jdx/mise/pull/3864)

## [2024.12.22](https://github.com/jdx/mise/compare/v2024.12.21..v2024.12.22) - 2024-12-30

### 🚀 Features

- colorize banner by [@jdx](https://github.com/jdx) in [ad3a5f0](https://github.com/jdx/mise/commit/ad3a5f040013bad046f2ca3abb9eebc941301368)

### 🐛 Bug Fixes

- add `:` escaping for tasks with multiple colons by [@eitamal](https://github.com/eitamal) in [#3853](https://github.com/jdx/mise/pull/3853)
- type issue in docs/JSON schema for python_create_args and uv_create_args by [@roele](https://github.com/roele) in [#3855](https://github.com/jdx/mise/pull/3855)

### 📚 Documentation

- **(settings)** fix link to precompiled python binaries by [@scop](https://github.com/scop) in [#3851](https://github.com/jdx/mise/pull/3851)
- Fix cargo install examples by [@orf](https://github.com/orf) in [#3862](https://github.com/jdx/mise/pull/3862)

### New Contributors

- @orf made their first contribution in [#3862](https://github.com/jdx/mise/pull/3862)
- @eitamal made their first contribution in [#3853](https://github.com/jdx/mise/pull/3853)

## [2024.12.21](https://github.com/jdx/mise/compare/v2024.12.20..v2024.12.21) - 2024-12-27

### 🐛 Bug Fixes

- **(python)** force precompiled setting warning message syntax by [@scop](https://github.com/scop) in [#3850](https://github.com/jdx/mise/pull/3850)
- zstd detection false positive on MacOS by [@roele](https://github.com/roele) in [#3845](https://github.com/jdx/mise/pull/3845)

### 📚 Documentation

- fix incorrect examples that were causing 'expected a sequence' error by [@ssbarnea](https://github.com/ssbarnea) in [#3839](https://github.com/jdx/mise/pull/3839)

### 📦️ Dependency Updates

- update rust crate ubi to 0.3 by [@renovate[bot]](https://github.com/renovate[bot]) in [#3836](https://github.com/jdx/mise/pull/3836)

## [2024.12.20](https://github.com/jdx/mise/compare/v2024.12.19..v2024.12.20) - 2024-12-25

### 🚀 Features

- **(hugo)** add extended registry from aqua and keep only one registry with all aliases by [@kilianpaquier](https://github.com/kilianpaquier) in [#3813](https://github.com/jdx/mise/pull/3813)
- build erlang with all cores by [@jdx](https://github.com/jdx) in [#3802](https://github.com/jdx/mise/pull/3802)
- Modify install_rubygems_hook to place plugin in site_ruby directory by [@zkhadikov](https://github.com/zkhadikov) in [#3812](https://github.com/jdx/mise/pull/3812)

### 🐛 Bug Fixes

- do not require "v" prefix in mise.run by [@jdx](https://github.com/jdx) in [#3800](https://github.com/jdx/mise/pull/3800)
- add checksum for macos-x86 by [@jdx](https://github.com/jdx) in [#3815](https://github.com/jdx/mise/pull/3815)

### 📚 Documentation

- Correct link to aqua registry by [@jesse-c](https://github.com/jesse-c) in [#3803](https://github.com/jdx/mise/pull/3803)

### 🧪 Testing

- skip dotnet if not installed by [@jdx](https://github.com/jdx) in [1a663dd](https://github.com/jdx/mise/commit/1a663dd63e17cc08a961b86b5b0b6a1d7e9b2a1f)

### New Contributors

- @zkhadikov made their first contribution in [#3812](https://github.com/jdx/mise/pull/3812)
- @kilianpaquier made their first contribution in [#3813](https://github.com/jdx/mise/pull/3813)
- @jesse-c made their first contribution in [#3803](https://github.com/jdx/mise/pull/3803)

## [2024.12.19](https://github.com/jdx/mise/compare/v2024.12.18..v2024.12.19) - 2024-12-23

### 🚀 Features

- use zstd in mise.run by [@jdx](https://github.com/jdx) in [#3798](https://github.com/jdx/mise/pull/3798)
- verify zig with minisign by [@jdx](https://github.com/jdx) in [#3793](https://github.com/jdx/mise/pull/3793)

### Chore

- increase tarball compression by [@jdx](https://github.com/jdx) in [a899155](https://github.com/jdx/mise/commit/a8991551bd7c61d1f75a800906d2f718b4bdf7c0)
- use max threads for zstd compression by [@jdx](https://github.com/jdx) in [a3f792a](https://github.com/jdx/mise/commit/a3f792a1eb0a395c7a82a063b96d30282b6343de)
- print all tarball sizes by [@jdx](https://github.com/jdx) in [29fbc04](https://github.com/jdx/mise/commit/29fbc04e52c76b16c9a72385ead4edbfaff984fb)

## [2024.12.18](https://github.com/jdx/mise/compare/v2024.12.17..v2024.12.18) - 2024-12-23

### 🚀 Features

- allow dotnet prerelease by [@acesyde](https://github.com/acesyde) in [#3753](https://github.com/jdx/mise/pull/3753)
- added minisign to registry by [@jdx](https://github.com/jdx) in [#3788](https://github.com/jdx/mise/pull/3788)
- `mise g bootstrap` by [@jdx](https://github.com/jdx) in [#3792](https://github.com/jdx/mise/pull/3792)
- `mise g bootstrap` by [@jdx](https://github.com/jdx) in [f79ce71](https://github.com/jdx/mise/commit/f79ce719f9121eb6e0e821cf271af306f2a9d6c8)

### 🐛 Bug Fixes

- hide task file extension in completions by [@jdx](https://github.com/jdx) in [#3772](https://github.com/jdx/mise/pull/3772)
- settings completions by [@jdx](https://github.com/jdx) in [#3787](https://github.com/jdx/mise/pull/3787)

### 📚 Documentation

- update IDE integration page by [@hverlin](https://github.com/hverlin) in [#3765](https://github.com/jdx/mise/pull/3765)
- add powershell sample by [@acesyde](https://github.com/acesyde) in [#3771](https://github.com/jdx/mise/pull/3771)
- add missing dotnet left menu by [@acesyde](https://github.com/acesyde) in [#3770](https://github.com/jdx/mise/pull/3770)

### 🧪 Testing

- added stubbed test for https://github.com/jdx/mise/discussions/3783 by [@jdx](https://github.com/jdx) in [f79a3a4](https://github.com/jdx/mise/commit/f79a3a41ebf833d2c49bdc91ae4026c46498d9f7)

### ◀️ Revert

- Revert "fix: Use arguments for to pass staged filenames to pre-commit task (#…" by [@jdx](https://github.com/jdx) in [#3791](https://github.com/jdx/mise/pull/3791)

### Chore

- add shell to user-agent by [@jdx](https://github.com/jdx) in [#3786](https://github.com/jdx/mise/pull/3786)
- sign releases with minisign by [@jdx](https://github.com/jdx) in [#3789](https://github.com/jdx/mise/pull/3789)
- create minisign secret key by [@jdx](https://github.com/jdx) in [dea4676](https://github.com/jdx/mise/commit/dea4676f53ee4d1a905ae17b004131c6dee3b385)
- create minisign secret key by [@jdx](https://github.com/jdx) in [ecebebe](https://github.com/jdx/mise/commit/ecebebee13cc20773eaefda706bad4e5ac8cc25f)
- fix minisign signing by [@jdx](https://github.com/jdx) in [6401ff8](https://github.com/jdx/mise/commit/6401ff84e0dcbdb890dd037aff6fbcf3edc51af5)
- added install.sh to releases by [@jdx](https://github.com/jdx) in [2946d58](https://github.com/jdx/mise/commit/2946d5864cffb65a1ee1260f3c38070531743854)
- install minisign by [@jdx](https://github.com/jdx) in [f22272c](https://github.com/jdx/mise/commit/f22272c3838fcb8de0365a4022f8aefc00c46f4c)
- use ubuntu-24 for release by [@jdx](https://github.com/jdx) in [40a13f8](https://github.com/jdx/mise/commit/40a13f8e7088ba13762178eccc5eb8438bc9ce6b)
- set minisign pub key by [@jdx](https://github.com/jdx) in [fd6aa1e](https://github.com/jdx/mise/commit/fd6aa1eccf23f97e82ff166ff8950721c236239b)
- age encrypt minisign key by [@jdx](https://github.com/jdx) in [02c30e2](https://github.com/jdx/mise/commit/02c30e2c9167d3f4bf5ac05a82a43bc82b703123)
- apt install age by [@jdx](https://github.com/jdx) in [769a088](https://github.com/jdx/mise/commit/769a08875b3651c3edd63fd4387497ce6b16cd4b)
- switch back to MINISIGN_KEY by [@jdx](https://github.com/jdx) in [66dc8cf](https://github.com/jdx/mise/commit/66dc8cf199adb57c22ac398b3333ba12abaaf106)
- fix minisign signing by [@jdx](https://github.com/jdx) in [a3f8173](https://github.com/jdx/mise/commit/a3f81738bb4ab0827eb6bfae4a1639c29f29da36)
- add zst tarballs by [@jdx](https://github.com/jdx) in [85a1192](https://github.com/jdx/mise/commit/85a1192091b7f37ab7c3712e4100c8b43d587857)
- add zst tarballs by [@jdx](https://github.com/jdx) in [5238124](https://github.com/jdx/mise/commit/5238124dbda89fe32380beab9b64d31cb2cb4ddb)
- add zst tarballs by [@jdx](https://github.com/jdx) in [2a4d0bf](https://github.com/jdx/mise/commit/2a4d0bf0ee78dfe672d97bc763643300516d5a9b)
- add zst tarballs by [@jdx](https://github.com/jdx) in [285d777](https://github.com/jdx/mise/commit/285d777b3f33bfa587070b3d15cd904fc83e111f)
- extract artifact with zstd by [@jdx](https://github.com/jdx) in [ba66d46](https://github.com/jdx/mise/commit/ba66d4659c6d8f3ffa589dacfe402d6988e46d9a)

## [2024.12.17](https://github.com/jdx/mise/compare/v2024.12.16..v2024.12.17) - 2024-12-21

### 🚀 Features

- added a banner to `mise --version` by [@jdx](https://github.com/jdx) in [#3748](https://github.com/jdx/mise/pull/3748)
- add usage field to tasks by [@jdx](https://github.com/jdx) in [#3746](https://github.com/jdx/mise/pull/3746)
- added keep-order task output type by [@jdx](https://github.com/jdx) in [#3763](https://github.com/jdx/mise/pull/3763)
- `replacing` task output type by [@jdx](https://github.com/jdx) in [#3764](https://github.com/jdx/mise/pull/3764)
- added timed task output type by [@jdx](https://github.com/jdx) in [#3766](https://github.com/jdx/mise/pull/3766)

### 🐛 Bug Fixes

- dotnet backend doc by [@acesyde](https://github.com/acesyde) in [#3752](https://github.com/jdx/mise/pull/3752)
- include full env in toolset tera_ctx by [@risu729](https://github.com/risu729) in [#3751](https://github.com/jdx/mise/pull/3751)
- set env vars in task templates by [@jdx](https://github.com/jdx) in [#3758](https://github.com/jdx/mise/pull/3758)

### 📚 Documentation

- update mise-action version in tips and tricks by [@scop](https://github.com/scop) in [#3749](https://github.com/jdx/mise/pull/3749)
- Small cookbooks fixes by [@hverlin](https://github.com/hverlin) in [#3754](https://github.com/jdx/mise/pull/3754)

### 🧪 Testing

- fix elixir release test by [@jdx](https://github.com/jdx) in [b4f11da](https://github.com/jdx/mise/commit/b4f11dabf7a16a875f9d7ab3ded6a516b481f6f8)
- add some test cases for env var templates by [@jdx](https://github.com/jdx) in [c938977](https://github.com/jdx/mise/commit/c938977ccc265c9530200e0b19bb0cce5f73ddbb)

### Chore

- updated usage by [@jdx](https://github.com/jdx) in [dad7857](https://github.com/jdx/mise/commit/dad785727c80efeb4bf498995ed5237f6cd94d79)

## [2024.12.16](https://github.com/jdx/mise/compare/v2024.12.15..v2024.12.16) - 2024-12-20

### 🚀 Features

- add dotnet backend by [@acesyde](https://github.com/acesyde) in [#3737](https://github.com/jdx/mise/pull/3737)
- added ignored_config_paths to `mise dr` by [@jdx](https://github.com/jdx) in [#3742](https://github.com/jdx/mise/pull/3742)

### 🐛 Bug Fixes

- **(ruby)** fix Ruby plugin to use `ruby_install` option correctly by [@yuhr](https://github.com/yuhr) in [#3732](https://github.com/jdx/mise/pull/3732)
- `mise run` shorthand with options by [@jdx](https://github.com/jdx) in [#3719](https://github.com/jdx/mise/pull/3719)
- zig on windows by [@jdx](https://github.com/jdx) in [#3739](https://github.com/jdx/mise/pull/3739)
- allow using previously defined vars by [@jdx](https://github.com/jdx) in [#3741](https://github.com/jdx/mise/pull/3741)
- make --help consistent with `mise run` and `mise <task>` by [@jdx](https://github.com/jdx) in [#3723](https://github.com/jdx/mise/pull/3723)
- use implicit keys for `mise config set` by [@jdx](https://github.com/jdx) in [#3744](https://github.com/jdx/mise/pull/3744)

### 📚 Documentation

- update cookbook by [@hverlin](https://github.com/hverlin) in [#3718](https://github.com/jdx/mise/pull/3718)
- remove reference to deprecated asdf_compat functionality by [@jdx](https://github.com/jdx) in [03a2afb](https://github.com/jdx/mise/commit/03a2afb4f8c738e3b172d0f5e1ca1465bf1d6a5c)
- describe behavior of `run --output` better by [@jdx](https://github.com/jdx) in [#3740](https://github.com/jdx/mise/pull/3740)

### 📦️ Dependency Updates

- update dependency bun to v1.1.40 by [@renovate[bot]](https://github.com/renovate[bot]) in [#3729](https://github.com/jdx/mise/pull/3729)

### Chore

- lint fix by [@jdx](https://github.com/jdx) in [118b8de](https://github.com/jdx/mise/commit/118b8de645712ff1d78c33b9a2c094a1f92c5b20)
- switch from home -> homedir crate by [@jdx](https://github.com/jdx) in [#3743](https://github.com/jdx/mise/pull/3743)

### New Contributors

- @acesyde made their first contribution in [#3737](https://github.com/jdx/mise/pull/3737)
- @ssbarnea made their first contribution in [#3735](https://github.com/jdx/mise/pull/3735)
- @yuhr made their first contribution in [#3732](https://github.com/jdx/mise/pull/3732)

## [2024.12.15](https://github.com/jdx/mise/compare/v2024.12.14..v2024.12.15) - 2024-12-19

### 🚀 Features

- unnest output when `mise run` is nested by [@jdx](https://github.com/jdx) in [#3686](https://github.com/jdx/mise/pull/3686)
- `mise rm` by [@jdx](https://github.com/jdx) in [#3627](https://github.com/jdx/mise/pull/3627)
- added *:_default task name by [@jdx](https://github.com/jdx) in [#3690](https://github.com/jdx/mise/pull/3690)
- `mise run --continue-on-error by [@jdx](https://github.com/jdx) in [#3692](https://github.com/jdx/mise/pull/3692)
- added .tool-versions -> mise.toml converter by [@jdx](https://github.com/jdx) in [#3693](https://github.com/jdx/mise/pull/3693)
- get mise sync python --uv to work by [@jdx](https://github.com/jdx) in [#3706](https://github.com/jdx/mise/pull/3706)
- `mise install-into` by [@jdx](https://github.com/jdx) in [#3711](https://github.com/jdx/mise/pull/3711)
- added `mise dr --json` by [@jdx](https://github.com/jdx) in [#3715](https://github.com/jdx/mise/pull/3715)

### 🐛 Bug Fixes

- retain "os" options in `mise up --bump` by [@jdx](https://github.com/jdx) in [#3688](https://github.com/jdx/mise/pull/3688)
- unnest task cmd output by [@jdx](https://github.com/jdx) in [#3691](https://github.com/jdx/mise/pull/3691)
- ensure MISE_PROJECT_ROOT is set with no mise.toml by [@jdx](https://github.com/jdx) in [#3695](https://github.com/jdx/mise/pull/3695)
- create venv uses absolute tool paths by [@syhol](https://github.com/syhol) in [#3698](https://github.com/jdx/mise/pull/3698)
- jj repository moved to an organization by [@phyrog](https://github.com/phyrog) in [#3703](https://github.com/jdx/mise/pull/3703)
- disable reverse uv syncing by [@jdx](https://github.com/jdx) in [#3704](https://github.com/jdx/mise/pull/3704)
- add full tera context to tasks by [@jdx](https://github.com/jdx) in [#3708](https://github.com/jdx/mise/pull/3708)
- powershell warning by [@jdx](https://github.com/jdx) in [#3713](https://github.com/jdx/mise/pull/3713)

### 🚜 Refactor

- **(registry)** use aqua for more tools by [@scop](https://github.com/scop) in [#3614](https://github.com/jdx/mise/pull/3614)
- **(registry)** use aqua:skaji/relocatable-perl for perl by [@scop](https://github.com/scop) in [#3716](https://github.com/jdx/mise/pull/3716)
- switch to std::sync::LazyLock by [@jdx](https://github.com/jdx) in [#3707](https://github.com/jdx/mise/pull/3707)

### 📚 Documentation

- fix some broken anchor links by [@hverlin](https://github.com/hverlin) in [#3694](https://github.com/jdx/mise/pull/3694)
- note hooks require `mise activate` by [@jdx](https://github.com/jdx) in [211d3d3](https://github.com/jdx/mise/commit/211d3d3b91c52e418a3e25af4a021da93c64ed4d)

### 🧪 Testing

- fix conduit test for new structure by [@jdx](https://github.com/jdx) in [8691331](https://github.com/jdx/mise/commit/86913318f7705e6cabb999970475c958605219d1)

### Chore

- hide non-functioning docker tasks by [@jdx](https://github.com/jdx) in [40fd3f6](https://github.com/jdx/mise/commit/40fd3f60ebde1d549503a6d9927b79b37622b1b0)

### New Contributors

- @highb made their first contribution in [#3696](https://github.com/jdx/mise/pull/3696)

## [2024.12.14](https://github.com/jdx/mise/compare/v2024.12.13..v2024.12.14) - 2024-12-18

### 🚀 Features

- **(registry)** Add lazydocker by [@hverlin](https://github.com/hverlin) in [#3655](https://github.com/jdx/mise/pull/3655)
- **(registry)** Add btop by [@hverlin](https://github.com/hverlin) in [#3667](https://github.com/jdx/mise/pull/3667)
- Allows control of config_root for global config by [@bnorick](https://github.com/bnorick) in [#3670](https://github.com/jdx/mise/pull/3670)
- allow inserting PATH in env._.source by [@jdx](https://github.com/jdx) in [#3685](https://github.com/jdx/mise/pull/3685)

### 🐛 Bug Fixes

- Can not find the bin files when using python venv on windows by [@NavyD](https://github.com/NavyD) in [#3664](https://github.com/jdx/mise/pull/3664)
- render tasks in task files by [@risu729](https://github.com/risu729) in [#3666](https://github.com/jdx/mise/pull/3666)
- dont require run script for `task add` by [@jdx](https://github.com/jdx) in [#3675](https://github.com/jdx/mise/pull/3675)
- auto-trust on `task add` by [@jdx](https://github.com/jdx) in [#3676](https://github.com/jdx/mise/pull/3676)
- completions getting wrapped in quotes by [@jdx](https://github.com/jdx) in [#3679](https://github.com/jdx/mise/pull/3679)
- pass pristine env to tera in final_env by [@risu729](https://github.com/risu729) in [#3682](https://github.com/jdx/mise/pull/3682)
- trap panics in task resolving by [@jdx](https://github.com/jdx) in [#3677](https://github.com/jdx/mise/pull/3677)

### 📚 Documentation

- mark new features as experimental by [@syhol](https://github.com/syhol) in [#3659](https://github.com/jdx/mise/pull/3659)

### 🧪 Testing

- add test cases for venv templates by [@jdx](https://github.com/jdx) in [#3683](https://github.com/jdx/mise/pull/3683)

### New Contributors

- @NavyD made their first contribution in [#3664](https://github.com/jdx/mise/pull/3664)

## [2024.12.13](https://github.com/jdx/mise/compare/v2024.12.12..v2024.12.13) - 2024-12-17

### 🚀 Features

- `mise task add` by [@jdx](https://github.com/jdx) in [#3616](https://github.com/jdx/mise/pull/3616)
- elixir core tool by [@jdx](https://github.com/jdx) in [#3620](https://github.com/jdx/mise/pull/3620)
- elixir on windows by [@jdx](https://github.com/jdx) in [#3623](https://github.com/jdx/mise/pull/3623)
- added install_env tool option by [@jdx](https://github.com/jdx) in [#3622](https://github.com/jdx/mise/pull/3622)
- Add Powershell support by [@fgilcc](https://github.com/fgilcc) in [#3506](https://github.com/jdx/mise/pull/3506)
- improve redactions by [@jdx](https://github.com/jdx) in [#3647](https://github.com/jdx/mise/pull/3647)

### 🐛 Bug Fixes

- run venv after tools are loaded by [@jdx](https://github.com/jdx) in [#3612](https://github.com/jdx/mise/pull/3612)
- some improvements to `mise fmt` by [@jdx](https://github.com/jdx) in [#3615](https://github.com/jdx/mise/pull/3615)
- always run postinstall hook by [@jdx](https://github.com/jdx) in [#3618](https://github.com/jdx/mise/pull/3618)
- move bat from aqua to ubi by [@jdx](https://github.com/jdx) in [60d0c79](https://github.com/jdx/mise/commit/60d0c798f695199bdc81f8beec737f0e2a8589e0)
- do not require version for `mise sh --unset` by [@jdx](https://github.com/jdx) in [#3628](https://github.com/jdx/mise/pull/3628)
- back nomad with nomad, not levant by [@rliebz](https://github.com/rliebz) in [#3633](https://github.com/jdx/mise/pull/3633)
- correct python precompiled urls for freebsd by [@jdx](https://github.com/jdx) in [#3637](https://github.com/jdx/mise/pull/3637)
- bug fixes with tools=true in env by [@jdx](https://github.com/jdx) in [#3639](https://github.com/jdx/mise/pull/3639)
- sort keys in `__MISE_DIFF` to make the serialised value deterministic by [@joshbode](https://github.com/joshbode) in [#3640](https://github.com/jdx/mise/pull/3640)
- resolve config_root for dir tasks option by [@risu729](https://github.com/risu729) in [#3649](https://github.com/jdx/mise/pull/3649)

### 📚 Documentation

- add getting-started carousel by [@hverlin](https://github.com/hverlin) in [#3613](https://github.com/jdx/mise/pull/3613)
- Fix Sops URL by [@matthew-snyder](https://github.com/matthew-snyder) in [#3619](https://github.com/jdx/mise/pull/3619)
- add elixir to sidebar by [@risu729](https://github.com/risu729) in [#3650](https://github.com/jdx/mise/pull/3650)
- update task documentation by [@hverlin](https://github.com/hverlin) in [#3651](https://github.com/jdx/mise/pull/3651)

### Chore

- format toml with taplo by [@jdx](https://github.com/jdx) in [#3625](https://github.com/jdx/mise/pull/3625)
- add platform field to registry backends by [@jdx](https://github.com/jdx) in [#3626](https://github.com/jdx/mise/pull/3626)

### New Contributors

- @fgilcc made their first contribution in [#3506](https://github.com/jdx/mise/pull/3506)
- @rliebz made their first contribution in [#3633](https://github.com/jdx/mise/pull/3633)
- @matthew-snyder made their first contribution in [#3619](https://github.com/jdx/mise/pull/3619)

## [2024.12.12](https://github.com/jdx/mise/compare/v2024.12.11..v2024.12.12) - 2024-12-16

### 🚀 Features

- Add upx,actionlint and correct ripsecret error by [@boris-smidt-klarrio](https://github.com/boris-smidt-klarrio) in [#3601](https://github.com/jdx/mise/pull/3601)
- aqua:argo-cd by [@boris-smidt-klarrio](https://github.com/boris-smidt-klarrio) in [#3600](https://github.com/jdx/mise/pull/3600)
- task tools by [@jdx](https://github.com/jdx) in [#3599](https://github.com/jdx/mise/pull/3599)
- lazy env eval by [@jdx](https://github.com/jdx) in [#3598](https://github.com/jdx/mise/pull/3598)
- added cache feature to templates by [@jdx](https://github.com/jdx) in [#3608](https://github.com/jdx/mise/pull/3608)

### 🐛 Bug Fixes

- added MISE_SOPS_ROPS setting by [@jdx](https://github.com/jdx) in [#3603](https://github.com/jdx/mise/pull/3603)
- respect CLICOLOR_FORCE by [@jdx](https://github.com/jdx) in [#3607](https://github.com/jdx/mise/pull/3607)
- only create 1 venv by [@jdx](https://github.com/jdx) in [#3610](https://github.com/jdx/mise/pull/3610)
- set bash --noprofile for env._.source by [@jdx](https://github.com/jdx) in [#3611](https://github.com/jdx/mise/pull/3611)

### 📚 Documentation

- improve settings a bit by [@jdx](https://github.com/jdx) in [d53d011](https://github.com/jdx/mise/commit/d53d01195e88e82d9a88a410e8feb991c1e8179d)
- Install on Windows - Update doc on install on Windows with Scoop and WinGet + fix NOTE section by [@o-l-a-v](https://github.com/o-l-a-v) in [#3604](https://github.com/jdx/mise/pull/3604)
- remove note about winget by [@jdx](https://github.com/jdx) in [9c0c1ce](https://github.com/jdx/mise/commit/9c0c1ce943c6fb54ca049d6cdfb81c1122987d05)

### Chore

- disable automatic cargo up on release by [@jdx](https://github.com/jdx) in [3f0d91a](https://github.com/jdx/mise/commit/3f0d91a40928df8ed10cef1837730d8c3a15efea)

### New Contributors

- @o-l-a-v made their first contribution in [#3604](https://github.com/jdx/mise/pull/3604)

## [2024.12.11](https://github.com/jdx/mise/compare/v2024.12.10..v2024.12.11) - 2024-12-15

### 🚀 Features

- added selector for `mise use` with no args by [@jdx](https://github.com/jdx) in [#3570](https://github.com/jdx/mise/pull/3570)
- added tool descriptions by [@jdx](https://github.com/jdx) in [#3571](https://github.com/jdx/mise/pull/3571)
- added `mise sync python --uv` by [@jdx](https://github.com/jdx) in [#3575](https://github.com/jdx/mise/pull/3575)
- `sync ruby --brew` by [@jdx](https://github.com/jdx) in [#3577](https://github.com/jdx/mise/pull/3577)
- encrypted configs by [@jdx](https://github.com/jdx) in [#3584](https://github.com/jdx/mise/pull/3584)
- added `mise --no-config` by [@jdx](https://github.com/jdx) in [#3590](https://github.com/jdx/mise/pull/3590)
- allow _.file in vars by [@jdx](https://github.com/jdx) in [#3593](https://github.com/jdx/mise/pull/3593)

### 🐛 Bug Fixes

- **(python)** reduce network usage for python precompiled manifests by [@jdx](https://github.com/jdx) in [#3568](https://github.com/jdx/mise/pull/3568)
- **(python)** check only if first or specified python is installed for _.venv by [@jdx](https://github.com/jdx) in [#3576](https://github.com/jdx/mise/pull/3576)
- **(swift)** prevent swift from using linux platforms that are not available by [@jdx](https://github.com/jdx) in [#3583](https://github.com/jdx/mise/pull/3583)
- correct headers on `mise ls` by [@jdx](https://github.com/jdx) in [5af3b17](https://github.com/jdx/mise/commit/5af3b17a41decd2d7368f5985f2cb5d3e3b341e8)
- correct message truncation in `mise run` by [@jdx](https://github.com/jdx) in [c668857](https://github.com/jdx/mise/commit/c6688571cfb0eca70a55377b70ec6b9cd0cb6a68)
- include uv in path for hook-env by [@jdx](https://github.com/jdx) in [#3572](https://github.com/jdx/mise/pull/3572)
- correct subtitle in `mise use` selector by [@jdx](https://github.com/jdx) in [4be6d79](https://github.com/jdx/mise/commit/4be6d798f9398f9e072d4067a56e134463e71b41)
- some bugs with status.show_tools and status.show_env by [@jdx](https://github.com/jdx) in [#3586](https://github.com/jdx/mise/pull/3586)
- use task.display_name for `mise run` by [@jdx](https://github.com/jdx) in [a009de1](https://github.com/jdx/mise/commit/a009de13ffa4319de89b0fcaf1ba54ae2524a9b6)
- path is treated differently in nushell by [@samuelallan72](https://github.com/samuelallan72) in [#3592](https://github.com/jdx/mise/pull/3592)
- allow number/bool in .env.json by [@jdx](https://github.com/jdx) in [#3594](https://github.com/jdx/mise/pull/3594)

### 🚜 Refactor

- break up env_directive by [@jdx](https://github.com/jdx) in [#3587](https://github.com/jdx/mise/pull/3587)

### 📚 Documentation

- better warning when venv auto create is skipped by [@syhol](https://github.com/syhol) in [#3573](https://github.com/jdx/mise/pull/3573)
- added rendered go settings by [@jdx](https://github.com/jdx) in [b41c3dd](https://github.com/jdx/mise/commit/b41c3dd8cfd97f97352900a9d856194185347e8d)

### New Contributors

- @fhalim made their first contribution in [#3595](https://github.com/jdx/mise/pull/3595)

## [2024.12.10](https://github.com/jdx/mise/compare/v2024.12.9..v2024.12.10) - 2024-12-14

### 🚀 Features

- **(python)** add other indygreg flavors by [@jdx](https://github.com/jdx) in [#3565](https://github.com/jdx/mise/pull/3565)
- redactions by [@jdx](https://github.com/jdx) in [#3529](https://github.com/jdx/mise/pull/3529)
- show unload messages/run leave hook by [@jdx](https://github.com/jdx) in [#3532](https://github.com/jdx/mise/pull/3532)
- update demand and default `mise run` to filtering by [@jdx](https://github.com/jdx) in [48c366d](https://github.com/jdx/mise/commit/48c366d4d2256f6b12aabcbe82abe429622b120e)

### 🐛 Bug Fixes

- **(go)** only use "v" prefix if version is semver-like by [@jdx](https://github.com/jdx) in [#3556](https://github.com/jdx/mise/pull/3556)
- **(go)** fix non-v installs by [@jdx](https://github.com/jdx) in [36e7631](https://github.com/jdx/mise/commit/36e7631e26445f9f2bc34fd09a93ba9a15363c98)
- disable libgit2 for updating plugin repos for now by [@jdx](https://github.com/jdx) in [#3533](https://github.com/jdx/mise/pull/3533)
- rename kubelogin to azure-kubelogin and add replace it with more popular kubelogin cli by [@jdx](https://github.com/jdx) in [#3534](https://github.com/jdx/mise/pull/3534)
- add backend to lockfile by [@jdx](https://github.com/jdx) in [#3535](https://github.com/jdx/mise/pull/3535)
- parse task env vars as templates by [@jdx](https://github.com/jdx) in [#3536](https://github.com/jdx/mise/pull/3536)
- do not add ignore file if not tty by [@jdx](https://github.com/jdx) in [#3558](https://github.com/jdx/mise/pull/3558)
- improve output of `mise tasks` by [@jdx](https://github.com/jdx) in [#3562](https://github.com/jdx/mise/pull/3562)

### 📚 Documentation

- add installation via zinit by [@Finkregh](https://github.com/Finkregh) in [#3563](https://github.com/jdx/mise/pull/3563)

### Chore

- added comfy-table by [@jdx](https://github.com/jdx) in [#3561](https://github.com/jdx/mise/pull/3561)
- pitchfork by [@jdx](https://github.com/jdx) in [2c47f72](https://github.com/jdx/mise/commit/2c47f721c03e8fed57a8ae5ed2f63a0649ffaa9b)
- updated usage by [@jdx](https://github.com/jdx) in [#3564](https://github.com/jdx/mise/pull/3564)
- added install-dev task by [@jdx](https://github.com/jdx) in [0c351a8](https://github.com/jdx/mise/commit/0c351a83d952cff8b953fd5c244698a14d74c305)

### New Contributors

- @Finkregh made their first contribution in [#3563](https://github.com/jdx/mise/pull/3563)

## [2024.12.9](https://github.com/jdx/mise/compare/v2024.12.8..v2024.12.9) - 2024-12-14

### 🚀 Features

- **(tasks)** optional automatic outputs by [@jdx](https://github.com/jdx) in [#3528](https://github.com/jdx/mise/pull/3528)
- added quiet field to tasks by [@jdx](https://github.com/jdx) in [#3514](https://github.com/jdx/mise/pull/3514)
- show instructions for updating when min_version does not match by [@jdx](https://github.com/jdx) in [#3520](https://github.com/jdx/mise/pull/3520)
- several enhancements to tasks by [@jdx](https://github.com/jdx) in [#3526](https://github.com/jdx/mise/pull/3526)

### 🐛 Bug Fixes

- make bash_completions lib optional by [@jdx](https://github.com/jdx) in [#3516](https://github.com/jdx/mise/pull/3516)
- make plugin update work with libgit2 by [@jdx](https://github.com/jdx) in [#3519](https://github.com/jdx/mise/pull/3519)
- bug with `mise task edit` and new tasks by [@jdx](https://github.com/jdx) in [#3521](https://github.com/jdx/mise/pull/3521)
- correct self-update message by [@jdx](https://github.com/jdx) in [eff0cff](https://github.com/jdx/mise/commit/eff0cffca079ee58fc2297396604b96e0253c324)
- task source bug fixes by [@jdx](https://github.com/jdx) in [#3522](https://github.com/jdx/mise/pull/3522)

### 📚 Documentation

- add explanation about shebang by [@hverlin](https://github.com/hverlin) in [#3501](https://github.com/jdx/mise/pull/3501)
- add vitepress-plugin-group-icons by [@hverlin](https://github.com/hverlin) in [#3527](https://github.com/jdx/mise/pull/3527)

### 🧪 Testing

- pin swift version by [@jdx](https://github.com/jdx) in [2b966a4](https://github.com/jdx/mise/commit/2b966a4945851b35be593182527bd40a80279fe4)
- skip firebase by [@jdx](https://github.com/jdx) in [e5714bc](https://github.com/jdx/mise/commit/e5714bcfe9cd45f173aecefcbd3c95fbeab83417)

### 📦️ Dependency Updates

- update rust crate bzip2 to 0.5 by [@renovate[bot]](https://github.com/renovate[bot]) in [#3511](https://github.com/jdx/mise/pull/3511)

## [2024.12.8](https://github.com/jdx/mise/compare/v2024.12.7..v2024.12.8) - 2024-12-12

### 🚀 Features

- **(registry)** use pipx for pdm by [@risu729](https://github.com/risu729) in [#3504](https://github.com/jdx/mise/pull/3504)
- added pitchfork by [@jdx](https://github.com/jdx) in [bac731e](https://github.com/jdx/mise/commit/bac731e47f00245ce13e7eec5716509704519d71)

### 🐛 Bug Fixes

- Adds support for multi-use args by [@bnorick](https://github.com/bnorick) in [#3505](https://github.com/jdx/mise/pull/3505)
- make task completion script POSIX by [@jdx](https://github.com/jdx) in [b92b560](https://github.com/jdx/mise/commit/b92b5603bb23d55b58e7ee8effe8d6293036c5a9)

### 📚 Documentation

- Add more examples for toml tasks by [@hverlin](https://github.com/hverlin) in [#3491](https://github.com/jdx/mise/pull/3491)

### Chore

- use main branch for winget by [@jdx](https://github.com/jdx) in [b4036cf](https://github.com/jdx/mise/commit/b4036cf0d10f6ccd8758b0bebc341963c8777d2e)

### New Contributors

- @bnorick made their first contribution in [#3505](https://github.com/jdx/mise/pull/3505)
- @biggusbeetus made their first contribution in [#3502](https://github.com/jdx/mise/pull/3502)

## [2024.12.7](https://github.com/jdx/mise/compare/v2024.12.6..v2024.12.7) - 2024-12-12

### 🚀 Features

- add the users PATH to `mise doctor` by [@syhol](https://github.com/syhol) in [#3474](https://github.com/jdx/mise/pull/3474)
- feat : Add superfile with aqua backend to registery by [@yodatak](https://github.com/yodatak) in [#3479](https://github.com/jdx/mise/pull/3479)
- added `task_auto_install` setting by [@jdx](https://github.com/jdx) in [#3481](https://github.com/jdx/mise/pull/3481)
- Add yazi with aqua backend to registery by [@yodatak](https://github.com/yodatak) in [#3485](https://github.com/jdx/mise/pull/3485)
- Migrating Terragrunt asdf plugin over to gruntwork-io by [@yhakbar](https://github.com/yhakbar) in [#3486](https://github.com/jdx/mise/pull/3486)
- add settings for python venv creation by [@jdx](https://github.com/jdx) in [#3489](https://github.com/jdx/mise/pull/3489)
- added MISE_ARCH setting by [@jdx](https://github.com/jdx) in [#3490](https://github.com/jdx/mise/pull/3490)
- add jj to registry by [@phyrog](https://github.com/phyrog) in [#3495](https://github.com/jdx/mise/pull/3495)
- add task descriptions to completions by [@jdx](https://github.com/jdx) in [#3497](https://github.com/jdx/mise/pull/3497)

### 🐛 Bug Fixes

- mise upgrade with rust by [@jdx](https://github.com/jdx) in [#3475](https://github.com/jdx/mise/pull/3475)
- improve arg parsing for mise watch by [@jdx](https://github.com/jdx) in [#3478](https://github.com/jdx/mise/pull/3478)
- skip reading ignored config dirs by [@jdx](https://github.com/jdx) in [#3480](https://github.com/jdx/mise/pull/3480)
- deprecated attribute in json schema by [@jdx](https://github.com/jdx) in [#3482](https://github.com/jdx/mise/pull/3482)
- simplify auto_install settings by [@jdx](https://github.com/jdx) in [#3483](https://github.com/jdx/mise/pull/3483)
- use config_root for env._.source by [@jdx](https://github.com/jdx) in [#3484](https://github.com/jdx/mise/pull/3484)
- allow directories as task source by [@jdx](https://github.com/jdx) in [#3488](https://github.com/jdx/mise/pull/3488)
- Use arguments for to pass staged filenames to pre-commit task by [@joshbode](https://github.com/joshbode) in [#3492](https://github.com/jdx/mise/pull/3492)

### 📚 Documentation

- updated `mise watch` docs to drop the `-t` by [@jdx](https://github.com/jdx) in [8ea6226](https://github.com/jdx/mise/commit/8ea622688cb01a0a0a2805692b38a4a7f1340ce5)

### Chore

- move debug log to trace by [@jdx](https://github.com/jdx) in [5c6c884](https://github.com/jdx/mise/commit/5c6c884cf51e704d1c8c347790ec30b30b0f401e)

### New Contributors

- @yhakbar made their first contribution in [#3486](https://github.com/jdx/mise/pull/3486)

## [2024.12.6](https://github.com/jdx/mise/compare/v2024.12.5..v2024.12.6) - 2024-12-11

### 🚀 Features

- added descriptions to `mise run` by [@jdx](https://github.com/jdx) in [#3460](https://github.com/jdx/mise/pull/3460)
- `mise format` by [@jdx](https://github.com/jdx) in [#3461](https://github.com/jdx/mise/pull/3461)
- `mise fmt` (renamed from `mise format`) by [@jdx](https://github.com/jdx) in [#3465](https://github.com/jdx/mise/pull/3465)
- `mise format` by [@jdx](https://github.com/jdx) in [d18b040](https://github.com/jdx/mise/commit/d18b040b8ae8eea16ed98b7f7b884a6f52797edc)

### 🐛 Bug Fixes

- **(swift)** remove clang bins by [@jdx](https://github.com/jdx) in [#3468](https://github.com/jdx/mise/pull/3468)
- use 7zip for windows zip by [@jdx](https://github.com/jdx) in [475ae62](https://github.com/jdx/mise/commit/475ae62d209795cf8fe9cc846f258755e1092918)
- disable filtering by default on `mise run` by [@jdx](https://github.com/jdx) in [507ee27](https://github.com/jdx/mise/commit/507ee27a736b8cd57714a8365fc88855edf62507)
- deprecate direnv integration by [@jdx](https://github.com/jdx) in [#3464](https://github.com/jdx/mise/pull/3464)
- remove hidden commands from docs by [@jdx](https://github.com/jdx) in [42a9a05](https://github.com/jdx/mise/commit/42a9a0567fbd8ef61550cf2bfe956074777c7d76)
- improve hook-env by [@jdx](https://github.com/jdx) in [#3466](https://github.com/jdx/mise/pull/3466)
- deprecate @system versions by [@jdx](https://github.com/jdx) in [#3467](https://github.com/jdx/mise/pull/3467)
- do not reuse local tool options for `mise use -g` by [@jdx](https://github.com/jdx) in [#3469](https://github.com/jdx/mise/pull/3469)
- allow "~" in python.default_packages_file by [@jdx](https://github.com/jdx) in [#3472](https://github.com/jdx/mise/pull/3472)
- read all config files for `mise set` by [@jdx](https://github.com/jdx) in [#3473](https://github.com/jdx/mise/pull/3473)

### 📚 Documentation

- fixing elvish install instructions by [@ejrichards](https://github.com/ejrichards) in [#3459](https://github.com/jdx/mise/pull/3459)
- remove bad formatting in setting by [@jdx](https://github.com/jdx) in [f33813b](https://github.com/jdx/mise/commit/f33813bde40cf65e946a3c1773a4275fce3cb0ef)
- added external links by [@jdx](https://github.com/jdx) in [8271e7b](https://github.com/jdx/mise/commit/8271e7ba0fa8628279cff0460715ec9c80a1c6bd)

### Chore

- fix windows zip structure by [@jdx](https://github.com/jdx) in [195039f](https://github.com/jdx/mise/commit/195039ff2bbe702c7e80ace3fcaeb95cb02d018b)

### New Contributors

- @ejrichards made their first contribution in [#3459](https://github.com/jdx/mise/pull/3459)

## [2024.12.5](https://github.com/jdx/mise/compare/v2024.12.4..v2024.12.5) - 2024-12-10

### 🚀 Features

- make `mise trust` act on directories instead of files by [@jdx](https://github.com/jdx) in [#3454](https://github.com/jdx/mise/pull/3454)

### 🐛 Bug Fixes

- correctly lowercase "zsh" for shell hooks by [@jdx](https://github.com/jdx) in [035ae59](https://github.com/jdx/mise/commit/035ae59bd898a16be4fcd55b708ae8ba620c60fe)
- read MISE_CONFIG_DIR/conf.d/*.toml configs by [@jdx](https://github.com/jdx) in [#3439](https://github.com/jdx/mise/pull/3439)
- retains spm artifacts by [@jdx](https://github.com/jdx) in [#3441](https://github.com/jdx/mise/pull/3441)
- add env var for MISE_NPM_BUN setting by [@jdx](https://github.com/jdx) in [b3c57e2](https://github.com/jdx/mise/commit/b3c57e29bd26d772e2f708351a3c61bf04ee3d65)
- hide hidden tasks in `mise run` selector UI by [@jdx](https://github.com/jdx) in [#3449](https://github.com/jdx/mise/pull/3449)
- trim run scripts whitespace by [@jdx](https://github.com/jdx) in [#3450](https://github.com/jdx/mise/pull/3450)
- shell-escape arg() in tasks by [@jdx](https://github.com/jdx) in [#3453](https://github.com/jdx/mise/pull/3453)
- use shebang in run script to determine how arg escaping should work by [@jdx](https://github.com/jdx) in [#3455](https://github.com/jdx/mise/pull/3455)

### 📚 Documentation

- example with required version by [@felixhummel](https://github.com/felixhummel) in [#3448](https://github.com/jdx/mise/pull/3448)
- document new windows installers by [@jdx](https://github.com/jdx) in [#3452](https://github.com/jdx/mise/pull/3452)

### Chore

- added winget workflow by [@jdx](https://github.com/jdx) in [901e048](https://github.com/jdx/mise/commit/901e04865842f765188dd687584f9120ad4e5519)

### New Contributors

- @felixhummel made their first contribution in [#3448](https://github.com/jdx/mise/pull/3448)

## [2024.12.4](https://github.com/jdx/mise/compare/v2024.12.3..v2024.12.4) - 2024-12-09

### 🚀 Features

- add staged files to `mise generate git-pre-commit` by [@jdx](https://github.com/jdx) in [#3410](https://github.com/jdx/mise/pull/3410)
- shell hooks by [@jdx](https://github.com/jdx) in [#3414](https://github.com/jdx/mise/pull/3414)
- added cowsay by [@jdx](https://github.com/jdx) in [#3420](https://github.com/jdx/mise/pull/3420)
- add openbao by [@phyrog](https://github.com/phyrog) in [#3426](https://github.com/jdx/mise/pull/3426)
- add gocryptfs by [@phyrog](https://github.com/phyrog) in [#3427](https://github.com/jdx/mise/pull/3427)
- use aqua for flyctl by [@jdx](https://github.com/jdx) in [f7ed363](https://github.com/jdx/mise/commit/f7ed363b3eebb82e6242061e78f9ebfdf050d154)

### 🐛 Bug Fixes

- do not set debug mode when calling `mise -v` by [@jdx](https://github.com/jdx) in [#3418](https://github.com/jdx/mise/pull/3418)
- issue with usage and arg completions by [@jdx](https://github.com/jdx) in [#3433](https://github.com/jdx/mise/pull/3433)

### 📚 Documentation

- Small documentation improvements by [@hverlin](https://github.com/hverlin) in [#3413](https://github.com/jdx/mise/pull/3413)
- updated demo.gif by [@jdx](https://github.com/jdx) in [#3419](https://github.com/jdx/mise/pull/3419)

### Build

- update default.nix by [@minhtrancccp](https://github.com/minhtrancccp) in [#3430](https://github.com/jdx/mise/pull/3430)

### New Contributors

- @will-ockmore made their first contribution in [#3435](https://github.com/jdx/mise/pull/3435)
- @minhtrancccp made their first contribution in [#3430](https://github.com/jdx/mise/pull/3430)
- @phyrog made their first contribution in [#3427](https://github.com/jdx/mise/pull/3427)

## [2024.12.3](https://github.com/jdx/mise/compare/v2024.12.2..v2024.12.3) - 2024-12-08

### 🚀 Features

- add danger-swift by [@msnazarow](https://github.com/msnazarow) in [#3406](https://github.com/jdx/mise/pull/3406)

### 📚 Documentation

- **(backend)** fix git url syntax example by [@risu729](https://github.com/risu729) in [#3404](https://github.com/jdx/mise/pull/3404)
- update dev-tools overview documentation by [@hverlin](https://github.com/hverlin) in [#3400](https://github.com/jdx/mise/pull/3400)

### ⚡ Performance

- increase performance of watch_files by [@jdx](https://github.com/jdx) in [#3407](https://github.com/jdx/mise/pull/3407)
- make `ls --offline` default behavior by [@jdx](https://github.com/jdx) in [#3409](https://github.com/jdx/mise/pull/3409)

### New Contributors

- @msnazarow made their first contribution in [#3406](https://github.com/jdx/mise/pull/3406)

## [2024.12.2](https://github.com/jdx/mise/compare/v2024.12.1..v2024.12.2) - 2024-12-07

### 🚀 Features

- **(registry)** add zls to registry by [@hverlin](https://github.com/hverlin) in [#3392](https://github.com/jdx/mise/pull/3392)
- Add --json-extended option to mise env by [@hverlin](https://github.com/hverlin) in [#3389](https://github.com/jdx/mise/pull/3389)

### 🐛 Bug Fixes

- **(config)** set config_root for tasks defined in included toml files by [@risu729](https://github.com/risu729) in [#3388](https://github.com/jdx/mise/pull/3388)
- global hooks by [@jdx](https://github.com/jdx) in [#3393](https://github.com/jdx/mise/pull/3393)
- only run watch_file hook when it has changed file by [@jdx](https://github.com/jdx) in [#3394](https://github.com/jdx/mise/pull/3394)
- bug with aliasing core tools by [@jdx](https://github.com/jdx) in [#3395](https://github.com/jdx/mise/pull/3395)
- remove shims directory before activating by [@jdx](https://github.com/jdx) in [#3396](https://github.com/jdx/mise/pull/3396)

### 🚜 Refactor

- use github crate to list zig releases by [@risu729](https://github.com/risu729) in [#3386](https://github.com/jdx/mise/pull/3386)

### 📚 Documentation

- add zig to core tools by [@risu729](https://github.com/risu729) in [#3385](https://github.com/jdx/mise/pull/3385)

### Chore

- debug log by [@jdx](https://github.com/jdx) in [0075db0](https://github.com/jdx/mise/commit/0075db05a24a9bc2e3015b8a48bcfe730fe80d07)

## [2024.12.1](https://github.com/jdx/mise/compare/v2024.12.0..v2024.12.1) - 2024-12-06

### 🚀 Features

- **(registry)** use aqua for some tools by [@risu729](https://github.com/risu729) in [#3375](https://github.com/jdx/mise/pull/3375)
- allow filtering `mise bin-paths` on tools by [@jdx](https://github.com/jdx) in [#3367](https://github.com/jdx/mise/pull/3367)
- added aws-cli from aqua by [@jdx](https://github.com/jdx) in [#3370](https://github.com/jdx/mise/pull/3370)
- multiple MISE_ENV environments by [@jdx](https://github.com/jdx) in [#3371](https://github.com/jdx/mise/pull/3371)
- add mise-task.json schema by [@hverlin](https://github.com/hverlin) in [#3374](https://github.com/jdx/mise/pull/3374)
- automatically call `hook-env` by [@jdx](https://github.com/jdx) in [#3373](https://github.com/jdx/mise/pull/3373)

### 🐛 Bug Fixes

- **(docs)** correct syntax error in IDE integration examples by [@EricGusmao](https://github.com/EricGusmao) in [#3360](https://github.com/jdx/mise/pull/3360)
- ensure version check message is displayed by [@jdx](https://github.com/jdx) in [#3358](https://github.com/jdx/mise/pull/3358)
- show warning if no precompiled pythons found by [@jdx](https://github.com/jdx) in [#3359](https://github.com/jdx/mise/pull/3359)
- allow compilation not on macOS, Linux, or Windows by [@avysk](https://github.com/avysk) in [#3363](https://github.com/jdx/mise/pull/3363)
- make hook-env compatible with zsh auto_name_dirs by [@jdx](https://github.com/jdx) in [#3366](https://github.com/jdx/mise/pull/3366)
- skip optional env._.file files by [@jdx](https://github.com/jdx) in [#3381](https://github.com/jdx/mise/pull/3381)
- .terraform-version by [@jdx](https://github.com/jdx) in [#3380](https://github.com/jdx/mise/pull/3380)

### 📚 Documentation

- update auto-completion docs by [@hverlin](https://github.com/hverlin) in [#3355](https://github.com/jdx/mise/pull/3355)
- fix `Environment variables passed to tasks` section by [@hverlin](https://github.com/hverlin) in [#3378](https://github.com/jdx/mise/pull/3378)

### 🧪 Testing

- try to fix coverage rate limits by [@jdx](https://github.com/jdx) in [#3384](https://github.com/jdx/mise/pull/3384)

### New Contributors

- @avysk made their first contribution in [#3363](https://github.com/jdx/mise/pull/3363)
- @EricGusmao made their first contribution in [#3360](https://github.com/jdx/mise/pull/3360)

## [2024.12.0](https://github.com/jdx/mise/compare/v2024.11.37..v2024.12.0) - 2024-12-04

### 🚀 Features

- **(erlang)** use precompiled binaries for macos by [@jdx](https://github.com/jdx) in [#3353](https://github.com/jdx/mise/pull/3353)
- add upctl by [@scop](https://github.com/scop) in [#3309](https://github.com/jdx/mise/pull/3309)
- Add `json-with-sources` option to settings ls by [@hverlin](https://github.com/hverlin) in [#3307](https://github.com/jdx/mise/pull/3307)
- add ripsecrets to registry.toml by [@boris-smidt-klarrio](https://github.com/boris-smidt-klarrio) in [#3334](https://github.com/jdx/mise/pull/3334)
- Add kyverno-cli by [@boris-smidt-klarrio](https://github.com/boris-smidt-klarrio) in [#3336](https://github.com/jdx/mise/pull/3336)

### 🐛 Bug Fixes

- add exec to `mise g git-pre-commit` by [@jdx](https://github.com/jdx) in [27a3aef](https://github.com/jdx/mise/commit/27a3aefa767c8ef142009dd54c4d7dcc19c235b2)
- bake gpg keys in by [@jdx](https://github.com/jdx) in [#3318](https://github.com/jdx/mise/pull/3318)
- deprecate `mise local|global` by [@jdx](https://github.com/jdx) in [#3350](https://github.com/jdx/mise/pull/3350)

### 🚜 Refactor

- use aqua for ruff by [@scop](https://github.com/scop) in [#3316](https://github.com/jdx/mise/pull/3316)

### 📚 Documentation

- add terraform recipe to the cookbook by [@AliSajid](https://github.com/AliSajid) in [#3305](https://github.com/jdx/mise/pull/3305)
- fix git examples for cargo backend by [@tmeijn](https://github.com/tmeijn) in [#3335](https://github.com/jdx/mise/pull/3335)

### 🧪 Testing

- remove non-working maven test by [@jdx](https://github.com/jdx) in [5a3ed16](https://github.com/jdx/mise/commit/5a3ed16efb29dbf80f5ac251eec39e3a462d2219)
- remove gleam by [@jdx](https://github.com/jdx) in [fdfe20b](https://github.com/jdx/mise/commit/fdfe20b32b16b835655551d3f12b5d6e90856b2e)
- use latest golang in e2e test by [@jdx](https://github.com/jdx) in [#3349](https://github.com/jdx/mise/pull/3349)

### Chore

- upgrade usage-lib by [@jdx](https://github.com/jdx) in [554d533](https://github.com/jdx/mise/commit/554d533a253a137c27c5cdac6da2ae09629029dc)
- use asdf:mise-plugins/mise-nim by [@jdx](https://github.com/jdx) in [#3352](https://github.com/jdx/mise/pull/3352)

### New Contributors

- @leogurja made their first contribution in [#3341](https://github.com/jdx/mise/pull/3341)
- @tmeijn made their first contribution in [#3335](https://github.com/jdx/mise/pull/3335)
- @boris-smidt-klarrio made their first contribution in [#3336](https://github.com/jdx/mise/pull/3336)
- @AliSajid made their first contribution in [#3305](https://github.com/jdx/mise/pull/3305)

## [2024.11.37](https://github.com/jdx/mise/compare/v2024.11.36..v2024.11.37) - 2024-11-30

### 🚀 Features

- add black by [@scop](https://github.com/scop) in [#3292](https://github.com/jdx/mise/pull/3292)
- migrate more tools away from asdf by [@jdx](https://github.com/jdx) in [40f92c6](https://github.com/jdx/mise/commit/40f92c6b0e1fefd171dd44ee9f62f1f597ee352c)

### 🐛 Bug Fixes

- handle General/Complex Versioning in --bump by [@liskin](https://github.com/liskin) in [#2889](https://github.com/jdx/mise/pull/2889)
- broken path example by [@minddust](https://github.com/minddust) in [#3296](https://github.com/jdx/mise/pull/3296)
- swift path on macos by [@jdx](https://github.com/jdx) in [#3299](https://github.com/jdx/mise/pull/3299)
- do not auto-install on `mise x` if some tools are passed by [@jdx](https://github.com/jdx) in [35d31a1](https://github.com/jdx/mise/commit/35d31a1baf96fe6f0e764e26228c1b03ba24ddce)
- fix: also make certain we are not auto installing inside shims by checking by [@jdx](https://github.com/jdx) in [b0c4a74](https://github.com/jdx/mise/commit/b0c4a749309064825852041d8d72c7eac9fb116c)
- cache github release information for 24 hours by [@jdx](https://github.com/jdx) in [#3300](https://github.com/jdx/mise/pull/3300)

### 🚜 Refactor

- use aqua for snyk by [@scop](https://github.com/scop) in [#3290](https://github.com/jdx/mise/pull/3290)

### ◀️ Revert

- Revert "fix: always prefer glibc to musl in mise run " by [@jdx](https://github.com/jdx) in [#3298](https://github.com/jdx/mise/pull/3298)

### Chore

- bump expr-lang by [@jdx](https://github.com/jdx) in [#3297](https://github.com/jdx/mise/pull/3297)
- mise up --bump by [@jdx](https://github.com/jdx) in [6872b54](https://github.com/jdx/mise/commit/6872b5469622140335a12131dfa4acf310fc0c2a)
- update mise.lock by [@jdx](https://github.com/jdx) in [4c12502](https://github.com/jdx/mise/commit/4c12502c459ba2e214689c3f55d964b8f75966af)
- disable tool tests until I can sort out gh rate limit issues by [@jdx](https://github.com/jdx) in [f42f010](https://github.com/jdx/mise/commit/f42f010f03a57cab128290c0b9d936fd7a90c785)

### New Contributors

- @minddust made their first contribution in [#3296](https://github.com/jdx/mise/pull/3296)

## [2024.11.36](https://github.com/jdx/mise/compare/v2024.11.35..v2024.11.36) - 2024-11-29

### Chore

- mise i by [@jdx](https://github.com/jdx) in [8150732](https://github.com/jdx/mise/commit/81507327e7f1c9f2137b3dadcf35a8245d43a8ba)

## [2024.11.35](https://github.com/jdx/mise/compare/v2024.11.34..v2024.11.35) - 2024-11-29

### 🚀 Features

- migrate more tools away from asdf by [@jdx](https://github.com/jdx) in [#3279](https://github.com/jdx/mise/pull/3279)

### 🐛 Bug Fixes

- remove conflicting MISE_SHELL setting by [@jdx](https://github.com/jdx) in [#3284](https://github.com/jdx/mise/pull/3284)

### 🚜 Refactor

- simplify __MISE_WATCH variable to only contain the most recent timestamp by [@jdx](https://github.com/jdx) in [#3282](https://github.com/jdx/mise/pull/3282)

### 🧪 Testing

- remove unnecessary cargo-binstall test by [@jdx](https://github.com/jdx) in [0a4da7a](https://github.com/jdx/mise/commit/0a4da7a023b1cb969b732afd3ad4b3cf02c42530)

### Chore

- dont require build-windows before unit-windows by [@jdx](https://github.com/jdx) in [c85e2ec](https://github.com/jdx/mise/commit/c85e2ec77193d73ff20d4ce8fb7e3787a6db223d)

## [2024.11.34](https://github.com/jdx/mise/compare/v2024.11.33..v2024.11.34) - 2024-11-29

### 🚀 Features

- fragmented configs by [@jdx](https://github.com/jdx) in [#3273](https://github.com/jdx/mise/pull/3273)
- hooks by [@jdx](https://github.com/jdx) in [#3256](https://github.com/jdx/mise/pull/3256)
- added MISE_TASK_DISABLE_PATHS setting by [@jdx](https://github.com/jdx) in [9c2e6e4](https://github.com/jdx/mise/commit/9c2e6e40f3a98f352fbf03107e1901dec445a7f5)
- gpg verification for node by [@jdx](https://github.com/jdx) in [#3277](https://github.com/jdx/mise/pull/3277)

### 🐛 Bug Fixes

- make _.file and _.source optional if the file is missing by [@jdx](https://github.com/jdx) in [#3275](https://github.com/jdx/mise/pull/3275)
- prevent deadlock when resetting by [@jdx](https://github.com/jdx) in [8e6d093](https://github.com/jdx/mise/commit/8e6d09377de81c65203684725fa9dfc2140db520)
- prevent deadlock when resetting by [@jdx](https://github.com/jdx) in [201ba90](https://github.com/jdx/mise/commit/201ba904052379595e399672d1657ed0e3c3a138)
- prevent deadlock when resetting by [@jdx](https://github.com/jdx) in [169338a](https://github.com/jdx/mise/commit/169338a2debb99ee4dd885376c4123740237af23)

### 🚜 Refactor

- clean up arcs by [@jdx](https://github.com/jdx) in [f49d330](https://github.com/jdx/mise/commit/f49d330b6f97b08e72b1a448af0021708b2a2417)

### 📚 Documentation

- added hooks to sidebar by [@jdx](https://github.com/jdx) in [4bbc340](https://github.com/jdx/mise/commit/4bbc3403e46aa817450e6936f37b5d4c983b43d4)
- added swift to sidebar by [@jdx](https://github.com/jdx) in [bc06cbf](https://github.com/jdx/mise/commit/bc06cbf240cc7aae2173575cfa83289ae526dad1)

### Chore

- skip checkov test by [@jdx](https://github.com/jdx) in [2ae18a3](https://github.com/jdx/mise/commit/2ae18a3e8329eb9913dc43ae94432f8f75b36a94)
- added timeout for release-plz by [@jdx](https://github.com/jdx) in [dae4bc3](https://github.com/jdx/mise/commit/dae4bc32bbb7de7873e3fa047a785c70f02a5c05)
- remove coverage by [@jdx](https://github.com/jdx) in [#3278](https://github.com/jdx/mise/pull/3278)

## [2024.11.33](https://github.com/jdx/mise/compare/v2024.11.32..v2024.11.33) - 2024-11-28

### 🚀 Features

- respect --quiet in `mise run` by [@jdx](https://github.com/jdx) in [#3257](https://github.com/jdx/mise/pull/3257)
- added special "_" portion of mise.toml for custom data by [@jdx](https://github.com/jdx) in [#3259](https://github.com/jdx/mise/pull/3259)
- **breaking** added MISE_OVERRIDE_CONFIG_FILENAMES config by [@jdx](https://github.com/jdx) in [#3266](https://github.com/jdx/mise/pull/3266)
- added swift by [@jdx](https://github.com/jdx) in [#3271](https://github.com/jdx/mise/pull/3271)

### 🐛 Bug Fixes

- **(spm)** git proxy config by [@jdx](https://github.com/jdx) in [#3264](https://github.com/jdx/mise/pull/3264)
- clean up some windows error cases by [@jdx](https://github.com/jdx) in [#3255](https://github.com/jdx/mise/pull/3255)
- run `hook-env` on directory change by [@jdx](https://github.com/jdx) in [#3258](https://github.com/jdx/mise/pull/3258)
- always prefer glibc to musl in mise run by [@jdx](https://github.com/jdx) in [#3261](https://github.com/jdx/mise/pull/3261)
- issue with non-default backends not getting tool options by [@jdx](https://github.com/jdx) in [#3265](https://github.com/jdx/mise/pull/3265)
- explicitly stop progress bars when exiting by [@jdx](https://github.com/jdx) in [#3272](https://github.com/jdx/mise/pull/3272)

### 🚜 Refactor

- use aqua for shellcheck by [@scop](https://github.com/scop) in [#3270](https://github.com/jdx/mise/pull/3270)
- use aqua for goreleaser by [@scop](https://github.com/scop) in [#3269](https://github.com/jdx/mise/pull/3269)
- use aqua for golangci-lint by [@scop](https://github.com/scop) in [#3268](https://github.com/jdx/mise/pull/3268)

### 📚 Documentation

- describe mise behavior when mise version is lower than min_version by [@erickguan](https://github.com/erickguan) in [#2994](https://github.com/jdx/mise/pull/2994)

### Chore

- wait for gh rate limit if expended by [@jdx](https://github.com/jdx) in [#3251](https://github.com/jdx/mise/pull/3251)
- set github token for docs job by [@jdx](https://github.com/jdx) in [908dd18](https://github.com/jdx/mise/commit/908dd18fe3ddf19d1531c93695ee3ff98d0995c5)
- skip hyperfine unless on release pr by [@jdx](https://github.com/jdx) in [#3253](https://github.com/jdx/mise/pull/3253)
- move tasks dir so it doesnt show up in unrelated projects by [@jdx](https://github.com/jdx) in [#3254](https://github.com/jdx/mise/pull/3254)

## [2024.11.32](https://github.com/jdx/mise/compare/v2024.11.31..v2024.11.32) - 2024-11-27

### 🚀 Features

- allow running tasks without `mise run`, e.g.: `mise test` as shorthand for `mise run test` by [@jdx](https://github.com/jdx) in [#3235](https://github.com/jdx/mise/pull/3235)
- default task directory config by [@jdx](https://github.com/jdx) in [#3238](https://github.com/jdx/mise/pull/3238)
- standalone tasks by [@jdx](https://github.com/jdx) in [#3240](https://github.com/jdx/mise/pull/3240)
- automatic uv venv activation by [@jdx](https://github.com/jdx) in [#3239](https://github.com/jdx/mise/pull/3239)
- migrate more tools away from asdf by [@jdx](https://github.com/jdx) in [#3242](https://github.com/jdx/mise/pull/3242)
- add committed by [@scop](https://github.com/scop) in [#3247](https://github.com/jdx/mise/pull/3247)
- use ubi for figma-export by [@jdx](https://github.com/jdx) in [19dbeac](https://github.com/jdx/mise/commit/19dbeac16a68248bb780a2de1056d16409714204)
- add vacuum by [@scop](https://github.com/scop) in [#3249](https://github.com/jdx/mise/pull/3249)

### 🐛 Bug Fixes

- skip _.source files if not present by [@jdx](https://github.com/jdx) in [#3236](https://github.com/jdx/mise/pull/3236)
- rust idiomatic file parsing by [@jdx](https://github.com/jdx) in [#3241](https://github.com/jdx/mise/pull/3241)
- automatic reinstall of uvx tools during python upgrades by [@jdx](https://github.com/jdx) in [#3243](https://github.com/jdx/mise/pull/3243)

### 🚜 Refactor

- use aqua for shfmt by [@scop](https://github.com/scop) in [#3244](https://github.com/jdx/mise/pull/3244)
- use aqua for lefthook by [@scop](https://github.com/scop) in [#3246](https://github.com/jdx/mise/pull/3246)
- use aqua for nfpm by [@scop](https://github.com/scop) in [#3248](https://github.com/jdx/mise/pull/3248)

### 📚 Documentation

- correction in aqua by [@jdx](https://github.com/jdx) in [b7de2f3](https://github.com/jdx/mise/commit/b7de2f32e6a23458bbd3573372f9c49733b80e62)
- typo by [@jdx](https://github.com/jdx) in [98aa6bd](https://github.com/jdx/mise/commit/98aa6bd7b2631a5904243cbf9aeb2eaf218c9c64)

### Chore

- bump tabled by [@jdx](https://github.com/jdx) in [#3245](https://github.com/jdx/mise/pull/3245)
- fix tools tests on release branch by [@jdx](https://github.com/jdx) in [675a2b0](https://github.com/jdx/mise/commit/675a2b086116f0afb431189c51136255b6f6c434)
- fix tools tests on release branch by [@jdx](https://github.com/jdx) in [130c3a4](https://github.com/jdx/mise/commit/130c3a4de60edfbed98642bc6dc71e67ba9b6ce1)
- fix tools tests on release branch by [@jdx](https://github.com/jdx) in [9feb3b6](https://github.com/jdx/mise/commit/9feb3b638ef634d320f576921b3e366f6cd73075)

### New Contributors

- @rmacklin made their first contribution in [#2295](https://github.com/jdx/mise/pull/2295)

## [2024.11.31](https://github.com/jdx/mise/compare/v2024.11.30..v2024.11.31) - 2024-11-27

### 🚀 Features

- rust in core by [@jdx](https://github.com/jdx) in [#3219](https://github.com/jdx/mise/pull/3219)

### 🐛 Bug Fixes

- use tv.pathname() in `mise ls` by [@jdx](https://github.com/jdx) in [#3217](https://github.com/jdx/mise/pull/3217)
- show gh rate limit reset time by [@jdx](https://github.com/jdx) in [#3221](https://github.com/jdx/mise/pull/3221)
- add @version back into show_tools by [@jdx](https://github.com/jdx) in [fd7d8d1](https://github.com/jdx/mise/commit/fd7d8d10395f8c80a80c60c0de89bf78e31fd762)
- use pipx for yamllint by [@jdx](https://github.com/jdx) in [#3227](https://github.com/jdx/mise/pull/3227)
- remove shims directory in `mise activate` by [@jdx](https://github.com/jdx) in [#3232](https://github.com/jdx/mise/pull/3232)

### 🚜 Refactor

- remove duplicate remote_versions_caches by [@jdx](https://github.com/jdx) in [#3220](https://github.com/jdx/mise/pull/3220)

### 📚 Documentation

- rename legacy version files to idiomatic version files by [@jdx](https://github.com/jdx) in [#3216](https://github.com/jdx/mise/pull/3216)
- document aqua better by [@jdx](https://github.com/jdx) in [#3234](https://github.com/jdx/mise/pull/3234)

### 🎨 Styling

- spelling and grammar fixes by [@scop](https://github.com/scop) in [#3225](https://github.com/jdx/mise/pull/3225)

### 🧪 Testing

- move some unit tests to e2e by [@jdx](https://github.com/jdx) in [#3218](https://github.com/jdx/mise/pull/3218)
- migrate tests from unit to e2e by [@jdx](https://github.com/jdx) in [#3231](https://github.com/jdx/mise/pull/3231)

## [2024.11.30](https://github.com/jdx/mise/compare/v2024.11.29..v2024.11.30) - 2024-11-26

### 🚀 Features

- migrate wren-cli to ubi by [@jdx](https://github.com/jdx) in [#3193](https://github.com/jdx/mise/pull/3193)
- migrate more tools away from asdf by [@jdx](https://github.com/jdx) in [#3202](https://github.com/jdx/mise/pull/3202)
- automatically set `set -e` in toml tasks by [@jdx](https://github.com/jdx) in [#3215](https://github.com/jdx/mise/pull/3215)
- added MISE_ORIGINAL_CWD to tasks by [@jdx](https://github.com/jdx) in [#3214](https://github.com/jdx/mise/pull/3214)
- add ruby backend by [@andrewthauer](https://github.com/andrewthauer) in [#1657](https://github.com/jdx/mise/pull/1657)
- migrate more tools away from asdf by [@jdx](https://github.com/jdx) in [#3205](https://github.com/jdx/mise/pull/3205)

### 🐛 Bug Fixes

- Make Rebar backend depend on Erlang by [@eproxus](https://github.com/eproxus) in [#3197](https://github.com/jdx/mise/pull/3197)
- trust system/global config by default by [@jdx](https://github.com/jdx) in [#3201](https://github.com/jdx/mise/pull/3201)
- use tv.short in show_tools by [@jdx](https://github.com/jdx) in [#3213](https://github.com/jdx/mise/pull/3213)

### 📚 Documentation

- flatten tools in sidebar by [@jdx](https://github.com/jdx) in [0556024](https://github.com/jdx/mise/commit/0556024b5abdb2d5f1cb025d105494c71aa79647)

### 🧪 Testing

- remove flaky maven test by [@jdx](https://github.com/jdx) in [65f6eb4](https://github.com/jdx/mise/commit/65f6eb48880b6322439c33b3cd53eab7b8b97439)
- added test for vault by [@jdx](https://github.com/jdx) in [#3194](https://github.com/jdx/mise/pull/3194)

### Chore

- bump expr-lang by [@jdx](https://github.com/jdx) in [#3199](https://github.com/jdx/mise/pull/3199)
- add aqua-registry as submodule by [@jdx](https://github.com/jdx) in [#3204](https://github.com/jdx/mise/pull/3204)

### New Contributors

- @eproxus made their first contribution in [#3197](https://github.com/jdx/mise/pull/3197)

## [2024.11.29](https://github.com/jdx/mise/compare/v2024.11.28..v2024.11.29) - 2024-11-25

### 🚀 Features

- **(env)** Allow exporting env vars as dotenv format by [@miguelmig](https://github.com/miguelmig) in [#3185](https://github.com/jdx/mise/pull/3185)
- move more tools away from asdf by [@jdx](https://github.com/jdx) in [#3184](https://github.com/jdx/mise/pull/3184)
- use aqua for cargo-binstall by [@jdx](https://github.com/jdx) in [#3182](https://github.com/jdx/mise/pull/3182)

### 🐛 Bug Fixes

- use shift_remove by [@jdx](https://github.com/jdx) in [#3188](https://github.com/jdx/mise/pull/3188)
- pass boolean tool options as strings by [@jdx](https://github.com/jdx) in [#3191](https://github.com/jdx/mise/pull/3191)
- move semver cmp errors to debug by [@jdx](https://github.com/jdx) in [ab4e638](https://github.com/jdx/mise/commit/ab4e638cdeda9845f3b7421a22a0d3bf71d81eae)
- show more accurate error if no tasks are available by [@jdx](https://github.com/jdx) in [e1b1b48](https://github.com/jdx/mise/commit/e1b1b48840b8c96e45a567a47922138544ab9f59)
- move semver cmp errors to debug by [@jdx](https://github.com/jdx) in [#3172](https://github.com/jdx/mise/pull/3172)
- use aqua for terraform by [@jdx](https://github.com/jdx) in [#3192](https://github.com/jdx/mise/pull/3192)

### 🧪 Testing

- disable cargo-binstall test by [@jdx](https://github.com/jdx) in [8fee82e](https://github.com/jdx/mise/commit/8fee82e652031a1c9a31dbb05437478c961b6107)

### Chore

- include aqua-registry yaml files in crate by [@jdx](https://github.com/jdx) in [#3186](https://github.com/jdx/mise/pull/3186)
- gitignore aqua-registry by [@jdx](https://github.com/jdx) in [1c38bca](https://github.com/jdx/mise/commit/1c38bca434cfc17792eb3053be2f4271a9e92fdd)
- gitignore aqua-registry by [@jdx](https://github.com/jdx) in [644cb6d](https://github.com/jdx/mise/commit/644cb6dfa762d6360b5aaa7fce0502fe61ac1067)

## [2024.11.28](https://github.com/jdx/mise/compare/v2023.12.0..v2024.11.28) - 2024-11-24

### 🚀 Features

- migrate more tools away from asdf by [@jdx](https://github.com/jdx) in [#3170](https://github.com/jdx/mise/pull/3170)
- auto-install tools on `mise run` by [@jdx](https://github.com/jdx) in [#3181](https://github.com/jdx/mise/pull/3181)
- move more tools away from asdf by [@jdx](https://github.com/jdx) in [#3179](https://github.com/jdx/mise/pull/3179)

### 🐛 Bug Fixes

- allow passing integers to task env by [@jdx](https://github.com/jdx) in [#3177](https://github.com/jdx/mise/pull/3177)
- remove __MISE_WATCH,__MISE_DIFF env vars on `mise deactivate` by [@jdx](https://github.com/jdx) in [#3178](https://github.com/jdx/mise/pull/3178)

### 📚 Documentation

- **(security)** added information about checksums/cosign/slsa verification by [@jdx](https://github.com/jdx) in [1faef6e](https://github.com/jdx/mise/commit/1faef6ecbb48692955f4ce424d77d03472aa4617)
- **(security)** added release gpg key by [@jdx](https://github.com/jdx) in [8f5dfd6](https://github.com/jdx/mise/commit/8f5dfd6dd2903c55fd792aeecd8ec97ef9f7f7ba)
- typos by [@jdx](https://github.com/jdx) in [#3173](https://github.com/jdx/mise/pull/3173)

### Chore

- clean up CHANGELOG by [@jdx](https://github.com/jdx) in [8ec0ca2](https://github.com/jdx/mise/commit/8ec0ca20fce57d07d769209fd9043a129daa86f1)

## [2023.12.0] - 2023-12-01

### New Contributors

- @jdx made their first contribution
- @dependabot[bot] made their first contribution
- @benweint made their first contribution
- @CharString made their first contribution
- @Nezteb made their first contribution
- @duckpuppy made their first contribution
- @jguddas made their first contribution
- @Jaxwood made their first contribution
- @booniepepper made their first contribution
- @fiadliel made their first contribution
- @roele made their first contribution
- @scottdotau made their first contribution
- @xanderificnl made their first contribution
- @dfang made their first contribution
- @tsukimizake made their first contribution
- @scop made their first contribution
- @Sacro made their first contribution
- @ProducerMatt made their first contribution
- @splinter98 made their first contribution
- @AlecRust made their first contribution
- @pedorich-n made their first contribution
- @knu made their first contribution
- @smbl64 made their first contribution
- @tombh made their first contribution
- @luccahuguet made their first contribution
- @andrewthauer made their first contribution
- @KokaKiwi made their first contribution
- @toadslop made their first contribution
- @jonathanmorley made their first contribution
- @chadac made their first contribution
- @tomocrafter made their first contribution
- @issmo made their first contribution
- @jaybosamiya made their first contribution
- @nrjais made their first contribution
- @Alex23rodriguez made their first contribution
- @gerbal made their first contribution
- @chhe made their first contribution
- @kianmeng made their first contribution
- @cserino made their first contribution
- @amoosbr made their first contribution
- @eugenesvk made their first contribution
- @delicb made their first contribution

<!-- generated by git-cliff -->
