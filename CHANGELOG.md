# Changelog

## [2024.10.7](https://github.com/jdx/mise/compare/v2024.10.6..v2024.10.7) - 2024-10-14

### 🐛 Bug Fixes

- remove schema/settings.json from release script by [@jdx](https://github.com/jdx) in [3086cae](https://github.com/jdx/mise/commit/3086cae4c8874ec85b8af776cafdd85a69dc80b5)

## [2024.10.6](https://github.com/jdx/mise/compare/v2024.10.5..v2024.10.6) - 2024-10-14

### 🚀 Features

- add rustic backup plugin to registry by [@jahands](https://github.com/jahands) in [#2754](https://github.com/jdx/mise/pull/2754)
- created custom logger by [@jdx](https://github.com/jdx) in [#2758](https://github.com/jdx/mise/pull/2758)
- render task help via usage by [@jdx](https://github.com/jdx) in [#2760](https://github.com/jdx/mise/pull/2760)

### 🐛 Bug Fixes

- replace asdf-rustic with ubi by [@jdx](https://github.com/jdx) in [#2757](https://github.com/jdx/mise/pull/2757)
- set trailing_var_arg on `mise run` by [@jdx](https://github.com/jdx) in [b1bb3d2](https://github.com/jdx/mise/commit/b1bb3d2a1e60eaccfc5e24250fc86966b6c9e69a)
- prevent loading logger multiple times by [@jdx](https://github.com/jdx) in [1b83acd](https://github.com/jdx/mise/commit/1b83acd5cfef954643f3b24519fb51c8531cf59b)

### 📚 Documentation

- http_timeout should be a duration string by [@roele](https://github.com/roele) in [#2755](https://github.com/jdx/mise/pull/2755)
- added hint about --bump flag for upgrade/outdated by [@jdx](https://github.com/jdx) in [#2761](https://github.com/jdx/mise/pull/2761)

### 🧪 Testing

- reset test by [@jdx](https://github.com/jdx) in [25f172f](https://github.com/jdx/mise/commit/25f172f226766b4e3e78b738bb14bff9a577c51b)

## [2024.10.5](https://github.com/jdx/mise/compare/v2024.10.4..v2024.10.5) - 2024-10-14

### 🐛 Bug Fixes

- remove some non-working vfox plugins by [@jdx](https://github.com/jdx) in [7248fcc](https://github.com/jdx/mise/commit/7248fcccb43361979f112268fc15ecd54f2a7344)
- use asdf by default unless on windows by [@jdx](https://github.com/jdx) in [b525a84](https://github.com/jdx/mise/commit/b525a84b816a418d3e7bfcca5fc03446d539c63a)

### 🧪 Testing

- snapshots by [@jdx](https://github.com/jdx) in [2817482](https://github.com/jdx/mise/commit/2817482678d38999ba9b866d1f5c7e47808ba13a)

### 🔍 Other Changes

- bump vfox by [@jdx](https://github.com/jdx) in [8549727](https://github.com/jdx/mise/commit/85497276a86067f3e1e7cf281840e4de968a0174)

## [2024.10.4](https://github.com/jdx/mise/compare/v2024.10.3..v2024.10.4) - 2024-10-14

### 🐛 Bug Fixes

- some bugs with vfox by [@jdx](https://github.com/jdx) in [0c90062](https://github.com/jdx/mise/commit/0c90062cf9e148bcae4f85262bd6e4883496f385)

### 🚜 Refactor

- use ci_info to determine if running on CI by [@jdx](https://github.com/jdx) in [ac9a35b](https://github.com/jdx/mise/commit/ac9a35bc762e5679ea56b02a4ee278a88d358f78)

### 🔍 Other Changes

- enable more colors for tasks by [@jdx](https://github.com/jdx) in [f3b0e33](https://github.com/jdx/mise/commit/f3b0e33071376172142e89c010663add8365524b)

## [2024.10.3](https://github.com/jdx/mise/compare/v2024.10.2..v2024.10.3) - 2024-10-14

### 🚀 Features

- improve dynamic settings by [@jdx](https://github.com/jdx) in [#2731](https://github.com/jdx/mise/pull/2731)
- added --force flag to reshim by [@jdx](https://github.com/jdx) in [#2734](https://github.com/jdx/mise/pull/2734)
- added `mise settings add` by [@jdx](https://github.com/jdx) in [#2741](https://github.com/jdx/mise/pull/2741)
- improve task scheduling by [@jdx](https://github.com/jdx) in [#2743](https://github.com/jdx/mise/pull/2743)
- include ubi into mise directly by [@autarch](https://github.com/autarch) in [#2290](https://github.com/jdx/mise/pull/2290)
- allow passing arbitrary args to pipx/uvx by [@jdx](https://github.com/jdx) in [#2746](https://github.com/jdx/mise/pull/2746)
- new cross-backend registry by [@jdx](https://github.com/jdx) in [#2748](https://github.com/jdx/mise/pull/2748)
- enable colors for CI services that support it by [@jdx](https://github.com/jdx) in [c892e27](https://github.com/jdx/mise/commit/c892e27bc21cdd32449195b643bea398eb339568)

### 🐛 Bug Fixes

- remove shims directory when running `mise x` by [@jdx](https://github.com/jdx) in [#2735](https://github.com/jdx/mise/pull/2735)
- remove shims directory from PATH when executing shims by [@jdx](https://github.com/jdx) in [#2736](https://github.com/jdx/mise/pull/2736)
- use same outdated logic for `mise ls` as `mise outdated` by [@jdx](https://github.com/jdx) in [#2737](https://github.com/jdx/mise/pull/2737)
- do not include shims dir in path_env when reinserting by [@jdx](https://github.com/jdx) in [#2745](https://github.com/jdx/mise/pull/2745)
- automatically prefix ubi installs with "v" if not found by [@jdx](https://github.com/jdx) in [#2747](https://github.com/jdx/mise/pull/2747)
- some issues with new registry by [@jdx](https://github.com/jdx) in [8ec6fb8](https://github.com/jdx/mise/commit/8ec6fb801b00e40634b2afc253f4a17cb23648d6)
- only enable colors for stderr by [@jdx](https://github.com/jdx) in [8d57b99](https://github.com/jdx/mise/commit/8d57b99f9e9ab989ce22eb663a8ae9e08936d5e5)

### 🚜 Refactor

- move task deps into its own file by [@jdx](https://github.com/jdx) in [bad9f68](https://github.com/jdx/mise/commit/bad9f68c383466811626185c3269b648b52319de)
- use settings singleton in more places by [@jdx](https://github.com/jdx) in [#2742](https://github.com/jdx/mise/pull/2742)
- clean up `settings set` by [@jdx](https://github.com/jdx) in [#2744](https://github.com/jdx/mise/pull/2744)

### 📚 Documentation

- use dashes in changelog by [@jdx](https://github.com/jdx) in [90bb910](https://github.com/jdx/mise/commit/90bb9108ff78ad5009632311550b841980020455)

### 🔍 Other Changes

- ran prettier on project by [@jdx](https://github.com/jdx) in [#2732](https://github.com/jdx/mise/pull/2732)
- Fix typo in method name: "depedency" => "dependency" by [@autarch](https://github.com/autarch) in [#2738](https://github.com/jdx/mise/pull/2738)
- bump usage-lib by [@jdx](https://github.com/jdx) in [f3a2e5f](https://github.com/jdx/mise/commit/f3a2e5f098b957a5a5745c7879cf98e27e32e403)
- bump usage-lib by [@jdx](https://github.com/jdx) in [60f942d](https://github.com/jdx/mise/commit/60f942ddf3f3bc64f5f49015eb8c4093d616787b)
- upgrade ubuntu version by [@jdx](https://github.com/jdx) in [978ea1a](https://github.com/jdx/mise/commit/978ea1a80a32574611d66aed552b34f7a10430d7)
- added registry.toml to crate by [@jdx](https://github.com/jdx) in [be641ca](https://github.com/jdx/mise/commit/be641ca235b5b3ff944dfc5851206aa396bdfb09)

### New Contributors

- @autarch made their first contribution in [#2290](https://github.com/jdx/mise/pull/2290)

## [2024.10.2](https://github.com/jdx/mise/compare/v2024.10.1..v2024.10.2) - 2024-10-12

### 🚀 Features

- enable deno core plugin on windows & download deno files from deno server by [@finalchild](https://github.com/finalchild) in [#2719](https://github.com/jdx/mise/pull/2719)
- use uv to create venv by default by [@erickguan](https://github.com/erickguan) in [#2705](https://github.com/jdx/mise/pull/2705)

### 🐛 Bug Fixes

- **(ubi)** update ubi identifiers by [@risu729](https://github.com/risu729) in [#2724](https://github.com/jdx/mise/pull/2724)
- use join_paths to create new_path by [@finalchild](https://github.com/finalchild) in [#2708](https://github.com/jdx/mise/pull/2708)
- issue with java@latest and metadatas (currently 23.0.0) by [@roele](https://github.com/roele) in [#2727](https://github.com/jdx/mise/pull/2727)
- several fixes/improvements for python version selection by [@jdx](https://github.com/jdx) in [#2730](https://github.com/jdx/mise/pull/2730)
- don't skip latest version fetch by [@finalchild](https://github.com/finalchild) in [#2720](https://github.com/jdx/mise/pull/2720)

### 🚜 Refactor

- remove duplicates in vfox registry by [@risu729](https://github.com/risu729) in [#2729](https://github.com/jdx/mise/pull/2729)

### 🧪 Testing

- reset test by [@jdx](https://github.com/jdx) in [e34788b](https://github.com/jdx/mise/commit/e34788b803b0f4817e2b8481c4ed7d94ed308d66)

### 🔍 Other Changes

- move /.mise/tasks to /tasks by [@jdx](https://github.com/jdx) in [#2728](https://github.com/jdx/mise/pull/2728)

### New Contributors

- @risu729 made their first contribution in [#2729](https://github.com/jdx/mise/pull/2729)

## [2024.10.1](https://github.com/jdx/mise/compare/v2024.10.0..v2024.10.1) - 2024-10-07

### 🚀 Features

- added MISE_PIN=1 setting by [@jdx](https://github.com/jdx) in [9f73952](https://github.com/jdx/mise/commit/9f73952ac2a782da6c10ec6a4b36093a74b8e251)
- add hint about how install missing plugins by [@roele](https://github.com/roele) in [#2706](https://github.com/jdx/mise/pull/2706)
- task shell by [@roele](https://github.com/roele) in [#2709](https://github.com/jdx/mise/pull/2709)

### 🐛 Bug Fixes

- **(windows)** escape backslash in nu script & use proper csv by [@finalchild](https://github.com/finalchild) in [#2710](https://github.com/jdx/mise/pull/2710)
- update name of cargo:ubi-cli package by [@jdx](https://github.com/jdx) in [d83fe3f](https://github.com/jdx/mise/commit/d83fe3f0b1eaeb0fc464d8eef589541ddc182673)
- only upgrade versions if there is a version to upgrade to by [@jdx](https://github.com/jdx) in [8dfc6be](https://github.com/jdx/mise/commit/8dfc6bededf68f747baddd734566074e0fec7773)
- use npm.cmd on windows by [@finalchild](https://github.com/finalchild) in [#2711](https://github.com/jdx/mise/pull/2711)

### 🧪 Testing

- fix snapshots by [@jdx](https://github.com/jdx) in [e1bc269](https://github.com/jdx/mise/commit/e1bc269b2d40bac21208969f8fb2c744586d8ed1)
- reset test by [@jdx](https://github.com/jdx) in [b06878d](https://github.com/jdx/mise/commit/b06878dcadfd8a0edc80ea39381f534435f24736)

## [2024.10.0](https://github.com/jdx/mise/compare/v2024.9.13..v2024.10.0) - 2024-10-03

### 🐛 Bug Fixes

- **(task)** more flexible parsing around `#MISE alias(es)=`, changed prefix from `# mise` to `#MISE` by [@jdx](https://github.com/jdx) in [#2694](https://github.com/jdx/mise/pull/2694)

### 📚 Documentation

- ubi cargo install instruction incorrect by [@roele](https://github.com/roele) in [#2696](https://github.com/jdx/mise/pull/2696)
- troubleshooting by [@jdx](https://github.com/jdx) in [820aac4](https://github.com/jdx/mise/commit/820aac408cf47e1ace420b0c1018b5816d2b30a0)
- fix broken link by [@jdx](https://github.com/jdx) in [ec43be3](https://github.com/jdx/mise/commit/ec43be3575bf29b8d79e51e3f029fe63012d5f2b)
- add Rust creates by [@yanskun](https://github.com/yanskun) in [#2701](https://github.com/jdx/mise/pull/2701)

### 🧪 Testing

- reset test by [@jdx](https://github.com/jdx) in [538863c](https://github.com/jdx/mise/commit/538863c898f44832e0374c9901954ebc29624bdf)

### 🔍 Other Changes

- Fix shim PATH for windows by [@TobiX](https://github.com/TobiX) in [#2697](https://github.com/jdx/mise/pull/2697)
- Fix `mise shell` setting env in nushell by [@samuelallan72](https://github.com/samuelallan72) in [#2393](https://github.com/jdx/mise/pull/2393)

### New Contributors

- @yanskun made their first contribution in [#2701](https://github.com/jdx/mise/pull/2701)
- @samuelallan72 made their first contribution in [#2393](https://github.com/jdx/mise/pull/2393)

## [2024.9.13](https://github.com/jdx/mise/compare/v2024.9.12..v2024.9.13) - 2024-09-29

### 🚀 Features

- enable Java core plugin on Windows by [@TobiX](https://github.com/TobiX) in [#2684](https://github.com/jdx/mise/pull/2684)

### 🐛 Bug Fixes

- logic with displaying hints by [@jdx](https://github.com/jdx) in [#2686](https://github.com/jdx/mise/pull/2686)
- tools installed via cargo:a/b@rev:012 immediately pruned by [@roele](https://github.com/roele) in [#2685](https://github.com/jdx/mise/pull/2685)

### 📚 Documentation

- added links for topic commands by [@jdx](https://github.com/jdx) in [15e4da5](https://github.com/jdx/mise/commit/15e4da578275a4aca26b5bdca20e066c4428f07e)
- improve CLI documentation by [@jdx](https://github.com/jdx) in [dacb5a3](https://github.com/jdx/mise/commit/dacb5a3a0a1f461bbc1f387d888da5f0b816a9ea)
- updated task list by [@jdx](https://github.com/jdx) in [031a7d0](https://github.com/jdx/mise/commit/031a7d0788971f1e2a1af22511c92483aad91743)

### 🧪 Testing

- fix snapshots by [@jdx](https://github.com/jdx) in [d5a90c0](https://github.com/jdx/mise/commit/d5a90c037332606a212c9ed36d497c3452e0e4a7)

### 🔍 Other Changes

- updated usage by [@jdx](https://github.com/jdx) in [1764c8b](https://github.com/jdx/mise/commit/1764c8bba912b59d61f87bd0f10e488a918e12ca)
- updated usage by [@jdx](https://github.com/jdx) in [9c18637](https://github.com/jdx/mise/commit/9c18637c52cbdabd81ab84167f94d4578662a995)

## [2024.9.12](https://github.com/jdx/mise/compare/v2024.9.11..v2024.9.12) - 2024-09-29

### 🚀 Features

- offer to chmod non-executable tasks by [@jdx](https://github.com/jdx) in [#2675](https://github.com/jdx/mise/pull/2675)
- added basic task markdown generation by [@jdx](https://github.com/jdx) in [#2677](https://github.com/jdx/mise/pull/2677)
- Lazily evaluated env template variables in path entries by [@josb](https://github.com/josb) in [#2310](https://github.com/jdx/mise/pull/2310)
- improving task docs and cli reference docs by [@jdx](https://github.com/jdx) in [#2678](https://github.com/jdx/mise/pull/2678)

### 🐛 Bug Fixes

- do not load symlinked config files more than once by [@jdx](https://github.com/jdx) in [eb53099](https://github.com/jdx/mise/commit/eb530995e5126787187ef221f814ebbe3cb64824)
- minor bugs with incomplete python-build by [@jdx](https://github.com/jdx) in [b56ff50](https://github.com/jdx/mise/commit/b56ff50728bea6a82ec700f89b4f348f52e3a67e)
- don't show use override warning if symlink file by [@jdx](https://github.com/jdx) in [face79b](https://github.com/jdx/mise/commit/face79bbb30aba064caf123e1da6993cd203490a)

### 🚜 Refactor

- use wrap_err instead of map_err by [@jdx](https://github.com/jdx) in [3ef8e78](https://github.com/jdx/mise/commit/3ef8e78fbd12ee4de398c36e4093f6c9b9e8d49f)

### 📚 Documentation

- link to task argument docs by [@jdx](https://github.com/jdx) in [04776a9](https://github.com/jdx/mise/commit/04776a95db65319ee9fd38ffa9c4bf88ecb78033)
- Update cargo.md by [@Shobhit0109](https://github.com/Shobhit0109) in [#2680](https://github.com/jdx/mise/pull/2680)

### 🔍 Other Changes

- use wrap_err instead of suggestions to display in non-debug by [@jdx](https://github.com/jdx) in [71937c8](https://github.com/jdx/mise/commit/71937c854272367775d63faf60d19994ad6841e7)

### New Contributors

- @josb made their first contribution in [#2310](https://github.com/jdx/mise/pull/2310)

## [2024.9.11](https://github.com/jdx/mise/compare/v2024.9.10..v2024.9.11) - 2024-09-27

### 🚀 Features

- added --type option to `toml set` by [@jdx](https://github.com/jdx) in [#2674](https://github.com/jdx/mise/pull/2674)
- added --bump option for outdated/upgrade by [@jdx](https://github.com/jdx) in [#2667](https://github.com/jdx/mise/pull/2667)

### 🐛 Bug Fixes

- improve task loading by [@jdx](https://github.com/jdx) in [#2664](https://github.com/jdx/mise/pull/2664)

### 📚 Documentation

- remove experimental badge from cargo and npm backends by [@matukoto](https://github.com/matukoto) in [#2669](https://github.com/jdx/mise/pull/2669)

### ⚡ Performance

- more task loading in parallel by [@jdx](https://github.com/jdx) in [#2673](https://github.com/jdx/mise/pull/2673)

### 🔍 Other Changes

- bump git-cliff by [@jdx](https://github.com/jdx) in [9e82496](https://github.com/jdx/mise/commit/9e824964d1aef9eb955d64bf6a94ab767fffde11)
- fix codesign by [@jdx](https://github.com/jdx) in [7bbcda2](https://github.com/jdx/mise/commit/7bbcda29759a2ceb96a0df0e20e2354772ac2acc)

### New Contributors

- @matukoto made their first contribution in [#2669](https://github.com/jdx/mise/pull/2669)

## [2024.9.10](https://github.com/jdx/mise/compare/v2024.9.9..v2024.9.10) - 2024-09-26

### 🚀 Features

- add arguments to file tasks by [@jdx](https://github.com/jdx) in [#2614](https://github.com/jdx/mise/pull/2614)
- added toml cli commands by [@jdx](https://github.com/jdx) in [#2657](https://github.com/jdx/mise/pull/2657)
- mount tasks args/flags via usage by [@jdx](https://github.com/jdx) in [#2661](https://github.com/jdx/mise/pull/2661)
- added mise info command by [@jdx](https://github.com/jdx) in [#2663](https://github.com/jdx/mise/pull/2663)

### 📚 Documentation

- Add tera features for the template documenation by [@erickguan](https://github.com/erickguan) in [#2584](https://github.com/jdx/mise/pull/2584)

### 🔍 Other Changes

- migrate away from deprecated git-cliff syntax by [@jdx](https://github.com/jdx) in [230897c](https://github.com/jdx/mise/commit/230897c41210502f69ed5c4270f13d6efc416f89)
- pin git-cliff by [@jdx](https://github.com/jdx) in [b2603b6](https://github.com/jdx/mise/commit/b2603b685ad74adabcb613be351117f0d949635e)
- upgraded usage by [@jdx](https://github.com/jdx) in [#2662](https://github.com/jdx/mise/pull/2662)
- retry windows-e2e on failure by [@jdx](https://github.com/jdx) in [fa7ec34](https://github.com/jdx/mise/commit/fa7ec34ea1cd0d8a030a7d253b6e025c04fdd47c)
- retry windows-e2e on failure by [@jdx](https://github.com/jdx) in [6516f7f](https://github.com/jdx/mise/commit/6516f7ffdbaada7ccb16bdf57a423d852f97a1a1)

## [2024.9.9](https://github.com/jdx/mise/compare/v2024.9.8..v2024.9.9) - 2024-09-25

### 🚀 Features

- added postinstall hook by [@jdx](https://github.com/jdx) in [#2654](https://github.com/jdx/mise/pull/2654)

### 🐛 Bug Fixes

- added nodejs to alpine build by [@jdx](https://github.com/jdx) in [550f64c](https://github.com/jdx/mise/commit/550f64cb9c0377e4102be24486491a5b9d7947f3)
- bug with exec on windows by [@jdx](https://github.com/jdx) in [#2648](https://github.com/jdx/mise/pull/2648)
- only show hints once per execution by [@jdx](https://github.com/jdx) in [#2647](https://github.com/jdx/mise/pull/2647)
- task args regression by [@jdx](https://github.com/jdx) in [#2651](https://github.com/jdx/mise/pull/2651)
- use correct xdg paths on windows by [@jdx](https://github.com/jdx) in [#2653](https://github.com/jdx/mise/pull/2653)

### 🧪 Testing

- added windows e2e tests by [@jdx](https://github.com/jdx) in [#2643](https://github.com/jdx/mise/pull/2643)
- added windows e2e tests by [@jdx](https://github.com/jdx) in [#2645](https://github.com/jdx/mise/pull/2645)
- reset by [@jdx](https://github.com/jdx) in [57d0223](https://github.com/jdx/mise/commit/57d0223ab2dfa731088f1634d0d12a6edd8dd8a6)
- fix mise cache in CI by [@jdx](https://github.com/jdx) in [#2649](https://github.com/jdx/mise/pull/2649)
- allow specifying full e2e test names by [@jdx](https://github.com/jdx) in [#2650](https://github.com/jdx/mise/pull/2650)
- split windows into windows-unit and windows-e2e by [@jdx](https://github.com/jdx) in [#2652](https://github.com/jdx/mise/pull/2652)

### 🔍 Other Changes

- **(docs)** fix `arch()` template doc by [@cwegener](https://github.com/cwegener) in [#2644](https://github.com/jdx/mise/pull/2644)

### New Contributors

- @cwegener made their first contribution in [#2644](https://github.com/jdx/mise/pull/2644)

## [2024.9.8](https://github.com/jdx/mise/compare/v2024.9.7..v2024.9.8) - 2024-09-25

### 🚀 Features

- **(node)** allow using node unofficial build flavors by [@jdx](https://github.com/jdx) in [#2637](https://github.com/jdx/mise/pull/2637)
- codegen settings by [@jdx](https://github.com/jdx) in [#2640](https://github.com/jdx/mise/pull/2640)

### 🐛 Bug Fixes

- release 2024.9.7 breaks configurations that were using v in version names with go backend by [@roele](https://github.com/roele) in [#2636](https://github.com/jdx/mise/pull/2636)
- add node mirror/flavor to cache key by [@jdx](https://github.com/jdx) in [#2638](https://github.com/jdx/mise/pull/2638)

### 📚 Documentation

- Update faq.md by [@jdx](https://github.com/jdx) in [9036759](https://github.com/jdx/mise/commit/903675950d3ccc7abb49a40d6794d75d52695e5e)
- Update configuration.md by [@jdx](https://github.com/jdx) in [1bc8342](https://github.com/jdx/mise/commit/1bc8342920cfb0259e35e578f68d1ec857420787)
- Update configuration.md by [@jdx](https://github.com/jdx) in [#2630](https://github.com/jdx/mise/pull/2630)
- document java shorthand and its limitations by [@roele](https://github.com/roele) in [#2635](https://github.com/jdx/mise/pull/2635)

### 🔍 Other Changes

- format schema by [@jdx](https://github.com/jdx) in [418bc24](https://github.com/jdx/mise/commit/418bc24292cacdec0d643a7c93355c0dea550678)
- format schema by [@jdx](https://github.com/jdx) in [a8f7493](https://github.com/jdx/mise/commit/a8f7493cd63535ae8e46d77545acfecf9a1451b2)

## [2024.9.7](https://github.com/jdx/mise/compare/v2024.9.6..v2024.9.7) - 2024-09-23

### 🚀 Features

- task argument declarations by [@jdx](https://github.com/jdx) in [#2612](https://github.com/jdx/mise/pull/2612)

### 🐛 Bug Fixes

- **(windows)** node bin path by [@jdx](https://github.com/jdx) in [eed0ecf](https://github.com/jdx/mise/commit/eed0ecfb528aa1fa04efcadf44afd353db76a7c4)
- **(windows)** fixed npm backend by [@jdx](https://github.com/jdx) in [#2617](https://github.com/jdx/mise/pull/2617)
- ensure that version is not "latest" in node by [@jdx](https://github.com/jdx) in [0e196d6](https://github.com/jdx/mise/commit/0e196d6d9c0b0851148ba9894191d766c0386356)
- prevent attempting to use python-build in windows by [@jdx](https://github.com/jdx) in [e15545b](https://github.com/jdx/mise/commit/e15545bb623da98bae72a41a57fa10ec311ee881)
- skip last modified time test for nix by [@laozc](https://github.com/laozc) in [#2622](https://github.com/jdx/mise/pull/2622)
- go backend can't install tools without 'v' prefix in git repo tags by [@roele](https://github.com/roele) in [#2606](https://github.com/jdx/mise/pull/2606)
- use "v" prefix first for go backend by [@jdx](https://github.com/jdx) in [8444597](https://github.com/jdx/mise/commit/8444597add58353f8fc3a84662e7a024a72104c8)

### 📚 Documentation

- Fix Options example in documentation by [@gauravkumar37](https://github.com/gauravkumar37) in [#2619](https://github.com/jdx/mise/pull/2619)
- remove reference to cache duration by [@jdx](https://github.com/jdx) in [bef6086](https://github.com/jdx/mise/commit/bef608633e814927707cd011875ce0bff28aa3d3)

### 🔍 Other Changes

- Update toml-tasks.md by [@jdx](https://github.com/jdx) in [9d26963](https://github.com/jdx/mise/commit/9d2696366bd21be47c5a6e25586e7061c0a7838c)
- change prune message to debug-level by [@jdx](https://github.com/jdx) in [f54dd0d](https://github.com/jdx/mise/commit/f54dd0de830e0249b07cc263707530c6795d512f)

### New Contributors

- @gauravkumar37 made their first contribution in [#2619](https://github.com/jdx/mise/pull/2619)

## [2024.9.6](https://github.com/jdx/mise/compare/v2024.9.5..v2024.9.6) - 2024-09-18

### 🚀 Features

- **(tasks)** allow mise-tasks or .mise-tasks directories by [@jdx](https://github.com/jdx) in [#2610](https://github.com/jdx/mise/pull/2610)
- **(windows)** added ruby core plugin by [@jdx](https://github.com/jdx) in [#2599](https://github.com/jdx/mise/pull/2599)
- periodically prune old cache files by [@jdx](https://github.com/jdx) in [#2603](https://github.com/jdx/mise/pull/2603)
- take npm/cargo backends out of experimental by [@jdx](https://github.com/jdx) in [5496cef](https://github.com/jdx/mise/commit/5496cef30819a3998a52a8f5e6e2d91cfa3e86b0)

### 🐛 Bug Fixes

- **(ruby)** fixed MISE_RUBY_BUILD_OPTS by [@jdx](https://github.com/jdx) in [#2609](https://github.com/jdx/mise/pull/2609)
- **(windows)** self_update by [@jdx](https://github.com/jdx) in [#2588](https://github.com/jdx/mise/pull/2588)
- **(windows)** mise -v by [@jdx](https://github.com/jdx) in [fcc2d35](https://github.com/jdx/mise/commit/fcc2d354b962aa4fe8cc1b422b96a7e455107adc)
- **(windows)** make tasks work by [@jdx](https://github.com/jdx) in [#2591](https://github.com/jdx/mise/pull/2591)
- **(windows)** mise doctor fixes by [@jdx](https://github.com/jdx) in [#2597](https://github.com/jdx/mise/pull/2597)
- **(windows)** make exec work by [@jdx](https://github.com/jdx) in [#2598](https://github.com/jdx/mise/pull/2598)
- **(windows)** fixed shims by [@jdx](https://github.com/jdx) in [#2600](https://github.com/jdx/mise/pull/2600)

### 🧪 Testing

- add macos to CI by [@jdx](https://github.com/jdx) in [#2605](https://github.com/jdx/mise/pull/2605)

### 🔍 Other Changes

- clean up console output during project linting by [@jdx](https://github.com/jdx) in [#2607](https://github.com/jdx/mise/pull/2607)

## [2024.9.5](https://github.com/jdx/mise/compare/v2024.9.4..v2024.9.5) - 2024-09-17

### 🔍 Other Changes

- change win -> windows by [@jdx](https://github.com/jdx) in [e45623c](https://github.com/jdx/mise/commit/e45623c88662a11f08db93068ac765efb3813855)

## [2024.9.4](https://github.com/jdx/mise/compare/v2024.9.3..v2024.9.4) - 2024-09-15

### 🚀 Features

- support for global configuration profiles by [@roele](https://github.com/roele) in [#2575](https://github.com/jdx/mise/pull/2575)
- add Atmos by [@mtweeman](https://github.com/mtweeman) in [#2577](https://github.com/jdx/mise/pull/2577)
- add semver matching in mise templates by [@erickguan](https://github.com/erickguan) in [#2578](https://github.com/jdx/mise/pull/2578)
- add rest of tera features for templates by [@erickguan](https://github.com/erickguan) in [#2582](https://github.com/jdx/mise/pull/2582)

### 🐛 Bug Fixes

- fix a few tera filter error messages by [@erickguan](https://github.com/erickguan) in [#2574](https://github.com/jdx/mise/pull/2574)
- use "windows" instead of "win" by [@jdx](https://github.com/jdx) in [3327e8c](https://github.com/jdx/mise/commit/3327e8c5eca4dc39529790c4b830fdcca57ebe65)
- fixed release-plz by [@jdx](https://github.com/jdx) in [bc4fae3](https://github.com/jdx/mise/commit/bc4fae3f1acefdf0fb05f8b97a0ec1703a216f57)
- cannot install truffelruby by [@roele](https://github.com/roele) in [#2581](https://github.com/jdx/mise/pull/2581)

### 📚 Documentation

- wrong version in the README example when install specific version by [@roele](https://github.com/roele) in [#2579](https://github.com/jdx/mise/pull/2579)

### 🔍 Other Changes

- fix nightly lint warning by [@jdx](https://github.com/jdx) in [0a41dc6](https://github.com/jdx/mise/commit/0a41dc67aa7b1faf6301a67386eabb3ebd31ed4d)

### New Contributors

- @mtweeman made their first contribution in [#2577](https://github.com/jdx/mise/pull/2577)

## [2024.9.3](https://github.com/jdx/mise/compare/v2024.9.2..v2024.9.3) - 2024-09-12

### 🐛 Bug Fixes

- Look for `-P` or `--profile` to get mise environment. by [@fiadliel](https://github.com/fiadliel) in [#2566](https://github.com/jdx/mise/pull/2566)
- use consistent names for tera platform information by [@jdx](https://github.com/jdx) in [#2569](https://github.com/jdx/mise/pull/2569)

### 📚 Documentation

- added contributors to readme by [@jdx](https://github.com/jdx) in [16cccdd](https://github.com/jdx/mise/commit/16cccdd821a2b78f6a2144ea82ea16f09cacf84f)
- pdate getting-started.md by [@fesplugas](https://github.com/fesplugas) in [#2570](https://github.com/jdx/mise/pull/2570)

### New Contributors

- @fesplugas made their first contribution in [#2570](https://github.com/jdx/mise/pull/2570)

## [2024.9.2](https://github.com/jdx/mise/compare/v2024.9.1..v2024.9.2) - 2024-09-11

### 🚀 Features

- implement a few tera functions for mise toml config by [@erickguan](https://github.com/erickguan) in [#2561](https://github.com/jdx/mise/pull/2561)

### 🐛 Bug Fixes

- ruby ls-remote not showing alternative implementations by [@roele](https://github.com/roele) in [#2555](https://github.com/jdx/mise/pull/2555)
- cannot disable hints during Zsh completion by [@roele](https://github.com/roele) in [#2559](https://github.com/jdx/mise/pull/2559)

### 📚 Documentation

- Create zig.md by [@MustCodeAl](https://github.com/MustCodeAl) in [#2563](https://github.com/jdx/mise/pull/2563)

## [2024.9.1](https://github.com/jdx/mise/compare/v2024.9.0..v2024.9.1) - 2024-09-10

### 🚀 Features

- add global --env argument by [@roele](https://github.com/roele) in [#2553](https://github.com/jdx/mise/pull/2553)

### 🐛 Bug Fixes

- mise plugins ls command should ignore .DS_Store file on macOS by [@roele](https://github.com/roele) in [#2549](https://github.com/jdx/mise/pull/2549)
- mise deactivate zsh does not work, but mise deactivate does by [@roele](https://github.com/roele) in [#2550](https://github.com/jdx/mise/pull/2550)

### 🔍 Other Changes

- ignore RUSTSEC-2024-0370 by [@jdx](https://github.com/jdx) in [2de83b1](https://github.com/jdx/mise/commit/2de83b1af9e4c408886e8d756e734fa70f62e477)

## [2024.9.0](https://github.com/jdx/mise/compare/v2024.8.15..v2024.9.0) - 2024-09-05

### 🚀 Features

- **(pipx)** add support for specifying package extras by [@antoniomdk](https://github.com/antoniomdk) in [#2510](https://github.com/jdx/mise/pull/2510)
- mise hints by [@roele](https://github.com/roele) in [#2479](https://github.com/jdx/mise/pull/2479)

### 🐛 Bug Fixes

- **(asdf)** handle plugin URLs with trailing slash by [@jdx](https://github.com/jdx) in [4541fbe](https://github.com/jdx/mise/commit/4541fbe92700d6598a03479aa77278bfbc7035c0)
- ls-remote doesn't support @sub-X style versions by [@roele](https://github.com/roele) in [#2525](https://github.com/jdx/mise/pull/2525)
- ensure `mise install` installs missing runtimes listed in `mise ls` by [@stanhu](https://github.com/stanhu) in [#2524](https://github.com/jdx/mise/pull/2524)
- Ensure dependencies are available for alternative backends by [@xavdid](https://github.com/xavdid) in [#2532](https://github.com/jdx/mise/pull/2532)
- tweak hints by [@jdx](https://github.com/jdx) in [732fc58](https://github.com/jdx/mise/commit/732fc58deda43339e5dd0e5136c5b71dab275232)
- Update fish.rs for activation of mise by [@Shobhit0109](https://github.com/Shobhit0109) in [#2542](https://github.com/jdx/mise/pull/2542)
- resolve issue with prefixed dependencies by [@jdx](https://github.com/jdx) in [#2541](https://github.com/jdx/mise/pull/2541)

### 🧪 Testing

- added e2e env vars by [@jdx](https://github.com/jdx) in [585024f](https://github.com/jdx/mise/commit/585024fc882559beeef65c5a9772f40c8e1b5235)

### New Contributors

- @xavdid made their first contribution in [#2532](https://github.com/jdx/mise/pull/2532)
- @stanhu made their first contribution in [#2524](https://github.com/jdx/mise/pull/2524)

## [2024.8.15](https://github.com/jdx/mise/compare/v2024.8.14..v2024.8.15) - 2024-08-28

### 🚀 Features

- **(vfox)** added aliases like vfox:cmake -> vfox:version-fox/vfox-cmake by [@jdx](https://github.com/jdx) in [0654f6c](https://github.com/jdx/mise/commit/0654f6c3a4b15640fa64d5cee6cfec3f2f08a580)
- use https-only in paranoid by [@jdx](https://github.com/jdx) in [ad9f959](https://github.com/jdx/mise/commit/ad9f959ee0c7659596d8c3dc4e9ca33e82fec041)
- make use_versions_host a setting by [@jdx](https://github.com/jdx) in [d9d4d23](https://github.com/jdx/mise/commit/d9d4d23c56d1181c2ed5b7ce62475b9c469b9da4)

### 🐛 Bug Fixes

- **(pipx)** allow using uv provided by mise by [@jdx](https://github.com/jdx) in [b608a73](https://github.com/jdx/mise/commit/b608a736d94f3a97c4cd06226b194bef41b15d9d)
- **(pipx)** order pipx github releases correctly by [@jdx](https://github.com/jdx) in [054ff85](https://github.com/jdx/mise/commit/054ff85609d385ac0cd07dd9014a7bd6fe376271)
- **(vfox)** ensure plugin is installed before listing env vars by [@jdx](https://github.com/jdx) in [914d0b4](https://github.com/jdx/mise/commit/914d0b4ca78ef8144158ecde6158f7276879f4d8)
- correct aur fish completion directory by [@jdx](https://github.com/jdx) in [ff2f652](https://github.com/jdx/mise/commit/ff2f652a1419ccc7be2fd212a3275491e7f5cd49)

### 📚 Documentation

- **(readme)** remove failing green color by [@duhow](https://github.com/duhow) in [#2477](https://github.com/jdx/mise/pull/2477)
- document vfox by [@jdx](https://github.com/jdx) in [1084fc4](https://github.com/jdx/mise/commit/1084fc4896eec08921481ba24e263cda0b760875)
- render registry with asdf and not vfox by [@jdx](https://github.com/jdx) in [cc6876e](https://github.com/jdx/mise/commit/cc6876e51534d24a485c9f07568d11954bc87f90)
- document python_venv_auto_create by [@jdx](https://github.com/jdx) in [7fc7bd8](https://github.com/jdx/mise/commit/7fc7bd8c479e23242ce9afa071a99870cda40270)
- removed some references to rtx by [@jdx](https://github.com/jdx) in [44a7d2e](https://github.com/jdx/mise/commit/44a7d2e4558f1756677785b2afe2917cff8dfe63)

### 🧪 Testing

- set RUST_BACKTRACE in e2e tests by [@jdx](https://github.com/jdx) in [e1efb7f](https://github.com/jdx/mise/commit/e1efb7fd8dca45c8a337def418f48862ef63e1c6)
- added cargo_features test by [@jdx](https://github.com/jdx) in [3aa5f57](https://github.com/jdx/mise/commit/3aa5f5784ec63ec04f0ffeb5c1d2246687a65314)
- reset test by [@jdx](https://github.com/jdx) in [131cb0a](https://github.com/jdx/mise/commit/131cb0ada079efb7865e6666a12e6bf99e4d8150)

### 🔍 Other Changes

- set DEBUG=1 for alpine to find out why it is not creating MRs by [@jdx](https://github.com/jdx) in [313a2a0](https://github.com/jdx/mise/commit/313a2a062d08128c2d04484135ce3c2a9adb41f3)
- bump vfox.rs by [@jdx](https://github.com/jdx) in [9fbc562](https://github.com/jdx/mise/commit/9fbc56274ef134ddb8e1d400fc72765868981fb5)
- apply code lint fixes by [@jdx](https://github.com/jdx) in [c18dbc2](https://github.com/jdx/mise/commit/c18dbc2428ae2e585ecf5860a5577f7f93e30fdd)

## [2024.8.14](https://github.com/jdx/mise/compare/v2024.8.13..v2024.8.14) - 2024-08-27

### 🚀 Features

- **(cargo)** allow specifying features via tool options by [@jdx](https://github.com/jdx) in [#2515](https://github.com/jdx/mise/pull/2515)
- **(zig)** make dev builds installable by [@jdx](https://github.com/jdx) in [#2514](https://github.com/jdx/mise/pull/2514)
- add support for using `uv tool` as a replacement for pipx by [@antoniomdk](https://github.com/antoniomdk) in [#2509](https://github.com/jdx/mise/pull/2509)

### 🐛 Bug Fixes

- **(src/path_env.rs)** Issue 2504: Fix for JoinPathsError by [@mcallaway](https://github.com/mcallaway) in [#2511](https://github.com/jdx/mise/pull/2511)
- block remote versions which are not simple versions by [@jdx](https://github.com/jdx) in [ba90c3b](https://github.com/jdx/mise/commit/ba90c3bbe71bd33d628df607326da9f0cf363af1)
- npm backend not finding updates by [@roele](https://github.com/roele) in [#2512](https://github.com/jdx/mise/pull/2512)

### 🔍 Other Changes

- Update contributing.md by [@jdx](https://github.com/jdx) in [e9cc129](https://github.com/jdx/mise/commit/e9cc129f703ac2949900307a3b828c3a095644ca)
- fix nightly lint warning by [@jdx](https://github.com/jdx) in [6796a46](https://github.com/jdx/mise/commit/6796a46f95227286f3337bce374e7447536e9503)

### New Contributors

- @mcallaway made their first contribution in [#2511](https://github.com/jdx/mise/pull/2511)

## [2024.8.13](https://github.com/jdx/mise/compare/v2024.8.12..v2024.8.13) - 2024-08-26

### 🐛 Bug Fixes

- add suggestion for invalid use of repo_url by [@jdx](https://github.com/jdx) in [#2501](https://github.com/jdx/mise/pull/2501)

### 📚 Documentation

- add individual page for every CLI command by [@jdx](https://github.com/jdx) in [acea81c](https://github.com/jdx/mise/commit/acea81ca090fab76c4974a77a25c9557822d6263)
- add individual page for every CLI command by [@jdx](https://github.com/jdx) in [e379df7](https://github.com/jdx/mise/commit/e379df732bd85d77faead4fce650e388993f5999)
- add experimental badges to cli commands by [@jdx](https://github.com/jdx) in [4e50f33](https://github.com/jdx/mise/commit/4e50f330968b93b1af2ad4c93a78e82f9514324b)
- lint by [@jdx](https://github.com/jdx) in [26ebdec](https://github.com/jdx/mise/commit/26ebdec2765416c26adc1001451abb6a2ce71978)

### 🧪 Testing

- fixed render_help test by [@jdx](https://github.com/jdx) in [d39d861](https://github.com/jdx/mise/commit/d39d86152814e1f24ec8b648e79235a2e1f2bba5)

### 🔍 Other Changes

- make some gh workflows only run on jdx/mise by [@CharString](https://github.com/CharString) in [#2489](https://github.com/jdx/mise/pull/2489)
- Update index.md by [@jdx](https://github.com/jdx) in [b2c25f3](https://github.com/jdx/mise/commit/b2c25f39cd736c02174462d2e94cc0605d6c8e22)

### 📦️ Dependency Updates

- update dependency vitepress to v1.3.4 by [@renovate[bot]](https://github.com/renovate[bot]) in [#2499](https://github.com/jdx/mise/pull/2499)

## [2024.8.12](https://github.com/jdx/mise/compare/v2024.8.11..v2024.8.12) - 2024-08-20

### 🐛 Bug Fixes

- vendor git2 openssl by [@jdx](https://github.com/jdx) in [#2481](https://github.com/jdx/mise/pull/2481)
- python-compile setting by [@jdx](https://github.com/jdx) in [#2482](https://github.com/jdx/mise/pull/2482)

### 🧪 Testing

- reset test by [@jdx](https://github.com/jdx) in [000fdb8](https://github.com/jdx/mise/commit/000fdb8560b9994e7678924978cf1866bd58e623)
- reset test by [@jdx](https://github.com/jdx) in [2deb6ce](https://github.com/jdx/mise/commit/2deb6cef5bca37a5bb8e769293e4a665f533209e)
- reset test by [@jdx](https://github.com/jdx) in [385c09b](https://github.com/jdx/mise/commit/385c09b88013281af6a5adc9706a9d85e951ff61)

### 📦️ Dependency Updates

- update dependency vitepress to v1.3.3 by [@renovate[bot]](https://github.com/renovate[bot]) in [#2478](https://github.com/jdx/mise/pull/2478)

## [2024.8.11](https://github.com/jdx/mise/compare/v2024.8.10..v2024.8.11) - 2024-08-19

### 🐛 Bug Fixes

- bump xx by [@jdx](https://github.com/jdx) in [9a9d3c1](https://github.com/jdx/mise/commit/9a9d3c11e46028bcea0c7ec2fee10bf5c9b1fbe6)

## [2024.8.10](https://github.com/jdx/mise/compare/v2024.8.9..v2024.8.10) - 2024-08-18

### 🚀 Features

- python on windows by [@jdx](https://github.com/jdx) in [2d4cee2](https://github.com/jdx/mise/commit/2d4cee239f8e7d53f7be176369f6e2502f3c3032)

### 🐛 Bug Fixes

- hide non-working core plugins on windows by [@jdx](https://github.com/jdx) in [16a08fc](https://github.com/jdx/mise/commit/16a08fc0fa00fc8f9751f7a25cc4f5f5fc87b94d)
- windows compat by [@jdx](https://github.com/jdx) in [2084a37](https://github.com/jdx/mise/commit/2084a37436fd7f7af8958501adc7b6535f608816)
- vfox tweaks by [@jdx](https://github.com/jdx) in [c260ab2](https://github.com/jdx/mise/commit/c260ab220a31241eaca971d6ddf4046f1f57865b)
- remove windows warning by [@jdx](https://github.com/jdx) in [9be937e](https://github.com/jdx/mise/commit/9be937e15dece684c574bcccd6f66499361cb935)

### 📚 Documentation

- windows by [@jdx](https://github.com/jdx) in [437b63c](https://github.com/jdx/mise/commit/437b63cff94b5302a0527881d6b6e461e1e4d628)

### 🧪 Testing

- fixing tests by [@jdx](https://github.com/jdx) in [1206497](https://github.com/jdx/mise/commit/12064971a43f74cdb0f34276e07fb02aaf240096)
- reset test by [@jdx](https://github.com/jdx) in [c740cfd](https://github.com/jdx/mise/commit/c740cfddf45703444a52388d899c1deb52b73134)

### 🔍 Other Changes

- clippy by [@jdx](https://github.com/jdx) in [ee005ff](https://github.com/jdx/mise/commit/ee005ffac65093aad8949cdbfaf0761df4595851)
- fix windows build by [@jdx](https://github.com/jdx) in [28c5cb6](https://github.com/jdx/mise/commit/28c5cb64bd6506bf6db08769885d65c192fb20ce)
- set GITHUB_TOKEN in release task by [@jdx](https://github.com/jdx) in [0ae049b](https://github.com/jdx/mise/commit/0ae049baedaf2daf3056ec7d2043a8ba27f09df1)

## [2024.8.9](https://github.com/jdx/mise/compare/v2024.8.8..v2024.8.9) - 2024-08-18

### 🚀 Features

- use registry shortname for mise.toml/install dirs by [@jdx](https://github.com/jdx) in [#2470](https://github.com/jdx/mise/pull/2470)
- vfox backend by [@jdx](https://github.com/jdx) in [#2187](https://github.com/jdx/mise/pull/2187)

### 🐛 Bug Fixes

- hide file tasks starting with "." by [@jdx](https://github.com/jdx) in [#2466](https://github.com/jdx/mise/pull/2466)
- mise prune removes tool versions which are in use by [@roele](https://github.com/roele) in [#2469](https://github.com/jdx/mise/pull/2469)
- cargo_binstall missing from set commands by [@roele](https://github.com/roele) in [#2471](https://github.com/jdx/mise/pull/2471)
- only warn if config properties are not found by [@jdx](https://github.com/jdx) in [#2472](https://github.com/jdx/mise/pull/2472)

### 🚜 Refactor

- Asdf -> AsdfBackend by [@jdx](https://github.com/jdx) in [#2467](https://github.com/jdx/mise/pull/2467)
- backend repetition by [@jdx](https://github.com/jdx) in [d2f7f33](https://github.com/jdx/mise/commit/d2f7f33d81906aaee80ab0e333935111c7307b36)

## [2024.8.8](https://github.com/jdx/mise/compare/v2024.8.7..v2024.8.8) - 2024-08-17

### 🚜 Refactor

- split asdf into forge+plugin by [@jdx](https://github.com/jdx) in [#2226](https://github.com/jdx/mise/pull/2226)

### 🧪 Testing

- fix home directory for win tests by [@jdx](https://github.com/jdx) in [#2464](https://github.com/jdx/mise/pull/2464)

### 📦️ Dependency Updates

- update rust crate tabled to 0.16.0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#2452](https://github.com/jdx/mise/pull/2452)

## [2024.8.7](https://github.com/jdx/mise/compare/v2024.8.6..v2024.8.7) - 2024-08-16

### 🐛 Bug Fixes

- mise treats escaped newlines in env files differently than dotenvy by [@roele](https://github.com/roele) in [#2455](https://github.com/jdx/mise/pull/2455)
- wait for spawned tasks to die before exiting by [@jdx](https://github.com/jdx) in [#2461](https://github.com/jdx/mise/pull/2461)

### 📦️ Dependency Updates

- update dependency vitepress to v1.3.2 by [@renovate[bot]](https://github.com/renovate[bot]) in [#2450](https://github.com/jdx/mise/pull/2450)

## [2024.8.6](https://github.com/jdx/mise/compare/v2024.8.5..v2024.8.6) - 2024-08-12

### 🐛 Bug Fixes

- spm backend doesn't allow a GitHub repo name containing a dot by [@roele](https://github.com/roele) in [#2449](https://github.com/jdx/mise/pull/2449)

### 🚜 Refactor

- renamed tool_request_version to tool_request to match the class by [@jdx](https://github.com/jdx) in [76a611a](https://github.com/jdx/mise/commit/76a611ac0f3cfbc7ac58fdc87a528e86ef73507e)

### 📚 Documentation

- fix typos again by [@kianmeng](https://github.com/kianmeng) in [#2446](https://github.com/jdx/mise/pull/2446)
- add executable permission after installation by [@kianmeng](https://github.com/kianmeng) in [#2447](https://github.com/jdx/mise/pull/2447)

## [2024.8.5](https://github.com/jdx/mise/compare/v2024.8.4..v2024.8.5) - 2024-08-03

### 🚀 Features

- show friendly errors when not in verbose/debug mode by [@jdx](https://github.com/jdx) in [#2431](https://github.com/jdx/mise/pull/2431)
- allow installing cargo packages with `--git` by [@jdx](https://github.com/jdx) in [#2430](https://github.com/jdx/mise/pull/2430)
- some ux improvements to `mise sync nvm` by [@jdx](https://github.com/jdx) in [#2432](https://github.com/jdx/mise/pull/2432)

### 🐛 Bug Fixes

- display untrusted file on error by [@jdx](https://github.com/jdx) in [#2423](https://github.com/jdx/mise/pull/2423)
- `mise trust` issue with unstable hashing by [@jdx](https://github.com/jdx) in [#2427](https://github.com/jdx/mise/pull/2427)
- use newer eza in e2e test by [@jdx](https://github.com/jdx) in [eec3989](https://github.com/jdx/mise/commit/eec3989d8602ebc10304adbd5ded0574fc2981f0)
- take out home directory paths from `mise dr` output by [@jdx](https://github.com/jdx) in [#2433](https://github.com/jdx/mise/pull/2433)

### 🔍 Other Changes

- use pub(crate) to get notified about dead code by [@jdx](https://github.com/jdx) in [#2426](https://github.com/jdx/mise/pull/2426)

## [2024.8.4](https://github.com/jdx/mise/compare/v2024.8.3..v2024.8.4) - 2024-08-02

### 🐛 Bug Fixes

- alpine key madness by [@jdx](https://github.com/jdx) in [a7156e0](https://github.com/jdx/mise/commit/a7156e0042cf10fc3d43723ffd6a92860b4faa0a)
- alpine github key by [@jdx](https://github.com/jdx) in [a52b68d](https://github.com/jdx/mise/commit/a52b68d024a8ce9955bd84347cc591b249717312)
- alpine github key by [@jdx](https://github.com/jdx) in [ebc923f](https://github.com/jdx/mise/commit/ebc923ff3c140c6c282bb0c1a2896ad758b4a3c2)
- spm - cannot install package with null release name field by [@roele](https://github.com/roele) in [#2419](https://github.com/jdx/mise/pull/2419)

### 🔍 Other Changes

- removed dead code by [@jdx](https://github.com/jdx) in [#2416](https://github.com/jdx/mise/pull/2416)

## [2024.8.3](https://github.com/jdx/mise/compare/v2024.8.2..v2024.8.3) - 2024-08-01

### 🧪 Testing

- clean up global config test by [@jdx](https://github.com/jdx) in [c9f2ec5](https://github.com/jdx/mise/commit/c9f2ec514082c6b1816c52378ce5c29d24aa73cc)

### 🔍 Other Changes

- set extra alpine key by [@jdx](https://github.com/jdx) in [c6b152b](https://github.com/jdx/mise/commit/c6b152bd1864b49c392ad64becbff1b1722be52f)
- test alpine releases by [@jdx](https://github.com/jdx) in [08f7730](https://github.com/jdx/mise/commit/08f77301c772eb55cee376908f9d907e42c7fe4b)
- perform alpine at the very end by [@jdx](https://github.com/jdx) in [7c31e17](https://github.com/jdx/mise/commit/7c31e17cc6ff612298c8bdb335d86cab95c9473b)
- chmod by [@jdx](https://github.com/jdx) in [a3fe85b](https://github.com/jdx/mise/commit/a3fe85b7b71faecb220b33d6cc3b630884b4343a)
- added jq/gh to alpine docker by [@jdx](https://github.com/jdx) in [e1514cf](https://github.com/jdx/mise/commit/e1514cf95cc625085530c12afc6a7ceb57ff0b64)

## [2024.8.2](https://github.com/jdx/mise/compare/v2024.8.1..v2024.8.2) - 2024-08-01

### 🐛 Bug Fixes

- windows bug fixes by [@jdx](https://github.com/jdx) in [465ea89](https://github.com/jdx/mise/commit/465ea894f317eda025783e66a68f58ab10319790)
- made cmd! work on windows by [@jdx](https://github.com/jdx) in [c0cef5b](https://github.com/jdx/mise/commit/c0cef5b0941b476badfdbb4f46f24b117d72698d)
- got node to install on windows by [@jdx](https://github.com/jdx) in [e5aa94e](https://github.com/jdx/mise/commit/e5aa94ecb14c7700823ff7dd58a6e633ced5e054)
- windows shims by [@jdx](https://github.com/jdx) in [fc2cd48](https://github.com/jdx/mise/commit/fc2cd489babe834546424831a9613e1d0558aa7d)
- windows paths by [@jdx](https://github.com/jdx) in [a06bcce](https://github.com/jdx/mise/commit/a06bcce484ce405342e68a1ac5dbb667db376f5e)

### 🔍 Other Changes

- fix build by [@jdx](https://github.com/jdx) in [9d85182](https://github.com/jdx/mise/commit/9d8518249c783819a82366f8541f1ea20959e771)
- dry-run alpine releases by [@jdx](https://github.com/jdx) in [0ef2727](https://github.com/jdx/mise/commit/0ef2727905ce904e44b25cfe46c29645fd41405a)
- update bun version in e2e test by [@jdx](https://github.com/jdx) in [f4b339f](https://github.com/jdx/mise/commit/f4b339f7974dbb261e7e8a387d082f4090e01f21)
- fix bun test by [@jdx](https://github.com/jdx) in [00d7054](https://github.com/jdx/mise/commit/00d70543a5f3e0db891b7bfb505e65dacb66d8f0)

## [2024.8.1](https://github.com/jdx/mise/compare/v2024.8.0..v2024.8.1) - 2024-08-01

### 🐛 Bug Fixes

- various windows bug fixes by [@jdx](https://github.com/jdx) in [90b02eb](https://github.com/jdx/mise/commit/90b02eb49055bc7d458cd3cbfb0de00119539dfb)
- ignore PROMPT_DIRTRIM in diffing logic by [@jdx](https://github.com/jdx) in [7b5563c](https://github.com/jdx/mise/commit/7b5563cd007edf26bc17f07e6cddabacad451e00)

### 📚 Documentation

- added information on rolling alpine tokens by [@jdx](https://github.com/jdx) in [bd693b0](https://github.com/jdx/mise/commit/bd693b02fb4d1060ff7a07dcea07b4a7c5584a8b)

### 🔍 Other Changes

- mark releases as draft until they have been fully released by [@jdx](https://github.com/jdx) in [508f125](https://github.com/jdx/mise/commit/508f125dcea9c6d0457b59c36293204d25adc7ef)
- fix windows builds by [@jdx](https://github.com/jdx) in [91c90a2](https://github.com/jdx/mise/commit/91c90a2b2d373998433c64196254f7e4d0d8cd82)
- fix alpine release builds by [@jdx](https://github.com/jdx) in [a7534bb](https://github.com/jdx/mise/commit/a7534bbdd961e6a16852c947f1594d6a52034e58)
- only edit releases when not a dry run by [@jdx](https://github.com/jdx) in [2255522](https://github.com/jdx/mise/commit/2255522b5045e45ce0dea3699f6555a22a271971)

## [2024.8.0](https://github.com/jdx/mise/compare/v2024.7.5..v2024.8.0) - 2024-08-01

### 📚 Documentation

- Fix 'mise x' command snippet in the Continuous Integration section by [@mollyIV](https://github.com/mollyIV) in [#2411](https://github.com/jdx/mise/pull/2411)

### 🔍 Other Changes

- retry mise tests for docker-dev-test workflow by [@jdx](https://github.com/jdx) in [cc014dd](https://github.com/jdx/mise/commit/cc014dde3dedd1d891dab62fc37e4633dc995226)
- add BSD-2-Clause to allowed dep licenses by [@jdx](https://github.com/jdx) in [b4ea53c](https://github.com/jdx/mise/commit/b4ea53c4b2b01103ed93fc185dbca858730c3207)
- create new alpine gitlab token to replace the expired one by [@jdx](https://github.com/jdx) in [b30db04](https://github.com/jdx/mise/commit/b30db04aaa1f13ef0dcdf02e6df2f2afbdd73c94)

### New Contributors

- @mollyIV made their first contribution in [#2411](https://github.com/jdx/mise/pull/2411)

## [2024.7.5](https://github.com/jdx/mise/compare/v2024.7.4..v2024.7.5) - 2024-07-29

### 🐛 Bug Fixes

- mise use does not create a local .mise.toml anymore by [@roele](https://github.com/roele) in [#2406](https://github.com/jdx/mise/pull/2406)
- transform `master` to `ref:master` in ls-remote for zig by [@chasinglogic](https://github.com/chasinglogic) in [#2409](https://github.com/jdx/mise/pull/2409)

### 📦️ Dependency Updates

- bump openssl from 0.10.64 to 0.10.66 by [@dependabot[bot]](https://github.com/dependabot[bot]) in [#2397](https://github.com/jdx/mise/pull/2397)

### New Contributors

- @chasinglogic made their first contribution in [#2409](https://github.com/jdx/mise/pull/2409)

## [2024.7.4](https://github.com/jdx/mise/compare/v2024.7.3..v2024.7.4) - 2024-07-19

### 🚀 Features

- added MISE_LIBGIT2 setting by [@jdx](https://github.com/jdx) in [#2386](https://github.com/jdx/mise/pull/2386)

### 🐛 Bug Fixes

- keep RUBYLIB env var by [@jdx](https://github.com/jdx) in [#2387](https://github.com/jdx/mise/pull/2387)

### 📦️ Dependency Updates

- update dependency vitepress to v1.3.1 by [@renovate[bot]](https://github.com/renovate[bot]) in [#2376](https://github.com/jdx/mise/pull/2376)
- update docker/build-push-action action to v6 by [@renovate[bot]](https://github.com/renovate[bot]) in [#2377](https://github.com/jdx/mise/pull/2377)

## [2024.7.3](https://github.com/jdx/mise/compare/v2024.7.2..v2024.7.3) - 2024-07-14

### 🔍 Other Changes

- Use correct capitalization of GitHub by [@jahands](https://github.com/jahands) in [#2372](https://github.com/jdx/mise/pull/2372)
- loosen git2 requirements by [@jdx](https://github.com/jdx) in [#2374](https://github.com/jdx/mise/pull/2374)

## [2024.7.2](https://github.com/jdx/mise/compare/v2024.7.1..v2024.7.2) - 2024-07-13

### 🚀 Features

- support env vars in plugin urls by [@roele](https://github.com/roele) in [#2370](https://github.com/jdx/mise/pull/2370)

### 📦️ Dependency Updates

- update rust crate self_update to 0.41 by [@renovate[bot]](https://github.com/renovate[bot]) in [#2359](https://github.com/jdx/mise/pull/2359)
- update dependency vitepress to v1.3.0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#2358](https://github.com/jdx/mise/pull/2358)

## [2024.7.1](https://github.com/jdx/mise/compare/v2024.7.0..v2024.7.1) - 2024-07-08

### 🔍 Other Changes

- Fix link to Python venv activation doc section by [@gzurowski](https://github.com/gzurowski) in [#2353](https://github.com/jdx/mise/pull/2353)

### 📦️ Dependency Updates

- update built to 0.7.4 and git2 to 0.19.0 by [@roele](https://github.com/roele) in [#2357](https://github.com/jdx/mise/pull/2357)

### New Contributors

- @gzurowski made their first contribution in [#2353](https://github.com/jdx/mise/pull/2353)

## [2024.7.0](https://github.com/jdx/mise/compare/v2024.6.6..v2024.7.0) - 2024-07-03

### 📚 Documentation

- update actions/checkout version by [@light-planck](https://github.com/light-planck) in [#2349](https://github.com/jdx/mise/pull/2349)

### New Contributors

- @light-planck made their first contribution in [#2349](https://github.com/jdx/mise/pull/2349)

## [2024.6.6](https://github.com/jdx/mise/compare/v2024.6.5..v2024.6.6) - 2024-06-20

### 🐛 Bug Fixes

- improve error message for missing plugins by [@jdx](https://github.com/jdx) in [#2313](https://github.com/jdx/mise/pull/2313)

### 🔍 Other Changes

- Update configuration.md by [@jdx](https://github.com/jdx) in [a2f19cb](https://github.com/jdx/mise/commit/a2f19cbc655058472009d000c77d1fc8df8612fd)
- Update index.md by [@jdx](https://github.com/jdx) in [d9ef467](https://github.com/jdx/mise/commit/d9ef467ee9ef026039fa2220163f21a2214ebbfc)
- Update index.md by [@jdx](https://github.com/jdx) in [63739c8](https://github.com/jdx/mise/commit/63739c880dbfefdecab282736710d496d7e88dbc)

### 📦️ Dependency Updates

- bump curve25519-dalek from 4.1.2 to 4.1.3 by [@dependabot[bot]](https://github.com/dependabot[bot]) in [#2306](https://github.com/jdx/mise/pull/2306)

## [2024.6.5](https://github.com/jdx/mise/compare/v2024.6.4..v2024.6.5) - 2024-06-18

### 🔍 Other Changes

- Fixes nix flake by [@laozc](https://github.com/laozc) in [#2305](https://github.com/jdx/mise/pull/2305)

## [2024.6.4](https://github.com/jdx/mise/compare/v2024.6.3..v2024.6.4) - 2024-06-15

### 🐛 Bug Fixes

- allow glob patterns in task outputs and sources by [@adamdickinson](https://github.com/adamdickinson) in [#2286](https://github.com/jdx/mise/pull/2286)

### New Contributors

- @adamdickinson made their first contribution in [#2286](https://github.com/jdx/mise/pull/2286)

## [2024.6.3](https://github.com/jdx/mise/compare/v2024.6.2..v2024.6.3) - 2024-06-10

### 🐛 Bug Fixes

- github API rate limiting could be handled more explicitly by [@roele](https://github.com/roele) in [#2274](https://github.com/jdx/mise/pull/2274)
- group prefix not applied for script tasks by [@roele](https://github.com/roele) in [#2273](https://github.com/jdx/mise/pull/2273)
- mise plugins ls returns error immediately after install by [@roele](https://github.com/roele) in [#2271](https://github.com/jdx/mise/pull/2271)

### 📦️ Dependency Updates

- update dependency vitepress to v1.2.3 by [@renovate[bot]](https://github.com/renovate[bot]) in [#2277](https://github.com/jdx/mise/pull/2277)
- update rust crate regex to v1.10.5 by [@renovate[bot]](https://github.com/renovate[bot]) in [#2278](https://github.com/jdx/mise/pull/2278)
- update rust crate regex to v1.10.5 by [@renovate[bot]](https://github.com/renovate[bot]) in [577de17](https://github.com/jdx/mise/commit/577de1757c4bb4e6421d3e281c44825a8b8788b8)

## [2024.6.2](https://github.com/jdx/mise/compare/v2024.6.1..v2024.6.2) - 2024-06-07

### 🐛 Bug Fixes

- after installing the latest version, mise rolls back to the previous one by [@roele](https://github.com/roele) in [#2258](https://github.com/jdx/mise/pull/2258)

### 📚 Documentation

- add SPM backend page by [@kattouf](https://github.com/kattouf) in [#2252](https://github.com/jdx/mise/pull/2252)

## [2024.6.1](https://github.com/jdx/mise/compare/v2024.6.0..v2024.6.1) - 2024-06-03

### 🚀 Features

- SPM(Swift Package Manager) backend by [@kattouf](https://github.com/kattouf) in [#2241](https://github.com/jdx/mise/pull/2241)

### 🐛 Bug Fixes

- mise up node fails by [@roele](https://github.com/roele) in [#2243](https://github.com/jdx/mise/pull/2243)

### 📚 Documentation

- fixed syntax by [@jdx](https://github.com/jdx) in [56083f8](https://github.com/jdx/mise/commit/56083f858a4ee28a020a414c1addf0c2bb7968af)

### 🧪 Testing

- set GITHUB_TOKEN in dev-test by [@jdx](https://github.com/jdx) in [4334313](https://github.com/jdx/mise/commit/4334313da52c13d7f87656fb0e7978e4cf1f5d2f)

### 🔍 Other Changes

- Update getting-started.md: nushell by [@chrmod](https://github.com/chrmod) in [#2248](https://github.com/jdx/mise/pull/2248)

### 📦️ Dependency Updates

- update rust crate demand to v1.2.4 by [@renovate[bot]](https://github.com/renovate[bot]) in [#2246](https://github.com/jdx/mise/pull/2246)
- update rust crate zip to v2.1.2 by [@renovate[bot]](https://github.com/renovate[bot]) in [#2247](https://github.com/jdx/mise/pull/2247)

### New Contributors

- @chrmod made their first contribution in [#2248](https://github.com/jdx/mise/pull/2248)

## [2024.6.0](https://github.com/jdx/mise/compare/v2024.5.28..v2024.6.0) - 2024-06-01

### 🔍 Other Changes

- bump itertools by [@jdx](https://github.com/jdx) in [#2238](https://github.com/jdx/mise/pull/2238)
- migrate docs repo into this repo by [@jdx](https://github.com/jdx) in [#2237](https://github.com/jdx/mise/pull/2237)

## [2024.5.28](https://github.com/jdx/mise/compare/v2024.5.27..v2024.5.28) - 2024-05-31

### 🐛 Bug Fixes

- download keeps failing if it takes more than 30s by [@roele](https://github.com/roele) in [#2224](https://github.com/jdx/mise/pull/2224)
- settings unset does not work by [@roele](https://github.com/roele) in [#2230](https://github.com/jdx/mise/pull/2230)
- cleaner community-developed plugin warning by [@jdx](https://github.com/jdx) in [8dcf0f3](https://github.com/jdx/mise/commit/8dcf0f3a746fcae74d944412b6f0e141ded88860)
- correct `mise use` ordering by [@jdx](https://github.com/jdx) in [#2234](https://github.com/jdx/mise/pull/2234)

### 🚜 Refactor

- forge -> backend by [@jdx](https://github.com/jdx) in [#2227](https://github.com/jdx/mise/pull/2227)

### 🧪 Testing

- added reset() to more tests by [@jdx](https://github.com/jdx) in [5a6ea6a](https://github.com/jdx/mise/commit/5a6ea6afb9855827b5e6216aa20760dd45f5502f)

## [2024.5.27](https://github.com/jdx/mise/compare/v2024.5.26..v2024.5.27) - 2024-05-31

### 🚜 Refactor

- rename External plugins to Asdf by [@jdx](https://github.com/jdx) in [8e774ba](https://github.com/jdx/mise/commit/8e774ba44e933eedfb999259d1244d589fc7d847)
- split asdf into forge+plugin by [@jdx](https://github.com/jdx) in [#2225](https://github.com/jdx/mise/pull/2225)

### 🧪 Testing

- added reset() to more tests by [@jdx](https://github.com/jdx) in [1c76011](https://github.com/jdx/mise/commit/1c760112eef92eb51ada4ab00e45568adcf62b97)
- added reset() to more tests by [@jdx](https://github.com/jdx) in [402c5ce](https://github.com/jdx/mise/commit/402c5cee97ebdbeb42fc32d055f73794d4dfdf12)

### 🔍 Other Changes

- dont clean cache on win by [@jdx](https://github.com/jdx) in [ede6528](https://github.com/jdx/mise/commit/ede6528f5fe5e5beeabf0a007997f3abc188faa5)

## [2024.5.26](https://github.com/jdx/mise/compare/v2024.5.25..v2024.5.26) - 2024-05-30

### 🐛 Bug Fixes

- normalize remote urls by [@jdx](https://github.com/jdx) in [#2221](https://github.com/jdx/mise/pull/2221)

### 🧪 Testing

- added reset() to more tests by [@jdx](https://github.com/jdx) in [f9f65b3](https://github.com/jdx/mise/commit/f9f65b39214c9341bf44ad694c6659b6a17fdf9c)

### 🔍 Other Changes

- remove armv6 targets by [@jdx](https://github.com/jdx) in [90752f4](https://github.com/jdx/mise/commit/90752f4f08a8ca4095fb464edd79a7aed2b07e54)

## [2024.5.25](https://github.com/jdx/mise/compare/v2024.5.24..v2024.5.25) - 2024-05-30

### 🚀 Features

- use all tera features by [@jdx](https://github.com/jdx) in [48ca740](https://github.com/jdx/mise/commit/48ca74043e21fe12de18a8457e4554ac2cadb17b)

### 🚜 Refactor

- turn asdf into a forge by [@jdx](https://github.com/jdx) in [#2219](https://github.com/jdx/mise/pull/2219)

### 🧪 Testing

- clean cwd in unit tests by [@jdx](https://github.com/jdx) in [#2211](https://github.com/jdx/mise/pull/2211)
- windows by [@jdx](https://github.com/jdx) in [#2216](https://github.com/jdx/mise/pull/2216)
- add reset() to more tests by [@jdx](https://github.com/jdx) in [#2217](https://github.com/jdx/mise/pull/2217)
- added reset() to more tests by [@jdx](https://github.com/jdx) in [a22c9dd](https://github.com/jdx/mise/commit/a22c9dd1f0eb8c057046e23807abe3c5352faf66)

### 🔍 Other Changes

- fix build-tarball call by [@jdx](https://github.com/jdx) in [2a4b986](https://github.com/jdx/mise/commit/2a4b98685f0dc2c4c85c3ecee9634b08432354fc)
- **breaking** use kebab-case for backend-installs by [@jdx](https://github.com/jdx) in [#2218](https://github.com/jdx/mise/pull/2218)

## [2024.5.24](https://github.com/jdx/mise/compare/v2024.5.23..v2024.5.24) - 2024-05-28

### 🐛 Bug Fixes

- **(pipx)** version ordering by [@jdx](https://github.com/jdx) in [#2209](https://github.com/jdx/mise/pull/2209)
- **(use)** re-use mise.toml if exists by [@jdx](https://github.com/jdx) in [#2207](https://github.com/jdx/mise/pull/2207)
- mise trust works incorrectly with symlinked configuration file by [@roele](https://github.com/roele) in [#2186](https://github.com/jdx/mise/pull/2186)

### 🚜 Refactor

- simplify ForgeArg building by [@jdx](https://github.com/jdx) in [#2208](https://github.com/jdx/mise/pull/2208)

### 🔍 Other Changes

- resolve macros/derived-traits from crates w/ scopes rather than globally by [@donaldguy](https://github.com/donaldguy) in [#2198](https://github.com/jdx/mise/pull/2198)
- eliminate .tool-versions only used for jq by [@donaldguy](https://github.com/donaldguy) in [#2195](https://github.com/jdx/mise/pull/2195)

### New Contributors

- @donaldguy made their first contribution in [#2195](https://github.com/jdx/mise/pull/2195)

## [2024.5.23](https://github.com/jdx/mise/compare/v2024.5.22..v2024.5.23) - 2024-05-27

### 🐛 Bug Fixes

- **(self_update)** explicitly set target since there seems to be a bug with .identifier() by [@jdx](https://github.com/jdx) in [#2190](https://github.com/jdx/mise/pull/2190)
- minor race condition creating directories by [@jdx](https://github.com/jdx) in [23db391](https://github.com/jdx/mise/commit/23db39146c8edf7340472302e7f498f1d89cf5b4)
- vendor libgit2 for precompiled binaries by [@jdx](https://github.com/jdx) in [#2197](https://github.com/jdx/mise/pull/2197)

### 🧪 Testing

- break coverage tasks up a bit by [@jdx](https://github.com/jdx) in [#2192](https://github.com/jdx/mise/pull/2192)

### 🔍 Other Changes

- updated zip by [@jdx](https://github.com/jdx) in [#2191](https://github.com/jdx/mise/pull/2191)
- bump usage-lib by [@jdx](https://github.com/jdx) in [74fcd88](https://github.com/jdx/mise/commit/74fcd8863c8668f11c4886dd95fb7929f823eb14)
- Update bug_report.md by [@jdx](https://github.com/jdx) in [64271ed](https://github.com/jdx/mise/commit/64271edec6e8cbf68dd0ec5f646247fdc3f158e2)
- added git debug log by [@jdx](https://github.com/jdx) in [7df466e](https://github.com/jdx/mise/commit/7df466e8c9c287ad04b0a753df65c02d64e00451)
- retry build-tarball by [@jdx](https://github.com/jdx) in [1acf037](https://github.com/jdx/mise/commit/1acf0375072dbf4ae57ddfadf0daf5eea00d5b71)

## [2024.5.22](https://github.com/jdx/mise/compare/v2024.5.21..v2024.5.22) - 2024-05-25

### 🐛 Bug Fixes

- correctly use .mise/config.$MISE_ENV.toml files by [@jdx](https://github.com/jdx) in [cace97b](https://github.com/jdx/mise/commit/cace97b9fe7697a58354b93cc1109b14c9fbd30c)
- correctly use .mise/config.$MISE_ENV.toml files by [@jdx](https://github.com/jdx) in [262fa2e](https://github.com/jdx/mise/commit/262fa2e283dbd4c2fe4f44f15d81ab6eed54b79d)

### 🔍 Other Changes

- use async reqwest by [@jdx](https://github.com/jdx) in [#2178](https://github.com/jdx/mise/pull/2178)
- sign macos binary by [@jdx](https://github.com/jdx) in [88f43f8](https://github.com/jdx/mise/commit/88f43f8072a2a223d1be92504cd60b7191ef975b)
- use sccache by [@jdx](https://github.com/jdx) in [#2183](https://github.com/jdx/mise/pull/2183)
- compile on windows by [@jdx](https://github.com/jdx) in [#2184](https://github.com/jdx/mise/pull/2184)
- conditionally set sccache token by [@jdx](https://github.com/jdx) in [#2188](https://github.com/jdx/mise/pull/2188)

## [2024.5.21](https://github.com/jdx/mise/compare/v2024.5.20..v2024.5.21) - 2024-05-23

### 🐛 Bug Fixes

- **(git-pre-commit)** rewrite existing git hook to pre-commit.old by [@jdx](https://github.com/jdx) in [#2165](https://github.com/jdx/mise/pull/2165)
- handle issue running `mise install` with existing tools by [@jdx](https://github.com/jdx) in [#2161](https://github.com/jdx/mise/pull/2161)

### 🔍 Other Changes

- update kerl to 4.1.1 by [@bklebe](https://github.com/bklebe) in [#2173](https://github.com/jdx/mise/pull/2173)

### New Contributors

- @bklebe made their first contribution in [#2173](https://github.com/jdx/mise/pull/2173)

## [2024.5.20](https://github.com/jdx/mise/compare/v2024.5.18..v2024.5.20) - 2024-05-21

### 🐛 Bug Fixes

- **(prune)** make it not install the world by [@jdx](https://github.com/jdx) in [78f4aec](https://github.com/jdx/mise/commit/78f4aeca2647c3980feb68cd3c1e299c9c56b0d6)
- allow plugins overriding core plugins by [@jdx](https://github.com/jdx) in [#2155](https://github.com/jdx/mise/pull/2155)

### 🚜 Refactor

- toolset -> toolrequestset by [@jdx](https://github.com/jdx) in [#2150](https://github.com/jdx/mise/pull/2150)
- toolset -> toolrequestset by [@jdx](https://github.com/jdx) in [#2151](https://github.com/jdx/mise/pull/2151)

### 📚 Documentation

- fix core plugin registry urls by [@jdx](https://github.com/jdx) in [bb1556e](https://github.com/jdx/mise/commit/bb1556ee5a9c7806c28d9bf7472bd444ab70f35e)

### 🧪 Testing

- **(pipx)** use python3 instead of python by [@jdx](https://github.com/jdx) in [0ff52da](https://github.com/jdx/mise/commit/0ff52daf026d711d5001cc3af08caef0bdb4d163)
- name cache steps by [@jdx](https://github.com/jdx) in [532fe90](https://github.com/jdx/mise/commit/532fe9032a4f61c2ffbf47d29713ee3900770b55)
- fix lint-fix job by [@jdx](https://github.com/jdx) in [6439ca4](https://github.com/jdx/mise/commit/6439ca41820c240846686f9fbe6d67d24114934e)
- reset config after local tests by [@jdx](https://github.com/jdx) in [29077af](https://github.com/jdx/mise/commit/29077af3a0d04ad004a054e16e7e85e411058be1)
- fix implode running first when shuffled by [@jdx](https://github.com/jdx) in [7b07258](https://github.com/jdx/mise/commit/7b072589d46b4279574f99385f3515b6bd181bd5)
- added test for core plugin overloading by [@jdx](https://github.com/jdx) in [9a56129](https://github.com/jdx/mise/commit/9a5612993dc59359e0c876e8f948f2fece8ce93f)
- added shebang to e2e scripts by [@jdx](https://github.com/jdx) in [#2159](https://github.com/jdx/mise/pull/2159)

## [2024.5.18](https://github.com/jdx/mise/compare/v2024.5.17..v2024.5.18) - 2024-05-19

### 🚀 Features

- added plugin registry to docs by [@jdx](https://github.com/jdx) in [#2138](https://github.com/jdx/mise/pull/2138)
- added registry command by [@jdx](https://github.com/jdx) in [#2147](https://github.com/jdx/mise/pull/2147)
- pre-commit and github action generate commands by [@jdx](https://github.com/jdx) in [#2144](https://github.com/jdx/mise/pull/2144)

### 🐛 Bug Fixes

- raise error if resolve fails and is a CLI argument by [@jdx](https://github.com/jdx) in [#2136](https://github.com/jdx/mise/pull/2136)
- clean up architectures for precompiled binaries by [@jdx](https://github.com/jdx) in [#2137](https://github.com/jdx/mise/pull/2137)
- add target and other configs to cache key logic by [@jdx](https://github.com/jdx) in [#2141](https://github.com/jdx/mise/pull/2141)

### 🚜 Refactor

- remove cmd_forge by [@jdx](https://github.com/jdx) in [#2142](https://github.com/jdx/mise/pull/2142)

### 🧪 Testing

- separate nightly into its own job by [@jdx](https://github.com/jdx) in [#2145](https://github.com/jdx/mise/pull/2145)
- lint in nightly job by [@jdx](https://github.com/jdx) in [b5a3d08](https://github.com/jdx/mise/commit/b5a3d0884655f884319b23924d06566d597a4abe)

## [2024.5.17](https://github.com/jdx/mise/compare/v2024.5.16..v2024.5.17) - 2024-05-18

### 🚀 Features

- allow install specific version from https://mise.run #1800 by [@Its-Alex](https://github.com/Its-Alex) in [#2123](https://github.com/jdx/mise/pull/2123)
- confirm all plugins by [@roele](https://github.com/roele) in [#2126](https://github.com/jdx/mise/pull/2126)
- allow ignore missing plugin by [@roele](https://github.com/roele) in [#2127](https://github.com/jdx/mise/pull/2127)

### 🐛 Bug Fixes

- **(pipx)** depend on python by [@jdx](https://github.com/jdx) in [89b9c9a](https://github.com/jdx/mise/commit/89b9c9a7db4e1db624019bb760ed32a76d5a7597)

### 🚜 Refactor

- fetch transitive dependencies by [@jdx](https://github.com/jdx) in [#2131](https://github.com/jdx/mise/pull/2131)

### 🧪 Testing

- pass MISE_LOG_LEVEL through by [@jdx](https://github.com/jdx) in [7dea795](https://github.com/jdx/mise/commit/7dea795967ee11526af6e95a55e19bf7fddb3315)
- make unit tests work shuffled by [@jdx](https://github.com/jdx) in [#2133](https://github.com/jdx/mise/pull/2133)
- ensure tests reset by [@jdx](https://github.com/jdx) in [#2134](https://github.com/jdx/mise/pull/2134)
- ensure tests reset by [@jdx](https://github.com/jdx) in [feeaf8f](https://github.com/jdx/mise/commit/feeaf8f072a253305df9f59d357596a87fc0da36)
- clean up .test.mise.toml file by [@jdx](https://github.com/jdx) in [c41e0a3](https://github.com/jdx/mise/commit/c41e0a3adedf5502901d5c8b5f49d2f51e4f9428)

## [2024.5.16](https://github.com/jdx/mise/compare/v2024.5.15..v2024.5.16) - 2024-05-15

### 🚀 Features

- **(registry)** map ubi -> cargo:ubi by [@jdx](https://github.com/jdx) in [#2110](https://github.com/jdx/mise/pull/2110)
- **(tasks)** add --json flag by [@vrslev](https://github.com/vrslev) in [#2116](https://github.com/jdx/mise/pull/2116)

### 🐛 Bug Fixes

- support "mise.toml" filename by [@jdx](https://github.com/jdx) in [035745f](https://github.com/jdx/mise/commit/035745f95f5f143b62e6d3cdc6cfbaa4a6d887e0)

### 🔍 Other Changes

- add rustfmt to release-plz by [@jdx](https://github.com/jdx) in [2d530f6](https://github.com/jdx/mise/commit/2d530f645b6263c6162380684ab7914efc3dce39)

### New Contributors

- @vrslev made their first contribution in [#2116](https://github.com/jdx/mise/pull/2116)

## [2024.5.15](https://github.com/jdx/mise/compare/v2024.5.14..v2024.5.15) - 2024-05-14

### 🚀 Features

- support non-hidden configs by [@jdx](https://github.com/jdx) in [#2114](https://github.com/jdx/mise/pull/2114)

### 🐛 Bug Fixes

- handle sub-0.1 in new resolving logic by [@jdx](https://github.com/jdx) in [fd943a1](https://github.com/jdx/mise/commit/fd943a184bcc64866b761514788b5a0e4be07ac0)

### 🚜 Refactor

- ToolVersionRequest -> ToolRequest by [@jdx](https://github.com/jdx) in [45caece](https://github.com/jdx/mise/commit/45caece3517792b02444620edb96c18c2d7513c2)

### 🧪 Testing

- fail-fast by [@jdx](https://github.com/jdx) in [2338376](https://github.com/jdx/mise/commit/23383760900ede666865e073acb680dced37d8fc)
- update deno version by [@jdx](https://github.com/jdx) in [71f5480](https://github.com/jdx/mise/commit/71f5480e780953e03aa97682535a58767956a927)
- check plugin dependencies with python and pipx. by [@Adirelle](https://github.com/Adirelle) in [#2109](https://github.com/jdx/mise/pull/2109)
- wait a bit longer before retrying e2e test failures by [@jdx](https://github.com/jdx) in [d098c86](https://github.com/jdx/mise/commit/d098c866a415459981a5bb770f60b51067f444ce)

### 🔍 Other Changes

- optimize imports by [@jdx](https://github.com/jdx) in [892184f](https://github.com/jdx/mise/commit/892184f5681c7f1863cbd89f07fca0cf5fa3afb2)
- optimize imports by [@jdx](https://github.com/jdx) in [54bfee6](https://github.com/jdx/mise/commit/54bfee6b435f8b1cbfba7210f73b9dfde1a3c6f1)
- automatically optimize imports by [@jdx](https://github.com/jdx) in [#2113](https://github.com/jdx/mise/pull/2113)
- fix release-plz with nightly rustfmt by [@jdx](https://github.com/jdx) in [0b6521a](https://github.com/jdx/mise/commit/0b6521ab620cf6c16e36d9c5d3cf56b7b0ee81eb)

## [2024.5.14](https://github.com/jdx/mise/compare/v2024.5.13..v2024.5.14) - 2024-05-14

### 🚀 Features

- **(erlang)** make erlang core plugin stable by [@jdx](https://github.com/jdx) in [d4bde6a](https://github.com/jdx/mise/commit/d4bde6a15297d693a00e7194ea3e20f399ae4184)
- **(python)** make python_compile 3-way switch by [@jdx](https://github.com/jdx) in [#2100](https://github.com/jdx/mise/pull/2100)
- raise warning instead if install default gems failed by [@jiz4oh](https://github.com/jiz4oh) in [83350be](https://github.com/jdx/mise/commit/83350be1976185dd2dd2f13e8f7a9ee940449d16)

### 🐛 Bug Fixes

- **(python)** correct flavor for macos-x64 by [@jdx](https://github.com/jdx) in [#2104](https://github.com/jdx/mise/pull/2104)
- warn if failure installing default packages by [@jdx](https://github.com/jdx) in [#2102](https://github.com/jdx/mise/pull/2102)
- hide missing runtime warning in shim context by [@jdx](https://github.com/jdx) in [#2103](https://github.com/jdx/mise/pull/2103)
- handle tool_version parse failures by [@jdx](https://github.com/jdx) in [#2105](https://github.com/jdx/mise/pull/2105)

### ⚡ Performance

- memoize `which` results by [@jdx](https://github.com/jdx) in [89291ec](https://github.com/jdx/mise/commit/89291ecaa4bc53e99d61eaf3c24040f9fee11240)

### 🔍 Other Changes

- do not fail workflow if cant post message by [@jdx](https://github.com/jdx) in [0f3bfd3](https://github.com/jdx/mise/commit/0f3bfd38c5d9a7add05499bb230577ebe849060f)

### New Contributors

- @jiz4oh made their first contribution

## [2024.5.13](https://github.com/jdx/mise/compare/v2024.5.12..v2024.5.13) - 2024-05-14

### 🚀 Features

- pass github token to UBI and cargo-binstall backends. by [@Adirelle](https://github.com/Adirelle) in [#2090](https://github.com/jdx/mise/pull/2090)

### 🚜 Refactor

- bubble up resolve errors by [@jdx](https://github.com/jdx) in [#2094](https://github.com/jdx/mise/pull/2094)

### 🔍 Other Changes

- always build with git2 feature by [@jdx](https://github.com/jdx) in [fb51b57](https://github.com/jdx/mise/commit/fb51b57234e3227e00b1866f7ed93bf9d1bc90db)

## [2024.5.12](https://github.com/jdx/mise/compare/v2024.5.11..v2024.5.12) - 2024-05-13

### ⚡ Performance

- various performance tweaks by [@jdx](https://github.com/jdx) in [#2091](https://github.com/jdx/mise/pull/2091)

### 🧪 Testing

- only set realpath for macos by [@jdx](https://github.com/jdx) in [cdd1c93](https://github.com/jdx/mise/commit/cdd1c935f335e0119a7821b22415b792cc83109a)

## [2024.5.11](https://github.com/jdx/mise/compare/v2024.5.10..v2024.5.11) - 2024-05-13

### 🐛 Bug Fixes

- **(exec)** do not default to "latest" if a version is already configured by [@jdx](https://github.com/jdx) in [f55e8ef](https://github.com/jdx/mise/commit/f55e8efccc2050cbf1a9b14f6396d7ee6fc20828)
- **(self_update)** downgrade reqwest by [@jdx](https://github.com/jdx) in [0e17a84](https://github.com/jdx/mise/commit/0e17a84ebe9ea087d27a6c825a0bf6840cfcd3ca)
- prompt to trust config files with env vars by [@jdx](https://github.com/jdx) in [55b3a4b](https://github.com/jdx/mise/commit/55b3a4bb1e394a3830f476594514216a4490de82)

### 🧪 Testing

- work with macos /private tmp dir by [@jdx](https://github.com/jdx) in [7d8ffaf](https://github.com/jdx/mise/commit/7d8ffaf2bc3341293b4884df2cdf1e14913f5eb6)

## [2024.5.10](https://github.com/jdx/mise/compare/v2024.5.9..v2024.5.10) - 2024-05-13

### 🐛 Bug Fixes

- fixed misc bugs with ubi+pipx backends by [@jdx](https://github.com/jdx) in [#2083](https://github.com/jdx/mise/pull/2083)

### 🔍 Other Changes

- updated reqwest by [@jdx](https://github.com/jdx) in [d927085](https://github.com/jdx/mise/commit/d92708585b62d65a838e37c022a3796de5fefe1d)

### 📦️ Dependency Updates

- update rust crate xx to v1 by [@renovate[bot]](https://github.com/renovate[bot]) in [#2081](https://github.com/jdx/mise/pull/2081)

## [2024.5.9](https://github.com/jdx/mise/compare/v2024.5.8..v2024.5.9) - 2024-05-12

### 🐛 Bug Fixes

- `.` in `list-bin-paths` was taken as is to form `PATH` by [@FranklinYinanDing](https://github.com/FranklinYinanDing) in [#2077](https://github.com/jdx/mise/pull/2077)

### 🧪 Testing

- use fd instead of find for macos compat by [@jdx](https://github.com/jdx) in [#2074](https://github.com/jdx/mise/pull/2074)
- test_java_corretto is not slow by [@jdx](https://github.com/jdx) in [92267b1](https://github.com/jdx/mise/commit/92267b1eb861357433005b26134689b0ce43a2b0)
- mark some e2e tests slow by [@jdx](https://github.com/jdx) in [99f9454](https://github.com/jdx/mise/commit/99f9454e4f062914ab4e4cd950d2f11023bd06bc)
- mark test_pipx as slow by [@jdx](https://github.com/jdx) in [ced564a](https://github.com/jdx/mise/commit/ced564ab5b8786f74d25d2a92e68c58ca488c122)
- add homebrew to e2e PATH by [@jdx](https://github.com/jdx) in [f1c7fb3](https://github.com/jdx/mise/commit/f1c7fb3434edc18787a293dc033459f78dd39514)

### 🔍 Other Changes

- add fd to e2e-linux jobs by [@jdx](https://github.com/jdx) in [9f57dae](https://github.com/jdx/mise/commit/9f57dae9298c4124352c8e7528024265a068ecc9)
- bump usage-lib by [@jdx](https://github.com/jdx) in [#2078](https://github.com/jdx/mise/pull/2078)
- add permissions for pr comment tool by [@jdx](https://github.com/jdx) in [64cb8da](https://github.com/jdx/mise/commit/64cb8dacd1b5c39c21cafa03eab361e68ac3a1d9)

### New Contributors

- @FranklinYinanDing made their first contribution in [#2077](https://github.com/jdx/mise/pull/2077)

## [2024.5.8](https://github.com/jdx/mise/compare/v2024.5.7..v2024.5.8) - 2024-05-12

### 🐛 Bug Fixes

- use correct url for aur-bin by [@jdx](https://github.com/jdx) in [a683c15](https://github.com/jdx/mise/commit/a683c1593d3c83660a42e4e6685522edb20e0480)
- handle race condition when initializing backends with dependencies by [@jdx](https://github.com/jdx) in [#2071](https://github.com/jdx/mise/pull/2071)

## [2024.5.7](https://github.com/jdx/mise/compare/v2024.5.6..v2024.5.7) - 2024-05-12

### 🧪 Testing

- add coverage report summary by [@jdx](https://github.com/jdx) in [#2065](https://github.com/jdx/mise/pull/2065)

### 🔍 Other Changes

- fix release job by [@jdx](https://github.com/jdx) in [a491270](https://github.com/jdx/mise/commit/a49127029b67d39f80708e47cfc20351faca941f)
- fix release job by [@jdx](https://github.com/jdx) in [90268db](https://github.com/jdx/mise/commit/90268dbdbb71f6e0ba51dbc657536029c2aac099)

## [2024.5.6](https://github.com/jdx/mise/compare/v2024.5.5..v2024.5.6) - 2024-05-12

### 🚀 Features

- add cargo-binstall as dependency for cargo backend by [@jdx](https://github.com/jdx) in [94868af](https://github.com/jdx/mise/commit/94868afcca9731c43fb48670ed0d7d4f40a4fab8)

### 🐛 Bug Fixes

- performance fix for _.file/_.path by [@jdx](https://github.com/jdx) in [76202de](https://github.com/jdx/mise/commit/76202ded1bb47ecf9c1a5a7e6f71216aca26c68e)

### 🚜 Refactor

- **(cargo)** improve cargo-binstall check by [@jdx](https://github.com/jdx) in [d1432e0](https://github.com/jdx/mise/commit/d1432e0316a1e1b335022372ef0896c5b5b7b0df)

### 🧪 Testing

- **(e2e)** fix mise path by [@jdx](https://github.com/jdx) in [f6de41a](https://github.com/jdx/mise/commit/f6de41af71e7ad03d831bf602c291f38dd6c0fd8)
- isolation of end-to-end tests by [@Adirelle](https://github.com/Adirelle) in [#2047](https://github.com/jdx/mise/pull/2047)
- simplify release e2e jobs by [@jdx](https://github.com/jdx) in [b97a0bb](https://github.com/jdx/mise/commit/b97a0bb563762a4de40ea49a5bccb3a74daafb8f)

### 🔍 Other Changes

- **(aur)** added usage as optional dependency by [@jdx](https://github.com/jdx) in [5280ece](https://github.com/jdx/mise/commit/5280ece4f2f2337e7dd56c17062a09fdf1e1c808)
- **(codacy)** fix codacy on forks by [@jdx](https://github.com/jdx) in [c70d567](https://github.com/jdx/mise/commit/c70d567b2529e7054a79e461114a85c2fceb457d)
- switch back to secret for codacy by [@jdx](https://github.com/jdx) in [7622cfb](https://github.com/jdx/mise/commit/7622cfbb969c9a40638855d13009a72e4dc91ac8)
- added semantic-pr check by [@jdx](https://github.com/jdx) in [#2063](https://github.com/jdx/mise/pull/2063)
- fix whitespace by [@jdx](https://github.com/jdx) in [3eadcb5](https://github.com/jdx/mise/commit/3eadcb548960729e7168842af18c8200b3b70863)

## [2024.5.5](https://github.com/jdx/mise/compare/v2024.5.4..v2024.5.5) - 2024-05-12

### 🐛 Bug Fixes

- **(pipx)** remove unneeded unwrap by [@jdx](https://github.com/jdx) in [273c73d](https://github.com/jdx/mise/commit/273c73d15d77d42e8ff4ed732335cc418f903e0b)
- resolve bug with backends not resolving mise-installed tools by [@jdx](https://github.com/jdx) in [#2059](https://github.com/jdx/mise/pull/2059)

## [2024.5.4] - 2024-05-11

### 🚀 Features

- add more directory env var configs by [@jdx](https://github.com/jdx) in [#2056](https://github.com/jdx/mise/pull/2056)

### 🚜 Refactor

- move opts from ToolVersion to ToolVersionRequest struct by [@jdx](https://github.com/jdx) in [#2057](https://github.com/jdx/mise/pull/2057)
- remove use of mutex by [@jdx](https://github.com/jdx) in [278d028](https://github.com/jdx/mise/commit/278d028247adcd3a166f11281f81dd7a437e5547)

### 📚 Documentation

- **(changelog)** cleaning up changelog by [@jdx](https://github.com/jdx) in [845c1af](https://github.com/jdx/mise/commit/845c1afdc58437d083f0f3d50e4733142bef2281)

### 🔍 Other Changes

- Commit from GitHub Actions (test) by [@mise-en-dev](https://github.com/mise-en-dev) in [695f851](https://github.com/jdx/mise/commit/695f8513c0117623ca190c052c603a6b910814ad)
- Merge pull request #2019 from jdx/release by [@jdx](https://github.com/jdx) in [6bbd3d1](https://github.com/jdx/mise/commit/6bbd3d17d353eba1684eb11799f6b3684e38b578)
- include symlink error context in error message by [@KlotzAndrew](https://github.com/KlotzAndrew) in [ddd58fc](https://github.com/jdx/mise/commit/ddd58fc7eca72163dd0541596c5b6f06712aec28)
- Merge pull request #2040 from KlotzAndrew/aklotz/show_symlink_error by [@jdx](https://github.com/jdx) in [e71a8a0](https://github.com/jdx/mise/commit/e71a8a07e3385bf9bfe0985259325febd3bcf977)
- continue git subtree on error by [@jdx](https://github.com/jdx) in [a2c590c](https://github.com/jdx/mise/commit/a2c590c7dd82ac60c22844ef7e4ef88da3c1e507)
- squash registry by [@jdx](https://github.com/jdx) in [143ea6e](https://github.com/jdx/mise/commit/143ea6e589c8232c1d8a61aa33a576815754a3f0)
- reclone registry in release-plz job by [@jdx](https://github.com/jdx) in [05848a5](https://github.com/jdx/mise/commit/05848a52ea19c27e77ebf30310e7a4753c1b8ab0)
- reclone registry in release-plz job by [@jdx](https://github.com/jdx) in [c020c1e](https://github.com/jdx/mise/commit/c020c1e60347fcf9538293d141922eff1728500a)
- updated changelog by [@jdx](https://github.com/jdx) in [0465520](https://github.com/jdx/mise/commit/0465520f4c2d1d78a5ddc0c1d955a062d6f34d3b)
- show bash trace in release-plz by [@jdx](https://github.com/jdx) in [8a322bc](https://github.com/jdx/mise/commit/8a322bc2740a1c5676574cebdeb4c02726f36358)

### New Contributors

- @KlotzAndrew made their first contribution

<!-- generated by git-cliff -->
