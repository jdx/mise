# Changelog

All notable changes to this project will be documented in this file.

## [2024.3.2] - 2024-03-15

### 🚀 Features

- *(task)* Add option to show hidden tasks in dependency tree (#1756)

### 🐛 Bug Fixes

- *(task)* Document task.hide (#1754)
- Watch env._.source files (#1770)
- Prepend virtualenv path rather than append (#1751)
- *(npm)* Mise use -g npm:yarn@latest installs wrong version (#1752)
- *(go)* Go backend supports versions prefixed with 'v' (#1753)

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.3.2

## [2024.3.1] - 2024-03-04

### 🐛 Bug Fixes

- *(java)* Sdkmanrc zulu JVMs are missing in mise (#1719)

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.3.1

## [2024.2.19] - 2024-02-28

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.2.19

### Release

- Use normal mise data dir in justfile (#1718)

## [2024.2.18] - 2024-02-24

### 📚 Documentation

- Make README logo link to site (#1695)

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.2.18

### Release

- Auto-install plugins

## [2024.2.17] - 2024-02-22

### 🐛 Bug Fixes

- *(go)* Reflect on proper path for `GOROOT` (#1661)
- Allow go forge to install SHA versions when no tagged versions present (#1683)
- *(bun)* Install bunx symlink (#1688)

### 🚜 Refactor

- Auto-try miseprintln macro

### 📚 Documentation

- Add missing alt text (#1691)
- Improve formatting/colors
- Revamped output (#1694)

### 🧪 Testing

- *(integration)* Introduce rust based integration suite (#1612)

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.2.17

## [2024.2.16] - 2024-02-15

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.2.16

### Compeltions

- Use dash compatible syntax

## [2024.2.15] - 2024-02-13

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.2.15

## [2024.2.14] - 2024-02-11

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.2.14

## [2024.2.13] - 2024-02-11

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.2.13

## [2024.2.12] - 2024-02-11

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.2.12

## [2024.2.11] - 2024-02-10

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.2.11

## [2024.2.10] - 2024-02-10

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.2.10

## [2024.2.9] - 2024-02-09

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.2.9

## [2024.2.8] - 2024-02-09

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.2.8

### Go

- GOROOT/GOBIN/GOPATH changes (#1641)

### Tasks

- Ignore non-executable tasks (#1642)

## [2024.2.7] - 2024-02-08

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.2.7

### Fish

- Fix command not found handler

### Ls

- Add installed/active flags (#1629)

### Tasks

- Support global file tasks (#1627)

## [2024.2.6] - 2024-02-07

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.2.6

### Fish

- Reuse existing command_not_found handler (#1624)

## [2024.2.5] - 2024-02-06

### 📚 Documentation

- Add some info (#1614)
- Cli help

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.2.5

### Env-file

- Add dotenv paths to watch files (#1615)

### Tasks

- Support "false" env vars (#1603)

## [2024.2.4] - 2024-02-03

### 🐛 Bug Fixes

- *(tasks)* Fix parsing of alias attribute (#1596)

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.2.4

## [2024.2.3] - 2024-02-02

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.2.3

### Tasks

- Skip running glob if no patterns

## [2024.2.2] - 2024-02-02

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.2.2

### Plugins

- Ui tweak

### Python

- Minor UI tweak

### Release

- Clear cache on mise.run

## [2024.2.1] - 2024-02-01

### 📚 Documentation

- Add "dr" alias

### ⚙️ Miscellaneous Tasks

- Use m1 macs
- Release mise version 2024.2.1

### Settings

- Improve set/ls commands (#1579)

## [2024.2.0] - 2024-02-01

### 🚀 Features

- *(tasks)* Make script task dirs configurable (#1571)

### 🐛 Bug Fixes

- *(tasks)* Prevent dependency cycles (#1575)

### 📚 Documentation

- Fix github action
- Fix github action
- Skip cargo-msrv
- Fix test runner
- Fix dev test

### ⚙️ Miscellaneous Tasks

- Skip checkout for homebrew bump
- Release mise version 2024.2.0

### Status

- Make missing tool warning more granular (#1577)

### Tasks

- Refactor to use BTreeMap instead of sorting

## [2024.1.35] - 2024-01-31

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.35

### Shims

- Use activate_agressive setting

## [2024.1.34] - 2024-01-31

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.34

## [2024.1.33] - 2024-01-30

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.33

### Shims

- Treat anything not rtx/mise as a shim

## [2024.1.32] - 2024-01-30

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.32

### Poetry

- Use compiled python

### Python

- Fix settings env vars

## [2024.1.31] - 2024-01-30

### 🚀 Features

- *(tasks)* Add task timing to run command (#1536)

### 🐛 Bug Fixes

- Properly handle executable shims when getting diffs (#1545)

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.31

### Go

- Clean up e2e tests

### Python

- Only show precompiled warning if going to use precompiled
- Fix linux precompiled (#1559)

## [2024.1.30] - 2024-01-27

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.29
- Release mise version 2024.1.30

## [2024.1.29] - 2024-01-27

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.29

## [2024.1.28] - 2024-01-27

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.28

## [2024.1.27] - 2024-01-26

### 🚀 Features

- *(run)* Match tasks to run with glob patterns (#1528)
- *(tasks)* Unify glob strategy for tasks and dependencies (#1533)

### 📚 Documentation

- Display missing/extra shims (#1529)

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.27

### Env

- Resolve env vars in order (#1519)

## [2024.1.26] - 2024-01-25

### 🚀 Features

- *(tasks)* Infer bash task topics from folder structure (#1520)
- *(doctor)* Identify missing/extra shims (#1524)

### 🚜 Refactor

- Env parsing (#1515)

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.26

### Bun|python

- Use target_feature to use correct precompiled runtimes (#1512)

### Config

- Do not follow symbolic links for trusted paths (#1513)
- Refactor min_version logic (#1516)

### Env

- Sort env vars coming back from exec-env (#1518)
- Order flags in docs

## [2024.1.25] - 2024-01-24

### 🚀 Features

- *(config)* Support arrays of env tables (#1503)
- *(template)* Add join_path filter (#1508)
- Add other arm targets for cargo-binstall (#1510)

### 🐛 Bug Fixes

- *(tasks)* Prevent implicit globbing of sources/outputs (#1509)

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.25

### Cargo

- Allow cargo-binstall from mise itself (#1507)

## [2024.1.24] - 2024-01-20

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.24

### Activate

- Added --shims (#1483)

### Aur

- Fix conflicts

### Fish_completion

- Use `sort -r` instead of `tac` (#1486)

### Runtime_symlinks

- Do not fail if version parsing fails

## [2024.1.23] - 2024-01-18

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.23

### Plugins

- Improve post-plugin-update script (#1479)

### Tasks

- Only show select if no task specified (#1481)
- Show cursor on ctrl-c
- Fix project_root when using .config/mise.toml or .mise/config.toml (#1482)

## [2024.1.22] - 2024-01-17

### 🐛 Bug Fixes

- No panic on missing current dir (#1462)
- Always load global configs (#1466)

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.22

### Tasks

- Support array of commands directly (#1474)

## [2024.1.21] - 2024-01-15

### 🐛 Bug Fixes

- Bail out of task suggestion if there are no tasks (#1460)

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.21

## [2024.1.20] - 2024-01-14

### 🚀 Features

- Add command to print task dependency tree (#1440)
- Add completions for task deps command (#1456)
- Add interactive selection for tasks if task was not found (#1459)

### ⚙️ Miscellaneous Tasks

- Re-enable standalone test
- Release mise version 2024.1.20

### Tasks

- Enable stdin under interleaved

## [2024.1.19] - 2024-01-13

### 🚜 Refactor

- Remove PluginName type alias
- Rename Plugin trait to Forge
- Clean up arg imports
- Clean up arg imports (#1451)

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.19

### Config

- Allow using "env._.file|env._.path" instead of "env.mise.file|env.mise.path"

### Npm

- Testing

## [2024.1.18] - 2024-01-12

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.18

### Release

- Fix mise-docs publishing
- Temporarily disable standalone test

## [2024.1.17] - 2024-01-12

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.17

### Activate

- Use less aggressive PATH modifications by default

### Settings

- Remove warning about moving to settings.toml
- Read from config.toml (#1439)

## [2024.1.16] - 2024-01-11

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.16

### Env-vars

- Improvements (#1435)

### Python

- Do not panic if precompiled arch/os is not supported (#1434)

## [2024.1.15] - 2024-01-10

### 🐛 Bug Fixes

- *(python)* Fixes #1419 (#1420)

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.15

### Python

- Fix some precompiled issues (#1431)

## [2024.1.14] - 2024-01-09

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.14

## [2024.1.13] - 2024-01-08

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.13

## [2024.1.12] - 2024-01-07

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.12

### Python

- Fixed python_compile and all_compile settings

## [2024.1.11] - 2024-01-07

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.11

### Settings.toml

- Add to doctor and fix warning

### Toml

- Check min_version field

## [2024.1.10] - 2024-01-07

### 🐛 Bug Fixes

- Nix flake build errors (#1390)

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.10

## [2024.1.9] - 2024-01-07

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.9

### Python

- Add support for precompiled binaries (#1388)

## [2024.1.8] - 2024-01-06

### 🐛 Bug Fixes

- *(java)* Enable macOS integration hint for Zulu distribution (#1381)

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.8

## [2024.1.7] - 2024-01-05

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.7

## [2024.1.6] - 2024-01-04

### 🧪 Testing

- Fixed elixir test case

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.6

### Tasks

- Set CLICOLOR_FORCE=1 and FORCE_COLOR=1 (#1364)
- Set --interleaved if graph is linear (#1365)

## [2024.1.5] - 2024-01-04

### 🐛 Bug Fixes

- Remove comma from conflicts (#1353)

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.5

### Env

- Use `mise.file`/`mise.path` config (#1361)

### Logging

- Prevent loading multiple times (#1358)

### Migrate

- Skip ruby installs

### Not-found

- Use "[" instead of "test" (#1355)

## [2024.1.4] - 2024-01-04

### 🐛 Bug Fixes

- *(java)* Use tar.gz archives to enable symlink support (#1343)

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.4

### Aur

- Add "replaces" field (#1345)

### Install

- Docs

### Plugin-install

- Fix ssh urls (#1349)

## [2024.1.3] - 2024-01-03

### ⚙️ Miscellaneous Tasks

- Use mise docker containers
- Skip committing docs if no changes
- Release mise version 2024.1.3

### Standalone

- Use ~/.local/bin/mise instead of ~/.local/share/mise/bin/mise

## [2024.1.2] - 2024-01-03

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.2

### Python

- Fix venv python path

## [2024.1.1] - 2024-01-03

### 📚 Documentation

- Tweak cli reference
- Fixed reading settings from config

### ⚙️ Miscellaneous Tasks

- Release mise version 2024.1.1

### Use

- Fix MISE_ASDF_COMPAT=1 (#1340)

<!-- generated by git-cliff -->
