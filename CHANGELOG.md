# Changelog

## [unreleased]

### 🔍 Other Changes

- include symlink error context in error message by Andrew Klotz in [9dc822a](https://github.com/jdx/mise/commit/9dc822a39df6cc32d6c61ac146fc4c345297866f)
- Merge pull request #2040 from KlotzAndrew/aklotz/show_symlink_error by jdx in [04f856a](https://github.com/jdx/mise/commit/04f856ab01c01c8cb0a95c18c9d23021337e29c2)
- continue git subtree on error by Jeff Dickey in [7bd1de7](https://github.com/jdx/mise/commit/7bd1de7613fe50b56c0c182bb4e34de9accde24d)

## [2024.5.3](https://github.com/jdx/mise/compare/v2024.5.2..v2024.5.3) - 2024-05-07

### 🚀 Features

- **(env)** supports glob patterns in `env._.file` and `env._.source` (fix #1916) by Axel H in [b08c5cc](https://github.com/jdx/mise/commit/b08c5cccfc8c3806de6ffef762d1a11550038f1c)
- cleanup invalid symlinks in .local/state/mise/(tracked|trusted)-configs by Roland Schaer in [6c557cf](https://github.com/jdx/mise/commit/6c557cfadfb689d9315ac56db666e85ea7cf188d)

### 🐛 Bug Fixes

- **(plugin-update)** Handle errors from the underlying plugin updates by Chris Rose in [11d6f71](https://github.com/jdx/mise/commit/11d6f7124f660befa67228cc3739b457193455d6)
- backend install directory not removed if empty by Roland Schaer in [991fcdb](https://github.com/jdx/mise/commit/991fcdbc9a9fe3160fe52360a772881fef7b81fc)
- mise trust doesn't handle relative paths by Roland Schaer in [4e8c2fc](https://github.com/jdx/mise/commit/4e8c2fc022ec40cf38dc706b77c2795613a22164)

### 🔍 Other Changes

- Update README.md by jdx in [40e82be](https://github.com/jdx/mise/commit/40e82be7e187cb09d2dad1c0d8b61078c4f7cebe)
- move kachick plugins to mise-plugins by Jeff Dickey in [a41b296](https://github.com/jdx/mise/commit/a41b296d7f599de3bccfb31c71da9606fd508216)
- Commit from GitHub Actions (test) by mise[bot] in [f91a48e](https://github.com/jdx/mise/commit/f91a48ec163d8c6be14f8cd9eddb107f9be1ab9f)
- Merge pull request #2019 from jdx/release by jdx in [8fcc5ea](https://github.com/jdx/mise/commit/8fcc5eac687d9f2bfb21f0e3d6ee14bb18adccab)

### 📦️ Dependency Updates

- update rust crate zip to v1.1.4 by renovate[bot] in [1741250](https://github.com/jdx/mise/commit/1741250c2a5d29caf66b574f80e6d5e3e6b17e6e)

## [2024.5.2](https://github.com/jdx/mise/compare/v2024.5.1..v2024.5.2) - 2024-05-02

### 🐛 Bug Fixes

- **(self_update)** show --version param by jdx in [a330837](https://github.com/jdx/mise/commit/a330837bf81dedf5e0687828c3478f1755274001)

## [2024.5.1](https://github.com/jdx/mise/compare/v2024.5.0..v2024.5.1) - 2024-05-02

### 🐛 Bug Fixes

- **(ruby)** handle github rate limits when fetching ruby-build version by Jeff Dickey in [4a538a7](https://github.com/jdx/mise/commit/4a538a7e282de8c59ba51ec6de64bb715debe022)
- **(ruby)** attempt to update ruby-build if it cannot check version by Jeff Dickey in [9f6e2ef](https://github.com/jdx/mise/commit/9f6e2efeb03a9cfc1786f88e84ce03ed6399a304)
- prevent crashing if "latest" is not a symlink by Jeff Dickey in [91291e0](https://github.com/jdx/mise/commit/91291e09e8fd3395f8a1265c9af2bd22eff46993)
- edge case around "latest" being the "latest" version by Jeff Dickey in [33f5473](https://github.com/jdx/mise/commit/33f547357e9f082115e81ff852c523b406e5226d)
- show source file on resolve error by Jeff Dickey in [881dbeb](https://github.com/jdx/mise/commit/881dbeb9a34fcb231bc83e14d1a12314bb995870)

### 📚 Documentation

- **(python)** warn about precompiled python and poetry by Jeff Dickey in [3c07dce](https://github.com/jdx/mise/commit/3c07dced232970fce3d585277089cbf374e4d64a)

### 🧪 Testing

- **(self-update)** try to enable self update test by Jeff Dickey in [778e90a](https://github.com/jdx/mise/commit/778e90af1bdfb5095e2f0b5c1b625c5abab7ee45)
- fix the test-plugins job by Jeff Dickey in [669530c](https://github.com/jdx/mise/commit/669530ce5bbd902ad0cd39e87e6c442d184353b9)

### 🔍 Other Changes

- **(release)** disable cache by Jeff Dickey in [b69edc6](https://github.com/jdx/mise/commit/b69edc67c83284ee758c92258608298eaba25929)
- **(ruby)** change ruby-build update failure to warn-level by Jeff Dickey in [d6f7f22](https://github.com/jdx/mise/commit/d6f7f22df93a862929ebbc8f3b6a1309e1e3c875)

## [2024.5.0](https://github.com/jdx/mise/compare/v2024.4.12..v2024.5.0) - 2024-05-01

### 🐛 Bug Fixes

- **(release)** use target/release dir by Jeff Dickey in [e6448b3](https://github.com/jdx/mise/commit/e6448b335cf99db6fb2bdfd4c3f49ba255c2d8de)
- **(release)** fixed the "serious" profile by Jeff Dickey in [487a1a0](https://github.com/jdx/mise/commit/487a1a0d336fed180123659ac59d1106d79f2d60)

### 🔍 Other Changes

- **(release)** added "serious" profile by Jeff Dickey in [f8ce139](https://github.com/jdx/mise/commit/f8ce139c1d0b41006dbbf1707801bf665f201ec6)

### 📦️ Dependency Updates

- update rust crate rmp-serde to 1.3.0 by renovate[bot] in [249c8c9](https://github.com/jdx/mise/commit/249c8c964ec550da5f1c695b98bb11e3e9e156ec)
- update rust crate base64 to 0.22.1 by renovate[bot] in [93d4d29](https://github.com/jdx/mise/commit/93d4d2972ab7e67439aa4bee1daa8e47aff10f22)

## [2024.4.12](https://github.com/jdx/mise/compare/v2024.4.11..v2024.4.12) - 2024-04-30

### 🐛 Bug Fixes

- **(self_update)** downgrade to fix signature verification issue by Jeff Dickey in [dbe1971](https://github.com/jdx/mise/commit/dbe1971c337a29f2e92fd1b765436e67abf7f04e)

## [2024.4.11](https://github.com/jdx/mise/compare/v2024.4.10..v2024.4.11) - 2024-04-30

### 🐛 Bug Fixes

- **(self-update)** always use rustls by Jeff Dickey in [93a9c57](https://github.com/jdx/mise/commit/93a9c57ae895f1772a5ae8146d83713f631c77f1)

### 🧪 Testing

- **(java)** added e2e test for corretto-8 shorthand by jdx in [9c8ceec](https://github.com/jdx/mise/commit/9c8ceec314b2732e749f0b4f6244446063894725)

### 🔍 Other Changes

- **(release)** fix cache by Jeff Dickey in [b54b25d](https://github.com/jdx/mise/commit/b54b25d06c49b5116ed37dda4c08005dfe7e6e11)
- fix clippy warnings in latest rust beta by jdx in [da7d29b](https://github.com/jdx/mise/commit/da7d29b5b0aab03b6daa17eafad5dd958d010043)

### 📦️ Dependency Updates

- update rust crate flate2 to 1.0.30 by renovate[bot] in [41ac582](https://github.com/jdx/mise/commit/41ac582194d989c2240a888f9e68a7d52a86b6c5)

## [2024.4.10](https://github.com/jdx/mise/compare/v2024.4.9..v2024.4.10) - 2024-04-29

### 🐛 Bug Fixes

- **(docker)** create path to cargo registry cache by Jeff Dickey in [ed91c1c](https://github.com/jdx/mise/commit/ed91c1c5f928751c6bc1ce23ac0595c063648677)

### 🔍 Other Changes

- Revert "fix(java): inconsistent version resolution " by jdx in [07884eb](https://github.com/jdx/mise/commit/07884eb9270f0dcdf3922f1c44dc2ca3c3eacd4d)

## [2024.4.9](https://github.com/jdx/mise/compare/v2024.4.8..v2024.4.9) - 2024-04-29

### 🚀 Features

- **(node)** support comments in .nvmrc/.node-version by Jeff Dickey in [5915ae0](https://github.com/jdx/mise/commit/5915ae0a23d322e37f22847be11638f8ba108c15)
- cli command for listing backends by Roland Schaer in [5f8a3ce](https://github.com/jdx/mise/commit/5f8a3ce9d5ebd598551baaf5dcd076cc9449fff8)

### 🐛 Bug Fixes

- **(ci)** git2 reference by jdx in [eaa99af](https://github.com/jdx/mise/commit/eaa99af6f067c6e668d4278db29569a3d8f5b4c4)
- **(docker)** Ensure the e2e tests pass in the dev container by Adirelle in [d13511e](https://github.com/jdx/mise/commit/d13511ed2853911b22ecb4d8e6925a0bb69cf177)
- **(java)** inconsistent version resolution by Roland Schaer in [454bee9](https://github.com/jdx/mise/commit/454bee985704f7dfab3525bf4e1527da29c78de1)
- **(zig)** can't install zig@master from v2024.4.6 by Roland Schaer in [437bfd7](https://github.com/jdx/mise/commit/437bfd7fe245e93c50a6197a3ce1c5702446c344)
- use mise fork of asdf-maven by Jeff Dickey in [5a01c1b](https://github.com/jdx/mise/commit/5a01c1b336a6e0a2ca0167aee6fa865318bd7f81)
- deal with missing go/cargo/npm/etc in backends by jdx in [e6424b8](https://github.com/jdx/mise/commit/e6424b870af99cabd7971d73179cf7d126a827ed)
- mise doesn't change the trust hash file by Roland Schaer in [05fd68b](https://github.com/jdx/mise/commit/05fd68bd113ccf0eba47d836d554bb74c033580c)

### 🚜 Refactor

- converted just tasks in mise tasks. by Adirelle in [e8c5ec2](https://github.com/jdx/mise/commit/e8c5ec2a50bb6a399e974335dad94253d3814197)

### 🧪 Testing

- added cache for docker tests by jdx in [9f3285f](https://github.com/jdx/mise/commit/9f3285f4d66cc9d58fc20b958c80f4bacec20c43)

### 🔍 Other Changes

- **(docker)** removed unused image by jdx in [4150207](https://github.com/jdx/mise/commit/4150207c3464bf47207ea1c3c0959e7141ab27b8)
- **(renovate)** ignore changes to registry/ subtree by Jeff Dickey in [c556149](https://github.com/jdx/mise/commit/c556149a88e73825306d98e3e3ea5b53692e0900)
- buildjet by jdx in [26e9cfd](https://github.com/jdx/mise/commit/26e9cfd73a8378de8043af10ed17dc5867e39b44)
- make git2 an optional build dependency by jdx in [1060234](https://github.com/jdx/mise/commit/1060234f97debb1cb3a46cd39ecda62a81e61149)
- remove CODEOWNERS by Jeff Dickey in [304ba17](https://github.com/jdx/mise/commit/304ba171fd95701c04beb3d2a76bde0463a54209)

### 📦️ Dependency Updates

- update rust crate color-print to 0.3.6 by renovate[bot] in [7efd9ac](https://github.com/jdx/mise/commit/7efd9ac8196907879dee07bdc2bf1e0cc121b308)
- update amannn/action-semantic-pull-request action to v5.5.0 by renovate[bot] in [6659826](https://github.com/jdx/mise/commit/66598268fc197f90e321ca17ffcf30f029e44a7a)
- update rust crate demand to 1.1.1 by renovate[bot] in [e8eea10](https://github.com/jdx/mise/commit/e8eea1084bc75d26653a29868033e50637a30426)
- update rust crate self_update to 0.40.0 by renovate[bot] in [ef1765c](https://github.com/jdx/mise/commit/ef1765cd644cc7002016ad12593326f2398c56c9)
- update rust crate flate2 to 1.0.29 by renovate[bot] in [471b07d](https://github.com/jdx/mise/commit/471b07d09aa03fa7b48d0b32fffe2650c20b8880)
- update serde monorepo to 1.0.199 by renovate[bot] in [70dd298](https://github.com/jdx/mise/commit/70dd298d0d758b7f82c77c4d1df9117682a38d2b)
- update rust crate demand to 1.1.2 by renovate[bot] in [f8e4f57](https://github.com/jdx/mise/commit/f8e4f577eb71e787cf2cd76381dc9d26b6f5eac9)
- update rust crate zip to 1.1.2 by renovate[bot] in [ad9f803](https://github.com/jdx/mise/commit/ad9f803c69e6f2147b45be469150da1dbc858d1c)

## [2024.4.8](https://github.com/jdx/mise/compare/v2024.4.7..v2024.4.8) - 2024-04-23

### 🚀 Features

- add periphery by MontakOleg in [7f51540](https://github.com/jdx/mise/commit/7f51540695664412dab4008b0d061bfaca5b0bc2)
- add danger-js by MontakOleg in [6e61cf7](https://github.com/jdx/mise/commit/6e61cf7c97d03094a6ac86656b64fdeb85e84df5)

### 🐛 Bug Fixes

- **(exec)** default to @latest version by Zander Hill in [82e7077](https://github.com/jdx/mise/commit/82e7077e74928f9630647d6596cb5a2d15ea5f03)
- rename bin -> ubi by jdx in [0843b78](https://github.com/jdx/mise/commit/0843b78e6ab9a3dd2965f0218760c1a3336c4ca5)

### 📚 Documentation

- **(changelog)** reorder changelog topics by jdx in [d10257f](https://github.com/jdx/mise/commit/d10257ff792429cf565e2a8816475b13028e3377)
- fixed asdf-xcbeautify url by Jeff Dickey in [d4134bc](https://github.com/jdx/mise/commit/d4134bcb399a8d9da4e9670500e01d832b9a8e46)

### 🔍 Other Changes

- use https to get gpgkey by Stephen Palfreyman in [87327f0](https://github.com/jdx/mise/commit/87327f094267ca9688eb94109c7ff182baf9e975)
- Update xcbeautify by jdx in [cb48b68](https://github.com/jdx/mise/commit/cb48b68bb6a0c7962b1ef95641514ba64ac63bd1)
- Include e2e folder in shfmt editorconfig for 2 spaces indenting by Zander Hill in [75810a2](https://github.com/jdx/mise/commit/75810a2aa5fdd258f4db2f4e2f42f0a49fcc03c9)
- disable megalinter by Jeff Dickey in [3dd1006](https://github.com/jdx/mise/commit/3dd1006a8367a852a6f415256b8301771f8fa8d6)

## [2024.4.7](https://github.com/jdx/mise/compare/v2024.4.6..v2024.4.7) - 2024-04-22

### 🐛 Bug Fixes

- **(zig)** make zig core plugin experimental by Jeff Dickey in [45274bc](https://github.com/jdx/mise/commit/45274bc1415ac5dc307a82a93db952a1cf811210)

## [2024.4.6](https://github.com/jdx/mise/compare/v2024.4.5..v2024.4.6) - 2024-04-22

### 🚀 Features

- Pipx Backend by Zander Hill in [8b85eaa](https://github.com/jdx/mise/commit/8b85eaabc5d33ab3127de7ee276e51657cfd839c)
- ubi backend by Zander Hill in [20d59ef](https://github.com/jdx/mise/commit/20d59ef4b366e48b12ea0b223c328f1f1574f55c)

### 🐛 Bug Fixes

- **(gleam)** use asdf-community fork by Jesse Cooke in [06599d8](https://github.com/jdx/mise/commit/06599d8977baaa2a2db7e2d144939049bbe9d20b)

### 🚜 Refactor

- use a metadata file for forges by Roland Schaer in [2ded275](https://github.com/jdx/mise/commit/2ded2757470beef7f121cbab2af7c291ea1c1803)

### 🔍 Other Changes

- Add Zig language plugin by Albert in [ff8d0d8](https://github.com/jdx/mise/commit/ff8d0d8af48b146cd33fe08cffc054ed7b357506)

### 📦️ Dependency Updates

- update rust crate chrono to 0.4.38 by renovate[bot] in [a7b713b](https://github.com/jdx/mise/commit/a7b713bfd613295584e8b4d760f942960e03ede7)
- update rust crate serde_json to 1.0.116 by renovate[bot] in [e79187f](https://github.com/jdx/mise/commit/e79187f11814b8aefbd2046df5d05715afd96207)
- update rust crate toml_edit to 0.22.11 by renovate[bot] in [c8720af](https://github.com/jdx/mise/commit/c8720afced007657e375c180777c1f2d1d29e1d0)
- bump rustls from 0.21.10 to 0.21.11 by dependabot[bot] in [6f83e29](https://github.com/jdx/mise/commit/6f83e2946a48818fb10c76f44a1efa6928616aa4)
- update rust crate rmp-serde to 1.2.0 by renovate[bot] in [f3d2ee3](https://github.com/jdx/mise/commit/f3d2ee3b8bf992260ca446162450cdaffa6dc162)
- update rust crate toml_edit to 0.22.12 by renovate[bot] in [df6648b](https://github.com/jdx/mise/commit/df6648b2582fc7a6689e91c722fa7395b8721726)
- update rust crate usage-lib to 0.1.18 by renovate[bot] in [1009d97](https://github.com/jdx/mise/commit/1009d977a7e831e393e3d54643d597045fa36354)
- update rust crate ctor to 0.2.8 by renovate[bot] in [a46d3a5](https://github.com/jdx/mise/commit/a46d3a56d61b6a47519fbbcb54a1959cb3358625)
- update serde monorepo to 1.0.198 by renovate[bot] in [c9d3986](https://github.com/jdx/mise/commit/c9d3986bb8c848afd3d27a22b383956746d29b70)
- update rust crate thiserror to 1.0.59 by renovate[bot] in [8b4f4eb](https://github.com/jdx/mise/commit/8b4f4eb41add440c1bbd52daf2e3b26f819fb9af)
- update rust crate zip to v1 by renovate[bot] in [8c43fa4](https://github.com/jdx/mise/commit/8c43fa49e6ed33c2f64038d997d298c3bb11102e)

## [2024.4.5](https://github.com/jdx/mise/compare/v2024.4.3..v2024.4.5) - 2024-04-15

### 🚀 Features

- **(doctor)** warn if a plugin overwrites a core plugin by Roland Schaer in [6c96bc6](https://github.com/jdx/mise/commit/6c96bc6037a7cc504c20c281230caeee0bf21b7b)
- add option to list installed (backend) binaries by Roland Schaer in [16339b5](https://github.com/jdx/mise/commit/16339b542442afb6f44b6437dc506f39935c01bd)
- add powerpipe by Jesse Cooke in [7369b74](https://github.com/jdx/mise/commit/7369b74550b28b45d7195d0b11c11e98cf5e4d29)
- add xcresultparser by Vitalii Budnik in [92b4aeb](https://github.com/jdx/mise/commit/92b4aeb13e8350a76a9ceae9df1bb6270a2b2182)

### 🐛 Bug Fixes

- **(alpine)** use mise docker image by Jeff Dickey in [db65c3f](https://github.com/jdx/mise/commit/db65c3f5de1b1117bc6708b881de86f490057b68)
- **(heroku-cli)** use mise-plugins fork by Jeff Dickey in [2a92d9d](https://github.com/jdx/mise/commit/2a92d9d1bd0b275c7d27ca020f63e3089d789c8c)
- enable markdown-magic since it is working again by Jeff Dickey in [2b7b943](https://github.com/jdx/mise/commit/2b7b943d33ac91ea6eaded7f2fe84b472f73e073)
- mise panics if prefix: is used on certain core plugins by Roland Schaer in [878a9fa](https://github.com/jdx/mise/commit/878a9fa4788811186593a809fae3bbdf9b67e6cc)
- go backend naming inconsistency (in mise ls and mise prune) by Roland Schaer in [35e8054](https://github.com/jdx/mise/commit/35e805478d33fb794ccd4670824f443a81e59219)

### 🧪 Testing

- fix github action branch by Jeff Dickey in [39eb2ab](https://github.com/jdx/mise/commit/39eb2abbdb7b136c541f84696dc038637280d8a7)

### 🔍 Other Changes

- **(move)** added TODO by Jeff Dickey in [5ffbcc1](https://github.com/jdx/mise/commit/5ffbcc134f27800109bb65335b4b9423742b6807)
- **(pre-commit)** added pre-commit by Jeff Dickey in [b2ff8cd](https://github.com/jdx/mise/commit/b2ff8cd88c5951326781fcc5c1405d3883ef21c1)
- **(pre-commit)** check json and toml files by Jeff Dickey in [5281712](https://github.com/jdx/mise/commit/5281712f63bb673b301be139274b5f2eab64c205)
- added podman plugin by Carlos Fernandes in [24155e8](https://github.com/jdx/mise/commit/24155e8b9d5f342d52ccdd212f187243022efa0b)

### 📦️ Dependency Updates

- update rust crate built to 0.7.2 by renovate[bot] in [74bfe8a](https://github.com/jdx/mise/commit/74bfe8a8825177cbb197cbad5a6c90dc8729ce83)
- update rust crate either to 1.11.0 by renovate[bot] in [c01af12](https://github.com/jdx/mise/commit/c01af120f06aac00206234d42788ed012f2bcfa9)

## [2024.4.3](https://github.com/jdx/mise/compare/v2024.4.2..v2024.4.3) - 2024-04-09

### 🐛 Bug Fixes

- **(docker)** repo fetch by Jeff Dickey in [bb68fc3](https://github.com/jdx/mise/commit/bb68fc33f98e5fe4518478f963d448bca61d54fe)
- **(docker)** repo fetch by jdx in [1e81b3a](https://github.com/jdx/mise/commit/1e81b3a2c9520c87ac9ca0d8f8f32d6a2b51a1d2)
- asdf-yarn by Jeff Dickey in [fc8de34](https://github.com/jdx/mise/commit/fc8de34e2e535ed7262c079087ff444a44dd5731)

### 🔍 Other Changes

- **(release-plz)** clean up PR/release description by Jeff Dickey in [14b4fc5](https://github.com/jdx/mise/commit/14b4fc5525e06cc150ae160c406ee06b58d95ce5)
- **(release-plz)** clean up PR/release description by Jeff Dickey in [769e7fe](https://github.com/jdx/mise/commit/769e7fe16e4f4df7380414f91e67345148e059de)
- **(release-plz)** disable subtree push by Jeff Dickey in [c74a12c](https://github.com/jdx/mise/commit/c74a12c3c31ba087a154d7b328374670a675f00a)
- **(sync)** added workflow by Jeff Dickey in [b773033](https://github.com/jdx/mise/commit/b7730335949235085001b23ddc382a7c44b18f12)
- **(sync)** pull and push changes by Jeff Dickey in [52bb0ae](https://github.com/jdx/mise/commit/52bb0aee4a91c66bb2966cf462c23eedfc0c5058)
- **(sync)** pull and push changes by Jeff Dickey in [28b9a52](https://github.com/jdx/mise/commit/28b9a52fce891ab28e9e4d2f7fc1207de07d0a84)
- **(sync)** pull and push changes by Jeff Dickey in [202c900](https://github.com/jdx/mise/commit/202c90051a81eff9ed12ef1483bc86cee355371c)
- **(sync)** pull and push changes by Jeff Dickey in [e3aefb1](https://github.com/jdx/mise/commit/e3aefb1eb915387ca08b484158e5f13c069912d1)
- **(sync)** pull and push changes by Jeff Dickey in [60f5b7a](https://github.com/jdx/mise/commit/60f5b7a44479385650e563d234ceb2ac0e135994)

## [2024.4.2](https://github.com/jdx/mise/compare/v2024.4.1..v2024.4.2) - 2024-04-09

### 🚀 Features

- **(completions)** switch to usage for zsh completions by jdx in [4e56b9f](https://github.com/jdx/mise/commit/4e56b9f3b29cb2cbd61e16a89fb827ac15d6f40e)

### 🚜 Refactor

- **(default_shorthands)** automatically mark mise-plugins as trusted by Jeff Dickey in [538a90f](https://github.com/jdx/mise/commit/538a90f04447b306c0ea6d8009cdef3f92cd4735)

### 🔍 Other Changes

- **(cliff)** ignore previous registry commits by Jeff Dickey in [64f326d](https://github.com/jdx/mise/commit/64f326d903e13a104b88eb4b0986285ad2411f19)
- **(cliff)** ignore merge commits by Jeff Dickey in [d54b0a2](https://github.com/jdx/mise/commit/d54b0a2cd4d4bd02816719101cbe77905818d07a)
- **(default_shorthands)** fix count by Jeff Dickey in [a098228](https://github.com/jdx/mise/commit/a0982283273b4126c2d9b22ac2395de5cd3f73eb)
- **(homebrew)** delete unused script by jdx in [f3b092f](https://github.com/jdx/mise/commit/f3b092fdaf2bcfc789485543770cc2374a469523)
- **(markdown-magic)** do not fail if markdown-magic fails by Jeff Dickey in [073fce7](https://github.com/jdx/mise/commit/073fce750dbe97818db6de5a7ae6482904730de2)
- **(markdownlint)** ignore registry/ files by Jeff Dickey in [dd8f47e](https://github.com/jdx/mise/commit/dd8f47e31574e103215b81ee07d19456066c4dfc)
- **(mega-linter)** ignore registry/ files by Jeff Dickey in [427574f](https://github.com/jdx/mise/commit/427574f31f976ee6868306807f54db9169b3140d)
- **(prettier)** ignore registry/ files by Jeff Dickey in [188c0e4](https://github.com/jdx/mise/commit/188c0e4480dc52eaf4ee782d887bd136f0c6dc42)
- **(python)** added debug info when no precompiled version is found by jdx in [9c00917](https://github.com/jdx/mise/commit/9c00917d4efc5153abd954ad8f23cc81757be62f)
- **(registry)** auto-update registry subtree by Jeff Dickey in [fba690c](https://github.com/jdx/mise/commit/fba690c2185e6c66256203f42ee538e13fba6b83)
- **(release)** fixing registry autosync by Jeff Dickey in [266004b](https://github.com/jdx/mise/commit/266004b038588d791d3cefb11ab0094cdf79a929)
- **(release-plz)** push registry subtree changes by Jeff Dickey in [076c822](https://github.com/jdx/mise/commit/076c82228b3d79d7e9926c602825d9dbd101bd06)
- **(renovate)** disable lock file maintenance by Jeff Dickey in [f919db4](https://github.com/jdx/mise/commit/f919db4588f11b7bd2ba951b77ecf70c15ecbcc4)
- Add 'registry/' from commit 'c5d91ebfbf1b7a03203e8442a3f6348c41ce086d' by Jeff Dickey in [d6d46d0](https://github.com/jdx/mise/commit/d6d46d004b02b8dd2947da2314604c870f1221c8)

## [2024.4.1](https://github.com/jdx/mise/compare/v2024.4.0..v2024.4.1) - 2024-04-08

### 🐛 Bug Fixes

- **(doctor)** sort missing shims by Jeff Dickey in [f12d335](https://github.com/jdx/mise/commit/f12d3359b054c9b31a785ed9fdc41e20b317ddb4)
- **(uninstall)** fix uninstall completions by jdx in [2738afd](https://github.com/jdx/mise/commit/2738afd45afdf9864a3badda13d8d1075f5a30bd)

### 🧪 Testing

- **(audit)** removed workflow since dependabot is already doing this by Jeff Dickey in [9138e2e](https://github.com/jdx/mise/commit/9138e2e6ec09a36c7791c9a6b4e5f7ab138fcb63)
- **(mega-linter)** disable RUST_CLIPPY (slow) by Jeff Dickey in [8c64153](https://github.com/jdx/mise/commit/8c64153f7c5b3990d3b857b55fb0a94e7c11a6fc)

### 📦️ Dependency Updates

- bump h2 from 0.3.25 to 0.3.26 by dependabot[bot] in [996759a](https://github.com/jdx/mise/commit/996759a1498640c0229be98b639ea8ec04fa5a0c)

## [2024.4.0](https://github.com/jdx/mise/compare/v2024.3.11..v2024.4.0) - 2024-04-02

### 🐛 Bug Fixes

- **(python)** install python when pip is disabled outside virtualenv by Gabriel Dugny in [a8ac6cd](https://github.com/jdx/mise/commit/a8ac6cdca6d3322b6d406fa32d27c89ec488a9ee)

### 🔍 Other Changes

- **(release)** only save 1 build cache by Jeff Dickey in [f37f11d](https://github.com/jdx/mise/commit/f37f11dd56cb30c1df30d4a2a3df37290ce95a0b)
- **(release-plz)** rebuild release branch daily by Jeff Dickey in [3606d96](https://github.com/jdx/mise/commit/3606d9687ec205754269f7402a7f8095533627ae)
- Move logic to set current directory before loading other config by Josh Bode in [4113012](https://github.com/jdx/mise/commit/4113012adbfd8c912bb34e66ee2e652cf4e25b56)

## [2024.3.11](https://github.com/jdx/mise/compare/v2024.3.10..v2024.3.11) - 2024-03-30

### 🚀 Features

- **(task)** extend mise tasks output by Roland Schaer in [104307c](https://github.com/jdx/mise/commit/104307cd4b9539ee6ef36a18a79253d008a86bdf)

### 🐛 Bug Fixes

- **(self-update)** respect yes setting in config by Jeff Dickey in [b4c4608](https://github.com/jdx/mise/commit/b4c4608ff2dbbde071e10acf6931204acf6d7d40)

### 📚 Documentation

- **(changelog)** fix commit message for releases by Jeff Dickey in [646df55](https://github.com/jdx/mise/commit/646df55f0627c80099026849dc235a8c3076a8e3)
- **(changelog)** fix commit message for releases by Jeff Dickey in [00d8728](https://github.com/jdx/mise/commit/00d87283181467e73b01b27179c096bb08203619)
- **(changelog)** fix commit message for releases by Jeff Dickey in [c5612f9](https://github.com/jdx/mise/commit/c5612f90b4e47bdf12ee74e7d33412e3c0b6184c)

### 🔍 Other Changes

- **(audit)** added workflow by Jeff Dickey in [9263fb4](https://github.com/jdx/mise/commit/9263fb4e1bc374145d9eff609e025559f9d4d7d1)
- **(deny)** remove multiple-versions warnings by Jeff Dickey in [efa133e](https://github.com/jdx/mise/commit/efa133e1fad5bc97c44f04494e5ce7cb9ccc3033)
- **(release-plz)** improve caching by Jeff Dickey in [97c79ee](https://github.com/jdx/mise/commit/97c79ee394c4ae3106cfd4dcfe5ed771b4330d19)
- **(release-plz)** use actions-rust-lang/setup-rust-toolchain@v1 by Jeff Dickey in [4813288](https://github.com/jdx/mise/commit/481328895a91eeae0d9a03fc1f0c18b211b491ab)
- **(test)** improve caching by Jeff Dickey in [ac919a1](https://github.com/jdx/mise/commit/ac919a1db9e8c03fc92a3077cf04edfda6bb971c)
- **(test)** only run lint-fix on main repo by Jeff Dickey in [aee7694](https://github.com/jdx/mise/commit/aee7694b47341baaba9fa5ef628f9540c6f93d72)

## [2024.3.10](https://github.com/jdx/mise/compare/v2024.3.9..v2024.3.10) - 2024-03-30

### 🐛 Bug Fixes

- use correct type for --cd by Jeff Dickey in [cf4f03e](https://github.com/jdx/mise/commit/cf4f03ed0145c5678e1ecbdb98c4426c9428d29a)

### 🚜 Refactor

- completions command by jdx in [cd13f49](https://github.com/jdx/mise/commit/cd13f491d78e7ed0278a1114531c57f9ab9677d6)

### 📚 Documentation

- improve CHANGELOG by jdx in [3393f5d](https://github.com/jdx/mise/commit/3393f5d9dcb474c643338193de493dca98fac8f4)
- improve CHANGELOG by jdx in [46825f5](https://github.com/jdx/mise/commit/46825f592f8bacd1c6d619bac3274c6f808c18f5)
- remove duplicate PR labels in CHANGELOG by Jeff Dickey in [a3b27ef](https://github.com/jdx/mise/commit/a3b27efc37191f8be106345586cab08055ea476f)

## [2024.3.9](https://github.com/jdx/mise/compare/v2024.3.8..v2024.3.9) - 2024-03-24

### 🐛 Bug Fixes

- **(task)** script tasks don't pick up alias from comments by Roland Schaer in [7e9b4b7](https://github.com/jdx/mise/commit/7e9b4b7fe17530fd9f5358bb4ccbaabc58576c3a)
- downgrade reqwest to fix self-update by Jeff Dickey in [2f0820b](https://github.com/jdx/mise/commit/2f0820b8b0438f5224c6b2689f51f43b7f907bf5)

### 📦️ Dependency Updates

- update rust crate rayon to 1.10.0 by renovate[bot] in [fba1ed3](https://github.com/jdx/mise/commit/fba1ed38e6f00ea8b5a149237c69fab06cc98cbf)

## [2024.3.8](https://github.com/jdx/mise/compare/v2024.3.7..v2024.3.8) - 2024-03-23

### 🚀 Features

- use http2 for reqwest by jdx in [7ac7198](https://github.com/jdx/mise/commit/7ac71985e1a7060e2adfc0c1d9a3e70a2fba09c9)

### 🐛 Bug Fixes

- **(nu)** Gracefully handle missing `$env.config` by Texas Toland in [770e00b](https://github.com/jdx/mise/commit/770e00b8a541097544de1d1ef1c753acd0fdbf21)
- Apple x64 version of mise doesn't work by Roland Schaer in [0c0074a](https://github.com/jdx/mise/commit/0c0074a1607f55fbff33115bd33dc1c4f8c7cf4e)

### 🧪 Testing

- fix warnings by Jeff Dickey in [f0604a3](https://github.com/jdx/mise/commit/f0604a3224d5081012101d5266879c6d0af0d39d)

### 🔍 Other Changes

- automatically bump minor version if month/year changes by mise-en-dev in [96ad08d](https://github.com/jdx/mise/commit/96ad08d8acb6b7a4eff0be2f49022080d10b9b71)
- updated cargo-deny config by jdx in [02c7e5c](https://github.com/jdx/mise/commit/02c7e5c262a428477d8c12db2d6c59b8d90b367f)
- fix version set by Jeff Dickey in [2be7fe5](https://github.com/jdx/mise/commit/2be7fe51c0fb9f66c43cd6e940f4eb18ee83c822)

### 📦️ Dependency Updates

- update rust crate toml_edit to 0.22.9 by renovate[bot] in [ffe7dac](https://github.com/jdx/mise/commit/ffe7daced11cd8992720054fa49d962d8f534c6e)
- update rust crate toml to 0.8.12 by renovate[bot] in [3c3dc5e](https://github.com/jdx/mise/commit/3c3dc5e0562fc6c75921473a1d8e55101c0e3344)
- update rust crate indexmap to 2.2.6 by renovate[bot] in [8d645ac](https://github.com/jdx/mise/commit/8d645ac04cb094b8c6381155e9a30e2e519efa33)
- update rust crate usage-lib to 0.1.17 by renovate[bot] in [a629649](https://github.com/jdx/mise/commit/a629649d7232c14469dc417a54c281dfa9c33ad0)
- update rust crate regex to 1.10.4 by renovate[bot] in [1813109](https://github.com/jdx/mise/commit/181310943b92b82b0bd73bcaf64368762757057b)
- update rust crate which to 6.0.1 by renovate[bot] in [9fdec13](https://github.com/jdx/mise/commit/9fdec1307cbfdbeeee1634d52b0ec640e23615ed)
- update rust crate indoc to 2.0.5 by renovate[bot] in [905c841](https://github.com/jdx/mise/commit/905c841df05809633da86d30a73c55b1eea3b049)
- update rust crate versions to 6.2.0 by renovate[bot] in [1d5669e](https://github.com/jdx/mise/commit/1d5669ee6ccef04c40ec1c7d92b8a0a5a9b51596)
- update rust crate reqwest to 0.12.1 by renovate[bot] in [25530af](https://github.com/jdx/mise/commit/25530af3f0e36c4186d2c115707ee6c149ad5cb5)

## [2024.3.7](https://github.com/jdx/mise/compare/v2024.3.6..v2024.3.7) - 2024-03-21

### 🐛 Bug Fixes

- **(task)** tasks not working in system config by Roland Schaer in [d5df98c](https://github.com/jdx/mise/commit/d5df98cd0b05a5bc0f0cb1cb83883138f56a109d)
- **(xonsh)** `shell` subcommand for xonsh by yggdr in [9153411](https://github.com/jdx/mise/commit/9153411b1e8a5bff8de4f71dda5656e297d4b79e)
- jq Installed Using x86_64 on Apple Silicon using mise by Roland Schaer in [2ea978d](https://github.com/jdx/mise/commit/2ea978d0f16ae06fa0749dcbee837662dc0484e6)

### 📚 Documentation

- **(changelog)** improve styling by Jeff Dickey in [403033d](https://github.com/jdx/mise/commit/403033d269f88aa0c1e571e5613231eca84fbaac)
- **(changelog)** improve styling by Jeff Dickey in [cf4811b](https://github.com/jdx/mise/commit/cf4811b0cfa16d7c002e155539eac7a8d5c3912a)

### 🎨 Styling

- format default_shorthands.rs by Jeff Dickey in [a8ea813](https://github.com/jdx/mise/commit/a8ea81337ffd9cfd9201cc49d6a64ba93e10a9a7)

### 🧪 Testing

- install python/poetry at the same time by Jeff Dickey in [08a3304](https://github.com/jdx/mise/commit/08a33048b92a8ce3b551d0f7e39a28ac0bc29f07)

### 🔍 Other Changes

- **(release-plz)** use different bot email by Jeff Dickey in [59b814f](https://github.com/jdx/mise/commit/59b814fae7eedd6565286a6865b6539e2c058a36)
- **(release-plz)** sign release git tags by Jeff Dickey in [8ce5d37](https://github.com/jdx/mise/commit/8ce5d371515d287b8e5a5ccdbddeafa6e5d18952)
- **(test)** run all e2e tests on the release pr by Jeff Dickey in [f21c84b](https://github.com/jdx/mise/commit/f21c84b5683e986b93cf2f3f16c120a7168aacba)
- **(test)** run all e2e tests on the release pr by Jeff Dickey in [cf19dc5](https://github.com/jdx/mise/commit/cf19dc5eac9245a780a9135f7483e431ef686f69)
- **(test)** skip aur/aur-bin on release PR by Jeff Dickey in [9ddb424](https://github.com/jdx/mise/commit/9ddb424c133452d4cb1e4304c263ff74ca65811b)
- Refactor Nushell script by Texas Toland in [4f13c63](https://github.com/jdx/mise/commit/4f13c6329f358bb0ff9dc750984ddeef14d1f4e4)
- rust 1.78 deprecation warning fixes by jdx in [cdc7ba0](https://github.com/jdx/mise/commit/cdc7ba0aafdc467451be667a038c01af9c79a981)
- Update a few phrases for mise install by Erick Guan in [283967e](https://github.com/jdx/mise/commit/283967e840e08baec1d3efec2305fb825df5ee82)
- fix caching by Jeff Dickey in [62cb250](https://github.com/jdx/mise/commit/62cb250007c443dc25e72292b178c5f51cda413c)

## [2024.3.6](https://github.com/jdx/mise/compare/v2024.3.2..v2024.3.6) - 2024-03-17

### 🚀 Features

- very basic dependency support by jdx in [7a53a44](https://github.com/jdx/mise/commit/7a53a44c5bbbea7eed281536d869ec4f39de2527)

### 🐛 Bug Fixes

- update shorthand for rabbitmq by Roland Schaer in [d232859](https://github.com/jdx/mise/commit/d232859b5334462a84df8f1f0b4189576712f571)
- display error message from calling usage by jdx in [63fc69b](https://github.com/jdx/mise/commit/63fc69bc751e6ed182243a6021995821d5f4611e)
- automatically trust config files in CI by jdx in [80b340d](https://github.com/jdx/mise/commit/80b340d8f4a548caa71685a6fca925e2657345dc)

### 🚜 Refactor

- move lint tasks from just to mise by Jeff Dickey in [4f78a8c](https://github.com/jdx/mise/commit/4f78a8cb648246e3f204b426c57662076cc17d5d)

### 📚 Documentation

- **(changelog)** use github handles by Jeff Dickey in [b5ef2f7](https://github.com/jdx/mise/commit/b5ef2f7976e04bf11889062181fc32574eff834a)

### 🎨 Styling

- add mise tasks to editorconfig by Jeff Dickey in [dae8ece](https://github.com/jdx/mise/commit/dae8ece2d891100f86cecea5920bc423e0f4d053)
- run lint-fix which has changed slightly by Jeff Dickey in [6e8dd2f](https://github.com/jdx/mise/commit/6e8dd2fe24adf6d44a17a460c1054738e58f4306)
- apply editorconfig changes by Jeff Dickey in [962bed0](https://github.com/jdx/mise/commit/962bed061ab9218f679f20aa5c53e905981133e0)
- new git-cliff format by Jeff Dickey in [854a4fa](https://github.com/jdx/mise/commit/854a4fae9255968887dc0b0647c993f633666442)
- ignore CHANGELOG.md style by Jeff Dickey in [790cb91](https://github.com/jdx/mise/commit/790cb91a210f5d1d37f4c933798c1802583db753)

### 🧪 Testing

- **(mega-linter)** do not use js-standard linter by Jeff Dickey in [6b63346](https://github.com/jdx/mise/commit/6b63346bdd985964bc824eff03973d2d58d1ad28)
- **(mega-linter)** ignore CHANGELOG.md by Jeff Dickey in [b63b3ac](https://github.com/jdx/mise/commit/b63b3aca3c597ee95db80613b2ea8ca19f0e74c3)

### 🔍 Other Changes

- **(release-plz)** removed some debugging logic by Jeff Dickey in [f7d7bea](https://github.com/jdx/mise/commit/f7d7bea616c13b31318f2e7da287aa71face8e57)
- **(release-plz)** show actual version in PR body by Jeff Dickey in [e1ef708](https://github.com/jdx/mise/commit/e1ef708745e79bd019c77740820daefca5491b2e)
- **(release-plz)** tweaking logic to prevent extra PR by Jeff Dickey in [8673000](https://github.com/jdx/mise/commit/86730008cd2f60d2767296f97175805225c83951)
- **(release-plz)** make logic work for calver by Jeff Dickey in [890c919](https://github.com/jdx/mise/commit/890c919081f984f3d506c2b1d2712c8cff6f5e6b)
- **(release-plz)** make logic work for calver by Jeff Dickey in [bb5a178](https://github.com/jdx/mise/commit/bb5a178b0642416d0e3dac8a9162a9f0732cf146)
- **(release-plz)** fix git diffs by Jeff Dickey in [6c7e779](https://github.com/jdx/mise/commit/6c7e77944a24b289aaba887f64b7f3c63cb9e5ab)
- **(release-plz)** create gh release by Jeff Dickey in [f9ff369](https://github.com/jdx/mise/commit/f9ff369eb1176e31044fc463fdca08397def5a81)
- **(release-plz)** fixing gpg key by Jeff Dickey in [8286ded](https://github.com/jdx/mise/commit/8286ded8297b858e7136831e75e4c37fa49e6186)
- **(release-plz)** fixing gpg key by Jeff Dickey in [abb1dfe](https://github.com/jdx/mise/commit/abb1dfed78e49cf2bee4a137e92879ffd7f2fb03)
- **(release-plz)** do not publish a new release PR immediately by Jeff Dickey in [b3ae753](https://github.com/jdx/mise/commit/b3ae753fdde1fef17b4f13a1ecc8b23cb1da575c)
- **(release-plz)** prefix versions with "v" by Jeff Dickey in [3354b55](https://github.com/jdx/mise/commit/3354b551adab7082d5cc533e5d9d0bfe272958b4)
- **(test)** cache mise installed tools by Jeff Dickey in [0e433b9](https://github.com/jdx/mise/commit/0e433b975a5d8c28ae5c0cbd86d3b19e03146a83)
- Update .mega-linter.yml by jdx in [831831c](https://github.com/jdx/mise/commit/831831c057d37826b9c34edec659e9836e616ad2)
- add --json flag by jdx in [ec8dbdf](https://github.com/jdx/mise/commit/ec8dbdf0659a73ba64ca8a5bd1bf0e021fce0b4b)
- cargo update by Jeff Dickey in [6391239](https://github.com/jdx/mise/commit/639123930eec8e057de7da790cb71d4a2b9e17a2)
- install tools before unit tests by Jeff Dickey in [f7456eb](https://github.com/jdx/mise/commit/f7456ebc539a4b27ec067bc480bc0aba1466e55b)
- added git-cliff by Jeff Dickey in [0ccdf36](https://github.com/jdx/mise/commit/0ccdf36df153ddc3ac1a2714ee9b4a2116dfc918)
- ensure `mise install` is run before lint-fix by Jeff Dickey in [e8a172f](https://github.com/jdx/mise/commit/e8a172f98ebc837619f3766777e489f3b99f36f4)
- added release-plz workflow by jdx in [83fe1ec](https://github.com/jdx/mise/commit/83fe1ecc266caf094fc1cfb251ef1c0cc35afe1b)
- set gpg key by Jeff Dickey in [467097f](https://github.com/jdx/mise/commit/467097f925053a27f0ede2a506e894562d191a09)
- temporarily disable self-update test by Jeff Dickey in [5cb39a4](https://github.com/jdx/mise/commit/5cb39a4259f332e5bccec082f1d7cd6127da5f55)

### 📦️ Dependency Updates

- update rust crate clap to 4.5.3 by renovate[bot] in [ea8e1c1](https://github.com/jdx/mise/commit/ea8e1c19a91db0939d5e42cdd1cd4c0d19309b9e)
- update rust crate color-eyre to 0.6.3 by renovate[bot] in [36d659d](https://github.com/jdx/mise/commit/36d659d9dfd4dc1672381e2d9b849c9093ec8d82)
- update rust crate thiserror to 1.0.58 by renovate[bot] in [6b24985](https://github.com/jdx/mise/commit/6b249853f0aa675450025871943e195080cc5d08)
- update rust crate strum to 0.26.2 by renovate[bot] in [5edf8dd](https://github.com/jdx/mise/commit/5edf8dd3c6569bdbcb43c9ea9a73345752add662)
- update rust crate toml_edit to 0.22.7 by renovate[bot] in [4187cd7](https://github.com/jdx/mise/commit/4187cd79681972bb42bd66437ca43bb0380aa9b2)
- update rust crate toml to 0.8.11 by renovate[bot] in [3f41b84](https://github.com/jdx/mise/commit/3f41b84ac36baeb2f436a0c51d300a4618f6a24d)
- update rust crate usage-lib to 0.1.10 by renovate[bot] in [03aca86](https://github.com/jdx/mise/commit/03aca867a372067a8107f7f5346f77491eab74f5)
- update rust crate usage-lib to 0.1.12 by renovate[bot] in [0603dfb](https://github.com/jdx/mise/commit/0603dfb019e5d25b4baa04d9f6ad95656f347391)

## [2024.3.2](https://github.com/jdx/mise/compare/v2024.3.1..v2024.3.2) - 2024-03-15

### 🚀 Features

- **(task)** add option to show hidden tasks in dependency tree by Roland Schaer in [b90ffea](https://github.com/jdx/mise/commit/b90ffea2dc2ee6628e78da84b4118572a3cb9938)

### 🐛 Bug Fixes

- **(go)** go backend supports versions prefixed with 'v' by Roland Schaer in [668acc3](https://github.com/jdx/mise/commit/668acc3e6431fdd6734f8a0f726d5d8a0d4ce687)
- **(npm)** mise use -g npm:yarn@latest installs wrong version by Roland Schaer in [b7a9067](https://github.com/jdx/mise/commit/b7a90677507b5d5bd8aec1a677cf61adc5288cad)
- **(task)** document task.hide by Roland Schaer in [ac829f0](https://github.com/jdx/mise/commit/ac829f093d62875e2715ef4c1c5c134ffdad7932)
- watch env._.source files by Nicolas Géniteau in [5863a19](https://github.com/jdx/mise/commit/5863a191fbf8a25b60632e71a120395256ac8933)
- prepend virtualenv path rather than append by Kalvin C in [5c9e82e](https://github.com/jdx/mise/commit/5c9e82ececcf5e5e0965b093cd45f46b9267e06f)

### 🔍 Other Changes

- bump rust version by Jeff Dickey in [0cd890c](https://github.com/jdx/mise/commit/0cd890c04a511b8b82e1e605810ae1081e44fccc)

### 📦️ Dependency Updates

- update rust crate chrono to 0.4.35 by renovate[bot] in [13a09d6](https://github.com/jdx/mise/commit/13a09d608dc4914736bd3742866e373909653f9a)
- update rust crate clap to 4.5.2 by renovate[bot] in [5562fcf](https://github.com/jdx/mise/commit/5562fcface3ea2fbb99b66545346871a055a6140)
- update softprops/action-gh-release action to v2 by renovate[bot] in [ba30c38](https://github.com/jdx/mise/commit/ba30c38865c1cf9cc8a3e99b2bc97381d501cb9c)
- update rust crate simplelog to 0.12.2 by renovate[bot] in [58930c9](https://github.com/jdx/mise/commit/58930c9c09eedd990d1e698120ffc7c2c0228c86)
- update rust crate reqwest to 0.11.26 by renovate[bot] in [8dcf9d2](https://github.com/jdx/mise/commit/8dcf9d2139f5b4a11053e0fe8c713e61143f886d)

## [2024.3.1](https://github.com/jdx/mise/compare/v2024.2.19..v2024.3.1) - 2024-03-04

### 🐛 Bug Fixes

- **(java)** sdkmanrc zulu JVMs are missing in mise by Roland Schaer in [4a529c0](https://github.com/jdx/mise/commit/4a529c02824392fe54b2618f3f740d01876bd4b3)

### 🔍 Other Changes

- added back "test:e2e" task by Jeff Dickey in [16e7da0](https://github.com/jdx/mise/commit/16e7da08fc135166e0f44e64d44fb3b3325943aa)
- Tiny grammar fix by Markus Nölle in [bf1ccee](https://github.com/jdx/mise/commit/bf1cceefe4a1d90d4b87d4d2c187701617fe82a4)

### 📦️ Dependency Updates

- bump mio from 0.8.10 to 0.8.11 by dependabot[bot] in [398fc96](https://github.com/jdx/mise/commit/398fc96a41b12d18d855edf8623a7e0632cb73bd)
- update rust crate insta to 1.36.1 by renovate[bot] in [f71cecf](https://github.com/jdx/mise/commit/f71cecfb3dd7be3842d42f42974136ec63b5f8f2)
- update rust crate walkdir to 2.5.0 by renovate[bot] in [3e685db](https://github.com/jdx/mise/commit/3e685db42496fee41df1e2b1dc65dc63227adf62)
- update rust crate indexmap to 2.2.5 by renovate[bot] in [05d89f3](https://github.com/jdx/mise/commit/05d89f36a9fcd9699dae472435063379a51d91a7)
- update rust crate log to 0.4.21 by renovate[bot] in [e3423fe](https://github.com/jdx/mise/commit/e3423fe76da9b1d79ca0ad1a1ace7d8098ba15ac)
- update rust crate tempfile to 3.10.1 by renovate[bot] in [a10e965](https://github.com/jdx/mise/commit/a10e96578f395d60268d1b777a123871c5a3cf51)
- update rust crate rayon to 1.9.0 by renovate[bot] in [b49b8ab](https://github.com/jdx/mise/commit/b49b8ab5dcf9a5343d216fa278f09b492e4770fa)
- update rust crate base64 to 0.22.0 by renovate[bot] in [88daa7b](https://github.com/jdx/mise/commit/88daa7ba78c800ac1d64b3f5aec27539b978cc00)
- update rust crate ctor to 0.2.7 by renovate[bot] in [5999984](https://github.com/jdx/mise/commit/599998483ab012803a563c5ad56eeff23a78c0d4)

## [2024.2.19](https://github.com/jdx/mise/compare/v2024.2.18..v2024.2.19) - 2024-02-28

### 🔍 Other Changes

- simplify tasks in .mise.toml by Jeff Dickey in [5e371e1](https://github.com/jdx/mise/commit/5e371e1d911a08e12ead28dcb14f8436ee4b5ef3)
- Fix MUSL check by Stephen Alderman in [0f7a1eb](https://github.com/jdx/mise/commit/0f7a1ebab63464e0b655cda770d9846701af9e46)
- use normal mise data dir in justfile by jdx in [1014d82](https://github.com/jdx/mise/commit/1014d820a451ab19cc32d552ffbc750fc9fab47f)

## [2024.2.18](https://github.com/jdx/mise/compare/v2024.2.17..v2024.2.18) - 2024-02-24

### 📚 Documentation

- make README logo link to site by Justin "J.R." Hill in [4adac60](https://github.com/jdx/mise/commit/4adac60c41767bb18b479ce2532324bf33d1c946)

### 🔍 Other Changes

- Update mise.json - fix missing_tools type by Felix Salazar in [431eca5](https://github.com/jdx/mise/commit/431eca587c92df417fa008c81f3851be7ac8a27c)
- added env._.python.venv directive by jdx in [055dd80](https://github.com/jdx/mise/commit/055dd80ed190bcd811836a78654ca2d08c427f79)
- auto-install plugins by Jeff Dickey in [3b665e2](https://github.com/jdx/mise/commit/3b665e238baad818aef8f66c74733d6c4e518312)

### 📦️ Dependency Updates

- update rust crate assert_cmd to 2.0.14 by renovate[bot] in [5d5dce0](https://github.com/jdx/mise/commit/5d5dce02c2855a9b53273eb627698d4216540abe)
- update rust crate serde_json to 1.0.114 by renovate[bot] in [9ef42c3](https://github.com/jdx/mise/commit/9ef42c352b659abda396c736de372c2e3b8e7276)
- update rust crate openssl to 0.10.64 by renovate[bot] in [507e265](https://github.com/jdx/mise/commit/507e2658ce7e59654fd19b24537a6a46babf0741)
- update rust crate demand to 1.1.0 by renovate[bot] in [03a7fd5](https://github.com/jdx/mise/commit/03a7fd50de92106331ad1b61d915f7c088d307f8)
- update serde monorepo to 1.0.197 by renovate[bot] in [b5d3f19](https://github.com/jdx/mise/commit/b5d3f19fe13f02acfd03c5bba571a22743dc92b4)
- update rust crate insta to 1.35.1 by renovate[bot] in [1f359a6](https://github.com/jdx/mise/commit/1f359a60e8a2e1aadb39aba1f5dc8dcbf0eee0ee)

## [2024.2.17](https://github.com/jdx/mise/compare/v2024.2.16..v2024.2.17) - 2024-02-22

### 🐛 Bug Fixes

- **(bun)** install bunx symlink by Justin "J.R." Hill in [28d4154](https://github.com/jdx/mise/commit/28d4154daa35015dc4e38fad1804301c3a2704ce)
- **(go)** reflect on proper path for `GOROOT` by Waldemar Heinze in [aed9563](https://github.com/jdx/mise/commit/aed9563a15e8107b61697a69aa2dff6252624faa)
- allow go forge to install SHA versions when no tagged versions present by Andrew Pantuso in [0958953](https://github.com/jdx/mise/commit/095895346e01b77b89454b95f538c1bb53b7aa98)

### 🚜 Refactor

- auto-try miseprintln macro by Jeff Dickey in [1d0fb78](https://github.com/jdx/mise/commit/1d0fb78377720fac356171ebd8d6cbf29a2f0ad6)

### 📚 Documentation

- add missing alt text by Waldemar Heinze in [0c7e69b](https://github.com/jdx/mise/commit/0c7e69b0a8483f218236f3e58a949f48c375940c)
- improve formatting/colors by Jeff Dickey in [5c6e4dc](https://github.com/jdx/mise/commit/5c6e4dc79828b96e5cfb35865a9176670c8f6737)
- revamped output by jdx in [54a5620](https://github.com/jdx/mise/commit/54a56208b3b8d4bac1d2e544d11e5a3d86685b17)

### 🧪 Testing

- **(integration)** introduce rust based integration suite by Andrew Pantuso in [6c656f8](https://github.com/jdx/mise/commit/6c656f8ce447bd41aa8d08ce5e1ed14bd0031490)

### 🔍 Other Changes

- Update README.md by jdx in [05869d9](https://github.com/jdx/mise/commit/05869d986f9b8543aec760f14a8539ce9ba288b3)
- cargo up by Jeff Dickey in [0d716d8](https://github.com/jdx/mise/commit/0d716d862600e0c59b8d4269e48385bf911164b1)
- downgrade openssl due to build failures by Jeff Dickey in [8c282b8](https://github.com/jdx/mise/commit/8c282b8a8786c726ed93a733aaf605529e19b172)
- Revert "cargo up" by Jeff Dickey in [6fb1fa7](https://github.com/jdx/mise/commit/6fb1fa75cdf8abf6e344e30308685238e9dd5570)
- cargo up (minus cc) by Jeff Dickey in [6142403](https://github.com/jdx/mise/commit/6142403894db91b39279e3544bef595bd17c631a)
- Retry with https if request fails by Grant G in [3417560](https://github.com/jdx/mise/commit/341756087c091848a46d14279203f96183d339e3)

### 📦️ Dependency Updates

- update rust crate usage-lib to 0.1.9 by renovate[bot] in [3cf1232](https://github.com/jdx/mise/commit/3cf123204f447f4e4ae046c3f1ad77895c55165c)
- update rust crate indexmap to 2.2.3 by renovate[bot] in [5a4851c](https://github.com/jdx/mise/commit/5a4851c5ecc1ae1ff144cbaa5089ffe97a522d71)
- update rust crate toml_edit to 0.22.6 by renovate[bot] in [b024a7a](https://github.com/jdx/mise/commit/b024a7a1fac796bfe95508a94f163c6d46ea4aa5)
- update rust crate demand to 1.0.2 by renovate[bot] in [48acf7d](https://github.com/jdx/mise/commit/48acf7dc4e681fd3c6d6e5e963b68907c4b765e2)
- update rust crate clap to 4.5.1 by renovate[bot] in [bf8ff55](https://github.com/jdx/mise/commit/bf8ff551d066b6a2c976687f6862cb7e2d50f231)

## [2024.2.16](https://github.com/jdx/mise/compare/v2024.2.15..v2024.2.16) - 2024-02-15

### 🔍 Other Changes

- use dash compatible syntax by Jeff Dickey in [10dbf54](https://github.com/jdx/mise/commit/10dbf54650b9ed90eb4a9ba86fe5499db23357d8)
- cargo up by Jeff Dickey in [7a02ac3](https://github.com/jdx/mise/commit/7a02ac3cfe4de715f807a0c1f27ac63cf840cf55)

## [2024.2.15](https://github.com/jdx/mise/compare/v2024.2.14..v2024.2.15) - 2024-02-13

### 🔍 Other Changes

- fish command_not_found handler fix by jdx in [b581b9d](https://github.com/jdx/mise/commit/b581b9dfa38d2235b4ab6dd0412293be526f2125)
- cargo up by Jeff Dickey in [122a9b2](https://github.com/jdx/mise/commit/122a9b25994adf081e25c15df7b22c80c5517126)
- run commit hook on main branch by Jeff Dickey in [7ced699](https://github.com/jdx/mise/commit/7ced699f638716387a3a35935c946d3df26eac49)
- Revert "run commit hook on main branch" by Jeff Dickey in [5ec8a5e](https://github.com/jdx/mise/commit/5ec8a5e343b7a6c181f92cb2d5650fe1b0bc5d50)

### 📦️ Dependency Updates

- update rust crate thiserror to 1.0.57 by renovate[bot] in [c6d4a4a](https://github.com/jdx/mise/commit/c6d4a4a7541cb6d226ebfad6f7eb6b377c64519d)

## [2024.2.14](https://github.com/jdx/mise/compare/v2024.2.13..v2024.2.14) - 2024-02-11

### 🐛 Bug Fixes

- fix completions in linux by Jeff Dickey in [2822554](https://github.com/jdx/mise/commit/2822554d1d876a80df02abdb7e4ad353416f80af)

### 📦️ Dependency Updates

- update rust crate chrono to 0.4.34 by renovate[bot] in [ba96f1f](https://github.com/jdx/mise/commit/ba96f1f6ab2fb4cd537d24532abe23d1e46dad72)

## [2024.2.13](https://github.com/jdx/mise/compare/v2024.2.12..v2024.2.13) - 2024-02-11

### 🐛 Bug Fixes

- fix completion generators if usage is not installed by Jeff Dickey in [e46fe04](https://github.com/jdx/mise/commit/e46fe04d1c50f893b6c2aa55222792faf16be64c)

## [2024.2.12](https://github.com/jdx/mise/compare/v2024.2.11..v2024.2.12) - 2024-02-11

### 🔍 Other Changes

- install usage via cargo-binstall by Jeff Dickey in [f3a0117](https://github.com/jdx/mise/commit/f3a0117fea9307d11f2df1540efe6761eec13b66)

### 📦️ Dependency Updates

- update rust crate usage-lib to 0.1.8 by renovate[bot] in [af9b541](https://github.com/jdx/mise/commit/af9b541cf73ba6d57acf65b2118cc06a30f482fc)

## [2024.2.11](https://github.com/jdx/mise/compare/v2024.2.10..v2024.2.11) - 2024-02-10

### 🔍 Other Changes

- default fish+bash to use usage for completions by Jeff Dickey in [8399b1f](https://github.com/jdx/mise/commit/8399b1fdbc7e7b507f6e2137d77c685f70b4345d)
- add usage to CI by Jeff Dickey in [0bc48ed](https://github.com/jdx/mise/commit/0bc48eddb7ca38f1e13bcbf2286d4e01041a9fc8)
- add usage to CI by Jeff Dickey in [4eba7c0](https://github.com/jdx/mise/commit/4eba7c026baa52055d5b5925bb9e5acf37f209af)

### 📦️ Dependency Updates

- update rust crate indicatif to 0.17.8 by renovate[bot] in [775db38](https://github.com/jdx/mise/commit/775db385feec0112eb7a2b54d0285a51aa6bb1c3)

## [2024.2.10](https://github.com/jdx/mise/compare/v2024.2.9..v2024.2.10) - 2024-02-10

### 🔍 Other Changes

- usage by jdx in [409f218](https://github.com/jdx/mise/commit/409f218163224cdf572fe1ce787e5dc64226b657)
- cargo up by Jeff Dickey in [4292537](https://github.com/jdx/mise/commit/42925377ba5b06d9d9f5402fca3b197b60acda82)

### 📦️ Dependency Updates

- update rust crate clap to 4.5.0 by renovate[bot] in [6ac66a8](https://github.com/jdx/mise/commit/6ac66a888537bb167e421db5fc6def3acd51af52)
- update rust crate clap_complete to 4.5.0 by renovate[bot] in [544b986](https://github.com/jdx/mise/commit/544b986cc188e73485ee43f16cc26e087888170f)
- update rust crate clap_mangen to 0.2.20 by renovate[bot] in [15f9810](https://github.com/jdx/mise/commit/15f9810261e03f80e5d9a3feb5f8452eb1f6e480)
- update rust crate tempfile to 3.10.0 by renovate[bot] in [a14a320](https://github.com/jdx/mise/commit/a14a320bf1854e3aeaf60d33f2115f2e9ff32cbb)
- update rust crate either to 1.10.0 by renovate[bot] in [6caaf77](https://github.com/jdx/mise/commit/6caaf77749644c38affbfb28152b7cf65ed4e14e)
- update rust crate toml to 0.8.10 by renovate[bot] in [3f615ca](https://github.com/jdx/mise/commit/3f615ca03206c601414a0f465fad8b10bc441ad3)
- update rust crate toml_edit to 0.22.4 by renovate[bot] in [2023664](https://github.com/jdx/mise/commit/2023664ad3b8387ee49a68df2802da441b75b9b6)

## [2024.2.9](https://github.com/jdx/mise/compare/v2024.2.8..v2024.2.9) - 2024-02-09

### 🔍 Other Changes

- bump msrv for clap compatibility by Jeff Dickey in [8a9a284](https://github.com/jdx/mise/commit/8a9a284f1520500a361c1bc2f4db09648a49acd2)

## [2024.2.8](https://github.com/jdx/mise/compare/v2024.2.7..v2024.2.8) - 2024-02-09

### 🐛 Bug Fixes

- fix support for tera templates in tool version strings by jdx in [68ff913](https://github.com/jdx/mise/commit/68ff913de2ee58986154202691b3c306048cf0c1)

### 📚 Documentation

- docs by Jeff Dickey in [e291cc3](https://github.com/jdx/mise/commit/e291cc3802d027d9738b5060d9c68be8b20269e3)

### 🔍 Other Changes

- ignore non-executable tasks by jdx in [a334924](https://github.com/jdx/mise/commit/a3349240efb5a281a3895a9883f6ddc20d1af315)
- save space by Jeff Dickey in [638a426](https://github.com/jdx/mise/commit/638a426e636d65f83f7cd1e415c8aba2a71fe562)
- GOROOT/GOBIN/GOPATH changes by jdx in [786220c](https://github.com/jdx/mise/commit/786220c6178625980bdcc61403c32db19d51360f)
- save space by Jeff Dickey in [0c59c59](https://github.com/jdx/mise/commit/0c59c5980987f300ef6f3468c9a4d7cead2e1995)

## [2024.2.7](https://github.com/jdx/mise/compare/v2024.2.6..v2024.2.7) - 2024-02-08

### 🐛 Bug Fixes

- fix task loading by jdx in [fce42d7](https://github.com/jdx/mise/commit/fce42d776327c0b8c00c32fc48a4e8c47644efff)

### 🔍 Other Changes

- support global file tasks by jdx in [f288b40](https://github.com/jdx/mise/commit/f288b409c56a7fb0160de3c0d60075576dcf5995)
- add installed/active flags by jdx in [d8efa0e](https://github.com/jdx/mise/commit/d8efa0e49a8b30e46905aacc1592d35ce0364acb)
- fix command not found handler by Jeff Dickey in [a30842b](https://github.com/jdx/mise/commit/a30842b5062caca6d07b68307d66ebf376ff01c8)

## [2024.2.6](https://github.com/jdx/mise/compare/v2024.2.5..v2024.2.6) - 2024-02-07

### 🔍 Other Changes

- calm io by jdx in [0dd07e7](https://github.com/jdx/mise/commit/0dd07e7cb7c9ed1d4a507d0a99bcc439b9638a71)
- use OnceLock where possible by Jeff Dickey in [92a3e87](https://github.com/jdx/mise/commit/92a3e87b578cc2e7af0b23b5244246a38be3584b)
- automatically try https if http fails by jdx in [fb9fdf9](https://github.com/jdx/mise/commit/fb9fdf976e1516c77ab68f27c79aa6313fab8a83)
- added optional pre-commit hook by Jeff Dickey in [ec03744](https://github.com/jdx/mise/commit/ec0374480d2b94e49fa8e06edbe929e6f6981951)
- reuse existing command_not_found handler by jdx in [521c31e](https://github.com/jdx/mise/commit/521c31eb2877d5fdb7f7460f7d9006321a09a097)

## [2024.2.5](https://github.com/jdx/mise/compare/v2024.2.4..v2024.2.5) - 2024-02-06

### 🐛 Bug Fixes

- fix lint issues in rust 1.77.0-beta.1 by Jeff Dickey in [cb9ab2d](https://github.com/jdx/mise/commit/cb9ab2de6c6d99cb747a3ef1b90dc2e4e84d0a0a)

### 📚 Documentation

- add some info by jdx in [6e8a97f](https://github.com/jdx/mise/commit/6e8a97f2e10f81f3c3546bd4dce45ac4718f5382)
- cli help by Jeff Dickey in [6a004a7](https://github.com/jdx/mise/commit/6a004a723d93cc3a253321ab9b83058dea6c6c89)

### 🔍 Other Changes

- use serde to parse tools by jdx in [1a7b3f0](https://github.com/jdx/mise/commit/1a7b3f00735a11a84727d830c975ac49afc21722)
- support "false" env vars by jdx in [d959790](https://github.com/jdx/mise/commit/d9597906d796900f751a1dc01a39b3942655ddcd)
- add dotenv paths to watch files by jdx in [d15ea44](https://github.com/jdx/mise/commit/d15ea44c8146429ee655b5404c94fa1c5c0e1d9e)

### 📦️ Dependency Updates

- update rust crate itertools to 0.12.1 by renovate[bot] in [2292c11](https://github.com/jdx/mise/commit/2292c1152fc037e71f5def24cea32bf1f3c3ffb2)
- update nick-fields/retry action to v3 by renovate[bot] in [8996bd8](https://github.com/jdx/mise/commit/8996bd849a649277136dccde263c6d4d36c8267f)
- update rust crate toml_edit to 0.21.1 by renovate[bot] in [0e2ead1](https://github.com/jdx/mise/commit/0e2ead1fd03219982c2d8629014d9460fe48081c)
- update rust crate toml to 0.8.9 by renovate[bot] in [831af03](https://github.com/jdx/mise/commit/831af03d68f0a383f85cb671c6fdfa4d2d322b4c)
- update peter-evans/create-pull-request action to v6 by renovate[bot] in [ded7e82](https://github.com/jdx/mise/commit/ded7e827650a293e962649b73cef7f013917d1cc)
- update rust crate serde_json to 1.0.113 by renovate[bot] in [9a75c7d](https://github.com/jdx/mise/commit/9a75c7d508fcd56a503ccfc527700e5dd674fe56)
- update rust crate reqwest to 0.11.24 by renovate[bot] in [7f01f34](https://github.com/jdx/mise/commit/7f01f345447d694e754a72330967a58085b0a706)

## [2024.2.4](https://github.com/jdx/mise/compare/v2024.2.3..v2024.2.4) - 2024-02-03

### 🐛 Bug Fixes

- **(tasks)** fix parsing of alias attribute by Andrew Pantuso in [a43f40b](https://github.com/jdx/mise/commit/a43f40bdf9b9898789db0125e139df8b29045021)

### 📦️ Dependency Updates

- update rust crate clap_mangen to 0.2.19 by renovate[bot] in [e2e9af7](https://github.com/jdx/mise/commit/e2e9af7c7c52efeab4896445c45e89f17c92d9a2)
- update rust crate clap_complete to 4.4.10 by renovate[bot] in [5159491](https://github.com/jdx/mise/commit/51594917d1860c434aeeb0095e64ad247518c858)
- update rust crate eyre to 0.6.12 by renovate[bot] in [b64b0a8](https://github.com/jdx/mise/commit/b64b0a8a34d40529b51a6909120e08a4db47ac49)
- update rust crate indexmap to 2.2.2 by renovate[bot] in [96afcd3](https://github.com/jdx/mise/commit/96afcd302d97f844de2a5956acd691aeee6189bd)

## [2024.2.3](https://github.com/jdx/mise/compare/v2024.2.2..v2024.2.3) - 2024-02-02

### 🔍 Other Changes

- show curl progress during install.sh by Jeff Dickey in [9e786e8](https://github.com/jdx/mise/commit/9e786e842ff8a1ed25b5d65e76f68ec3ce07850c)
- install tools in order listed in config file when --jobs=1 by jdx in [57a6fa6](https://github.com/jdx/mise/commit/57a6fa6e14e3b3799650b1184ee7af35d4778ebc)
- actionlint by Jeff Dickey in [8f067cb](https://github.com/jdx/mise/commit/8f067cb79fa0f11c1f81ffe3496be9a91c28403e)
- remove unused property by Jeff Dickey in [13d7d29](https://github.com/jdx/mise/commit/13d7d29302004c770a51ff5298a261635460962f)
- property should not be public by Jeff Dickey in [fee5b72](https://github.com/jdx/mise/commit/fee5b72fd552e9f00e13fe60f04959eb629147cd)
- remove more unused config file props by Jeff Dickey in [13daec0](https://github.com/jdx/mise/commit/13daec0b4b92b51bd3ed4137cd51f665afd3f007)
- allow _.path to have : delimiters by Jeff Dickey in [be34b76](https://github.com/jdx/mise/commit/be34b768d9c09feda3c59d9a949a40609c294dcf)
- use serde to parse tasks by jdx in [2baa9d9](https://github.com/jdx/mise/commit/2baa9d9aacc47349a04b9ec20bc605ac7f3b24be)
- skip running glob if no patterns by Jeff Dickey in [0eae892](https://github.com/jdx/mise/commit/0eae892c67598c788b7ca6311aaaac075279717b)
- lazy-load toml_edit by jdx in [ae658f8](https://github.com/jdx/mise/commit/ae658f8f307341c2e859f73f44558e9e82709ea9)

## [2024.2.2](https://github.com/jdx/mise/compare/v2024.2.1..v2024.2.2) - 2024-02-02

### 🔍 Other Changes

- minor UI tweak by Jeff Dickey in [fbe2578](https://github.com/jdx/mise/commit/fbe2578e8770c8913e6bb029ea08ce7b18e6db4a)
- ui tweak by Jeff Dickey in [d3748ef](https://github.com/jdx/mise/commit/d3748efb24bb7b7894c5a877e4d49aff1738c0b8)
- clear cache on mise.run by Jeff Dickey in [1d00fbd](https://github.com/jdx/mise/commit/1d00fbdb904ce83737898e4dc2f8ba5edbf2a568)
- download progress bars by jdx in [554e688](https://github.com/jdx/mise/commit/554e688c1383cb7d6ca2cfaf201c51bf2b5c2581)
- improve output of shorthand update script by Jeff Dickey in [0633c07](https://github.com/jdx/mise/commit/0633c0790e0858919f0ac2b2c27a3d2d7b836c8a)

## [2024.2.1](https://github.com/jdx/mise/compare/v2024.2.0..v2024.2.1) - 2024-02-01

### 🐛 Bug Fixes

- fixed ctrlc handler by jdx in [95a50c3](https://github.com/jdx/mise/commit/95a50c34fd53e34821c2f0fa2af0faf5ada75cdd)

### 📚 Documentation

- add "dr" alias by Jeff Dickey in [67e9e30](https://github.com/jdx/mise/commit/67e9e302c979ca16e8e1160e3a7123f08dd1ab82)

### 🔍 Other Changes

- use m1 macs by Jeff Dickey in [98a6d1f](https://github.com/jdx/mise/commit/98a6d1f2441a8fb839f65a5a66d7053bdffef36b)
- add env.mise.source to schame by Vlad Sirenko in [3cc2cb9](https://github.com/jdx/mise/commit/3cc2cb96f5e16e03f0124788d3ae26bee4ecb2d6)
- improve set/ls commands by jdx in [dc0e793](https://github.com/jdx/mise/commit/dc0e793d5584461809bcdc799662184964427b4a)
- Update README.md by jdx in [3412fa1](https://github.com/jdx/mise/commit/3412fa19e40ca66c4a1811a226b29804ed1f4d3b)
- added mise.run by Jeff Dickey in [9ab7159](https://github.com/jdx/mise/commit/9ab71597c6c3cda0ce500fe9174263ed0c940d44)
- use a bodged loop to handle go forge submodules by endigma in [0cb3988](https://github.com/jdx/mise/commit/0cb39882bc1ae6c38b50752eab46770dfb758e61)
- Additional arch install by Thomas Lockney in [bbaf112](https://github.com/jdx/mise/commit/bbaf11248fbdc413f76283bd38ce6e9c9e3a1711)

## [2024.2.0](https://github.com/jdx/mise/compare/v2024.1.35..v2024.2.0) - 2024-02-01

### 🚀 Features

- **(tasks)** make script task dirs configurable by Andrew Pantuso in [90c35ab](https://github.com/jdx/mise/commit/90c35ab8885759c570a31fe73f8fec458d92a7ef)

### 🐛 Bug Fixes

- **(tasks)** prevent dependency cycles by Andrew Pantuso in [08429bb](https://github.com/jdx/mise/commit/08429bbee21d2400282d584cca2c26fc1f469226)

### 🚜 Refactor

- refactor task_config by Jeff Dickey in [7568969](https://github.com/jdx/mise/commit/7568969f281a428c07144d79643b31699b068c54)

### 📚 Documentation

- docker by jdx in [2cd8ad5](https://github.com/jdx/mise/commit/2cd8ad5d61784edaa7f0f234cef3ba41c2fbb47f)
- fix github action by Jeff Dickey in [9adc718](https://github.com/jdx/mise/commit/9adc7186b86a539e6f3e6a358d5822834e8be8fa)
- fix github action by Jeff Dickey in [3849cdb](https://github.com/jdx/mise/commit/3849cdb8d0d4396e32fa9f555d03662efb2c41ab)
- skip cargo-msrv by Jeff Dickey in [ff3a555](https://github.com/jdx/mise/commit/ff3a5559dde35bd47ed072704bf2bc67478ce307)
- fix test runner by Jeff Dickey in [779c484](https://github.com/jdx/mise/commit/779c48491dfc223c2a7c8c80b8396ba9050ec54d)
- fix dev test by Jeff Dickey in [b92566f](https://github.com/jdx/mise/commit/b92566ffc2ccf2336fafddff3bb5dd62536b1f5f)

### 🔍 Other Changes

- tag version in docker by Jeff Dickey in [fda1be6](https://github.com/jdx/mise/commit/fda1be6c61a23361606ce9e87c10d92b5f619344)
- refactor to use BTreeMap instead of sorting by Jeff Dickey in [438e6a4](https://github.com/jdx/mise/commit/438e6a4dec10e17b0cffca1d921acedf7d6db324)
- skip checkout for homebrew bump by Jeff Dickey in [de5e5b6](https://github.com/jdx/mise/commit/de5e5b6b33063e577f53ceb8f8de14b5035c1c4d)
- make missing tool warning more granular by jdx in [6c6afe1](https://github.com/jdx/mise/commit/6c6afe194872030ec0fc3be7f8ffacd9ca71de25)
- default --quiet to error level by Jeff Dickey in [50c1468](https://github.com/jdx/mise/commit/50c146802aaf4f5f0046ccac620712a5338b1860)

## [2024.1.35](https://github.com/jdx/mise/compare/v2024.1.34..v2024.1.35) - 2024-01-31

### 🔍 Other Changes

- use activate_agressive setting by Jeff Dickey in [c8837fe](https://github.com/jdx/mise/commit/c8837fea7605167c9be2e964acbb29a6ba4e48aa)

## [2024.1.34](https://github.com/jdx/mise/compare/v2024.1.33..v2024.1.34) - 2024-01-31

### 🐛 Bug Fixes

- fix bash command not found override by jdx in [2840fc8](https://github.com/jdx/mise/commit/2840fc815826f32c22d41e43fb8cad17d28a89ce)

### 🔍 Other Changes

- build on macos-latest by Jeff Dickey in [3ca3f7e](https://github.com/jdx/mise/commit/3ca3f7eb5fa72b08938262b9665fabc2db650f28)
- removed outdated conditional by jdx in [7f900c4](https://github.com/jdx/mise/commit/7f900c4326ac50ca2773d320e7bd9b2790063b63)
- update CONTRIBUTING.md by Jeff Dickey in [56be60f](https://github.com/jdx/mise/commit/56be60f2dee9398b181f83965d3a1caa8efe7b16)
- label experimental error by Jeff Dickey in [0e38477](https://github.com/jdx/mise/commit/0e3847791d59df8eb36249ff8faf2eb13c287aa3)
- convert more things to mise tasks from just by jdx in [e9b036e](https://github.com/jdx/mise/commit/e9b036e3baef0f83bdf142fe7228917a00f715e9)
- use Cargo.* as source by Jeff Dickey in [ee10dba](https://github.com/jdx/mise/commit/ee10dba7712acb7420ab807331dc5b37216db080)

## [2024.1.33](https://github.com/jdx/mise/compare/v2024.1.32..v2024.1.33) - 2024-01-30

### 🔍 Other Changes

- treat anything not rtx/mise as a shim by Jeff Dickey in [fae51a7](https://github.com/jdx/mise/commit/fae51a7ef38890fbf3f864957e0c0c6f1be0cf65)

## [2024.1.32](https://github.com/jdx/mise/compare/v2024.1.31..v2024.1.32) - 2024-01-30

### 🔍 Other Changes

- added "plugins up" alias" by Jeff Dickey in [f68bf52](https://github.com/jdx/mise/commit/f68bf520fd726544bfbc09ce8fd1035ffc0d7e20)
- fix settings env vars by Jeff Dickey in [b122c19](https://github.com/jdx/mise/commit/b122c19935297a3220c438607798fc7fe52df1c1)
- use compiled python by Jeff Dickey in [d3020cc](https://github.com/jdx/mise/commit/d3020cc26575864a38dbffd530ad1f7ebff64f64)

## [2024.1.31](https://github.com/jdx/mise/compare/v2024.1.30..v2024.1.31) - 2024-01-30

### 🚀 Features

- **(tasks)** add task timing to run command by Andrew Pantuso in [6a16dc0](https://github.com/jdx/mise/commit/6a16dc0fe0beea743ed474eee7f29239887f418d)

### 🐛 Bug Fixes

- properly handle executable shims when getting diffs by Andrew Pantuso in [add7253](https://github.com/jdx/mise/commit/add725381b2e798e6efbdf40ac356e4f02a17dbd)
- fix bash not_found handler by jdx in [838bbaf](https://github.com/jdx/mise/commit/838bbaf61bd80e540146f3fe3f541dc9ae080aa1)

### 🔍 Other Changes

- updated indexmap by Jeff Dickey in [d7cb481](https://github.com/jdx/mise/commit/d7cb4816e9165cde5ac715126a004f924898af0f)
- hide system versions from env/bin_paths by jdx in [8d29b59](https://github.com/jdx/mise/commit/8d29b59fe6344dc129efb15149ceede40e5cb73c)
- codacy badge by Jeff Dickey in [711d6d7](https://github.com/jdx/mise/commit/711d6d7ced808abd4e24b7dc5952085b9132047d)
- codacy badge by Jeff Dickey in [dc76ec4](https://github.com/jdx/mise/commit/dc76ec4288d2b25c37eb2745028f6593c56facf7)
- codacy badge by Jeff Dickey in [2e97b24](https://github.com/jdx/mise/commit/2e97b24540c3f020dbb2a650512dc97f78b3f6f1)
- codacy badge by Jeff Dickey in [711110c](https://github.com/jdx/mise/commit/711110ca510228df421a584b11e7b62e8590be08)
- only show precompiled warning if going to use precompiled by Jeff Dickey in [74fd185](https://github.com/jdx/mise/commit/74fd1852bef8244f2cb4c51b58f11116d10d0c11)
- fix linux precompiled by jdx in [d885c66](https://github.com/jdx/mise/commit/d885c6693f1a6fd4260a6a4313396cd953d9da80)
- clean up e2e tests by Jeff Dickey in [2660406](https://github.com/jdx/mise/commit/2660406a4744e789ab39a58e1732f880dcd26b4d)

### 📦️ Dependency Updates

- update rust crate serde_json to 1.0.112 by renovate[bot] in [b3e0296](https://github.com/jdx/mise/commit/b3e0296939bb8406f59e694d3173bbb7baa9b2f4)
- update serde monorepo to 1.0.196 by renovate[bot] in [7b8a527](https://github.com/jdx/mise/commit/7b8a527c892f740fc5ff034dc43ebca239510fd6)
- update rust crate strum to 0.26.0 by renovate[bot] in [54fd770](https://github.com/jdx/mise/commit/54fd770428133d02c3094ac899243a9bb6a1a442)
- update rust crate strum to 0.26.1 by renovate[bot] in [7ab1166](https://github.com/jdx/mise/commit/7ab1166465ac3e971d4e5dbd86d4397345d2664a)
- update rust crate indexmap to 2.2.0 by renovate[bot] in [986aa0c](https://github.com/jdx/mise/commit/986aa0ccc3b4d6745b5fd3ba300353499785acac)

## [2024.1.30](https://github.com/jdx/mise/compare/v2024.1.29..v2024.1.30) - 2024-01-27

### 🐛 Bug Fixes

- fix mangen by Jeff Dickey in [da2b1c9](https://github.com/jdx/mise/commit/da2b1c9d0bbbeba3e566020c0d48e16f579d8eeb)

### 🔍 Other Changes

- default to precompiled python by Jeff Dickey in [0fac002](https://github.com/jdx/mise/commit/0fac002dbeba699ae8949c3d94e89d08128dae57)

## [2024.1.29](https://github.com/jdx/mise/compare/v2024.1.28..v2024.1.29) - 2024-01-27

### 🔍 Other Changes

- use nodejs/golang for writing to .tool-versions by Jeff Dickey in [14fb790](https://github.com/jdx/mise/commit/14fb790ac9953430794719b38b83c8c2242f1759)
- read system and local config settings by jdx in [6b8e211](https://github.com/jdx/mise/commit/6b8e21146d4918367d9b794a1c76154401768240)

## [2024.1.28](https://github.com/jdx/mise/compare/v2024.1.27..v2024.1.28) - 2024-01-27

### 🔍 Other Changes

- added `env._.source` feature by jdx in [00b756a](https://github.com/jdx/mise/commit/00b756a2d316687a99a755712096e96dd9e27f36)
- force update alpine by Jeff Dickey in [633c3ff](https://github.com/jdx/mise/commit/633c3ffe139c1201f20ce0e7145cb361d547a39a)

### 📦️ Dependency Updates

- update rust crate chrono to 0.4.33 by renovate[bot] in [3ac2cf7](https://github.com/jdx/mise/commit/3ac2cf7bdc010e206d24ed15afef7fb00c45bc6e)
- update rust crate clap_complete to 4.4.9 by renovate[bot] in [292b987](https://github.com/jdx/mise/commit/292b987f67a14397a2b21a183aa0f590c9eeff4e)

## [2024.1.27](https://github.com/jdx/mise/compare/v2024.1.26..v2024.1.27) - 2024-01-26

### 🚀 Features

- **(run)** match tasks to run with glob patterns by Andrew Pantuso in [7b3ae2e](https://github.com/jdx/mise/commit/7b3ae2e7a6f42f23d79586cd7a2e6ddc1f9efa89)
- **(tasks)** unify glob strategy for tasks and dependencies by Andrew Pantuso in [6be2c83](https://github.com/jdx/mise/commit/6be2c83c2ef2d0eccef77b3315033a2613ec8fb3)

### 🐛 Bug Fixes

- fix global config with asdf_compat by jdx in [81aacb0](https://github.com/jdx/mise/commit/81aacb045700ff2a3d2484f36ea04ef795e1720b)

### 📚 Documentation

- display missing/extra shims by jdx in [a4b6418](https://github.com/jdx/mise/commit/a4b641825f28cf6511321c1d28bb997c73b77402)

### 🔍 Other Changes

- pass signals to tasks by jdx in [59c7216](https://github.com/jdx/mise/commit/59c721690fc7fe5ae5f54e3a530f1523114bdba0)
- added settings_message settings by jdx in [91b9fc1](https://github.com/jdx/mise/commit/91b9fc176861ccd202babede7520fbcb4e87ee07)
- resolve env vars in order by jdx in [7dce359](https://github.com/jdx/mise/commit/7dce359a31f06e7f32366ee75c1f975d667000d7)
- parse alias + plugins with serde by jdx in [ab7f771](https://github.com/jdx/mise/commit/ab7f77111470ad669725646fb647dd71f268fe50)

## [2024.1.26](https://github.com/jdx/mise/compare/v2024.1.25..v2024.1.26) - 2024-01-25

### 🚀 Features

- **(doctor)** identify missing/extra shims by Andrew Pantuso in [0737239](https://github.com/jdx/mise/commit/07372390fdc6336856d6f3f6fb18efe03f099715)
- **(tasks)** infer bash task topics from folder structure by Andrew Pantuso in [2d63b59](https://github.com/jdx/mise/commit/2d63b59fd4f4c2a0cecd357f0b25cec3397fff61)

### 🚜 Refactor

- env parsing by jdx in [a5573cc](https://github.com/jdx/mise/commit/a5573ccd5a78f5fed1f449f5c4135ed168c03d51)

### 🔍 Other Changes

- use target_feature to use correct precompiled runtimes by jdx in [578ff24](https://github.com/jdx/mise/commit/578ff24321c6254acadaed4b91498dc03a03911b)
- do not follow symbolic links for trusted paths by jdx in [032e325](https://github.com/jdx/mise/commit/032e325f9f44b80e920c9e4698c17233c7011ca7)
- refactor min_version logic by jdx in [7ce6d3f](https://github.com/jdx/mise/commit/7ce6d3fe52cf5bc3df66748e16703a0a0e5bcbc5)
- sort env vars coming back from exec-env by jdx in [278878e](https://github.com/jdx/mise/commit/278878e69bb4a85e8219fb74aab51e55be651f0a)
- order flags in docs by Jeff Dickey in [1018b56](https://github.com/jdx/mise/commit/1018b5622c3bda4d0d9fa36b4fa9c1143aabd676)
- demand 1.0.0 by Jeff Dickey in [c97bb79](https://github.com/jdx/mise/commit/c97bb7993aa9432ad38879cdc0ab17f251715feb)

## [2024.1.25](https://github.com/jdx/mise/compare/v2024.1.24..v2024.1.25) - 2024-01-24

### 🚀 Features

- **(config)** support arrays of env tables by Andrew Pantuso in [12d87c2](https://github.com/jdx/mise/commit/12d87c215fc292df84484de810ff1975477e2513)
- **(template)** add join_path filter by Andrew Pantuso in [9341810](https://github.com/jdx/mise/commit/9341810203d3e66dd6498400900ad6d6e1eb7c14)
- add other arm targets for cargo-binstall by Yuto Yoshino in [6845239](https://github.com/jdx/mise/commit/6845239648dbd08d097064a519250c32650a60ea)

### 🐛 Bug Fixes

- **(tasks)** prevent implicit globbing of sources/outputs by Andrew Pantuso in [9ac1435](https://github.com/jdx/mise/commit/9ac14357c7f23c00c29da1ada37644609df85234)
- fix release script by Jeff Dickey in [59498ea](https://github.com/jdx/mise/commit/59498ea5a312507535d139957bac90fad2d96ebf)

### 🔍 Other Changes

- updated clap_complete by Jeff Dickey in [4034674](https://github.com/jdx/mise/commit/4034674436f786691e767c6ac09921b06e968a86)
- allow cargo-binstall from mise itself by jdx in [651ec02](https://github.com/jdx/mise/commit/651ec029c52fdcddb00f8f8c13dbbaa2f08426aa)
- Delete lefthook.yml by jdx in [a756db4](https://github.com/jdx/mise/commit/a756db4a34afee4d6ce0fcfea4bc016025d1d188)
- turn back on `cargo update` on release by Jeff Dickey in [51f269a](https://github.com/jdx/mise/commit/51f269a8d07cf1f34f0d237b17b493986aaa864d)

### 📦️ Dependency Updates

- update rust crate regex to 1.10.3 by renovate[bot] in [bcadbf8](https://github.com/jdx/mise/commit/bcadbf8d3835708e208125f2eb1f67920c168bb0)

## [2024.1.24](https://github.com/jdx/mise/compare/v2024.1.23..v2024.1.24) - 2024-01-20

### 🐛 Bug Fixes

- fix cwd error by Jeff Dickey in [1c0bc12](https://github.com/jdx/mise/commit/1c0bc1236fce943ed9b012e95e3cc047cdc38ab0)

### 🔍 Other Changes

- bump demand by Jeff Dickey in [5231179](https://github.com/jdx/mise/commit/523117975bbb9c3211f0f438f55d1d7dc392f8b2)
- do not fail if version parsing fails by Jeff Dickey in [8d39995](https://github.com/jdx/mise/commit/8d39995e615527ba7187b3d25369a506bcb21e0c)
- added --shims by jdx in [73b9b72](https://github.com/jdx/mise/commit/73b9b7244060b0fd32470c9b31f153b1a7ee6a45)
- use `sort -r` instead of `tac` by jdx in [334ee48](https://github.com/jdx/mise/commit/334ee48138448bc5ba320da45c8d60e9cdcec2c2)
- Update README.md by jdx in [f3291d1](https://github.com/jdx/mise/commit/f3291d15f94c0a0cc602c01d5b7b6ef7c3cb60bf)
- fix conflicts by Jeff Dickey in [729de0c](https://github.com/jdx/mise/commit/729de0cb6c27646e30ee7be99d2f478f3431258c)

### 📦️ Dependency Updates

- update actions/cache action to v4 by renovate[bot] in [1bbba92](https://github.com/jdx/mise/commit/1bbba92bd11a82697010f793e3c23b21da387769)
- update rust crate which to v6 by renovate[bot] in [0875293](https://github.com/jdx/mise/commit/08752930c1a0e9aa0961745fc2647489468e10d7)
- update rust crate which to v6 by jdx in [df3dd08](https://github.com/jdx/mise/commit/df3dd08fbdc1843b45a67ba01dfa85cb0097b8a5)

## [2024.1.23](https://github.com/jdx/mise/compare/v2024.1.22..v2024.1.23) - 2024-01-18

### 🐛 Bug Fixes

- fix config_root path by jdx in [205e43a](https://github.com/jdx/mise/commit/205e43ade7e530d46e7b81015959417285039805)

### 🔍 Other Changes

- use mise to get development dependencies by jdx in [d26640f](https://github.com/jdx/mise/commit/d26640f28c9c12fe6d6025d3711c19c7bbd44c5f)
- improve post-plugin-update script by jdx in [383600c](https://github.com/jdx/mise/commit/383600cc7631663fdaae6db9e2ab033db36a3bb8)
- only show select if no task specified by jdx in [8667bc5](https://github.com/jdx/mise/commit/8667bc51dd7af25966e423b4d84992dc8ff4fccf)
- show cursor on ctrl-c by Jeff Dickey in [ebc5fe7](https://github.com/jdx/mise/commit/ebc5fe78bc97ecf99251438e6f305908bb134833)
- fix project_root when using .config/mise.toml or .mise/config.toml by jdx in [f0965ad](https://github.com/jdx/mise/commit/f0965ad57faa36f14adf1809535eae6738f6578c)

## [2024.1.22](https://github.com/jdx/mise/compare/v2024.1.21..v2024.1.22) - 2024-01-17

### 🐛 Bug Fixes

- no panic on missing current dir by Ferenc Tamás in [9c4b7fb](https://github.com/jdx/mise/commit/9c4b7fb652cab04864841b02d59ccd7581a1e805)
- fix not_found handler when command start with "--" by jdx in [c8e02e4](https://github.com/jdx/mise/commit/c8e02e425c05dfb19a10f04b6baa57f2640fd991)
- always load global configs by Ferenc Tamás in [fd9da12](https://github.com/jdx/mise/commit/fd9da129e093332113ca10098e14bf21660017db)

### 🔍 Other Changes

- remove dirs_next in favor of simpler home crate by jdx in [152e6b7](https://github.com/jdx/mise/commit/152e6b78eb65a249e73a22860cadc532dfdc5e2a)
- rename internal MISE_BIN env var to __MISE_BIN by jdx in [ec609a2](https://github.com/jdx/mise/commit/ec609a2b845976f5ab8421790130b59c7eb38a9a)
- allow using templates in task files by jdx in [1a8481c](https://github.com/jdx/mise/commit/1a8481cdd655068ce6c10f87b022d1880bf70bb5)
- support array of commands directly by jdx in [62679b3](https://github.com/jdx/mise/commit/62679b3b25281b53710f195d698269a2883c8626)
- updated dependencies by jdx in [8863a21](https://github.com/jdx/mise/commit/8863a21eebcc2e8c1621bee8223dd3245438bac8)
- add support for installing directly with go modules by endigma in [05f76ec](https://github.com/jdx/mise/commit/05f76ecfbad3382ec6d42584784216d5e14aaf48)
- ensure forge type matches by jdx in [b46fa17](https://github.com/jdx/mise/commit/b46fa170f4fdfe4b39342389fd537aa8a4d15b10)

## [2024.1.21](https://github.com/jdx/mise/compare/v2024.1.20..v2024.1.21) - 2024-01-15

### 🐛 Bug Fixes

- bail out of task suggestion if there are no tasks by Roland Schaer in [d52d2ca](https://github.com/jdx/mise/commit/d52d2ca064f3ceed70ed96db3912cda909d02c23)
- fixed urls by Jeff Dickey in [22265e5](https://github.com/jdx/mise/commit/22265e5ea6d3b9498eb11eef14a77c3ba46cae03)
- fixed urls by Jeff Dickey in [8c24e48](https://github.com/jdx/mise/commit/8c24e4873c6fd4f5b94f959c17795ff9f910da0f)
- fixed deprecated plugins migrate by Jeff Dickey in [94bfc46](https://github.com/jdx/mise/commit/94bfc46bccb99144a542d5b678b33537a36bea6c)

### 🔍 Other Changes

- Update README.md by jdx in [74c5210](https://github.com/jdx/mise/commit/74c5210b9f35bd05ac53a417ac5e152dda256a9e)

## [2024.1.20](https://github.com/jdx/mise/compare/v2024.1.19..v2024.1.20) - 2024-01-14

### 🚀 Features

- add command to print task dependency tree by Roland Schaer in [ef2cc0c](https://github.com/jdx/mise/commit/ef2cc0c9e536838e0cf89cc1cc2b67b017517cdb)
- add completions for task deps command by Roland Schaer in [e0ba235](https://github.com/jdx/mise/commit/e0ba235d8127a488f29f74dd07a714489ed6bab3)
- add interactive selection for tasks if task was not found by Roland Schaer in [6a93748](https://github.com/jdx/mise/commit/6a93748572e61c18ec1a798e8e658a72a574ae50)

### 🔍 Other Changes

- enable stdin under interleaved by Jeff Dickey in [b6dfb31](https://github.com/jdx/mise/commit/b6dfb311e412e119e137186d6143644d018a6cfc)
- re-enable standalone test by Jeff Dickey in [7e4e79b](https://github.com/jdx/mise/commit/7e4e79bcdcc541027bc3ea2fccc11fb0f0c07a5d)

## [2024.1.19](https://github.com/jdx/mise/compare/v2024.1.18..v2024.1.19) - 2024-01-13

### 🐛 Bug Fixes

- fix loading npm from mise by jdx in [c24976f](https://github.com/jdx/mise/commit/c24976f30c92572ab15f68cfa8fa6417ca3b38da)

### 🚜 Refactor

- remove PluginName type alias by Jeff Dickey in [dedb762](https://github.com/jdx/mise/commit/dedb7624ad4708ce0434a963737a17754075d3a0)
- rename Plugin trait to Forge by Jeff Dickey in [ec4efea](https://github.com/jdx/mise/commit/ec4efea054626f9451bb54831abdd95ff98c64d1)
- clean up arg imports by Jeff Dickey in [5091fc6](https://github.com/jdx/mise/commit/5091fc6b04fd1e4795bbd636772c30432b825ef3)
- clean up arg imports by jdx in [5e36828](https://github.com/jdx/mise/commit/5e368289e5a80913aa000564bb500e69d6b3008f)

### 📚 Documentation

- document npm/cargo by Jeff Dickey in [d1e6e4b](https://github.com/jdx/mise/commit/d1e6e4b951637762d3562a89996a2bb3422c3341)

### 🔍 Other Changes

- Update nushell.rs - Add explicit spread by Brian Heise in [36fdc55](https://github.com/jdx/mise/commit/36fdc55af0676271fced65be8d50046e673a3516)
- allow using "env._.file|env._.path" instead of "env.mise.file|env.mise.path" by Jeff Dickey in [cf93693](https://github.com/jdx/mise/commit/cf936931201d6597ad556bd17556d47dc3d125c6)
- added "forge" infra by jdx in [db82e62](https://github.com/jdx/mise/commit/db82e62c920c1cc48cc6f32993b5bf0de1ce7441)
- added support for installing directly from npm by jdx in [8d678e0](https://github.com/jdx/mise/commit/8d678e0ffb3b21a2874d6984a8d77e3e2372b4c7)
- testing by Jeff Dickey in [2ee66cb](https://github.com/jdx/mise/commit/2ee66cb91837fde144bf7acbb1028372c1cd7d9a)
- skip slow cargo test if TEST_ALL is not set by jdx in [90f4ed8](https://github.com/jdx/mise/commit/90f4ed85735a6b6c3d2a0439ed8a654bd3ced515)

## [2024.1.18](https://github.com/jdx/mise/compare/v2024.1.17..v2024.1.18) - 2024-01-12

### 🔍 Other Changes

- Revert "miette " by Jeff Dickey in [dbf7d01](https://github.com/jdx/mise/commit/dbf7d01a66ee53c584d7568b0e7c39b73262d85c)
- fix mise-docs publishing by Jeff Dickey in [1dcac6d](https://github.com/jdx/mise/commit/1dcac6d4e05c80b56d1371f434776057d3ca9dc7)
- temporarily disable standalone test by Jeff Dickey in [d4f54ad](https://github.com/jdx/mise/commit/d4f54adbbf840599aeb4229c9330262569b563b5)

## [2024.1.17](https://github.com/jdx/mise/compare/v2024.1.16..v2024.1.17) - 2024-01-12

### 🚜 Refactor

- refactor env_var arg by jdx in [eb08d82](https://github.com/jdx/mise/commit/eb08d82beb54f999788119ee5c36bb88bcdc845d)
- refactor ToolArg by Jeff Dickey in [5b66532](https://github.com/jdx/mise/commit/5b665325e4474f3247242a2d81c860ac17b8af5f)

### 🔍 Other Changes

- Fix missing ASDF_PLUGIN_PATH environment variable by Ryan Egesdahl in [8f1c422](https://github.com/jdx/mise/commit/8f1c422b5c7f9a237403c3ec47814b5d4e51f51a)
- remove warning about moving to settings.toml by Jeff Dickey in [750141e](https://github.com/jdx/mise/commit/750141eff2721e2fbe4ab116952d04b67d2ee187)
- renovate config by Jeff Dickey in [abebe93](https://github.com/jdx/mise/commit/abebe93a3e9d79846cc566b2664451817b1ac47b)
- read from config.toml by jdx in [cdfda7d](https://github.com/jdx/mise/commit/cdfda7d7e94f82f091bf394d50f28aaa6139dbf2)
- use less aggressive PATH modifications by default by Jeff Dickey in [07e1921](https://github.com/jdx/mise/commit/07e19212053bdaf4ea2ca3968e3f3559d6f49668)
- move release from justfile by Jeff Dickey in [89c9271](https://github.com/jdx/mise/commit/89c927198cfa66f332929acb9e692296dda13e2e)
- bump local version of shfmt by Jeff Dickey in [d4be898](https://github.com/jdx/mise/commit/d4be89844aa462e199d5c7278661650c22d126da)

## [2024.1.16](https://github.com/jdx/mise/compare/v2024.1.15..v2024.1.16) - 2024-01-11

### 🐛 Bug Fixes

- fix test suite on alpine by jdx in [09e04f9](https://github.com/jdx/mise/commit/09e04f92da4d2a6c7f9530dfb76747fb6dec3df1)

### 🔍 Other Changes

- do not panic if precompiled arch/os is not supported by jdx in [3d12e5a](https://github.com/jdx/mise/commit/3d12e5aeac333e6a98425ec6031016dfd792ac6e)
- improvements by jdx in [f386503](https://github.com/jdx/mise/commit/f386503d54bd32726e9ded773360abd5d8d00ab8)

## [2024.1.15](https://github.com/jdx/mise/compare/v2024.1.14..v2024.1.15) - 2024-01-10

### 🐛 Bug Fixes

- **(python)** fixes #1419 by HARADA Tomoyuki in [2003c6b](https://github.com/jdx/mise/commit/2003c6b045559421be756db0ca403b1a6d76f64b)

### 🔍 Other Changes

- Update README.md by jdx in [e3ff351](https://github.com/jdx/mise/commit/e3ff351bce362ec5d8bddd9fb9bb13827fce083d)
- rename rtx-vm -> mise-en-dev by Jeff Dickey in [03061f9](https://github.com/jdx/mise/commit/03061f973543c54b5076b26b8611f3ec378e6a61)
- fix some precompiled issues by jdx in [ffb6489](https://github.com/jdx/mise/commit/ffb6489c1b0e54f0caa2e6ca4ddf855469950809)

## [2024.1.14](https://github.com/jdx/mise/compare/v2024.1.13..v2024.1.14) - 2024-01-09

### 🔍 Other Changes

- Correct PATH for python venvs by Ali Kefia in [d78db1e](https://github.com/jdx/mise/commit/d78db1e2e21f09983fe3c95f6c151fa1a8042d3e)
- downgrade rpm dockerfile by Jeff Dickey in [5a0cbe7](https://github.com/jdx/mise/commit/5a0cbe7f250a5d7586c45264e0d4bb1914325748)
- loosen regex for runtime symlink generation by jdx in [9d746de](https://github.com/jdx/mise/commit/9d746de2d04bc2629d5d93c16fb4480232a2e12f)

## [2024.1.13](https://github.com/jdx/mise/compare/v2024.1.12..v2024.1.13) - 2024-01-08

### 🔍 Other Changes

- add path separator by Michael Coulter in [e14901f](https://github.com/jdx/mise/commit/e14901fb9b047ae0fba18b5f85434141fab734d5)
- prevent adding relative/empty paths during activation by Michael Coulter in [576f2b9](https://github.com/jdx/mise/commit/576f2b9d873c3c23457d9f51b1f6071e3a4bbd27)
- handle 404s by jdx in [d0f5aac](https://github.com/jdx/mise/commit/d0f5aacaa188034522b5a75cf438531b73fe0a49)
- allow expanding "~" for trusted_config_paths by jdx in [4aad252](https://github.com/jdx/mise/commit/4aad252f351468922f6a4184a4d2724f22cd4343)
- disallow [settings] header in settings.toml by jdx in [0f5616d](https://github.com/jdx/mise/commit/0f5616d43e791195e187fcb03cfa6092bb6ce434)
- use ~/.tool-versions globally by jdx in [b668c71](https://github.com/jdx/mise/commit/b668c71c846a7e10e9ce1a01740ae782cf4b421c)

## [2024.1.12](https://github.com/jdx/mise/compare/v2024.1.11..v2024.1.12) - 2024-01-07

### 🔍 Other Changes

- added missing settings from `mise settings set` by Jeff Dickey in [8a7880b](https://github.com/jdx/mise/commit/8a7880bc912bbcef874d7428d6b0f7d772715fc5)
- fixed python_compile and all_compile settings by Jeff Dickey in [5ddbf68](https://github.com/jdx/mise/commit/5ddbf68af1f32abbf8cff406a6d17d0898d4c81f)

## [2024.1.11](https://github.com/jdx/mise/compare/v2024.1.10..v2024.1.11) - 2024-01-07

### 🔍 Other Changes

- check min_version field by Jeff Dickey in [8de42a0](https://github.com/jdx/mise/commit/8de42a0be94098c722ba8b9eef8eca505f5838c2)
- add to doctor and fix warning by Jeff Dickey in [fcf9173](https://github.com/jdx/mise/commit/fcf91739bc0241114242afb9e8de6bdf819cd7ba)
- publish schema to r2 by Jeff Dickey in [3576984](https://github.com/jdx/mise/commit/3576984b0ce89910c7bb4ae63a41b8c82381cc44)

## [2024.1.10](https://github.com/jdx/mise/compare/v2024.1.9..v2024.1.10) - 2024-01-07

### 🐛 Bug Fixes

- nix flake build errors by nokazn in [f42759d](https://github.com/jdx/mise/commit/f42759d1cafaa206357e2eeaf3b1843cb80f65fb)

### 🔍 Other Changes

- do not display error if settings is missing by Jeff Dickey in [21cb004](https://github.com/jdx/mise/commit/21cb004402a7bfad2c50dbd56e584555715f1597)

## [2024.1.9](https://github.com/jdx/mise/compare/v2024.1.8..v2024.1.9) - 2024-01-07

### 🔍 Other Changes

- sort settings by Jeff Dickey in [a8c15bb](https://github.com/jdx/mise/commit/a8c15bb6e84a6e49e4d7660ac4923d8eeaac76cf)
- clean up community-developed plugin warning by Jeff Dickey in [92b5188](https://github.com/jdx/mise/commit/92b51884a522dc7991824594e0228f014c7a1413)
- use ~/.config/mise/settings.toml by jdx in [176ce00](https://github.com/jdx/mise/commit/176ce0079b4a9a23be8d7aa3dcda38117b4131c2)
- add support for precompiled binaries by jdx in [128142f](https://github.com/jdx/mise/commit/128142f545f79d23c581eba3f2c0fcc122764134)

## [2024.1.8](https://github.com/jdx/mise/compare/v2024.1.7..v2024.1.8) - 2024-01-06

### 🐛 Bug Fixes

- **(java)** enable macOS integration hint for Zulu distribution by Roland Schaer in [3bfb33e](https://github.com/jdx/mise/commit/3bfb33e2b6ea00c461ccfe32b4f72fc43769b80b)
- fixed config load order by jdx in [46cbe0a](https://github.com/jdx/mise/commit/46cbe0a50cf51efd29beb4f287c295a819ecefb5)

### 🔍 Other Changes

- Add `description` to task object in JSON schema by Gary Coady in [ab6e912](https://github.com/jdx/mise/commit/ab6e912e98b429e79494ae65a9d37c3633bdd0ac)
- added ideavim config by Jeff Dickey in [15cfa1e](https://github.com/jdx/mise/commit/15cfa1eebd18ee77b931b5e4343a4ef1d7c2473f)
- paranoid by jdx in [35c97e7](https://github.com/jdx/mise/commit/35c97e7f90918514025601c3bedbb55f525b012d)
- miette by jdx in [332648d](https://github.com/jdx/mise/commit/332648d5283fa5b6984426919d2fe964df813a58)

## [2024.1.7](https://github.com/jdx/mise/compare/v2024.1.6..v2024.1.7) - 2024-01-05

### 🐛 Bug Fixes

- fixed migration script by Jeff Dickey in [54097ee](https://github.com/jdx/mise/commit/54097eed2050681f6ed74084809a438a70000cab)
- fixed not-found handler by Jeff Dickey in [69f354d](https://github.com/jdx/mise/commit/69f354df0e463edcdcbd12364a88013e5f5029f9)

### 🔍 Other Changes

- show better error when attemping to install core plugin by jdx in [4902791](https://github.com/jdx/mise/commit/49027919665af8a568c60356f8022d757a26e68e)
- read rtx.plugin.toml if it exists by Jeff Dickey in [db19252](https://github.com/jdx/mise/commit/db19252f3c5f23426f2d8c5a899939a575453779)

## [2024.1.6](https://github.com/jdx/mise/compare/v2024.1.5..v2024.1.6) - 2024-01-04

### 🧪 Testing

- fixed elixir test case by Jeff Dickey in [9b596c6](https://github.com/jdx/mise/commit/9b596c6dadcf0f54b3637d10e1885281e1a1b534)

### 🔍 Other Changes

- set CLICOLOR_FORCE=1 and FORCE_COLOR=1 by jdx in [3d2e132](https://github.com/jdx/mise/commit/3d2e132f1df5aa20e9d712df697746ddeea6c465)
- set --interleaved if graph is linear by jdx in [fb2b218](https://github.com/jdx/mise/commit/fb2b218da96a09b1f9db3984aa217c1b11e1a3de)

## [2024.1.5](https://github.com/jdx/mise/compare/v2024.1.4..v2024.1.5) - 2024-01-04

### 🐛 Bug Fixes

- fixed man page by Jeff Dickey in [581b6e8](https://github.com/jdx/mise/commit/581b6e8aa56476d8d184c2cae2bd7657c8690143)
- remove comma from conflicts by Patrick Decat in [38381a6](https://github.com/jdx/mise/commit/38381a69d46a7fa4afd8d3254b2290bc5a28019b)

### 🔍 Other Changes

- skip ruby installs by Jeff Dickey in [c23e467](https://github.com/jdx/mise/commit/c23e467717105e34ac805638dfeb5fcac3f991a2)
- Update README.md to link to rtx page by Silas Baronda in [c375cd8](https://github.com/jdx/mise/commit/c375cd814da4b15dfcdf849071443f01c3b0e6fa)
- use "[" instead of "test" by jdx in [ee6a18c](https://github.com/jdx/mise/commit/ee6a18c1416d51202e046b8703891184daee772e)
- prevent loading multiple times by jdx in [01a20ad](https://github.com/jdx/mise/commit/01a20ad0dd8bb073ac200b5b4459994c77512020)
- use `mise.file`/`mise.path` config by jdx in [fb8a9df](https://github.com/jdx/mise/commit/fb8a9dfbb052ecb770e0ef7ffd4f811f7de522b7)

## [2024.1.4](https://github.com/jdx/mise/compare/v2024.1.3..v2024.1.4) - 2024-01-04

### 🐛 Bug Fixes

- **(java)** use tar.gz archives to enable symlink support by Roland Schaer in [fd3ecdf](https://github.com/jdx/mise/commit/fd3ecdfa1b8198e3c79883afc9f984c49c3aa3a0)

### 🔍 Other Changes

- rtx-plugins -> mise-plugins by Jeff Dickey in [04f55cd](https://github.com/jdx/mise/commit/04f55cd677a3041232887c2f3731d17f775e3627)
- rtx -> mise by Jeff Dickey in [ed794d1](https://github.com/jdx/mise/commit/ed794d15cf035a993e0c286e84dac0335ffe8967)
- add "replaces" field by jdx in [581a1fe](https://github.com/jdx/mise/commit/581a1fec088fdbf90c38dc9e79fc0449df2218a5)
- Add additional conflicts by Malachi Soord in [0b18f02](https://github.com/jdx/mise/commit/0b18f026d31cbf8b297386101ee5ee213d9c82d8)
- docs by Jeff Dickey in [eb73edf](https://github.com/jdx/mise/commit/eb73edfab75d8a2b5bd58be71b2ccbd172b92413)
- demo by jdx in [756c719](https://github.com/jdx/mise/commit/756c719777fc794e49ee707985ca07d66fdaa835)
- fix ssh urls by jdx in [9e252d0](https://github.com/jdx/mise/commit/9e252d0b97a2a6649beff42884dbc5cd4e799c19)

## [2024.1.3](https://github.com/jdx/mise/compare/v2024.1.2..v2024.1.3) - 2024-01-03

### 🔍 Other Changes

- use mise docker containers by Jeff Dickey in [d5d2d39](https://github.com/jdx/mise/commit/d5d2d39aa1a44a6421dff150da42083c4247cff9)
- skip committing docs if no changes by Jeff Dickey in [7f6545c](https://github.com/jdx/mise/commit/7f6545c2630a1f54b864903851c24e68b3da3d2f)
- use ~/.local/bin/mise instead of ~/.local/share/mise/bin/mise by Jeff Dickey in [cd2045d](https://github.com/jdx/mise/commit/cd2045d793c76b9dcf7d26c567cf163a6138f408)

## [2024.1.2](https://github.com/jdx/mise/compare/v2024.1.1..v2024.1.2) - 2024-01-03

### 🔍 Other Changes

- fix venv python path by Jeff Dickey in [e2d50a2](https://github.com/jdx/mise/commit/e2d50a2f25c0c64c207f82e957e691671d52ddbd)

## [2024.1.1](https://github.com/jdx/mise/compare/v2024.1.0..v2024.1.1) - 2024-01-03

### 🐛 Bug Fixes

- fixed email addresses by Jeff Dickey in [b5e9d3c](https://github.com/jdx/mise/commit/b5e9d3cc3a2500c932593d7931647fbc3d972708)
- fixed crate badge by Jeff Dickey in [c4bb224](https://github.com/jdx/mise/commit/c4bb224acb197e9f67eda56a4be3c7f3c5bdcee6)

### 📚 Documentation

- tweak cli reference by Jeff Dickey in [ba5f610](https://github.com/jdx/mise/commit/ba5f6108b1b91952295e4871f63c559ff01c7c64)
- fixed reading settings from config by Jeff Dickey in [a30a5f1](https://github.com/jdx/mise/commit/a30a5f104da41794aa8a2813919f046945ed9ae6)

### 🔍 Other Changes

- rtx -> mise by Jeff Dickey in [9b7975e](https://github.com/jdx/mise/commit/9b7975e5cd43121d22436893acdc7dbfe36ee960)
- readme by Jeff Dickey in [7d3a2ca](https://github.com/jdx/mise/commit/7d3a2ca707a7779041df559bba23bd552ef01775)
- Update README.md by jdx in [884147b](https://github.com/jdx/mise/commit/884147b16e94880e915a53291af21647546d6a04)
- fail on r2 error by Jeff Dickey in [c4011da](https://github.com/jdx/mise/commit/c4011da5261f254f118c3cd5740bbf8d50ac8733)
- update CONTRIBUTING.md by Jeff Dickey in [91e9bef](https://github.com/jdx/mise/commit/91e9befabec3f87dec4f2c6513f52b29ca53f5b8)
- 2024 by Jeff Dickey in [fbcc3ee](https://github.com/jdx/mise/commit/fbcc3ee610f38633e2ce583d9c43fc9df8c4f368)
- auto-publish cli reference to docs by Jeff Dickey in [a2f59c6](https://github.com/jdx/mise/commit/a2f59c6933833e0a2f15066d952ce1119a0928c8)
- fix MISE_ASDF_COMPAT=1 by jdx in [edbdc7c](https://github.com/jdx/mise/commit/edbdc7c448e1db522d1304c004aa36ed0e99f0c4)
- migrate improvements by Jeff Dickey in [2c0ccf4](https://github.com/jdx/mise/commit/2c0ccf43fd23de03c25a872fe6d91f1d63c77c1a)

## [2024.1.0] - 2024-01-02

### 🔍 Other Changes

- added "ev" alias by Jeff Dickey in [8d98b91](https://github.com/jdx/mise/commit/8d98b9158b6dc4d6c36332a5f52061e81cc87d91)
- added "ev" alias by Jeff Dickey in [4bfe580](https://github.com/jdx/mise/commit/4bfe580eef8a8192f621ea729c8013ef141dacf3)
- added RTX_ENV_FILE config by jdx in [484806f](https://github.com/jdx/mise/commit/484806fd980d6c39aaa76e4066b18f54edd35137)
- Update CONTRIBUTING.md by jdx in [0737393](https://github.com/jdx/mise/commit/0737393b7b167fd57d168dfbf886405bb0a8cecb)
- Configure Renovate by renovate[bot] in [0f980b2](https://github.com/jdx/mise/commit/0f980b22382b4da002336f6b456d5181416bf75b)
- consistent dependency versions by Jeff Dickey in [43b37bc](https://github.com/jdx/mise/commit/43b37bc2296460e8b222ab0cbb815ac457717074)
- ignore asdf/nodejs by Jeff Dickey in [acc9a68](https://github.com/jdx/mise/commit/acc9a6803d6d3087a847529baa7d7e341ef46cc2)
- ignore nodenv by Jeff Dickey in [4d921c7](https://github.com/jdx/mise/commit/4d921c7608e4807ae765383253e100763d04bd75)
- tuck away by Jeff Dickey in [4361f03](https://github.com/jdx/mise/commit/4361f0385a82da470cfe47a5044a00ca783c9ddc)
- disable dashboard by Jeff Dickey in [2c569fc](https://github.com/jdx/mise/commit/2c569fc01a77987e6823dc749eb917f1fe5a0cf0)
- disable auto package updates by Jeff Dickey in [e00fb1f](https://github.com/jdx/mise/commit/e00fb1fde649ecc85aa40ac8846f71316d679e54)
- disable dashboard by Jeff Dickey in [400ac0a](https://github.com/jdx/mise/commit/400ac0a0ff64cf5a6846f662df5dc432237e87b2)
- updated description by Jeff Dickey in [83c0ffc](https://github.com/jdx/mise/commit/83c0ffcf210c51228f82e9eb586d09a5ea7933f4)
- rtx -> mise by Jeff Dickey in [e5897d0](https://github.com/jdx/mise/commit/e5897d097c1f90c8a263f0e685a56908e2c023da)

### 📦️ Dependency Updates

- update rust crate indexmap to 2.1 by renovate[bot] in [83f2088](https://github.com/jdx/mise/commit/83f208852f67504727e57187aa5c16d6a4f2e883)
- update rust crate num_cpus to 1.16 by renovate[bot] in [92b664e](https://github.com/jdx/mise/commit/92b664e9d70803243c2ed70882052b113196abf8)
- update rust crate once_cell to 1.19 by renovate[bot] in [4fbb420](https://github.com/jdx/mise/commit/4fbb420ca159030fcee17d0a6890d0b532042e49)
- update rust crate regex to 1.10 by renovate[bot] in [d2458e7](https://github.com/jdx/mise/commit/d2458e7f34ed1c899e950b18458cf437ffcd7c49)
- update rust crate url to 2.5 by renovate[bot] in [4884e23](https://github.com/jdx/mise/commit/4884e233c623c5ffa489373282dceac93e4f3f89)
- update actions/upload-artifact action to v4 by renovate[bot] in [b2fe480](https://github.com/jdx/mise/commit/b2fe4802289fc42f8339922541b683d7365cc6b8)
- update actions/download-artifact action to v4 by renovate[bot] in [1d0bd48](https://github.com/jdx/mise/commit/1d0bd4840f85897b2af5eacb703723073802834c)
- update fedora docker tag to v40 by renovate[bot] in [88f8c83](https://github.com/jdx/mise/commit/88f8c838668a99bba12145dc25973fdcdaf2c4e0)
- update mcr.microsoft.com/devcontainers/rust docker tag to v1 by renovate[bot] in [22efd8d](https://github.com/jdx/mise/commit/22efd8dffbf126c25e70cc0a953b916cb21beb26)
- update stefanzweifel/git-auto-commit-action action to v5 by renovate[bot] in [7078db0](https://github.com/jdx/mise/commit/7078db0243189a4325f2cdcd1910290dc9048ff4)
- update actions/checkout action to v4 by renovate[bot] in [b04ae5c](https://github.com/jdx/mise/commit/b04ae5ca195e5ff0dbc47becb5da0a5958a85ac7)

<!-- generated by git-cliff -->
