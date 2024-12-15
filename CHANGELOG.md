# Changelog

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

### 🔍 Other Changes

- Update comparison-to-asdf.md by [@jdx](https://github.com/jdx) in [e7715c8](https://github.com/jdx/mise/commit/e7715c87811cb30848e3c0475f647ef97e09f7a5)
- Update task-configuration.md by [@jdx](https://github.com/jdx) in [e3586b7](https://github.com/jdx/mise/commit/e3586b7ee6c47cd1dd8ca4706a7c83d6d4a93857)
- Update contributing.md by [@jdx](https://github.com/jdx) in [80d5b8d](https://github.com/jdx/mise/commit/80d5b8d78dbc15e57751c518fd6693fb4c432ab5)
- Fix concat for nushell script by [@samuelallan72](https://github.com/samuelallan72) in [#3591](https://github.com/jdx/mise/pull/3591)

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

### 🔍 Other Changes

- Update config.ts by [@jdx](https://github.com/jdx) in [7ba504c](https://github.com/jdx/mise/commit/7ba504cf2cf5b0f64ffc77e3c6ef03092971cdf1)
- added comfy-table by [@jdx](https://github.com/jdx) in [#3561](https://github.com/jdx/mise/pull/3561)
- Update tips-and-tricks.md by [@jdx](https://github.com/jdx) in [a09d4c2](https://github.com/jdx/mise/commit/a09d4c29a95f72b7c41855bc8cae35b168e31cc8)
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

### 🔍 Other Changes

- Update pipx.md by [@jdx](https://github.com/jdx) in [5fc9d9d](https://github.com/jdx/mise/commit/5fc9d9df43221a63d17dcf39ebacd2d5fabb1f39)

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

### 🔍 Other Changes

- Fix README link. by [@biggusbeetus](https://github.com/biggusbeetus) in [#3502](https://github.com/jdx/mise/pull/3502)
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

### 🔍 Other Changes

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

### 🔍 Other Changes

- fix windows zip structure by [@jdx](https://github.com/jdx) in [195039f](https://github.com/jdx/mise/commit/195039ff2bbe702c7e80ace3fcaeb95cb02d018b)
- Update contributing.md by [@jdx](https://github.com/jdx) in [bdd06e5](https://github.com/jdx/mise/commit/bdd06e5716d92e157c809f0f73823c9df9d3133b)

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

### 🔍 Other Changes

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

### 🔍 Other Changes

- Update walkthrough.md by [@jdx](https://github.com/jdx) in [c3aa2d0](https://github.com/jdx/mise/commit/c3aa2d0f0b5269e432fa78ba4545b0320be55826)
- Update hooks.md by [@jdx](https://github.com/jdx) in [9c71e44](https://github.com/jdx/mise/commit/9c71e44cc12871cd69f2a4829390e912cb8519a8)
- Update installing-mise.md by [@jdx](https://github.com/jdx) in [2cc97ca](https://github.com/jdx/mise/commit/2cc97ca317df356da19bc9b25fb37cc74d89b8a4)
- update default.nix by [@minhtrancccp](https://github.com/minhtrancccp) in [#3430](https://github.com/jdx/mise/pull/3430)
- Fix mention of slsa-verifier in documentation by [@will-ockmore](https://github.com/will-ockmore) in [#3435](https://github.com/jdx/mise/pull/3435)

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

### 🔍 Other Changes

- Update environments.md by [@jdx](https://github.com/jdx) in [aa5eeff](https://github.com/jdx/mise/commit/aa5eeff161a8b01435c87dcae124fd54f8ddcf4d)

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

### 🔍 Other Changes

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

### 🔍 Other Changes

- Update shims.md by [@jdx](https://github.com/jdx) in [2d48109](https://github.com/jdx/mise/commit/2d48109a77ae4432b0fd6cede3196a0819710186)
- Update hooks.md by [@jdx](https://github.com/jdx) in [2693f94](https://github.com/jdx/mise/commit/2693f946f7cbb2819a4d4df37b6314759e38e9f3)

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

### 🔍 Other Changes

- Update tips-and-tricks.md by [@jdx](https://github.com/jdx) in [5071419](https://github.com/jdx/mise/commit/5071419b988d3655b87e7413a4577fab2684ddf8)
- Update tips-and-tricks.md by [@jdx](https://github.com/jdx) in [fcc6b59](https://github.com/jdx/mise/commit/fcc6b59740306ee2065f365d230b30abbefcc7d2)
- Update tips-and-tricks.md by [@jdx](https://github.com/jdx) in [039b19d](https://github.com/jdx/mise/commit/039b19dd9dc68e3047b23127483af2f9efd11e1b)
- Update configuration.md by [@jdx](https://github.com/jdx) in [b0cac9e](https://github.com/jdx/mise/commit/b0cac9e7573ccb5dd70c3b3b1e53a0a7911c2e18)
- Update tips-and-tricks.md by [@jdx](https://github.com/jdx) in [9347be8](https://github.com/jdx/mise/commit/9347be89a9a86c0bde40c3986c01b98e4f8d68b8)
- Update tips-and-tricks.md by [@jdx](https://github.com/jdx) in [1cfc822](https://github.com/jdx/mise/commit/1cfc8228541c98111c36c5470323f9fe52d2125f)
- Update registry.toml by [@jdx](https://github.com/jdx) in [5a28860](https://github.com/jdx/mise/commit/5a28860ac7f8d81194926d6b14eb394ecbe7dc0d)
- upgrade usage-lib by [@jdx](https://github.com/jdx) in [554d533](https://github.com/jdx/mise/commit/554d533a253a137c27c5cdac6da2ae09629029dc)
- add rust to core tools list by [@gurgelio](https://github.com/gurgelio) in [#3341](https://github.com/jdx/mise/pull/3341)
- use asdf:mise-plugins/mise-nim by [@jdx](https://github.com/jdx) in [#3352](https://github.com/jdx/mise/pull/3352)

### New Contributors

- @gurgelio made their first contribution in [#3341](https://github.com/jdx/mise/pull/3341)
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

### 🔍 Other Changes

- Revert "fix: always prefer glibc to musl in mise run " by [@jdx](https://github.com/jdx) in [#3298](https://github.com/jdx/mise/pull/3298)
- bump expr-lang by [@jdx](https://github.com/jdx) in [#3297](https://github.com/jdx/mise/pull/3297)
- mise up --bump by [@jdx](https://github.com/jdx) in [6872b54](https://github.com/jdx/mise/commit/6872b5469622140335a12131dfa4acf310fc0c2a)
- update mise.lock by [@jdx](https://github.com/jdx) in [4c12502](https://github.com/jdx/mise/commit/4c12502c459ba2e214689c3f55d964b8f75966af)
- disable tool tests until I can sort out gh rate limit issues by [@jdx](https://github.com/jdx) in [f42f010](https://github.com/jdx/mise/commit/f42f010f03a57cab128290c0b9d936fd7a90c785)

### New Contributors

- @minddust made their first contribution in [#3296](https://github.com/jdx/mise/pull/3296)

## [2024.11.36](https://github.com/jdx/mise/compare/v2024.11.35..v2024.11.36) - 2024-11-29

### 🔍 Other Changes

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

### 🔍 Other Changes

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

### 🔍 Other Changes

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

### 🔍 Other Changes

- wait for gh rate limit if expended by [@jdx](https://github.com/jdx) in [#3251](https://github.com/jdx/mise/pull/3251)
- set github token for docs job by [@jdx](https://github.com/jdx) in [908dd18](https://github.com/jdx/mise/commit/908dd18fe3ddf19d1531c93695ee3ff98d0995c5)
- skip hyperfine unless on release pr by [@jdx](https://github.com/jdx) in [#3253](https://github.com/jdx/mise/pull/3253)
- move tasks dir so it doesnt show up in unrelated projects by [@jdx](https://github.com/jdx) in [#3254](https://github.com/jdx/mise/pull/3254)
- Update comparison-to-asdf.md by [@jdx](https://github.com/jdx) in [fe50c72](https://github.com/jdx/mise/commit/fe50c72ab9786e17651ede49862bab7820492ac0)
- added "en" command by [@jdx](https://github.com/jdx) in [#1697](https://github.com/jdx/mise/pull/1697)

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

### 🔍 Other Changes

- bump tabled by [@jdx](https://github.com/jdx) in [#3245](https://github.com/jdx/mise/pull/3245)
- fix tools tests on release branch by [@jdx](https://github.com/jdx) in [675a2b0](https://github.com/jdx/mise/commit/675a2b086116f0afb431189c51136255b6f6c434)
- fix tools tests on release branch by [@jdx](https://github.com/jdx) in [130c3a4](https://github.com/jdx/mise/commit/130c3a4de60edfbed98642bc6dc71e67ba9b6ce1)
- Mention the fish shell's automatic activation in the Quickstart section by [@rmacklin](https://github.com/rmacklin) in [#2295](https://github.com/jdx/mise/pull/2295)
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

### 🔍 Other Changes

- bump expr-lang by [@jdx](https://github.com/jdx) in [#3199](https://github.com/jdx/mise/pull/3199)
- add aqua-registry as submodule by [@jdx](https://github.com/jdx) in [#3204](https://github.com/jdx/mise/pull/3204)
- Update plugins.md by [@jdx](https://github.com/jdx) in [1a38802](https://github.com/jdx/mise/commit/1a38802dd2c729805654638a6e2464afed6e8e14)
- Update plugins.md by [@jdx](https://github.com/jdx) in [8ca6f5f](https://github.com/jdx/mise/commit/8ca6f5f9e8df0be7b714ffe6d030fd60bf04fcd7)
- Update plugins.md by [@jdx](https://github.com/jdx) in [c82d4d7](https://github.com/jdx/mise/commit/c82d4d7e16cd79a6c6cab759065f0ec0d9d2badd)

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

### 🔍 Other Changes

- include aqua-registry yaml files in crate by [@jdx](https://github.com/jdx) in [#3186](https://github.com/jdx/mise/pull/3186)
- gitignore aqua-registry by [@jdx](https://github.com/jdx) in [1c38bca](https://github.com/jdx/mise/commit/1c38bca434cfc17792eb3053be2f4271a9e92fdd)
- gitignore aqua-registry by [@jdx](https://github.com/jdx) in [644cb6d](https://github.com/jdx/mise/commit/644cb6dfa762d6360b5aaa7fce0502fe61ac1067)

## [2024.11.28] - 2024-11-24

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

### 🔍 Other Changes

- clean up CHANGELOG by [@jdx](https://github.com/jdx) in [8ec0ca2](https://github.com/jdx/mise/commit/8ec0ca20fce57d07d769209fd9043a129daa86f1)

<!-- generated by git-cliff -->
