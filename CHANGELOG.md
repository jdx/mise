# Changelog

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

- Fixes fish_command_not_found glob error by [@halostatue](https://github.com/halostatue) in [#4133](https://github.com/jdx/mise/pull/4133)
- completions for `mise use` by [@jdx](https://github.com/jdx) in [#4147](https://github.com/jdx/mise/pull/4147)

### 🛡️ Security

- **(ruby)** remove ruby/gem tests by [@jdx](https://github.com/jdx) in [#4130](https://github.com/jdx/mise/pull/4130)

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

### Chore

- clean up CHANGELOG by [@jdx](https://github.com/jdx) in [8ec0ca2](https://github.com/jdx/mise/commit/8ec0ca20fce57d07d769209fd9043a129daa86f1)

<!-- generated by git-cliff -->
