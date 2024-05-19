# Changelog

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

* @vrslev made their first contribution in [#2116](https://github.com/jdx/mise/pull/2116)

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

* @jiz4oh made their first contribution

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

* @FranklinYinanDing made their first contribution in [#2077](https://github.com/jdx/mise/pull/2077)

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

* @KlotzAndrew made their first contribution

<!-- generated by git-cliff -->
