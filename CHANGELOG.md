# Changelog

## [2024.5.3](https://github.com/jdx/mise/compare/v2024.5.2..v2024.5.3) - 2024-05-06

### 🚀 Features

- **(env)** supports glob patterns in `env._.file` and `env._.source` (fix #1916) by [@noirbizarre](https://github.com/noirbizarre) in [#2016](https://github.com/jdx/mise/pull/2016)
- cleanup invalid symlinks in .local/state/mise/(tracked|trusted)-configs by [@roele](https://github.com/roele) in [#2036](https://github.com/jdx/mise/pull/2036)

### 🐛 Bug Fixes

- **(plugin-update)** Handle errors from the underlying plugin updates by [@offbyone](https://github.com/offbyone) in [#2024](https://github.com/jdx/mise/pull/2024)
- backend install directory not removed if empty by [@roele](https://github.com/roele) in [#2018](https://github.com/jdx/mise/pull/2018)
- mise trust doesn't handle relative paths by [@roele](https://github.com/roele) in [#2037](https://github.com/jdx/mise/pull/2037)

### 🔍 Other Changes

- Update README.md by [@jdx](https://github.com/jdx) in [40e82be](https://github.com/jdx/mise/commit/40e82be7e187cb09d2dad1c0d8b61078c4f7cebe)
- move kachick plugins to mise-plugins by Jeff Dickey in [a41b296](https://github.com/jdx/mise/commit/a41b296d7f599de3bccfb31c71da9606fd508216)

### 📦️ Dependency Updates

- update rust crate zip to v1.1.4 by [@renovate[bot]](https://github.com/renovate[bot]) in [#2030](https://github.com/jdx/mise/pull/2030)

### New Contributors

* @noirbizarre made their first contribution in [#2016](https://github.com/jdx/mise/pull/2016)

## [2024.5.2](https://github.com/jdx/mise/compare/v2024.5.1..v2024.5.2) - 2024-05-02

### 🐛 Bug Fixes

- **(self_update)** show --version param by [@jdx](https://github.com/jdx) in [#2013](https://github.com/jdx/mise/pull/2013)

## [2024.5.1](https://github.com/jdx/mise/compare/v2024.5.0..v2024.5.1) - 2024-05-02

### 🐛 Bug Fixes

- **(ruby)** handle github rate limits when fetching ruby-build version by [@jdx](https://github.com/jdx) in [4a538a7](https://github.com/jdx/mise/commit/4a538a7e282de8c59ba51ec6de64bb715debe022)
- **(ruby)** attempt to update ruby-build if it cannot check version by [@jdx](https://github.com/jdx) in [9f6e2ef](https://github.com/jdx/mise/commit/9f6e2efeb03a9cfc1786f88e84ce03ed6399a304)
- prevent crashing if "latest" is not a symlink by [@jdx](https://github.com/jdx) in [91291e0](https://github.com/jdx/mise/commit/91291e09e8fd3395f8a1265c9af2bd22eff46993)
- edge case around "latest" being the "latest" version by [@jdx](https://github.com/jdx) in [33f5473](https://github.com/jdx/mise/commit/33f547357e9f082115e81ff852c523b406e5226d)
- show source file on resolve error by [@jdx](https://github.com/jdx) in [881dbeb](https://github.com/jdx/mise/commit/881dbeb9a34fcb231bc83e14d1a12314bb995870)

### 📚 Documentation

- **(python)** warn about precompiled python and poetry by [@jdx](https://github.com/jdx) in [3c07dce](https://github.com/jdx/mise/commit/3c07dced232970fce3d585277089cbf374e4d64a)

### 🧪 Testing

- **(self-update)** try to enable self update test by [@jdx](https://github.com/jdx) in [778e90a](https://github.com/jdx/mise/commit/778e90af1bdfb5095e2f0b5c1b625c5abab7ee45)
- fix the test-plugins job by [@jdx](https://github.com/jdx) in [669530c](https://github.com/jdx/mise/commit/669530ce5bbd902ad0cd39e87e6c442d184353b9)

### 🔍 Other Changes

- **(release)** disable cache by [@jdx](https://github.com/jdx) in [b69edc6](https://github.com/jdx/mise/commit/b69edc67c83284ee758c92258608298eaba25929)
- **(ruby)** change ruby-build update failure to warn-level by [@jdx](https://github.com/jdx) in [d6f7f22](https://github.com/jdx/mise/commit/d6f7f22df93a862929ebbc8f3b6a1309e1e3c875)

## [2024.5.0](https://github.com/jdx/mise/compare/v2024.4.12..v2024.5.0) - 2024-05-01

### 🐛 Bug Fixes

- **(release)** use target/release dir by [@jdx](https://github.com/jdx) in [e6448b3](https://github.com/jdx/mise/commit/e6448b335cf99db6fb2bdfd4c3f49ba255c2d8de)
- **(release)** fixed the "serious" profile by [@jdx](https://github.com/jdx) in [487a1a0](https://github.com/jdx/mise/commit/487a1a0d336fed180123659ac59d1106d79f2d60)

### 🔍 Other Changes

- **(release)** added "serious" profile by [@jdx](https://github.com/jdx) in [f8ce139](https://github.com/jdx/mise/commit/f8ce139c1d0b41006dbbf1707801bf665f201ec6)

### 📦️ Dependency Updates

- update rust crate rmp-serde to 1.3.0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#2007](https://github.com/jdx/mise/pull/2007)
- update rust crate base64 to 0.22.1 by [@renovate[bot]](https://github.com/renovate[bot]) in [#2006](https://github.com/jdx/mise/pull/2006)

## [2024.4.12](https://github.com/jdx/mise/compare/v2024.4.11..v2024.4.12) - 2024-04-30

### 🐛 Bug Fixes

- **(self_update)** downgrade to fix signature verification issue by [@jdx](https://github.com/jdx) in [dbe1971](https://github.com/jdx/mise/commit/dbe1971c337a29f2e92fd1b765436e67abf7f04e)

## [2024.4.11](https://github.com/jdx/mise/compare/v2024.4.10..v2024.4.11) - 2024-04-30

### 🐛 Bug Fixes

- **(self-update)** always use rustls by [@jdx](https://github.com/jdx) in [93a9c57](https://github.com/jdx/mise/commit/93a9c57ae895f1772a5ae8146d83713f631c77f1)

### 🧪 Testing

- **(java)** added e2e test for corretto-8 shorthand by [@jdx](https://github.com/jdx) in [#1995](https://github.com/jdx/mise/pull/1995)

### 🔍 Other Changes

- **(release)** fix cache by [@jdx](https://github.com/jdx) in [b54b25d](https://github.com/jdx/mise/commit/b54b25d06c49b5116ed37dda4c08005dfe7e6e11)
- fix clippy warnings in latest rust beta by [@jdx](https://github.com/jdx) in [#1994](https://github.com/jdx/mise/pull/1994)

### 📦️ Dependency Updates

- update rust crate flate2 to 1.0.30 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1997](https://github.com/jdx/mise/pull/1997)

## [2024.4.10](https://github.com/jdx/mise/compare/v2024.4.9..v2024.4.10) - 2024-04-29

### 🐛 Bug Fixes

- **(docker)** create path to cargo registry cache by [@jdx](https://github.com/jdx) in [ed91c1c](https://github.com/jdx/mise/commit/ed91c1c5f928751c6bc1ce23ac0595c063648677)

### 🔍 Other Changes

- Revert "fix(java): inconsistent version resolution " by [@jdx](https://github.com/jdx) in [#1993](https://github.com/jdx/mise/pull/1993)

## [2024.4.9](https://github.com/jdx/mise/compare/v2024.4.8..v2024.4.9) - 2024-04-29

### 🚀 Features

- **(node)** support comments in .nvmrc/.node-version by [@jdx](https://github.com/jdx) in [5915ae0](https://github.com/jdx/mise/commit/5915ae0a23d322e37f22847be11638f8ba108c15)
- cli command for listing backends by [@roele](https://github.com/roele) in [#1989](https://github.com/jdx/mise/pull/1989)

### 🐛 Bug Fixes

- **(ci)** git2 reference by [@jdx](https://github.com/jdx) in [#1961](https://github.com/jdx/mise/pull/1961)
- **(docker)** Ensure the e2e tests pass in the dev container by [@Adirelle](https://github.com/Adirelle) in [#1942](https://github.com/jdx/mise/pull/1942)
- **(java)** inconsistent version resolution by [@roele](https://github.com/roele) in [#1957](https://github.com/jdx/mise/pull/1957)
- **(zig)** can't install zig@master from v2024.4.6 by [@roele](https://github.com/roele) in [#1958](https://github.com/jdx/mise/pull/1958)
- use mise fork of asdf-maven by [@jdx](https://github.com/jdx) in [5a01c1b](https://github.com/jdx/mise/commit/5a01c1b336a6e0a2ca0167aee6fa865318bd7f81)
- deal with missing go/cargo/npm/etc in backends by [@jdx](https://github.com/jdx) in [#1976](https://github.com/jdx/mise/pull/1976)
- mise doesn't change the trust hash file by [@roele](https://github.com/roele) in [#1979](https://github.com/jdx/mise/pull/1979)

### 🚜 Refactor

- converted just tasks in mise tasks. by [@Adirelle](https://github.com/Adirelle) in [#1948](https://github.com/jdx/mise/pull/1948)

### 🧪 Testing

- added cache for docker tests by [@jdx](https://github.com/jdx) in [#1977](https://github.com/jdx/mise/pull/1977)

### 🔍 Other Changes

- **(docker)** removed unused image by [@jdx](https://github.com/jdx) in [4150207](https://github.com/jdx/mise/commit/4150207c3464bf47207ea1c3c0959e7141ab27b8)
- **(renovate)** ignore changes to registry/ subtree by [@jdx](https://github.com/jdx) in [c556149](https://github.com/jdx/mise/commit/c556149a88e73825306d98e3e3ea5b53692e0900)
- buildjet by [@jdx](https://github.com/jdx) in [#1953](https://github.com/jdx/mise/pull/1953)
- make git2 an optional build dependency by [@jdx](https://github.com/jdx) in [#1960](https://github.com/jdx/mise/pull/1960)
- remove CODEOWNERS by [@jdx](https://github.com/jdx) in [304ba17](https://github.com/jdx/mise/commit/304ba171fd95701c04beb3d2a76bde0463a54209)

### 📦️ Dependency Updates

- update rust crate color-print to 0.3.6 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1943](https://github.com/jdx/mise/pull/1943)
- update amannn/action-semantic-pull-request action to v5.5.0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1947](https://github.com/jdx/mise/pull/1947)
- update rust crate demand to 1.1.1 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1944](https://github.com/jdx/mise/pull/1944)
- update rust crate self_update to 0.40.0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1954](https://github.com/jdx/mise/pull/1954)
- update rust crate flate2 to 1.0.29 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1963](https://github.com/jdx/mise/pull/1963)
- update serde monorepo to 1.0.199 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1964](https://github.com/jdx/mise/pull/1964)
- update rust crate demand to 1.1.2 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1965](https://github.com/jdx/mise/pull/1965)
- update rust crate zip to 1.1.2 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1985](https://github.com/jdx/mise/pull/1985)

### New Contributors

* @Adirelle made their first contribution in [#1948](https://github.com/jdx/mise/pull/1948)

## [2024.4.8](https://github.com/jdx/mise/compare/v2024.4.7..v2024.4.8) - 2024-04-23

### 🚀 Features

- add periphery by [@MontakOleg](https://github.com/MontakOleg) in [7f51540](https://github.com/jdx/mise/commit/7f51540695664412dab4008b0d061bfaca5b0bc2)
- add danger-js by [@MontakOleg](https://github.com/MontakOleg) in [6e61cf7](https://github.com/jdx/mise/commit/6e61cf7c97d03094a6ac86656b64fdeb85e84df5)

### 🐛 Bug Fixes

- **(exec)** default to @latest version by [@zph](https://github.com/zph) in [#1926](https://github.com/jdx/mise/pull/1926)
- rename bin -> ubi by [@jdx](https://github.com/jdx) in [0843b78](https://github.com/jdx/mise/commit/0843b78e6ab9a3dd2965f0218760c1a3336c4ca5)

### 📚 Documentation

- **(changelog)** reorder changelog topics by [@jdx](https://github.com/jdx) in [#1939](https://github.com/jdx/mise/pull/1939)
- fixed asdf-xcbeautify url by [@jdx](https://github.com/jdx) in [d4134bc](https://github.com/jdx/mise/commit/d4134bcb399a8d9da4e9670500e01d832b9a8e46)

### 🔍 Other Changes

- use https to get gpgkey by [@sjpalf](https://github.com/sjpalf) in [#1936](https://github.com/jdx/mise/pull/1936)
- Update xcbeautify by [@jdx](https://github.com/jdx) in [cb48b68](https://github.com/jdx/mise/commit/cb48b68bb6a0c7962b1ef95641514ba64ac63bd1)
- Include e2e folder in shfmt editorconfig for 2 spaces indenting by [@zph](https://github.com/zph) in [#1937](https://github.com/jdx/mise/pull/1937)
- disable megalinter by [@jdx](https://github.com/jdx) in [3dd1006](https://github.com/jdx/mise/commit/3dd1006a8367a852a6f415256b8301771f8fa8d6)

### New Contributors

* @MontakOleg made their first contribution
* @sjpalf made their first contribution in [#1936](https://github.com/jdx/mise/pull/1936)

## [2024.4.7](https://github.com/jdx/mise/compare/v2024.4.6..v2024.4.7) - 2024-04-22

### 🐛 Bug Fixes

- **(zig)** make zig core plugin experimental by [@jdx](https://github.com/jdx) in [45274bc](https://github.com/jdx/mise/commit/45274bc1415ac5dc307a82a93db952a1cf811210)

## [2024.4.6](https://github.com/jdx/mise/compare/v2024.4.5..v2024.4.6) - 2024-04-22

### 🚀 Features

- Pipx Backend by [@zph](https://github.com/zph) in [#1923](https://github.com/jdx/mise/pull/1923)
- ubi backend by [@zph](https://github.com/zph) in [#1932](https://github.com/jdx/mise/pull/1932)

### 🐛 Bug Fixes

- **(gleam)** use asdf-community fork by [@jc00ke](https://github.com/jc00ke) in [06599d8](https://github.com/jdx/mise/commit/06599d8977baaa2a2db7e2d144939049bbe9d20b)

### 🚜 Refactor

- use a metadata file for forges by [@roele](https://github.com/roele) in [#1909](https://github.com/jdx/mise/pull/1909)

### 🔍 Other Changes

- Add Zig language plugin by [@MustCodeAl](https://github.com/MustCodeAl) in [#1927](https://github.com/jdx/mise/pull/1927)

### 📦️ Dependency Updates

- update rust crate chrono to 0.4.38 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1907](https://github.com/jdx/mise/pull/1907)
- update rust crate serde_json to 1.0.116 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1908](https://github.com/jdx/mise/pull/1908)
- update rust crate toml_edit to 0.22.11 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1921](https://github.com/jdx/mise/pull/1921)
- bump rustls from 0.21.10 to 0.21.11 by [@dependabot[bot]](https://github.com/dependabot[bot]) in [#1922](https://github.com/jdx/mise/pull/1922)
- update rust crate rmp-serde to 1.2.0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1917](https://github.com/jdx/mise/pull/1917)
- update rust crate toml_edit to 0.22.12 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1924](https://github.com/jdx/mise/pull/1924)
- update rust crate usage-lib to 0.1.18 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1896](https://github.com/jdx/mise/pull/1896)
- update rust crate ctor to 0.2.8 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1913](https://github.com/jdx/mise/pull/1913)
- update serde monorepo to 1.0.198 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1914](https://github.com/jdx/mise/pull/1914)
- update rust crate thiserror to 1.0.59 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1929](https://github.com/jdx/mise/pull/1929)
- update rust crate zip to v1 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1930](https://github.com/jdx/mise/pull/1930)

### New Contributors

* @MustCodeAl made their first contribution in [#1927](https://github.com/jdx/mise/pull/1927)

## [2024.4.5](https://github.com/jdx/mise/compare/v2024.4.3..v2024.4.5) - 2024-04-15

### 🚀 Features

- **(doctor)** warn if a plugin overwrites a core plugin by [@roele](https://github.com/roele) in [#1900](https://github.com/jdx/mise/pull/1900)
- add option to list installed (backend) binaries by [@roele](https://github.com/roele) in [#1885](https://github.com/jdx/mise/pull/1885)
- add powerpipe by [@jc00ke](https://github.com/jc00ke) in [7369b74](https://github.com/jdx/mise/commit/7369b74550b28b45d7195d0b11c11e98cf5e4d29)
- add xcresultparser by [@nekrich](https://github.com/nekrich) in [92b4aeb](https://github.com/jdx/mise/commit/92b4aeb13e8350a76a9ceae9df1bb6270a2b2182)

### 🐛 Bug Fixes

- **(alpine)** use mise docker image by [@jdx](https://github.com/jdx) in [db65c3f](https://github.com/jdx/mise/commit/db65c3f5de1b1117bc6708b881de86f490057b68)
- **(heroku-cli)** use mise-plugins fork by [@jdx](https://github.com/jdx) in [2a92d9d](https://github.com/jdx/mise/commit/2a92d9d1bd0b275c7d27ca020f63e3089d789c8c)
- enable markdown-magic since it is working again by [@jdx](https://github.com/jdx) in [2b7b943](https://github.com/jdx/mise/commit/2b7b943d33ac91ea6eaded7f2fe84b472f73e073)
- mise panics if prefix: is used on certain core plugins by [@roele](https://github.com/roele) in [#1889](https://github.com/jdx/mise/pull/1889)
- go backend naming inconsistency (in mise ls and mise prune) by [@roele](https://github.com/roele) in [#1905](https://github.com/jdx/mise/pull/1905)

### 🧪 Testing

- fix github action branch by [@jdx](https://github.com/jdx) in [39eb2ab](https://github.com/jdx/mise/commit/39eb2abbdb7b136c541f84696dc038637280d8a7)

### 🔍 Other Changes

- **(move)** added TODO by [@jdx](https://github.com/jdx) in [5ffbcc1](https://github.com/jdx/mise/commit/5ffbcc134f27800109bb65335b4b9423742b6807)
- **(pre-commit)** added pre-commit by [@jdx](https://github.com/jdx) in [b2ff8cd](https://github.com/jdx/mise/commit/b2ff8cd88c5951326781fcc5c1405d3883ef21c1)
- **(pre-commit)** check json and toml files by [@jdx](https://github.com/jdx) in [5281712](https://github.com/jdx/mise/commit/5281712f63bb673b301be139274b5f2eab64c205)
- added podman plugin by [@carlosrtf](https://github.com/carlosrtf) in [24155e8](https://github.com/jdx/mise/commit/24155e8b9d5f342d52ccdd212f187243022efa0b)

### 📦️ Dependency Updates

- update rust crate built to 0.7.2 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1895](https://github.com/jdx/mise/pull/1895)
- update rust crate either to 1.11.0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1899](https://github.com/jdx/mise/pull/1899)

### New Contributors

* @carlosrtf made their first contribution

## [2024.4.3](https://github.com/jdx/mise/compare/v2024.4.2..v2024.4.3) - 2024-04-09

### 🐛 Bug Fixes

- **(docker)** repo fetch by [@jdx](https://github.com/jdx) in [bb68fc3](https://github.com/jdx/mise/commit/bb68fc33f98e5fe4518478f963d448bca61d54fe)
- **(docker)** repo fetch by [@jdx](https://github.com/jdx) in [#1878](https://github.com/jdx/mise/pull/1878)
- asdf-yarn by [@jdx](https://github.com/jdx) in [fc8de34](https://github.com/jdx/mise/commit/fc8de34e2e535ed7262c079087ff444a44dd5731)

### 🔍 Other Changes

- **(release-plz)** clean up PR/release description by [@jdx](https://github.com/jdx) in [14b4fc5](https://github.com/jdx/mise/commit/14b4fc5525e06cc150ae160c406ee06b58d95ce5)
- **(release-plz)** clean up PR/release description by [@jdx](https://github.com/jdx) in [769e7fe](https://github.com/jdx/mise/commit/769e7fe16e4f4df7380414f91e67345148e059de)
- **(release-plz)** disable subtree push by [@jdx](https://github.com/jdx) in [c74a12c](https://github.com/jdx/mise/commit/c74a12c3c31ba087a154d7b328374670a675f00a)
- **(sync)** added workflow by [@jdx](https://github.com/jdx) in [b773033](https://github.com/jdx/mise/commit/b7730335949235085001b23ddc382a7c44b18f12)
- **(sync)** pull and push changes by [@jdx](https://github.com/jdx) in [52bb0ae](https://github.com/jdx/mise/commit/52bb0aee4a91c66bb2966cf462c23eedfc0c5058)
- **(sync)** pull and push changes by [@jdx](https://github.com/jdx) in [28b9a52](https://github.com/jdx/mise/commit/28b9a52fce891ab28e9e4d2f7fc1207de07d0a84)
- **(sync)** pull and push changes by [@jdx](https://github.com/jdx) in [202c900](https://github.com/jdx/mise/commit/202c90051a81eff9ed12ef1483bc86cee355371c)
- **(sync)** pull and push changes by [@jdx](https://github.com/jdx) in [e3aefb1](https://github.com/jdx/mise/commit/e3aefb1eb915387ca08b484158e5f13c069912d1)
- **(sync)** pull and push changes by [@jdx](https://github.com/jdx) in [60f5b7a](https://github.com/jdx/mise/commit/60f5b7a44479385650e563d234ceb2ac0e135994)

## [2024.4.2](https://github.com/jdx/mise/compare/v2024.4.1..v2024.4.2) - 2024-04-09

### 🚀 Features

- **(completions)** switch to usage for zsh completions by [@jdx](https://github.com/jdx) in [#1875](https://github.com/jdx/mise/pull/1875)

### 🚜 Refactor

- **(default_shorthands)** automatically mark mise-plugins as trusted by [@jdx](https://github.com/jdx) in [538a90f](https://github.com/jdx/mise/commit/538a90f04447b306c0ea6d8009cdef3f92cd4735)

### 🔍 Other Changes

- **(cliff)** ignore previous registry commits by [@jdx](https://github.com/jdx) in [64f326d](https://github.com/jdx/mise/commit/64f326d903e13a104b88eb4b0986285ad2411f19)
- **(cliff)** ignore merge commits by [@jdx](https://github.com/jdx) in [d54b0a2](https://github.com/jdx/mise/commit/d54b0a2cd4d4bd02816719101cbe77905818d07a)
- **(default_shorthands)** fix count by [@jdx](https://github.com/jdx) in [a098228](https://github.com/jdx/mise/commit/a0982283273b4126c2d9b22ac2395de5cd3f73eb)
- **(homebrew)** delete unused script by [@jdx](https://github.com/jdx) in [f3b092f](https://github.com/jdx/mise/commit/f3b092fdaf2bcfc789485543770cc2374a469523)
- **(markdown-magic)** do not fail if markdown-magic fails by [@jdx](https://github.com/jdx) in [073fce7](https://github.com/jdx/mise/commit/073fce750dbe97818db6de5a7ae6482904730de2)
- **(markdownlint)** ignore registry/ files by [@jdx](https://github.com/jdx) in [dd8f47e](https://github.com/jdx/mise/commit/dd8f47e31574e103215b81ee07d19456066c4dfc)
- **(mega-linter)** ignore registry/ files by [@jdx](https://github.com/jdx) in [427574f](https://github.com/jdx/mise/commit/427574f31f976ee6868306807f54db9169b3140d)
- **(prettier)** ignore registry/ files by [@jdx](https://github.com/jdx) in [188c0e4](https://github.com/jdx/mise/commit/188c0e4480dc52eaf4ee782d887bd136f0c6dc42)
- **(python)** added debug info when no precompiled version is found by [@jdx](https://github.com/jdx) in [#1874](https://github.com/jdx/mise/pull/1874)
- **(registry)** auto-update registry subtree by [@jdx](https://github.com/jdx) in [fba690c](https://github.com/jdx/mise/commit/fba690c2185e6c66256203f42ee538e13fba6b83)
- **(release)** fixing registry autosync by [@jdx](https://github.com/jdx) in [266004b](https://github.com/jdx/mise/commit/266004b038588d791d3cefb11ab0094cdf79a929)
- **(release-plz)** push registry subtree changes by [@jdx](https://github.com/jdx) in [076c822](https://github.com/jdx/mise/commit/076c82228b3d79d7e9926c602825d9dbd101bd06)
- **(renovate)** disable lock file maintenance by [@jdx](https://github.com/jdx) in [f919db4](https://github.com/jdx/mise/commit/f919db4588f11b7bd2ba951b77ecf70c15ecbcc4)
- Add 'registry/' from commit 'c5d91ebfbf1b7a03203e8442a3f6348c41ce086d' by [@jdx](https://github.com/jdx) in [d6d46d0](https://github.com/jdx/mise/commit/d6d46d004b02b8dd2947da2314604c870f1221c8)

## [2024.4.1](https://github.com/jdx/mise/compare/v2024.4.0..v2024.4.1) - 2024-04-08

### 🐛 Bug Fixes

- **(doctor)** sort missing shims by [@jdx](https://github.com/jdx) in [f12d335](https://github.com/jdx/mise/commit/f12d3359b054c9b31a785ed9fdc41e20b317ddb4)
- **(uninstall)** fix uninstall completions by [@jdx](https://github.com/jdx) in [#1869](https://github.com/jdx/mise/pull/1869)

### 🧪 Testing

- **(audit)** removed workflow since dependabot is already doing this by [@jdx](https://github.com/jdx) in [9138e2e](https://github.com/jdx/mise/commit/9138e2e6ec09a36c7791c9a6b4e5f7ab138fcb63)
- **(mega-linter)** disable RUST_CLIPPY (slow) by [@jdx](https://github.com/jdx) in [8c64153](https://github.com/jdx/mise/commit/8c64153f7c5b3990d3b857b55fb0a94e7c11a6fc)

### 📦️ Dependency Updates

- bump h2 from 0.3.25 to 0.3.26 by [@dependabot[bot]](https://github.com/dependabot[bot]) in [#1866](https://github.com/jdx/mise/pull/1866)

## [2024.4.0](https://github.com/jdx/mise/compare/v2024.3.11..v2024.4.0) - 2024-04-02

### 🐛 Bug Fixes

- **(python)** install python when pip is disabled outside virtualenv by [@GabDug](https://github.com/GabDug) in [#1847](https://github.com/jdx/mise/pull/1847)

### 🔍 Other Changes

- **(release)** only save 1 build cache by [@jdx](https://github.com/jdx) in [f37f11d](https://github.com/jdx/mise/commit/f37f11dd56cb30c1df30d4a2a3df37290ce95a0b)
- **(release-plz)** rebuild release branch daily by [@jdx](https://github.com/jdx) in [3606d96](https://github.com/jdx/mise/commit/3606d9687ec205754269f7402a7f8095533627ae)
- Move logic to set current directory before loading other config by [@joshbode](https://github.com/joshbode) in [#1848](https://github.com/jdx/mise/pull/1848)

### New Contributors

* @GabDug made their first contribution in [#1847](https://github.com/jdx/mise/pull/1847)
* @joshbode made their first contribution in [#1848](https://github.com/jdx/mise/pull/1848)

## [2024.3.11](https://github.com/jdx/mise/compare/v2024.3.10..v2024.3.11) - 2024-03-30

### 🚀 Features

- **(task)** extend mise tasks output by [@roele](https://github.com/roele) in [#1845](https://github.com/jdx/mise/pull/1845)

### 🐛 Bug Fixes

- **(self-update)** respect yes setting in config by [@jdx](https://github.com/jdx) in [b4c4608](https://github.com/jdx/mise/commit/b4c4608ff2dbbde071e10acf6931204acf6d7d40)

### 📚 Documentation

- **(changelog)** fix commit message for releases by [@jdx](https://github.com/jdx) in [646df55](https://github.com/jdx/mise/commit/646df55f0627c80099026849dc235a8c3076a8e3)
- **(changelog)** fix commit message for releases by [@jdx](https://github.com/jdx) in [00d8728](https://github.com/jdx/mise/commit/00d87283181467e73b01b27179c096bb08203619)
- **(changelog)** fix commit message for releases by [@jdx](https://github.com/jdx) in [c5612f9](https://github.com/jdx/mise/commit/c5612f90b4e47bdf12ee74e7d33412e3c0b6184c)

### 🔍 Other Changes

- **(audit)** added workflow by [@jdx](https://github.com/jdx) in [9263fb4](https://github.com/jdx/mise/commit/9263fb4e1bc374145d9eff609e025559f9d4d7d1)
- **(deny)** remove multiple-versions warnings by [@jdx](https://github.com/jdx) in [efa133e](https://github.com/jdx/mise/commit/efa133e1fad5bc97c44f04494e5ce7cb9ccc3033)
- **(release-plz)** improve caching by [@jdx](https://github.com/jdx) in [97c79ee](https://github.com/jdx/mise/commit/97c79ee394c4ae3106cfd4dcfe5ed771b4330d19)
- **(release-plz)** use actions-rust-lang/setup-rust-toolchain@v1 by [@jdx](https://github.com/jdx) in [4813288](https://github.com/jdx/mise/commit/481328895a91eeae0d9a03fc1f0c18b211b491ab)
- **(test)** improve caching by [@jdx](https://github.com/jdx) in [ac919a1](https://github.com/jdx/mise/commit/ac919a1db9e8c03fc92a3077cf04edfda6bb971c)
- **(test)** only run lint-fix on main repo by [@jdx](https://github.com/jdx) in [aee7694](https://github.com/jdx/mise/commit/aee7694b47341baaba9fa5ef628f9540c6f93d72)

## [2024.3.10](https://github.com/jdx/mise/compare/v2024.3.9..v2024.3.10) - 2024-03-30

### 🐛 Bug Fixes

- use correct type for --cd by [@jdx](https://github.com/jdx) in [cf4f03e](https://github.com/jdx/mise/commit/cf4f03ed0145c5678e1ecbdb98c4426c9428d29a)

### 🚜 Refactor

- completions command by [@jdx](https://github.com/jdx) in [#1838](https://github.com/jdx/mise/pull/1838)

### 📚 Documentation

- improve CHANGELOG by [@jdx](https://github.com/jdx) in [#1839](https://github.com/jdx/mise/pull/1839)
- improve CHANGELOG by [@jdx](https://github.com/jdx) in [#1841](https://github.com/jdx/mise/pull/1841)
- remove duplicate PR labels in CHANGELOG by [@jdx](https://github.com/jdx) in [a3b27ef](https://github.com/jdx/mise/commit/a3b27efc37191f8be106345586cab08055ea476f)

## [2024.3.9](https://github.com/jdx/mise/compare/v2024.3.8..v2024.3.9) - 2024-03-24

### 🐛 Bug Fixes

- **(task)** script tasks don't pick up alias from comments by [@roele](https://github.com/roele) in [#1828](https://github.com/jdx/mise/pull/1828)
- downgrade reqwest to fix self-update by [@jdx](https://github.com/jdx) in [2f0820b](https://github.com/jdx/mise/commit/2f0820b8b0438f5224c6b2689f51f43b7f907bf5)

### 📦️ Dependency Updates

- update rust crate rayon to 1.10.0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1827](https://github.com/jdx/mise/pull/1827)

## [2024.3.8](https://github.com/jdx/mise/compare/v2024.3.7..v2024.3.8) - 2024-03-23

### 🚀 Features

- use http2 for reqwest by [@jdx](https://github.com/jdx) in [#1825](https://github.com/jdx/mise/pull/1825)

### 🐛 Bug Fixes

- **(nu)** Gracefully handle missing `$env.config` by [@texastoland](https://github.com/texastoland) in [#1809](https://github.com/jdx/mise/pull/1809)
- Apple x64 version of mise doesn't work by [@roele](https://github.com/roele) in [#1821](https://github.com/jdx/mise/pull/1821)

### 🧪 Testing

- fix warnings by [@jdx](https://github.com/jdx) in [f0604a3](https://github.com/jdx/mise/commit/f0604a3224d5081012101d5266879c6d0af0d39d)

### 🔍 Other Changes

- automatically bump minor version if month/year changes by [@mise-en-dev](https://github.com/mise-en-dev) in [96ad08d](https://github.com/jdx/mise/commit/96ad08d8acb6b7a4eff0be2f49022080d10b9b71)
- updated cargo-deny config by [@jdx](https://github.com/jdx) in [#1824](https://github.com/jdx/mise/pull/1824)
- fix version set by [@jdx](https://github.com/jdx) in [2be7fe5](https://github.com/jdx/mise/commit/2be7fe51c0fb9f66c43cd6e940f4eb18ee83c822)

### 📦️ Dependency Updates

- update rust crate toml_edit to 0.22.9 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1814](https://github.com/jdx/mise/pull/1814)
- update rust crate toml to 0.8.12 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1813](https://github.com/jdx/mise/pull/1813)
- update rust crate indexmap to 2.2.6 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1815](https://github.com/jdx/mise/pull/1815)
- update rust crate usage-lib to 0.1.17 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1818](https://github.com/jdx/mise/pull/1818)
- update rust crate regex to 1.10.4 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1817](https://github.com/jdx/mise/pull/1817)
- update rust crate which to 6.0.1 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1820](https://github.com/jdx/mise/pull/1820)
- update rust crate indoc to 2.0.5 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1816](https://github.com/jdx/mise/pull/1816)
- update rust crate versions to 6.2.0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1819](https://github.com/jdx/mise/pull/1819)
- update rust crate reqwest to 0.12.1 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1822](https://github.com/jdx/mise/pull/1822)

## [2024.3.7](https://github.com/jdx/mise/compare/v2024.3.6..v2024.3.7) - 2024-03-21

### 🐛 Bug Fixes

- **(task)** tasks not working in system config by [@roele](https://github.com/roele) in [#1803](https://github.com/jdx/mise/pull/1803)
- **(xonsh)** `shell` subcommand for xonsh by [@yggdr](https://github.com/yggdr) in [#1801](https://github.com/jdx/mise/pull/1801)
- jq Installed Using x86_64 on Apple Silicon using mise by [@roele](https://github.com/roele) in [#1804](https://github.com/jdx/mise/pull/1804)

### 📚 Documentation

- **(changelog)** improve styling by [@jdx](https://github.com/jdx) in [403033d](https://github.com/jdx/mise/commit/403033d269f88aa0c1e571e5613231eca84fbaac)
- **(changelog)** improve styling by [@jdx](https://github.com/jdx) in [cf4811b](https://github.com/jdx/mise/commit/cf4811b0cfa16d7c002e155539eac7a8d5c3912a)

### 🎨 Styling

- format default_shorthands.rs by [@jdx](https://github.com/jdx) in [a8ea813](https://github.com/jdx/mise/commit/a8ea81337ffd9cfd9201cc49d6a64ba93e10a9a7)

### 🧪 Testing

- install python/poetry at the same time by [@jdx](https://github.com/jdx) in [08a3304](https://github.com/jdx/mise/commit/08a33048b92a8ce3b551d0f7e39a28ac0bc29f07)

### 🔍 Other Changes

- **(release-plz)** use different bot email by [@jdx](https://github.com/jdx) in [59b814f](https://github.com/jdx/mise/commit/59b814fae7eedd6565286a6865b6539e2c058a36)
- **(release-plz)** sign release git tags by [@jdx](https://github.com/jdx) in [8ce5d37](https://github.com/jdx/mise/commit/8ce5d371515d287b8e5a5ccdbddeafa6e5d18952)
- **(test)** run all e2e tests on the release pr by [@jdx](https://github.com/jdx) in [f21c84b](https://github.com/jdx/mise/commit/f21c84b5683e986b93cf2f3f16c120a7168aacba)
- **(test)** run all e2e tests on the release pr by [@jdx](https://github.com/jdx) in [cf19dc5](https://github.com/jdx/mise/commit/cf19dc5eac9245a780a9135f7483e431ef686f69)
- **(test)** skip aur/aur-bin on release PR by [@jdx](https://github.com/jdx) in [9ddb424](https://github.com/jdx/mise/commit/9ddb424c133452d4cb1e4304c263ff74ca65811b)
- Refactor Nushell script by [@texastoland](https://github.com/texastoland) in [#1763](https://github.com/jdx/mise/pull/1763)
- rust 1.78 deprecation warning fixes by [@jdx](https://github.com/jdx) in [#1805](https://github.com/jdx/mise/pull/1805)
- Update a few phrases for mise install by [@erickguan](https://github.com/erickguan) in [#1712](https://github.com/jdx/mise/pull/1712)
- fix caching by [@jdx](https://github.com/jdx) in [62cb250](https://github.com/jdx/mise/commit/62cb250007c443dc25e72292b178c5f51cda413c)

### New Contributors

* @erickguan made their first contribution in [#1712](https://github.com/jdx/mise/pull/1712)
* @yggdr made their first contribution in [#1801](https://github.com/jdx/mise/pull/1801)

## [2024.3.6](https://github.com/jdx/mise/compare/v2024.3.2..v2024.3.6) - 2024-03-17

### 🚀 Features

- very basic dependency support by [@jdx](https://github.com/jdx) in [#1788](https://github.com/jdx/mise/pull/1788)

### 🐛 Bug Fixes

- update shorthand for rabbitmq by [@roele](https://github.com/roele) in [#1784](https://github.com/jdx/mise/pull/1784)
- display error message from calling usage by [@jdx](https://github.com/jdx) in [#1786](https://github.com/jdx/mise/pull/1786)
- automatically trust config files in CI by [@jdx](https://github.com/jdx) in [#1791](https://github.com/jdx/mise/pull/1791)

### 🚜 Refactor

- move lint tasks from just to mise by [@jdx](https://github.com/jdx) in [4f78a8c](https://github.com/jdx/mise/commit/4f78a8cb648246e3f204b426c57662076cc17d5d)

### 📚 Documentation

- **(changelog)** use github handles by [@jdx](https://github.com/jdx) in [b5ef2f7](https://github.com/jdx/mise/commit/b5ef2f7976e04bf11889062181fc32574eff834a)

### 🎨 Styling

- add mise tasks to editorconfig by [@jdx](https://github.com/jdx) in [dae8ece](https://github.com/jdx/mise/commit/dae8ece2d891100f86cecea5920bc423e0f4d053)
- run lint-fix which has changed slightly by [@jdx](https://github.com/jdx) in [6e8dd2f](https://github.com/jdx/mise/commit/6e8dd2fe24adf6d44a17a460c1054738e58f4306)
- apply editorconfig changes by [@jdx](https://github.com/jdx) in [962bed0](https://github.com/jdx/mise/commit/962bed061ab9218f679f20aa5c53e905981133e0)
- new git-cliff format by [@jdx](https://github.com/jdx) in [854a4fa](https://github.com/jdx/mise/commit/854a4fae9255968887dc0b0647c993f633666442)
- ignore CHANGELOG.md style by [@jdx](https://github.com/jdx) in [790cb91](https://github.com/jdx/mise/commit/790cb91a210f5d1d37f4c933798c1802583db753)

### 🧪 Testing

- **(mega-linter)** do not use js-standard linter by [@jdx](https://github.com/jdx) in [6b63346](https://github.com/jdx/mise/commit/6b63346bdd985964bc824eff03973d2d58d1ad28)
- **(mega-linter)** ignore CHANGELOG.md by [@jdx](https://github.com/jdx) in [b63b3ac](https://github.com/jdx/mise/commit/b63b3aca3c597ee95db80613b2ea8ca19f0e74c3)

### 🔍 Other Changes

- **(release-plz)** removed some debugging logic by [@jdx](https://github.com/jdx) in [f7d7bea](https://github.com/jdx/mise/commit/f7d7bea616c13b31318f2e7da287aa71face8e57)
- **(release-plz)** show actual version in PR body by [@jdx](https://github.com/jdx) in [e1ef708](https://github.com/jdx/mise/commit/e1ef708745e79bd019c77740820daefca5491b2e)
- **(release-plz)** tweaking logic to prevent extra PR by [@jdx](https://github.com/jdx) in [8673000](https://github.com/jdx/mise/commit/86730008cd2f60d2767296f97175805225c83951)
- **(release-plz)** make logic work for calver by [@jdx](https://github.com/jdx) in [890c919](https://github.com/jdx/mise/commit/890c919081f984f3d506c2b1d2712c8cff6f5e6b)
- **(release-plz)** make logic work for calver by [@jdx](https://github.com/jdx) in [bb5a178](https://github.com/jdx/mise/commit/bb5a178b0642416d0e3dac8a9162a9f0732cf146)
- **(release-plz)** fix git diffs by [@jdx](https://github.com/jdx) in [6c7e779](https://github.com/jdx/mise/commit/6c7e77944a24b289aaba887f64b7f3c63cb9e5ab)
- **(release-plz)** create gh release by [@jdx](https://github.com/jdx) in [f9ff369](https://github.com/jdx/mise/commit/f9ff369eb1176e31044fc463fdca08397def5a81)
- **(release-plz)** fixing gpg key by [@jdx](https://github.com/jdx) in [8286ded](https://github.com/jdx/mise/commit/8286ded8297b858e7136831e75e4c37fa49e6186)
- **(release-plz)** fixing gpg key by [@jdx](https://github.com/jdx) in [abb1dfe](https://github.com/jdx/mise/commit/abb1dfed78e49cf2bee4a137e92879ffd7f2fb03)
- **(release-plz)** do not publish a new release PR immediately by [@jdx](https://github.com/jdx) in [b3ae753](https://github.com/jdx/mise/commit/b3ae753fdde1fef17b4f13a1ecc8b23cb1da575c)
- **(release-plz)** prefix versions with "v" by [@jdx](https://github.com/jdx) in [3354b55](https://github.com/jdx/mise/commit/3354b551adab7082d5cc533e5d9d0bfe272958b4)
- **(test)** cache mise installed tools by [@jdx](https://github.com/jdx) in [0e433b9](https://github.com/jdx/mise/commit/0e433b975a5d8c28ae5c0cbd86d3b19e03146a83)
- Update .mega-linter.yml by [@jdx](https://github.com/jdx) in [831831c](https://github.com/jdx/mise/commit/831831c057d37826b9c34edec659e9836e616ad2)
- add --json flag by [@jdx](https://github.com/jdx) in [#1785](https://github.com/jdx/mise/pull/1785)
- cargo update by [@jdx](https://github.com/jdx) in [6391239](https://github.com/jdx/mise/commit/639123930eec8e057de7da790cb71d4a2b9e17a2)
- install tools before unit tests by [@jdx](https://github.com/jdx) in [f7456eb](https://github.com/jdx/mise/commit/f7456ebc539a4b27ec067bc480bc0aba1466e55b)
- added git-cliff by [@jdx](https://github.com/jdx) in [0ccdf36](https://github.com/jdx/mise/commit/0ccdf36df153ddc3ac1a2714ee9b4a2116dfc918)
- ensure `mise install` is run before lint-fix by [@jdx](https://github.com/jdx) in [e8a172f](https://github.com/jdx/mise/commit/e8a172f98ebc837619f3766777e489f3b99f36f4)
- added release-plz workflow by [@jdx](https://github.com/jdx) in [#1787](https://github.com/jdx/mise/pull/1787)
- set gpg key by [@jdx](https://github.com/jdx) in [467097f](https://github.com/jdx/mise/commit/467097f925053a27f0ede2a506e894562d191a09)
- temporarily disable self-update test by [@jdx](https://github.com/jdx) in [5cb39a4](https://github.com/jdx/mise/commit/5cb39a4259f332e5bccec082f1d7cd6127da5f55)

### 📦️ Dependency Updates

- update rust crate clap to 4.5.3 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1778](https://github.com/jdx/mise/pull/1778)
- update rust crate color-eyre to 0.6.3 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1779](https://github.com/jdx/mise/pull/1779)
- update rust crate thiserror to 1.0.58 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1781](https://github.com/jdx/mise/pull/1781)
- update rust crate strum to 0.26.2 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1780](https://github.com/jdx/mise/pull/1780)
- update rust crate toml_edit to 0.22.7 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1783](https://github.com/jdx/mise/pull/1783)
- update rust crate toml to 0.8.11 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1782](https://github.com/jdx/mise/pull/1782)
- update rust crate usage-lib to 0.1.10 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1790](https://github.com/jdx/mise/pull/1790)
- update rust crate usage-lib to 0.1.12 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1792](https://github.com/jdx/mise/pull/1792)

## [2024.3.2](https://github.com/jdx/mise/compare/v2024.3.1..v2024.3.2) - 2024-03-15

### 🚀 Features

- **(task)** add option to show hidden tasks in dependency tree by [@roele](https://github.com/roele) in [#1756](https://github.com/jdx/mise/pull/1756)

### 🐛 Bug Fixes

- **(go)** go backend supports versions prefixed with 'v' by [@roele](https://github.com/roele) in [#1753](https://github.com/jdx/mise/pull/1753)
- **(npm)** mise use -g npm:yarn@latest installs wrong version by [@roele](https://github.com/roele) in [#1752](https://github.com/jdx/mise/pull/1752)
- **(task)** document task.hide by [@roele](https://github.com/roele) in [#1754](https://github.com/jdx/mise/pull/1754)
- watch env._.source files by [@nicolas-geniteau](https://github.com/nicolas-geniteau) in [#1770](https://github.com/jdx/mise/pull/1770)
- prepend virtualenv path rather than append by [@kalvinnchau](https://github.com/kalvinnchau) in [#1751](https://github.com/jdx/mise/pull/1751)

### 🔍 Other Changes

- bump rust version by [@jdx](https://github.com/jdx) in [0cd890c](https://github.com/jdx/mise/commit/0cd890c04a511b8b82e1e605810ae1081e44fccc)

### 📦️ Dependency Updates

- update rust crate chrono to 0.4.35 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1757](https://github.com/jdx/mise/pull/1757)
- update rust crate clap to 4.5.2 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1758](https://github.com/jdx/mise/pull/1758)
- update softprops/action-gh-release action to v2 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1761](https://github.com/jdx/mise/pull/1761)
- update rust crate simplelog to 0.12.2 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1760](https://github.com/jdx/mise/pull/1760)
- update rust crate reqwest to 0.11.26 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1759](https://github.com/jdx/mise/pull/1759)

### New Contributors

* @kalvinnchau made their first contribution in [#1751](https://github.com/jdx/mise/pull/1751)
* @nicolas-geniteau made their first contribution in [#1770](https://github.com/jdx/mise/pull/1770)

## [2024.3.1](https://github.com/jdx/mise/compare/v2024.2.19..v2024.3.1) - 2024-03-04

### 🐛 Bug Fixes

- **(java)** sdkmanrc zulu JVMs are missing in mise by [@roele](https://github.com/roele) in [#1719](https://github.com/jdx/mise/pull/1719)

### 🔍 Other Changes

- added back "test:e2e" task by [@jdx](https://github.com/jdx) in [16e7da0](https://github.com/jdx/mise/commit/16e7da08fc135166e0f44e64d44fb3b3325943aa)
- Tiny grammar fix by [@MartyBeGood](https://github.com/MartyBeGood) in [#1744](https://github.com/jdx/mise/pull/1744)

### 📦️ Dependency Updates

- bump mio from 0.8.10 to 0.8.11 by [@dependabot[bot]](https://github.com/dependabot[bot]) in [#1747](https://github.com/jdx/mise/pull/1747)
- update rust crate insta to 1.36.1 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1745](https://github.com/jdx/mise/pull/1745)
- update rust crate walkdir to 2.5.0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1736](https://github.com/jdx/mise/pull/1736)
- update rust crate indexmap to 2.2.5 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1730](https://github.com/jdx/mise/pull/1730)
- update rust crate log to 0.4.21 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1732](https://github.com/jdx/mise/pull/1732)
- update rust crate tempfile to 3.10.1 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1733](https://github.com/jdx/mise/pull/1733)
- update rust crate rayon to 1.9.0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1735](https://github.com/jdx/mise/pull/1735)
- update rust crate base64 to 0.22.0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1734](https://github.com/jdx/mise/pull/1734)
- update rust crate ctor to 0.2.7 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1729](https://github.com/jdx/mise/pull/1729)

### New Contributors

* @MartyBeGood made their first contribution in [#1744](https://github.com/jdx/mise/pull/1744)

## [2024.2.19](https://github.com/jdx/mise/compare/v2024.2.18..v2024.2.19) - 2024-02-28

### 🔍 Other Changes

- simplify tasks in .mise.toml by [@jdx](https://github.com/jdx) in [5e371e1](https://github.com/jdx/mise/commit/5e371e1d911a08e12ead28dcb14f8436ee4b5ef3)
- Fix MUSL check by [@splinter98](https://github.com/splinter98) in [#1717](https://github.com/jdx/mise/pull/1717)
- use normal mise data dir in justfile by [@jdx](https://github.com/jdx) in [#1718](https://github.com/jdx/mise/pull/1718)

## [2024.2.18](https://github.com/jdx/mise/compare/v2024.2.17..v2024.2.18) - 2024-02-24

### 📚 Documentation

- make README logo link to site by [@booniepepper](https://github.com/booniepepper) in [#1695](https://github.com/jdx/mise/pull/1695)

### 🔍 Other Changes

- Update mise.json - fix missing_tools type by [@fxsalazar](https://github.com/fxsalazar) in [#1699](https://github.com/jdx/mise/pull/1699)
- added env._.python.venv directive by [@jdx](https://github.com/jdx) in [#1706](https://github.com/jdx/mise/pull/1706)
- auto-install plugins by [@jdx](https://github.com/jdx) in [3b665e2](https://github.com/jdx/mise/commit/3b665e238baad818aef8f66c74733d6c4e518312)

### 📦️ Dependency Updates

- update rust crate assert_cmd to 2.0.14 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1700](https://github.com/jdx/mise/pull/1700)
- update rust crate serde_json to 1.0.114 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1703](https://github.com/jdx/mise/pull/1703)
- update rust crate openssl to 0.10.64 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1702](https://github.com/jdx/mise/pull/1702)
- update rust crate demand to 1.1.0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1705](https://github.com/jdx/mise/pull/1705)
- update serde monorepo to 1.0.197 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1704](https://github.com/jdx/mise/pull/1704)
- update rust crate insta to 1.35.1 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1701](https://github.com/jdx/mise/pull/1701)

### New Contributors

* @fxsalazar made their first contribution in [#1699](https://github.com/jdx/mise/pull/1699)

## [2024.2.17](https://github.com/jdx/mise/compare/v2024.2.16..v2024.2.17) - 2024-02-22

### 🐛 Bug Fixes

- **(bun)** install bunx symlink by [@booniepepper](https://github.com/booniepepper) in [#1688](https://github.com/jdx/mise/pull/1688)
- **(go)** reflect on proper path for `GOROOT` by [@wheinze](https://github.com/wheinze) in [#1661](https://github.com/jdx/mise/pull/1661)
- allow go forge to install SHA versions when no tagged versions present by [@Ajpantuso](https://github.com/Ajpantuso) in [#1683](https://github.com/jdx/mise/pull/1683)

### 🚜 Refactor

- auto-try miseprintln macro by [@jdx](https://github.com/jdx) in [1d0fb78](https://github.com/jdx/mise/commit/1d0fb78377720fac356171ebd8d6cbf29a2f0ad6)

### 📚 Documentation

- add missing alt text by [@wheinze](https://github.com/wheinze) in [#1691](https://github.com/jdx/mise/pull/1691)
- improve formatting/colors by [@jdx](https://github.com/jdx) in [5c6e4dc](https://github.com/jdx/mise/commit/5c6e4dc79828b96e5cfb35865a9176670c8f6737)
- revamped output by [@jdx](https://github.com/jdx) in [#1694](https://github.com/jdx/mise/pull/1694)

### 🧪 Testing

- **(integration)** introduce rust based integration suite by [@Ajpantuso](https://github.com/Ajpantuso) in [#1612](https://github.com/jdx/mise/pull/1612)

### 🔍 Other Changes

- Update README.md by [@jdx](https://github.com/jdx) in [05869d9](https://github.com/jdx/mise/commit/05869d986f9b8543aec760f14a8539ce9ba288b3)
- cargo up by [@jdx](https://github.com/jdx) in [0d716d8](https://github.com/jdx/mise/commit/0d716d862600e0c59b8d4269e48385bf911164b1)
- downgrade openssl due to build failures by [@jdx](https://github.com/jdx) in [8c282b8](https://github.com/jdx/mise/commit/8c282b8a8786c726ed93a733aaf605529e19b172)
- Revert "cargo up" by [@jdx](https://github.com/jdx) in [6fb1fa7](https://github.com/jdx/mise/commit/6fb1fa75cdf8abf6e344e30308685238e9dd5570)
- cargo up (minus cc) by [@jdx](https://github.com/jdx) in [6142403](https://github.com/jdx/mise/commit/6142403894db91b39279e3544bef595bd17c631a)
- Retry with https if request fails by [@grant0417](https://github.com/grant0417) in [#1690](https://github.com/jdx/mise/pull/1690)

### 📦️ Dependency Updates

- update rust crate usage-lib to 0.1.9 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1679](https://github.com/jdx/mise/pull/1679)
- update rust crate indexmap to 2.2.3 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1677](https://github.com/jdx/mise/pull/1677)
- update rust crate toml_edit to 0.22.6 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1678](https://github.com/jdx/mise/pull/1678)
- update rust crate demand to 1.0.2 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1676](https://github.com/jdx/mise/pull/1676)
- update rust crate clap to 4.5.1 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1675](https://github.com/jdx/mise/pull/1675)

### New Contributors

* @grant0417 made their first contribution in [#1690](https://github.com/jdx/mise/pull/1690)
* @wheinze made their first contribution in [#1691](https://github.com/jdx/mise/pull/1691)

## [2024.2.16](https://github.com/jdx/mise/compare/v2024.2.15..v2024.2.16) - 2024-02-15

### 🔍 Other Changes

- use dash compatible syntax by [@jdx](https://github.com/jdx) in [10dbf54](https://github.com/jdx/mise/commit/10dbf54650b9ed90eb4a9ba86fe5499db23357d8)
- cargo up by [@jdx](https://github.com/jdx) in [7a02ac3](https://github.com/jdx/mise/commit/7a02ac3cfe4de715f807a0c1f27ac63cf840cf55)

## [2024.2.15](https://github.com/jdx/mise/compare/v2024.2.14..v2024.2.15) - 2024-02-13

### 🔍 Other Changes

- fish command_not_found handler fix by [@jdx](https://github.com/jdx) in [#1665](https://github.com/jdx/mise/pull/1665)
- cargo up by [@jdx](https://github.com/jdx) in [122a9b2](https://github.com/jdx/mise/commit/122a9b25994adf081e25c15df7b22c80c5517126)
- run commit hook on main branch by [@jdx](https://github.com/jdx) in [7ced699](https://github.com/jdx/mise/commit/7ced699f638716387a3a35935c946d3df26eac49)
- Revert "run commit hook on main branch" by [@jdx](https://github.com/jdx) in [5ec8a5e](https://github.com/jdx/mise/commit/5ec8a5e343b7a6c181f92cb2d5650fe1b0bc5d50)

### 📦️ Dependency Updates

- update rust crate thiserror to 1.0.57 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1659](https://github.com/jdx/mise/pull/1659)

## [2024.2.14](https://github.com/jdx/mise/compare/v2024.2.13..v2024.2.14) - 2024-02-11

### 🐛 Bug Fixes

- fix completions in linux by [@jdx](https://github.com/jdx) in [2822554](https://github.com/jdx/mise/commit/2822554d1d876a80df02abdb7e4ad353416f80af)

### 📦️ Dependency Updates

- update rust crate chrono to 0.4.34 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1654](https://github.com/jdx/mise/pull/1654)

## [2024.2.13](https://github.com/jdx/mise/compare/v2024.2.12..v2024.2.13) - 2024-02-11

### 🐛 Bug Fixes

- fix completion generators if usage is not installed by [@jdx](https://github.com/jdx) in [e46fe04](https://github.com/jdx/mise/commit/e46fe04d1c50f893b6c2aa55222792faf16be64c)

## [2024.2.12](https://github.com/jdx/mise/compare/v2024.2.11..v2024.2.12) - 2024-02-11

### 🔍 Other Changes

- install usage via cargo-binstall by [@jdx](https://github.com/jdx) in [f3a0117](https://github.com/jdx/mise/commit/f3a0117fea9307d11f2df1540efe6761eec13b66)

### 📦️ Dependency Updates

- update rust crate usage-lib to 0.1.8 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1653](https://github.com/jdx/mise/pull/1653)

## [2024.2.11](https://github.com/jdx/mise/compare/v2024.2.10..v2024.2.11) - 2024-02-10

### 🔍 Other Changes

- default fish+bash to use usage for completions by [@jdx](https://github.com/jdx) in [8399b1f](https://github.com/jdx/mise/commit/8399b1fdbc7e7b507f6e2137d77c685f70b4345d)
- add usage to CI by [@jdx](https://github.com/jdx) in [0bc48ed](https://github.com/jdx/mise/commit/0bc48eddb7ca38f1e13bcbf2286d4e01041a9fc8)
- add usage to CI by [@jdx](https://github.com/jdx) in [4eba7c0](https://github.com/jdx/mise/commit/4eba7c026baa52055d5b5925bb9e5acf37f209af)

### 📦️ Dependency Updates

- update rust crate indicatif to 0.17.8 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1651](https://github.com/jdx/mise/pull/1651)

## [2024.2.10](https://github.com/jdx/mise/compare/v2024.2.9..v2024.2.10) - 2024-02-10

### 🔍 Other Changes

- usage by [@jdx](https://github.com/jdx) in [#1652](https://github.com/jdx/mise/pull/1652)
- cargo up by [@jdx](https://github.com/jdx) in [4292537](https://github.com/jdx/mise/commit/42925377ba5b06d9d9f5402fca3b197b60acda82)

### 📦️ Dependency Updates

- update rust crate clap to 4.5.0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1644](https://github.com/jdx/mise/pull/1644)
- update rust crate clap_complete to 4.5.0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1645](https://github.com/jdx/mise/pull/1645)
- update rust crate clap_mangen to 0.2.20 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1646](https://github.com/jdx/mise/pull/1646)
- update rust crate tempfile to 3.10.0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1647](https://github.com/jdx/mise/pull/1647)
- update rust crate either to 1.10.0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1649](https://github.com/jdx/mise/pull/1649)
- update rust crate toml to 0.8.10 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1648](https://github.com/jdx/mise/pull/1648)
- update rust crate toml_edit to 0.22.4 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1650](https://github.com/jdx/mise/pull/1650)

## [2024.2.9](https://github.com/jdx/mise/compare/v2024.2.8..v2024.2.9) - 2024-02-09

### 🔍 Other Changes

- bump msrv for clap compatibility by [@jdx](https://github.com/jdx) in [8a9a284](https://github.com/jdx/mise/commit/8a9a284f1520500a361c1bc2f4db09648a49acd2)

## [2024.2.8](https://github.com/jdx/mise/compare/v2024.2.7..v2024.2.8) - 2024-02-09

### 🐛 Bug Fixes

- fix support for tera templates in tool version strings by [@jdx](https://github.com/jdx) in [#1643](https://github.com/jdx/mise/pull/1643)

### 📚 Documentation

- docs by [@jdx](https://github.com/jdx) in [e291cc3](https://github.com/jdx/mise/commit/e291cc3802d027d9738b5060d9c68be8b20269e3)

### 🔍 Other Changes

- ignore non-executable tasks by [@jdx](https://github.com/jdx) in [#1642](https://github.com/jdx/mise/pull/1642)
- save space by [@jdx](https://github.com/jdx) in [638a426](https://github.com/jdx/mise/commit/638a426e636d65f83f7cd1e415c8aba2a71fe562)
- GOROOT/GOBIN/GOPATH changes by [@jdx](https://github.com/jdx) in [#1641](https://github.com/jdx/mise/pull/1641)
- save space by [@jdx](https://github.com/jdx) in [0c59c59](https://github.com/jdx/mise/commit/0c59c5980987f300ef6f3468c9a4d7cead2e1995)

## [2024.2.7](https://github.com/jdx/mise/compare/v2024.2.6..v2024.2.7) - 2024-02-08

### 🐛 Bug Fixes

- fix task loading by [@jdx](https://github.com/jdx) in [#1625](https://github.com/jdx/mise/pull/1625)

### 🔍 Other Changes

- support global file tasks by [@jdx](https://github.com/jdx) in [#1627](https://github.com/jdx/mise/pull/1627)
- add installed/active flags by [@jdx](https://github.com/jdx) in [#1629](https://github.com/jdx/mise/pull/1629)
- fix command not found handler by [@jdx](https://github.com/jdx) in [a30842b](https://github.com/jdx/mise/commit/a30842b5062caca6d07b68307d66ebf376ff01c8)

## [2024.2.6](https://github.com/jdx/mise/compare/v2024.2.5..v2024.2.6) - 2024-02-07

### 🔍 Other Changes

- calm io by [@jdx](https://github.com/jdx) in [#1621](https://github.com/jdx/mise/pull/1621)
- use OnceLock where possible by [@jdx](https://github.com/jdx) in [92a3e87](https://github.com/jdx/mise/commit/92a3e87b578cc2e7af0b23b5244246a38be3584b)
- automatically try https if http fails by [@jdx](https://github.com/jdx) in [#1622](https://github.com/jdx/mise/pull/1622)
- added optional pre-commit hook by [@jdx](https://github.com/jdx) in [ec03744](https://github.com/jdx/mise/commit/ec0374480d2b94e49fa8e06edbe929e6f6981951)
- reuse existing command_not_found handler by [@jdx](https://github.com/jdx) in [#1624](https://github.com/jdx/mise/pull/1624)

## [2024.2.5](https://github.com/jdx/mise/compare/v2024.2.4..v2024.2.5) - 2024-02-06

### 🐛 Bug Fixes

- fix lint issues in rust 1.77.0-beta.1 by [@jdx](https://github.com/jdx) in [cb9ab2d](https://github.com/jdx/mise/commit/cb9ab2de6c6d99cb747a3ef1b90dc2e4e84d0a0a)

### 📚 Documentation

- add some info by [@jdx](https://github.com/jdx) in [#1614](https://github.com/jdx/mise/pull/1614)
- cli help by [@jdx](https://github.com/jdx) in [6a004a7](https://github.com/jdx/mise/commit/6a004a723d93cc3a253321ab9b83058dea6c6c89)

### 🔍 Other Changes

- use serde to parse tools by [@jdx](https://github.com/jdx) in [#1599](https://github.com/jdx/mise/pull/1599)
- support "false" env vars by [@jdx](https://github.com/jdx) in [#1603](https://github.com/jdx/mise/pull/1603)
- add dotenv paths to watch files by [@jdx](https://github.com/jdx) in [#1615](https://github.com/jdx/mise/pull/1615)

### 📦️ Dependency Updates

- update rust crate itertools to 0.12.1 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1604](https://github.com/jdx/mise/pull/1604)
- update nick-fields/retry action to v3 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1610](https://github.com/jdx/mise/pull/1610)
- update rust crate toml_edit to 0.21.1 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1609](https://github.com/jdx/mise/pull/1609)
- update rust crate toml to 0.8.9 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1608](https://github.com/jdx/mise/pull/1608)
- update peter-evans/create-pull-request action to v6 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1611](https://github.com/jdx/mise/pull/1611)
- update rust crate serde_json to 1.0.113 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1607](https://github.com/jdx/mise/pull/1607)
- update rust crate reqwest to 0.11.24 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1605](https://github.com/jdx/mise/pull/1605)

## [2024.2.4](https://github.com/jdx/mise/compare/v2024.2.3..v2024.2.4) - 2024-02-03

### 🐛 Bug Fixes

- **(tasks)** fix parsing of alias attribute by [@Ajpantuso](https://github.com/Ajpantuso) in [#1596](https://github.com/jdx/mise/pull/1596)

### 📦️ Dependency Updates

- update rust crate clap_mangen to 0.2.19 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1598](https://github.com/jdx/mise/pull/1598)
- update rust crate clap_complete to 4.4.10 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1597](https://github.com/jdx/mise/pull/1597)
- update rust crate eyre to 0.6.12 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1601](https://github.com/jdx/mise/pull/1601)
- update rust crate indexmap to 2.2.2 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1602](https://github.com/jdx/mise/pull/1602)

## [2024.2.3](https://github.com/jdx/mise/compare/v2024.2.2..v2024.2.3) - 2024-02-02

### 🔍 Other Changes

- show curl progress during install.sh by [@jdx](https://github.com/jdx) in [9e786e8](https://github.com/jdx/mise/commit/9e786e842ff8a1ed25b5d65e76f68ec3ce07850c)
- install tools in order listed in config file when --jobs=1 by [@jdx](https://github.com/jdx) in [#1587](https://github.com/jdx/mise/pull/1587)
- actionlint by [@jdx](https://github.com/jdx) in [8f067cb](https://github.com/jdx/mise/commit/8f067cb79fa0f11c1f81ffe3496be9a91c28403e)
- remove unused property by [@jdx](https://github.com/jdx) in [13d7d29](https://github.com/jdx/mise/commit/13d7d29302004c770a51ff5298a261635460962f)
- property should not be public by [@jdx](https://github.com/jdx) in [fee5b72](https://github.com/jdx/mise/commit/fee5b72fd552e9f00e13fe60f04959eb629147cd)
- remove more unused config file props by [@jdx](https://github.com/jdx) in [13daec0](https://github.com/jdx/mise/commit/13daec0b4b92b51bd3ed4137cd51f665afd3f007)
- allow _.path to have : delimiters by [@jdx](https://github.com/jdx) in [be34b76](https://github.com/jdx/mise/commit/be34b768d9c09feda3c59d9a949a40609c294dcf)
- use serde to parse tasks by [@jdx](https://github.com/jdx) in [#1592](https://github.com/jdx/mise/pull/1592)
- skip running glob if no patterns by [@jdx](https://github.com/jdx) in [0eae892](https://github.com/jdx/mise/commit/0eae892c67598c788b7ca6311aaaac075279717b)
- lazy-load toml_edit by [@jdx](https://github.com/jdx) in [#1594](https://github.com/jdx/mise/pull/1594)

## [2024.2.2](https://github.com/jdx/mise/compare/v2024.2.1..v2024.2.2) - 2024-02-02

### 🔍 Other Changes

- minor UI tweak by [@jdx](https://github.com/jdx) in [fbe2578](https://github.com/jdx/mise/commit/fbe2578e8770c8913e6bb029ea08ce7b18e6db4a)
- ui tweak by [@jdx](https://github.com/jdx) in [d3748ef](https://github.com/jdx/mise/commit/d3748efb24bb7b7894c5a877e4d49aff1738c0b8)
- clear cache on mise.run by [@jdx](https://github.com/jdx) in [1d00fbd](https://github.com/jdx/mise/commit/1d00fbdb904ce83737898e4dc2f8ba5edbf2a568)
- download progress bars by [@jdx](https://github.com/jdx) in [#1586](https://github.com/jdx/mise/pull/1586)
- improve output of shorthand update script by [@jdx](https://github.com/jdx) in [0633c07](https://github.com/jdx/mise/commit/0633c0790e0858919f0ac2b2c27a3d2d7b836c8a)

## [2024.2.1](https://github.com/jdx/mise/compare/v2024.2.0..v2024.2.1) - 2024-02-01

### 🐛 Bug Fixes

- fixed ctrlc handler by [@jdx](https://github.com/jdx) in [#1584](https://github.com/jdx/mise/pull/1584)

### 📚 Documentation

- add "dr" alias by [@jdx](https://github.com/jdx) in [67e9e30](https://github.com/jdx/mise/commit/67e9e302c979ca16e8e1160e3a7123f08dd1ab82)

### 🔍 Other Changes

- use m1 macs by [@jdx](https://github.com/jdx) in [98a6d1f](https://github.com/jdx/mise/commit/98a6d1f2441a8fb839f65a5a66d7053bdffef36b)
- add env.mise.source to schame by [@sirenkovladd](https://github.com/sirenkovladd) in [#1578](https://github.com/jdx/mise/pull/1578)
- improve set/ls commands by [@jdx](https://github.com/jdx) in [#1579](https://github.com/jdx/mise/pull/1579)
- Update README.md by [@jdx](https://github.com/jdx) in [3412fa1](https://github.com/jdx/mise/commit/3412fa19e40ca66c4a1811a226b29804ed1f4d3b)
- added mise.run by [@jdx](https://github.com/jdx) in [9ab7159](https://github.com/jdx/mise/commit/9ab71597c6c3cda0ce500fe9174263ed0c940d44)
- use a bodged loop to handle go forge submodules by [@endigma](https://github.com/endigma) in [#1583](https://github.com/jdx/mise/pull/1583)
- Additional arch install by [@tlockney](https://github.com/tlockney) in [#1562](https://github.com/jdx/mise/pull/1562)

### New Contributors

* @tlockney made their first contribution in [#1562](https://github.com/jdx/mise/pull/1562)
* @sirenkovladd made their first contribution in [#1578](https://github.com/jdx/mise/pull/1578)

## [2024.2.0](https://github.com/jdx/mise/compare/v2024.1.35..v2024.2.0) - 2024-02-01

### 🚀 Features

- **(tasks)** make script task dirs configurable by [@Ajpantuso](https://github.com/Ajpantuso) in [#1571](https://github.com/jdx/mise/pull/1571)

### 🐛 Bug Fixes

- **(tasks)** prevent dependency cycles by [@Ajpantuso](https://github.com/Ajpantuso) in [#1575](https://github.com/jdx/mise/pull/1575)

### 🚜 Refactor

- refactor task_config by [@jdx](https://github.com/jdx) in [7568969](https://github.com/jdx/mise/commit/7568969f281a428c07144d79643b31699b068c54)

### 📚 Documentation

- docker by [@jdx](https://github.com/jdx) in [#1570](https://github.com/jdx/mise/pull/1570)
- fix github action by [@jdx](https://github.com/jdx) in [9adc718](https://github.com/jdx/mise/commit/9adc7186b86a539e6f3e6a358d5822834e8be8fa)
- fix github action by [@jdx](https://github.com/jdx) in [3849cdb](https://github.com/jdx/mise/commit/3849cdb8d0d4396e32fa9f555d03662efb2c41ab)
- skip cargo-msrv by [@jdx](https://github.com/jdx) in [ff3a555](https://github.com/jdx/mise/commit/ff3a5559dde35bd47ed072704bf2bc67478ce307)
- fix test runner by [@jdx](https://github.com/jdx) in [779c484](https://github.com/jdx/mise/commit/779c48491dfc223c2a7c8c80b8396ba9050ec54d)
- fix dev test by [@jdx](https://github.com/jdx) in [b92566f](https://github.com/jdx/mise/commit/b92566ffc2ccf2336fafddff3bb5dd62536b1f5f)

### 🔍 Other Changes

- tag version in docker by [@jdx](https://github.com/jdx) in [fda1be6](https://github.com/jdx/mise/commit/fda1be6c61a23361606ce9e87c10d92b5f619344)
- refactor to use BTreeMap instead of sorting by [@jdx](https://github.com/jdx) in [438e6a4](https://github.com/jdx/mise/commit/438e6a4dec10e17b0cffca1d921acedf7d6db324)
- skip checkout for homebrew bump by [@jdx](https://github.com/jdx) in [de5e5b6](https://github.com/jdx/mise/commit/de5e5b6b33063e577f53ceb8f8de14b5035c1c4d)
- make missing tool warning more granular by [@jdx](https://github.com/jdx) in [#1577](https://github.com/jdx/mise/pull/1577)
- default --quiet to error level by [@jdx](https://github.com/jdx) in [50c1468](https://github.com/jdx/mise/commit/50c146802aaf4f5f0046ccac620712a5338b1860)

## [2024.1.35](https://github.com/jdx/mise/compare/v2024.1.34..v2024.1.35) - 2024-01-31

### 🔍 Other Changes

- use activate_agressive setting by [@jdx](https://github.com/jdx) in [c8837fe](https://github.com/jdx/mise/commit/c8837fea7605167c9be2e964acbb29a6ba4e48aa)

## [2024.1.34](https://github.com/jdx/mise/compare/v2024.1.33..v2024.1.34) - 2024-01-31

### 🐛 Bug Fixes

- fix bash command not found override by [@jdx](https://github.com/jdx) in [#1564](https://github.com/jdx/mise/pull/1564)

### 🔍 Other Changes

- build on macos-latest by [@jdx](https://github.com/jdx) in [3ca3f7e](https://github.com/jdx/mise/commit/3ca3f7eb5fa72b08938262b9665fabc2db650f28)
- removed outdated conditional by [@jdx](https://github.com/jdx) in [7f900c4](https://github.com/jdx/mise/commit/7f900c4326ac50ca2773d320e7bd9b2790063b63)
- update CONTRIBUTING.md by [@jdx](https://github.com/jdx) in [56be60f](https://github.com/jdx/mise/commit/56be60f2dee9398b181f83965d3a1caa8efe7b16)
- label experimental error by [@jdx](https://github.com/jdx) in [0e38477](https://github.com/jdx/mise/commit/0e3847791d59df8eb36249ff8faf2eb13c287aa3)
- convert more things to mise tasks from just by [@jdx](https://github.com/jdx) in [#1566](https://github.com/jdx/mise/pull/1566)
- use Cargo.* as source by [@jdx](https://github.com/jdx) in [ee10dba](https://github.com/jdx/mise/commit/ee10dba7712acb7420ab807331dc5b37216db080)

## [2024.1.33](https://github.com/jdx/mise/compare/v2024.1.32..v2024.1.33) - 2024-01-30

### 🔍 Other Changes

- treat anything not rtx/mise as a shim by [@jdx](https://github.com/jdx) in [fae51a7](https://github.com/jdx/mise/commit/fae51a7ef38890fbf3f864957e0c0c6f1be0cf65)

## [2024.1.32](https://github.com/jdx/mise/compare/v2024.1.31..v2024.1.32) - 2024-01-30

### 🔍 Other Changes

- added "plugins up" alias" by [@jdx](https://github.com/jdx) in [f68bf52](https://github.com/jdx/mise/commit/f68bf520fd726544bfbc09ce8fd1035ffc0d7e20)
- fix settings env vars by [@jdx](https://github.com/jdx) in [b122c19](https://github.com/jdx/mise/commit/b122c19935297a3220c438607798fc7fe52df1c1)
- use compiled python by [@jdx](https://github.com/jdx) in [d3020cc](https://github.com/jdx/mise/commit/d3020cc26575864a38dbffd530ad1f7ebff64f64)

## [2024.1.31](https://github.com/jdx/mise/compare/v2024.1.30..v2024.1.31) - 2024-01-30

### 🚀 Features

- **(tasks)** add task timing to run command by [@Ajpantuso](https://github.com/Ajpantuso) in [#1536](https://github.com/jdx/mise/pull/1536)

### 🐛 Bug Fixes

- properly handle executable shims when getting diffs by [@Ajpantuso](https://github.com/Ajpantuso) in [#1545](https://github.com/jdx/mise/pull/1545)
- fix bash not_found handler by [@jdx](https://github.com/jdx) in [#1558](https://github.com/jdx/mise/pull/1558)

### 🔍 Other Changes

- updated indexmap by [@jdx](https://github.com/jdx) in [d7cb481](https://github.com/jdx/mise/commit/d7cb4816e9165cde5ac715126a004f924898af0f)
- hide system versions from env/bin_paths by [@jdx](https://github.com/jdx) in [#1553](https://github.com/jdx/mise/pull/1553)
- codacy badge by [@jdx](https://github.com/jdx) in [711d6d7](https://github.com/jdx/mise/commit/711d6d7ced808abd4e24b7dc5952085b9132047d)
- codacy badge by [@jdx](https://github.com/jdx) in [dc76ec4](https://github.com/jdx/mise/commit/dc76ec4288d2b25c37eb2745028f6593c56facf7)
- codacy badge by [@jdx](https://github.com/jdx) in [2e97b24](https://github.com/jdx/mise/commit/2e97b24540c3f020dbb2a650512dc97f78b3f6f1)
- codacy badge by [@jdx](https://github.com/jdx) in [711110c](https://github.com/jdx/mise/commit/711110ca510228df421a584b11e7b62e8590be08)
- only show precompiled warning if going to use precompiled by [@jdx](https://github.com/jdx) in [74fd185](https://github.com/jdx/mise/commit/74fd1852bef8244f2cb4c51b58f11116d10d0c11)
- fix linux precompiled by [@jdx](https://github.com/jdx) in [#1559](https://github.com/jdx/mise/pull/1559)
- clean up e2e tests by [@jdx](https://github.com/jdx) in [2660406](https://github.com/jdx/mise/commit/2660406a4744e789ab39a58e1732f880dcd26b4d)

### 📦️ Dependency Updates

- update rust crate serde_json to 1.0.112 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1542](https://github.com/jdx/mise/pull/1542)
- update serde monorepo to 1.0.196 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1543](https://github.com/jdx/mise/pull/1543)
- update rust crate strum to 0.26.0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1546](https://github.com/jdx/mise/pull/1546)
- update rust crate strum to 0.26.1 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1547](https://github.com/jdx/mise/pull/1547)
- update rust crate indexmap to 2.2.0 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1548](https://github.com/jdx/mise/pull/1548)

## [2024.1.30](https://github.com/jdx/mise/compare/v2024.1.29..v2024.1.30) - 2024-01-27

### 🐛 Bug Fixes

- fix mangen by [@jdx](https://github.com/jdx) in [da2b1c9](https://github.com/jdx/mise/commit/da2b1c9d0bbbeba3e566020c0d48e16f579d8eeb)

### 🔍 Other Changes

- default to precompiled python by [@jdx](https://github.com/jdx) in [0fac002](https://github.com/jdx/mise/commit/0fac002dbeba699ae8949c3d94e89d08128dae57)

## [2024.1.29](https://github.com/jdx/mise/compare/v2024.1.28..v2024.1.29) - 2024-01-27

### 🔍 Other Changes

- use nodejs/golang for writing to .tool-versions by [@jdx](https://github.com/jdx) in [14fb790](https://github.com/jdx/mise/commit/14fb790ac9953430794719b38b83c8c2242f1759)
- read system and local config settings by [@jdx](https://github.com/jdx) in [#1541](https://github.com/jdx/mise/pull/1541)

## [2024.1.28](https://github.com/jdx/mise/compare/v2024.1.27..v2024.1.28) - 2024-01-27

### 🔍 Other Changes

- added `env._.source` feature by [@jdx](https://github.com/jdx) in [#1538](https://github.com/jdx/mise/pull/1538)
- force update alpine by [@jdx](https://github.com/jdx) in [633c3ff](https://github.com/jdx/mise/commit/633c3ffe139c1201f20ce0e7145cb361d547a39a)

### 📦️ Dependency Updates

- update rust crate chrono to 0.4.33 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1539](https://github.com/jdx/mise/pull/1539)
- update rust crate clap_complete to 4.4.9 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1540](https://github.com/jdx/mise/pull/1540)

## [2024.1.27](https://github.com/jdx/mise/compare/v2024.1.26..v2024.1.27) - 2024-01-26

### 🚀 Features

- **(run)** match tasks to run with glob patterns by [@Ajpantuso](https://github.com/Ajpantuso) in [#1528](https://github.com/jdx/mise/pull/1528)
- **(tasks)** unify glob strategy for tasks and dependencies by [@Ajpantuso](https://github.com/Ajpantuso) in [#1533](https://github.com/jdx/mise/pull/1533)

### 🐛 Bug Fixes

- fix global config with asdf_compat by [@jdx](https://github.com/jdx) in [#1534](https://github.com/jdx/mise/pull/1534)

### 📚 Documentation

- display missing/extra shims by [@jdx](https://github.com/jdx) in [#1529](https://github.com/jdx/mise/pull/1529)

### 🔍 Other Changes

- pass signals to tasks by [@jdx](https://github.com/jdx) in [#1527](https://github.com/jdx/mise/pull/1527)
- added settings_message settings by [@jdx](https://github.com/jdx) in [#1535](https://github.com/jdx/mise/pull/1535)
- resolve env vars in order by [@jdx](https://github.com/jdx) in [#1519](https://github.com/jdx/mise/pull/1519)
- parse alias + plugins with serde by [@jdx](https://github.com/jdx) in [#1537](https://github.com/jdx/mise/pull/1537)

## [2024.1.26](https://github.com/jdx/mise/compare/v2024.1.25..v2024.1.26) - 2024-01-25

### 🚀 Features

- **(doctor)** identify missing/extra shims by [@Ajpantuso](https://github.com/Ajpantuso) in [#1524](https://github.com/jdx/mise/pull/1524)
- **(tasks)** infer bash task topics from folder structure by [@Ajpantuso](https://github.com/Ajpantuso) in [#1520](https://github.com/jdx/mise/pull/1520)

### 🚜 Refactor

- env parsing by [@jdx](https://github.com/jdx) in [#1515](https://github.com/jdx/mise/pull/1515)

### 🔍 Other Changes

- use target_feature to use correct precompiled runtimes by [@jdx](https://github.com/jdx) in [#1512](https://github.com/jdx/mise/pull/1512)
- do not follow symbolic links for trusted paths by [@jdx](https://github.com/jdx) in [#1513](https://github.com/jdx/mise/pull/1513)
- refactor min_version logic by [@jdx](https://github.com/jdx) in [#1516](https://github.com/jdx/mise/pull/1516)
- sort env vars coming back from exec-env by [@jdx](https://github.com/jdx) in [#1518](https://github.com/jdx/mise/pull/1518)
- order flags in docs by [@jdx](https://github.com/jdx) in [1018b56](https://github.com/jdx/mise/commit/1018b5622c3bda4d0d9fa36b4fa9c1143aabd676)
- demand 1.0.0 by [@jdx](https://github.com/jdx) in [c97bb79](https://github.com/jdx/mise/commit/c97bb7993aa9432ad38879cdc0ab17f251715feb)

## [2024.1.25](https://github.com/jdx/mise/compare/v2024.1.24..v2024.1.25) - 2024-01-24

### 🚀 Features

- **(config)** support arrays of env tables by [@Ajpantuso](https://github.com/Ajpantuso) in [#1503](https://github.com/jdx/mise/pull/1503)
- **(template)** add join_path filter by [@Ajpantuso](https://github.com/Ajpantuso) in [#1508](https://github.com/jdx/mise/pull/1508)
- add other arm targets for cargo-binstall by [@yossydev](https://github.com/yossydev) in [#1510](https://github.com/jdx/mise/pull/1510)

### 🐛 Bug Fixes

- **(tasks)** prevent implicit globbing of sources/outputs by [@Ajpantuso](https://github.com/Ajpantuso) in [#1509](https://github.com/jdx/mise/pull/1509)
- fix release script by [@jdx](https://github.com/jdx) in [59498ea](https://github.com/jdx/mise/commit/59498ea5a312507535d139957bac90fad2d96ebf)

### 🔍 Other Changes

- updated clap_complete by [@jdx](https://github.com/jdx) in [4034674](https://github.com/jdx/mise/commit/4034674436f786691e767c6ac09921b06e968a86)
- allow cargo-binstall from mise itself by [@jdx](https://github.com/jdx) in [#1507](https://github.com/jdx/mise/pull/1507)
- Delete lefthook.yml by [@jdx](https://github.com/jdx) in [a756db4](https://github.com/jdx/mise/commit/a756db4a34afee4d6ce0fcfea4bc016025d1d188)
- turn back on `cargo update` on release by [@jdx](https://github.com/jdx) in [51f269a](https://github.com/jdx/mise/commit/51f269a8d07cf1f34f0d237b17b493986aaa864d)

### 📦️ Dependency Updates

- update rust crate regex to 1.10.3 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1496](https://github.com/jdx/mise/pull/1496)

### New Contributors

* @yossydev made their first contribution in [#1510](https://github.com/jdx/mise/pull/1510)

## [2024.1.24](https://github.com/jdx/mise/compare/v2024.1.23..v2024.1.24) - 2024-01-20

### 🐛 Bug Fixes

- fix cwd error by [@jdx](https://github.com/jdx) in [1c0bc12](https://github.com/jdx/mise/commit/1c0bc1236fce943ed9b012e95e3cc047cdc38ab0)

### 🔍 Other Changes

- bump demand by [@jdx](https://github.com/jdx) in [5231179](https://github.com/jdx/mise/commit/523117975bbb9c3211f0f438f55d1d7dc392f8b2)
- do not fail if version parsing fails by [@jdx](https://github.com/jdx) in [8d39995](https://github.com/jdx/mise/commit/8d39995e615527ba7187b3d25369a506bcb21e0c)
- added --shims by [@jdx](https://github.com/jdx) in [#1483](https://github.com/jdx/mise/pull/1483)
- use `sort -r` instead of `tac` by [@jdx](https://github.com/jdx) in [#1486](https://github.com/jdx/mise/pull/1486)
- Update README.md by [@jdx](https://github.com/jdx) in [f3291d1](https://github.com/jdx/mise/commit/f3291d15f94c0a0cc602c01d5b7b6ef7c3cb60bf)
- fix conflicts by [@jdx](https://github.com/jdx) in [729de0c](https://github.com/jdx/mise/commit/729de0cb6c27646e30ee7be99d2f478f3431258c)

### 📦️ Dependency Updates

- update actions/cache action to v4 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1488](https://github.com/jdx/mise/pull/1488)
- update rust crate which to v6 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1489](https://github.com/jdx/mise/pull/1489)
- update rust crate which to v6 by [@jdx](https://github.com/jdx) in [#1490](https://github.com/jdx/mise/pull/1490)

## [2024.1.23](https://github.com/jdx/mise/compare/v2024.1.22..v2024.1.23) - 2024-01-18

### 🐛 Bug Fixes

- fix config_root path by [@jdx](https://github.com/jdx) in [#1477](https://github.com/jdx/mise/pull/1477)

### 🔍 Other Changes

- use mise to get development dependencies by [@jdx](https://github.com/jdx) in [#1478](https://github.com/jdx/mise/pull/1478)
- improve post-plugin-update script by [@jdx](https://github.com/jdx) in [#1479](https://github.com/jdx/mise/pull/1479)
- only show select if no task specified by [@jdx](https://github.com/jdx) in [#1481](https://github.com/jdx/mise/pull/1481)
- show cursor on ctrl-c by [@jdx](https://github.com/jdx) in [ebc5fe7](https://github.com/jdx/mise/commit/ebc5fe78bc97ecf99251438e6f305908bb134833)
- fix project_root when using .config/mise.toml or .mise/config.toml by [@jdx](https://github.com/jdx) in [#1482](https://github.com/jdx/mise/pull/1482)

## [2024.1.22](https://github.com/jdx/mise/compare/v2024.1.21..v2024.1.22) - 2024-01-17

### 🐛 Bug Fixes

- no panic on missing current dir by [@tamasfe](https://github.com/tamasfe) in [#1462](https://github.com/jdx/mise/pull/1462)
- fix not_found handler when command start with "--" by [@jdx](https://github.com/jdx) in [#1464](https://github.com/jdx/mise/pull/1464)
- always load global configs by [@tamasfe](https://github.com/tamasfe) in [#1466](https://github.com/jdx/mise/pull/1466)

### 🔍 Other Changes

- remove dirs_next in favor of simpler home crate by [@jdx](https://github.com/jdx) in [#1471](https://github.com/jdx/mise/pull/1471)
- rename internal MISE_BIN env var to __MISE_BIN by [@jdx](https://github.com/jdx) in [#1472](https://github.com/jdx/mise/pull/1472)
- allow using templates in task files by [@jdx](https://github.com/jdx) in [#1473](https://github.com/jdx/mise/pull/1473)
- support array of commands directly by [@jdx](https://github.com/jdx) in [#1474](https://github.com/jdx/mise/pull/1474)
- updated dependencies by [@jdx](https://github.com/jdx) in [#1475](https://github.com/jdx/mise/pull/1475)
- add support for installing directly with go modules by [@endigma](https://github.com/endigma) in [#1470](https://github.com/jdx/mise/pull/1470)
- ensure forge type matches by [@jdx](https://github.com/jdx) in [#1476](https://github.com/jdx/mise/pull/1476)

### New Contributors

* @tamasfe made their first contribution in [#1466](https://github.com/jdx/mise/pull/1466)

## [2024.1.21](https://github.com/jdx/mise/compare/v2024.1.20..v2024.1.21) - 2024-01-15

### 🐛 Bug Fixes

- bail out of task suggestion if there are no tasks by [@roele](https://github.com/roele) in [#1460](https://github.com/jdx/mise/pull/1460)
- fixed urls by [@jdx](https://github.com/jdx) in [22265e5](https://github.com/jdx/mise/commit/22265e5ea6d3b9498eb11eef14a77c3ba46cae03)
- fixed urls by [@jdx](https://github.com/jdx) in [8c24e48](https://github.com/jdx/mise/commit/8c24e4873c6fd4f5b94f959c17795ff9f910da0f)
- fixed deprecated plugins migrate by [@jdx](https://github.com/jdx) in [94bfc46](https://github.com/jdx/mise/commit/94bfc46bccb99144a542d5b678b33537a36bea6c)

### 🔍 Other Changes

- Update README.md by [@jdx](https://github.com/jdx) in [74c5210](https://github.com/jdx/mise/commit/74c5210b9f35bd05ac53a417ac5e152dda256a9e)

## [2024.1.20](https://github.com/jdx/mise/compare/v2024.1.19..v2024.1.20) - 2024-01-14

### 🚀 Features

- add command to print task dependency tree by [@roele](https://github.com/roele) in [#1440](https://github.com/jdx/mise/pull/1440)
- add completions for task deps command by [@roele](https://github.com/roele) in [#1456](https://github.com/jdx/mise/pull/1456)
- add interactive selection for tasks if task was not found by [@roele](https://github.com/roele) in [#1459](https://github.com/jdx/mise/pull/1459)

### 🔍 Other Changes

- enable stdin under interleaved by [@jdx](https://github.com/jdx) in [b6dfb31](https://github.com/jdx/mise/commit/b6dfb311e412e119e137186d6143644d018a6cfc)
- re-enable standalone test by [@jdx](https://github.com/jdx) in [7e4e79b](https://github.com/jdx/mise/commit/7e4e79bcdcc541027bc3ea2fccc11fb0f0c07a5d)

## [2024.1.19](https://github.com/jdx/mise/compare/v2024.1.18..v2024.1.19) - 2024-01-13

### 🐛 Bug Fixes

- fix loading npm from mise by [@jdx](https://github.com/jdx) in [#1453](https://github.com/jdx/mise/pull/1453)

### 🚜 Refactor

- remove PluginName type alias by [@jdx](https://github.com/jdx) in [dedb762](https://github.com/jdx/mise/commit/dedb7624ad4708ce0434a963737a17754075d3a0)
- rename Plugin trait to Forge by [@jdx](https://github.com/jdx) in [ec4efea](https://github.com/jdx/mise/commit/ec4efea054626f9451bb54831abdd95ff98c64d1)
- clean up arg imports by [@jdx](https://github.com/jdx) in [5091fc6](https://github.com/jdx/mise/commit/5091fc6b04fd1e4795bbd636772c30432b825ef3)
- clean up arg imports by [@jdx](https://github.com/jdx) in [#1451](https://github.com/jdx/mise/pull/1451)

### 📚 Documentation

- document npm/cargo by [@jdx](https://github.com/jdx) in [d1e6e4b](https://github.com/jdx/mise/commit/d1e6e4b951637762d3562a89996a2bb3422c3341)

### 🔍 Other Changes

- Update nushell.rs - Add explicit spread by [@bnheise](https://github.com/bnheise) in [#1441](https://github.com/jdx/mise/pull/1441)
- allow using "env._.file|env._.path" instead of "env.mise.file|env.mise.path" by [@jdx](https://github.com/jdx) in [cf93693](https://github.com/jdx/mise/commit/cf936931201d6597ad556bd17556d47dc3d125c6)
- added "forge" infra by [@jdx](https://github.com/jdx) in [#1450](https://github.com/jdx/mise/pull/1450)
- added support for installing directly from npm by [@jdx](https://github.com/jdx) in [#1452](https://github.com/jdx/mise/pull/1452)
- testing by [@jdx](https://github.com/jdx) in [2ee66cb](https://github.com/jdx/mise/commit/2ee66cb91837fde144bf7acbb1028372c1cd7d9a)
- skip slow cargo test if TEST_ALL is not set by [@jdx](https://github.com/jdx) in [#1455](https://github.com/jdx/mise/pull/1455)

### New Contributors

* @bnheise made their first contribution in [#1441](https://github.com/jdx/mise/pull/1441)

## [2024.1.18](https://github.com/jdx/mise/compare/v2024.1.17..v2024.1.18) - 2024-01-12

### 🔍 Other Changes

- Revert "miette " by [@jdx](https://github.com/jdx) in [#1446](https://github.com/jdx/mise/pull/1446)
- fix mise-docs publishing by [@jdx](https://github.com/jdx) in [1dcac6d](https://github.com/jdx/mise/commit/1dcac6d4e05c80b56d1371f434776057d3ca9dc7)
- temporarily disable standalone test by [@jdx](https://github.com/jdx) in [d4f54ad](https://github.com/jdx/mise/commit/d4f54adbbf840599aeb4229c9330262569b563b5)

## [2024.1.17](https://github.com/jdx/mise/compare/v2024.1.16..v2024.1.17) - 2024-01-12

### 🚜 Refactor

- refactor env_var arg by [@jdx](https://github.com/jdx) in [#1443](https://github.com/jdx/mise/pull/1443)
- refactor ToolArg by [@jdx](https://github.com/jdx) in [5b66532](https://github.com/jdx/mise/commit/5b665325e4474f3247242a2d81c860ac17b8af5f)

### 🔍 Other Changes

- Fix missing ASDF_PLUGIN_PATH environment variable by [@deriamis](https://github.com/deriamis) in [#1437](https://github.com/jdx/mise/pull/1437)
- remove warning about moving to settings.toml by [@jdx](https://github.com/jdx) in [750141e](https://github.com/jdx/mise/commit/750141eff2721e2fbe4ab116952d04b67d2ee187)
- renovate config by [@jdx](https://github.com/jdx) in [abebe93](https://github.com/jdx/mise/commit/abebe93a3e9d79846cc566b2664451817b1ac47b)
- read from config.toml by [@jdx](https://github.com/jdx) in [#1439](https://github.com/jdx/mise/pull/1439)
- use less aggressive PATH modifications by default by [@jdx](https://github.com/jdx) in [07e1921](https://github.com/jdx/mise/commit/07e19212053bdaf4ea2ca3968e3f3559d6f49668)
- move release from justfile by [@jdx](https://github.com/jdx) in [89c9271](https://github.com/jdx/mise/commit/89c927198cfa66f332929acb9e692296dda13e2e)
- bump local version of shfmt by [@jdx](https://github.com/jdx) in [d4be898](https://github.com/jdx/mise/commit/d4be89844aa462e199d5c7278661650c22d126da)

### New Contributors

* @deriamis made their first contribution in [#1437](https://github.com/jdx/mise/pull/1437)

## [2024.1.16](https://github.com/jdx/mise/compare/v2024.1.15..v2024.1.16) - 2024-01-11

### 🐛 Bug Fixes

- fix test suite on alpine by [@jdx](https://github.com/jdx) in [#1433](https://github.com/jdx/mise/pull/1433)

### 🔍 Other Changes

- do not panic if precompiled arch/os is not supported by [@jdx](https://github.com/jdx) in [#1434](https://github.com/jdx/mise/pull/1434)
- improvements by [@jdx](https://github.com/jdx) in [#1435](https://github.com/jdx/mise/pull/1435)

## [2024.1.15](https://github.com/jdx/mise/compare/v2024.1.14..v2024.1.15) - 2024-01-10

### 🐛 Bug Fixes

- **(python)** fixes #1419 by [@gasuketsu](https://github.com/gasuketsu) in [#1420](https://github.com/jdx/mise/pull/1420)

### 🔍 Other Changes

- Update README.md by [@jdx](https://github.com/jdx) in [e3ff351](https://github.com/jdx/mise/commit/e3ff351bce362ec5d8bddd9fb9bb13827fce083d)
- rename rtx-vm -> mise-en-dev by [@jdx](https://github.com/jdx) in [03061f9](https://github.com/jdx/mise/commit/03061f973543c54b5076b26b8611f3ec378e6a61)
- fix some precompiled issues by [@jdx](https://github.com/jdx) in [#1431](https://github.com/jdx/mise/pull/1431)

## [2024.1.14](https://github.com/jdx/mise/compare/v2024.1.13..v2024.1.14) - 2024-01-09

### 🔍 Other Changes

- Correct PATH for python venvs by [@alikefia](https://github.com/alikefia) in [#1395](https://github.com/jdx/mise/pull/1395)
- downgrade rpm dockerfile by [@jdx](https://github.com/jdx) in [5a0cbe7](https://github.com/jdx/mise/commit/5a0cbe7f250a5d7586c45264e0d4bb1914325748)
- loosen regex for runtime symlink generation by [@jdx](https://github.com/jdx) in [#1392](https://github.com/jdx/mise/pull/1392)

### New Contributors

* @alikefia made their first contribution in [#1395](https://github.com/jdx/mise/pull/1395)

## [2024.1.13](https://github.com/jdx/mise/compare/v2024.1.12..v2024.1.13) - 2024-01-08

### 🔍 Other Changes

- add path separator by [@defhacks](https://github.com/defhacks) in [#1398](https://github.com/jdx/mise/pull/1398)
- prevent adding relative/empty paths during activation by [@defhacks](https://github.com/defhacks) in [#1400](https://github.com/jdx/mise/pull/1400)
- handle 404s by [@jdx](https://github.com/jdx) in [#1408](https://github.com/jdx/mise/pull/1408)
- allow expanding "~" for trusted_config_paths by [@jdx](https://github.com/jdx) in [#1409](https://github.com/jdx/mise/pull/1409)
- disallow [settings] header in settings.toml by [@jdx](https://github.com/jdx) in [#1410](https://github.com/jdx/mise/pull/1410)
- use ~/.tool-versions globally by [@jdx](https://github.com/jdx) in [#1414](https://github.com/jdx/mise/pull/1414)

### New Contributors

* @defhacks made their first contribution in [#1400](https://github.com/jdx/mise/pull/1400)

## [2024.1.12](https://github.com/jdx/mise/compare/v2024.1.11..v2024.1.12) - 2024-01-07

### 🔍 Other Changes

- added missing settings from `mise settings set` by [@jdx](https://github.com/jdx) in [8a7880b](https://github.com/jdx/mise/commit/8a7880bc912bbcef874d7428d6b0f7d772715fc5)
- fixed python_compile and all_compile settings by [@jdx](https://github.com/jdx) in [5ddbf68](https://github.com/jdx/mise/commit/5ddbf68af1f32abbf8cff406a6d17d0898d4c81f)

## [2024.1.11](https://github.com/jdx/mise/compare/v2024.1.10..v2024.1.11) - 2024-01-07

### 🔍 Other Changes

- check min_version field by [@jdx](https://github.com/jdx) in [8de42a0](https://github.com/jdx/mise/commit/8de42a0be94098c722ba8b9eef8eca505f5838c2)
- add to doctor and fix warning by [@jdx](https://github.com/jdx) in [fcf9173](https://github.com/jdx/mise/commit/fcf91739bc0241114242afb9e8de6bdf819cd7ba)
- publish schema to r2 by [@jdx](https://github.com/jdx) in [3576984](https://github.com/jdx/mise/commit/3576984b0ce89910c7bb4ae63a41b8c82381cc44)

## [2024.1.10](https://github.com/jdx/mise/compare/v2024.1.9..v2024.1.10) - 2024-01-07

### 🐛 Bug Fixes

- nix flake build errors by [@nokazn](https://github.com/nokazn) in [#1390](https://github.com/jdx/mise/pull/1390)

### 🔍 Other Changes

- do not display error if settings is missing by [@jdx](https://github.com/jdx) in [21cb004](https://github.com/jdx/mise/commit/21cb004402a7bfad2c50dbd56e584555715f1597)

### New Contributors

* @nokazn made their first contribution in [#1390](https://github.com/jdx/mise/pull/1390)

## [2024.1.9](https://github.com/jdx/mise/compare/v2024.1.8..v2024.1.9) - 2024-01-07

### 🔍 Other Changes

- sort settings by [@jdx](https://github.com/jdx) in [a8c15bb](https://github.com/jdx/mise/commit/a8c15bb6e84a6e49e4d7660ac4923d8eeaac76cf)
- clean up community-developed plugin warning by [@jdx](https://github.com/jdx) in [92b5188](https://github.com/jdx/mise/commit/92b51884a522dc7991824594e0228f014c7a1413)
- use ~/.config/mise/settings.toml by [@jdx](https://github.com/jdx) in [#1386](https://github.com/jdx/mise/pull/1386)
- add support for precompiled binaries by [@jdx](https://github.com/jdx) in [#1388](https://github.com/jdx/mise/pull/1388)

## [2024.1.8](https://github.com/jdx/mise/compare/v2024.1.7..v2024.1.8) - 2024-01-06

### 🐛 Bug Fixes

- **(java)** enable macOS integration hint for Zulu distribution by [@roele](https://github.com/roele) in [#1381](https://github.com/jdx/mise/pull/1381)
- fixed config load order by [@jdx](https://github.com/jdx) in [#1377](https://github.com/jdx/mise/pull/1377)

### 🔍 Other Changes

- Add `description` to task object in JSON schema by [@fiadliel](https://github.com/fiadliel) in [#1373](https://github.com/jdx/mise/pull/1373)
- added ideavim config by [@jdx](https://github.com/jdx) in [15cfa1e](https://github.com/jdx/mise/commit/15cfa1eebd18ee77b931b5e4343a4ef1d7c2473f)
- paranoid by [@jdx](https://github.com/jdx) in [#1382](https://github.com/jdx/mise/pull/1382)
- miette by [@jdx](https://github.com/jdx) in [#1368](https://github.com/jdx/mise/pull/1368)

## [2024.1.7](https://github.com/jdx/mise/compare/v2024.1.6..v2024.1.7) - 2024-01-05

### 🐛 Bug Fixes

- fixed migration script by [@jdx](https://github.com/jdx) in [54097ee](https://github.com/jdx/mise/commit/54097eed2050681f6ed74084809a438a70000cab)
- fixed not-found handler by [@jdx](https://github.com/jdx) in [69f354d](https://github.com/jdx/mise/commit/69f354df0e463edcdcbd12364a88013e5f5029f9)

### 🔍 Other Changes

- show better error when attemping to install core plugin by [@jdx](https://github.com/jdx) in [#1366](https://github.com/jdx/mise/pull/1366)
- read rtx.plugin.toml if it exists by [@jdx](https://github.com/jdx) in [db19252](https://github.com/jdx/mise/commit/db19252f3c5f23426f2d8c5a899939a575453779)

## [2024.1.6](https://github.com/jdx/mise/compare/v2024.1.5..v2024.1.6) - 2024-01-04

### 🧪 Testing

- fixed elixir test case by [@jdx](https://github.com/jdx) in [9b596c6](https://github.com/jdx/mise/commit/9b596c6dadcf0f54b3637d10e1885281e1a1b534)

### 🔍 Other Changes

- set CLICOLOR_FORCE=1 and FORCE_COLOR=1 by [@jdx](https://github.com/jdx) in [#1364](https://github.com/jdx/mise/pull/1364)
- set --interleaved if graph is linear by [@jdx](https://github.com/jdx) in [#1365](https://github.com/jdx/mise/pull/1365)

## [2024.1.5](https://github.com/jdx/mise/compare/v2024.1.4..v2024.1.5) - 2024-01-04

### 🐛 Bug Fixes

- fixed man page by [@jdx](https://github.com/jdx) in [581b6e8](https://github.com/jdx/mise/commit/581b6e8aa56476d8d184c2cae2bd7657c8690143)
- remove comma from conflicts by [@pdecat](https://github.com/pdecat) in [#1353](https://github.com/jdx/mise/pull/1353)

### 🔍 Other Changes

- skip ruby installs by [@jdx](https://github.com/jdx) in [c23e467](https://github.com/jdx/mise/commit/c23e467717105e34ac805638dfeb5fcac3f991a2)
- Update README.md to link to rtx page by [@silasb](https://github.com/silasb) in [#1352](https://github.com/jdx/mise/pull/1352)
- use "[" instead of "test" by [@jdx](https://github.com/jdx) in [#1355](https://github.com/jdx/mise/pull/1355)
- prevent loading multiple times by [@jdx](https://github.com/jdx) in [#1358](https://github.com/jdx/mise/pull/1358)
- use `mise.file`/`mise.path` config by [@jdx](https://github.com/jdx) in [#1361](https://github.com/jdx/mise/pull/1361)

### New Contributors

* @silasb made their first contribution in [#1352](https://github.com/jdx/mise/pull/1352)
* @pdecat made their first contribution in [#1353](https://github.com/jdx/mise/pull/1353)

## [2024.1.4](https://github.com/jdx/mise/compare/v2024.1.3..v2024.1.4) - 2024-01-04

### 🐛 Bug Fixes

- **(java)** use tar.gz archives to enable symlink support by [@roele](https://github.com/roele) in [#1343](https://github.com/jdx/mise/pull/1343)

### 🔍 Other Changes

- rtx-plugins -> mise-plugins by [@jdx](https://github.com/jdx) in [04f55cd](https://github.com/jdx/mise/commit/04f55cd677a3041232887c2f3731d17f775e3627)
- rtx -> mise by [@jdx](https://github.com/jdx) in [ed794d1](https://github.com/jdx/mise/commit/ed794d15cf035a993e0c286e84dac0335ffe8967)
- add "replaces" field by [@jdx](https://github.com/jdx) in [#1345](https://github.com/jdx/mise/pull/1345)
- Add additional conflicts by [@inverse](https://github.com/inverse) in [#1346](https://github.com/jdx/mise/pull/1346)
- docs by [@jdx](https://github.com/jdx) in [eb73edf](https://github.com/jdx/mise/commit/eb73edfab75d8a2b5bd58be71b2ccbd172b92413)
- demo by [@jdx](https://github.com/jdx) in [#1348](https://github.com/jdx/mise/pull/1348)
- fix ssh urls by [@jdx](https://github.com/jdx) in [#1349](https://github.com/jdx/mise/pull/1349)

### New Contributors

* @inverse made their first contribution in [#1346](https://github.com/jdx/mise/pull/1346)

## [2024.1.3](https://github.com/jdx/mise/compare/v2024.1.2..v2024.1.3) - 2024-01-03

### 🔍 Other Changes

- use mise docker containers by [@jdx](https://github.com/jdx) in [d5d2d39](https://github.com/jdx/mise/commit/d5d2d39aa1a44a6421dff150da42083c4247cff9)
- skip committing docs if no changes by [@jdx](https://github.com/jdx) in [7f6545c](https://github.com/jdx/mise/commit/7f6545c2630a1f54b864903851c24e68b3da3d2f)
- use ~/.local/bin/mise instead of ~/.local/share/mise/bin/mise by [@jdx](https://github.com/jdx) in [cd2045d](https://github.com/jdx/mise/commit/cd2045d793c76b9dcf7d26c567cf163a6138f408)

## [2024.1.2](https://github.com/jdx/mise/compare/v2024.1.1..v2024.1.2) - 2024-01-03

### 🔍 Other Changes

- fix venv python path by [@jdx](https://github.com/jdx) in [e2d50a2](https://github.com/jdx/mise/commit/e2d50a2f25c0c64c207f82e957e691671d52ddbd)

## [2024.1.1](https://github.com/jdx/mise/compare/v2024.1.0..v2024.1.1) - 2024-01-03

### 🐛 Bug Fixes

- fixed email addresses by [@jdx](https://github.com/jdx) in [b5e9d3c](https://github.com/jdx/mise/commit/b5e9d3cc3a2500c932593d7931647fbc3d972708)
- fixed crate badge by [@jdx](https://github.com/jdx) in [c4bb224](https://github.com/jdx/mise/commit/c4bb224acb197e9f67eda56a4be3c7f3c5bdcee6)

### 📚 Documentation

- tweak cli reference by [@jdx](https://github.com/jdx) in [ba5f610](https://github.com/jdx/mise/commit/ba5f6108b1b91952295e4871f63c559ff01c7c64)
- fixed reading settings from config by [@jdx](https://github.com/jdx) in [a30a5f1](https://github.com/jdx/mise/commit/a30a5f104da41794aa8a2813919f046945ed9ae6)

### 🔍 Other Changes

- rtx -> mise by [@jdx](https://github.com/jdx) in [9b7975e](https://github.com/jdx/mise/commit/9b7975e5cd43121d22436893acdc7dbfe36ee960)
- readme by [@jdx](https://github.com/jdx) in [7d3a2ca](https://github.com/jdx/mise/commit/7d3a2ca707a7779041df559bba23bd552ef01775)
- Update README.md by [@jdx](https://github.com/jdx) in [884147b](https://github.com/jdx/mise/commit/884147b16e94880e915a53291af21647546d6a04)
- fail on r2 error by [@jdx](https://github.com/jdx) in [c4011da](https://github.com/jdx/mise/commit/c4011da5261f254f118c3cd5740bbf8d50ac8733)
- update CONTRIBUTING.md by [@jdx](https://github.com/jdx) in [91e9bef](https://github.com/jdx/mise/commit/91e9befabec3f87dec4f2c6513f52b29ca53f5b8)
- 2024 by [@jdx](https://github.com/jdx) in [fbcc3ee](https://github.com/jdx/mise/commit/fbcc3ee610f38633e2ce583d9c43fc9df8c4f368)
- auto-publish cli reference to docs by [@jdx](https://github.com/jdx) in [a2f59c6](https://github.com/jdx/mise/commit/a2f59c6933833e0a2f15066d952ce1119a0928c8)
- fix MISE_ASDF_COMPAT=1 by [@jdx](https://github.com/jdx) in [#1340](https://github.com/jdx/mise/pull/1340)
- migrate improvements by [@jdx](https://github.com/jdx) in [2c0ccf4](https://github.com/jdx/mise/commit/2c0ccf43fd23de03c25a872fe6d91f1d63c77c1a)

## [2024.1.0] - 2024-01-02

### 🔍 Other Changes

- added "ev" alias by [@jdx](https://github.com/jdx) in [8d98b91](https://github.com/jdx/mise/commit/8d98b9158b6dc4d6c36332a5f52061e81cc87d91)
- added "ev" alias by [@jdx](https://github.com/jdx) in [4bfe580](https://github.com/jdx/mise/commit/4bfe580eef8a8192f621ea729c8013ef141dacf3)
- added RTX_ENV_FILE config by [@jdx](https://github.com/jdx) in [#1305](https://github.com/jdx/mise/pull/1305)
- Update CONTRIBUTING.md by [@jdx](https://github.com/jdx) in [0737393](https://github.com/jdx/mise/commit/0737393b7b167fd57d168dfbf886405bb0a8cecb)
- Configure Renovate by [@renovate[bot]](https://github.com/renovate[bot]) in [#1307](https://github.com/jdx/mise/pull/1307)
- consistent dependency versions by [@jdx](https://github.com/jdx) in [43b37bc](https://github.com/jdx/mise/commit/43b37bc2296460e8b222ab0cbb815ac457717074)
- ignore asdf/nodejs by [@jdx](https://github.com/jdx) in [acc9a68](https://github.com/jdx/mise/commit/acc9a6803d6d3087a847529baa7d7e341ef46cc2)
- ignore nodenv by [@jdx](https://github.com/jdx) in [4d921c7](https://github.com/jdx/mise/commit/4d921c7608e4807ae765383253e100763d04bd75)
- tuck away by [@jdx](https://github.com/jdx) in [4361f03](https://github.com/jdx/mise/commit/4361f0385a82da470cfe47a5044a00ca783c9ddc)
- disable dashboard by [@jdx](https://github.com/jdx) in [2c569fc](https://github.com/jdx/mise/commit/2c569fc01a77987e6823dc749eb917f1fe5a0cf0)
- disable auto package updates by [@jdx](https://github.com/jdx) in [e00fb1f](https://github.com/jdx/mise/commit/e00fb1fde649ecc85aa40ac8846f71316d679e54)
- disable dashboard by [@jdx](https://github.com/jdx) in [400ac0a](https://github.com/jdx/mise/commit/400ac0a0ff64cf5a6846f662df5dc432237e87b2)
- updated description by [@jdx](https://github.com/jdx) in [83c0ffc](https://github.com/jdx/mise/commit/83c0ffcf210c51228f82e9eb586d09a5ea7933f4)
- rtx -> mise by [@jdx](https://github.com/jdx) in [e5897d0](https://github.com/jdx/mise/commit/e5897d097c1f90c8a263f0e685a56908e2c023da)

### 📦️ Dependency Updates

- update rust crate indexmap to 2.1 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1308](https://github.com/jdx/mise/pull/1308)
- update rust crate num_cpus to 1.16 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1309](https://github.com/jdx/mise/pull/1309)
- update rust crate once_cell to 1.19 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1311](https://github.com/jdx/mise/pull/1311)
- update rust crate regex to 1.10 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1312](https://github.com/jdx/mise/pull/1312)
- update rust crate url to 2.5 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1315](https://github.com/jdx/mise/pull/1315)
- update actions/upload-artifact action to v4 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1320](https://github.com/jdx/mise/pull/1320)
- update actions/download-artifact action to v4 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1319](https://github.com/jdx/mise/pull/1319)
- update fedora docker tag to v40 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1322](https://github.com/jdx/mise/pull/1322)
- update mcr.microsoft.com/devcontainers/rust docker tag to v1 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1323](https://github.com/jdx/mise/pull/1323)
- update stefanzweifel/git-auto-commit-action action to v5 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1324](https://github.com/jdx/mise/pull/1324)
- update actions/checkout action to v4 by [@renovate[bot]](https://github.com/renovate[bot]) in [#1318](https://github.com/jdx/mise/pull/1318)

<!-- generated by git-cliff -->
