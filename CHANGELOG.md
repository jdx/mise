# Changelog

## [2024.9.10](https://github.com/jdx/mise/compare/v2024.9.9..v2024.9.10) - 2024-09-26

### 🚀 Features

- add arguments to file tasks by jdx in [1b20e09](https://github.com/jdx/mise/commit/1b20e09e1f2f5435dc133788861241a879ce92bb)
- added toml cli commands by jdx in [448f91c](https://github.com/jdx/mise/commit/448f91c88a785700144c1bc5287e55ceb069294b)
- mount tasks args/flags via usage by jdx in [1eaa731](https://github.com/jdx/mise/commit/1eaa7316c9eb30a8bba0ef5f7f3cad525d672945)
- added mise info command by jdx in [4880eb5](https://github.com/jdx/mise/commit/4880eb5df36a63d3442332574765e25c3754c2cb)

### 📚 Documentation

- Add tera features for the template documenation by Erick Guan in [496e964](https://github.com/jdx/mise/commit/496e96437a578fc564cde7a5e1f0e7c9731ec378)

### 🔍 Other Changes

- migrate away from deprecated git-cliff syntax by Jeff Dickey in [230897c](https://github.com/jdx/mise/commit/230897c41210502f69ed5c4270f13d6efc416f89)
- pin git-cliff by Jeff Dickey in [b2603b6](https://github.com/jdx/mise/commit/b2603b685ad74adabcb613be351117f0d949635e)
- upgraded usage by jdx in [443d0c7](https://github.com/jdx/mise/commit/443d0c7602e4e656fb515ad22a961ca9896cde2e)
- retry windows-e2e on failure by Jeff Dickey in [fa7ec34](https://github.com/jdx/mise/commit/fa7ec34ea1cd0d8a030a7d253b6e025c04fdd47c)
- retry windows-e2e on failure by Jeff Dickey in [6516f7f](https://github.com/jdx/mise/commit/6516f7ffdbaada7ccb16bdf57a423d852f97a1a1)

## [2024.9.9](https://github.com/jdx/mise/compare/v2024.9.8..v2024.9.9) - 2024-09-25

### 🚀 Features

- added postinstall hook by jdx in [90c72dc](https://github.com/jdx/mise/commit/90c72dccc485cde047e6892d1c3711bfcebabf48)

### 🐛 Bug Fixes

- added nodejs to alpine build by Jeff Dickey in [550f64c](https://github.com/jdx/mise/commit/550f64cb9c0377e4102be24486491a5b9d7947f3)
- bug with exec on windows by jdx in [3a680ef](https://github.com/jdx/mise/commit/3a680ef6404dd0a21460b4910a71da1cacc75fd0)
- only show hints once per execution by jdx in [56b2a4f](https://github.com/jdx/mise/commit/56b2a4f2436075cfe47797d0413fa5edb5a15ca6)
- task args regression by jdx in [3ac0bf5](https://github.com/jdx/mise/commit/3ac0bf589179ea531e2447c3034304dc61fa42e8)
- use correct xdg paths on windows by jdx in [6cb1c60](https://github.com/jdx/mise/commit/6cb1c60d4e86c2b4d5e803b80efb5009f06f2983)

### 🧪 Testing

- added windows e2e tests by jdx in [a6a323e](https://github.com/jdx/mise/commit/a6a323e0d1f837731f84fde142ab212a058133b5)
- added windows e2e tests by jdx in [0aeb7a3](https://github.com/jdx/mise/commit/0aeb7a39fe7760c14f65438b5dca66cfcf4c0155)
- reset by Jeff Dickey in [57d0223](https://github.com/jdx/mise/commit/57d0223ab2dfa731088f1634d0d12a6edd8dd8a6)
- fix mise cache in CI by jdx in [1e8e97e](https://github.com/jdx/mise/commit/1e8e97ecb470279cb62651c3dfb8398b43696ce1)
- allow specifying full e2e test names by jdx in [9f2af75](https://github.com/jdx/mise/commit/9f2af7529ee364139f5cbc8a746453f1c65839ff)
- split windows into windows-unit and windows-e2e by jdx in [22251ce](https://github.com/jdx/mise/commit/22251cebbfe8d3b8024fb4e25b0342c0d07628e3)

### 🔍 Other Changes

- **(docs)** fix `arch()` template doc by cwegener in [e948196](https://github.com/jdx/mise/commit/e948196a5e6244367514d99ed5df4ef6e138182c)

### New Contributors

* @cwegener made their first contribution in [#2644](https://github.com/jdx/mise/pull/2644)

## [2024.9.8](https://github.com/jdx/mise/compare/v2024.9.7..v2024.9.8) - 2024-09-25

### 🚀 Features

- **(node)** allow using node unofficial build flavors by jdx in [98d969d](https://github.com/jdx/mise/commit/98d969dc69ed15cbd383c55ae9d61088e15df21a)
- codegen settings by jdx in [ae24b5e](https://github.com/jdx/mise/commit/ae24b5ec803e5ae4413d91c5c91e15d425fc65dc)

### 🐛 Bug Fixes

- release 2024.9.7 breaks configurations that were using v in version names with go backend by Roland Schaer in [d9f3e7f](https://github.com/jdx/mise/commit/d9f3e7f60b7c3a729db3be6a805b7130f7da73f3)
- add node mirror/flavor to cache key by jdx in [bda6401](https://github.com/jdx/mise/commit/bda6401f79f38fd741b72c33278cc6289dc743d2)

### 📚 Documentation

- Update faq.md by jdx in [9036759](https://github.com/jdx/mise/commit/903675950d3ccc7abb49a40d6794d75d52695e5e)
- Update configuration.md by jdx in [1bc8342](https://github.com/jdx/mise/commit/1bc8342920cfb0259e35e578f68d1ec857420787)
- Update configuration.md by jdx in [8e0d2b2](https://github.com/jdx/mise/commit/8e0d2b2e8bb4cd0f1d63077d50677d33a2599d9b)
- document java shorthand and its limitations by Roland Schaer in [0902d7d](https://github.com/jdx/mise/commit/0902d7dbb11eb7c54cca3e2a11aad0f8afc2bbfe)

### 🔍 Other Changes

- format schema by Jeff Dickey in [418bc24](https://github.com/jdx/mise/commit/418bc24292cacdec0d643a7c93355c0dea550678)
- format schema by Jeff Dickey in [a8f7493](https://github.com/jdx/mise/commit/a8f7493cd63535ae8e46d77545acfecf9a1451b2)

## [2024.9.7](https://github.com/jdx/mise/compare/v2024.9.6..v2024.9.7) - 2024-09-23

### 🚀 Features

- task argument declarations by jdx in [adf6d31](https://github.com/jdx/mise/commit/adf6d318495e1229d114c1dd9a7a77ff9f9fccdc)

### 🐛 Bug Fixes

- **(windows)** node bin path by Jeff Dickey in [eed0ecf](https://github.com/jdx/mise/commit/eed0ecfb528aa1fa04efcadf44afd353db76a7c4)
- **(windows)** fixed npm backend by jdx in [6e6f841](https://github.com/jdx/mise/commit/6e6f841f8f1d15a83f0296a3a79895020a7eb9b6)
- ensure that version is not "latest" in node by Jeff Dickey in [0e196d6](https://github.com/jdx/mise/commit/0e196d6d9c0b0851148ba9894191d766c0386356)
- prevent attempting to use python-build in windows by Jeff Dickey in [e15545b](https://github.com/jdx/mise/commit/e15545bb623da98bae72a41a57fa10ec311ee881)
- skip last modified time test for nix by Zhongcheng Lao in [5f13fd0](https://github.com/jdx/mise/commit/5f13fd0f5a7364b1f3e46544034c4cd24654ec94)
- go backend can't install tools without 'v' prefix in git repo tags by Roland Schaer in [78c2647](https://github.com/jdx/mise/commit/78c264783f6eeaa374b00913fb957ba506f19ce3)
- use "v" prefix first for go backend by Jeff Dickey in [8444597](https://github.com/jdx/mise/commit/8444597add58353f8fc3a84662e7a024a72104c8)

### 📚 Documentation

- Fix Options example in documentation by Gaurav Kumar in [663170b](https://github.com/jdx/mise/commit/663170b7d427170feeab2e6880791aa87012ca34)
- remove reference to cache duration by Jeff Dickey in [bef6086](https://github.com/jdx/mise/commit/bef608633e814927707cd011875ce0bff28aa3d3)

### 🔍 Other Changes

- Update toml-tasks.md by jdx in [9d26963](https://github.com/jdx/mise/commit/9d2696366bd21be47c5a6e25586e7061c0a7838c)
- change prune message to debug-level by Jeff Dickey in [f54dd0d](https://github.com/jdx/mise/commit/f54dd0de830e0249b07cc263707530c6795d512f)

### New Contributors

* @gauravkumar37 made their first contribution in [#2619](https://github.com/jdx/mise/pull/2619)

## [2024.9.6](https://github.com/jdx/mise/compare/v2024.9.5..v2024.9.6) - 2024-09-18

### 🚀 Features

- **(tasks)** allow mise-tasks or .mise-tasks directories by jdx in [3427f77](https://github.com/jdx/mise/commit/3427f771dc90044b38795ff1f7806b09c34911f9)
- **(windows)** added ruby core plugin by jdx in [e4dccb7](https://github.com/jdx/mise/commit/e4dccb7c7405de90f52b841df6c2dadc58ccd524)
- periodically prune old cache files by jdx in [59bba25](https://github.com/jdx/mise/commit/59bba252daa225181301da18641b87c79ec74dc9)
- take npm/cargo backends out of experimental by Jeff Dickey in [5496cef](https://github.com/jdx/mise/commit/5496cef30819a3998a52a8f5e6e2d91cfa3e86b0)

### 🐛 Bug Fixes

- **(ruby)** fixed MISE_RUBY_BUILD_OPTS by jdx in [32e326d](https://github.com/jdx/mise/commit/32e326d8c2b1d16ff6a38aa23f2d2f70e3fe0e38)
- **(windows)** self_update by jdx in [d2c4cf3](https://github.com/jdx/mise/commit/d2c4cf3a7dc0a5a1d28796f18b5eb16c91c02582)
- **(windows)** mise -v by Jeff Dickey in [fcc2d35](https://github.com/jdx/mise/commit/fcc2d354b962aa4fe8cc1b422b96a7e455107adc)
- **(windows)** make tasks work by jdx in [64d3ed6](https://github.com/jdx/mise/commit/64d3ed6ed3acbec744632c03b056a979011867b3)
- **(windows)** mise doctor fixes by jdx in [c2186ce](https://github.com/jdx/mise/commit/c2186ce3548625c78308f9245b8895710164cf47)
- **(windows)** make exec work by jdx in [ed5cc94](https://github.com/jdx/mise/commit/ed5cc949505c42ecd5b49daaaada06def603cc0f)
- **(windows)** fixed shims by jdx in [a92e7bc](https://github.com/jdx/mise/commit/a92e7bc9e64e65d00c14ea0a79721a9e99a80fbe)

### 🧪 Testing

- add macos to CI by jdx in [f8ddf6b](https://github.com/jdx/mise/commit/f8ddf6b0cc4fbb4f491ead23c23f42879c9a3303)

### 🔍 Other Changes

- clean up console output during project linting by jdx in [a1b3355](https://github.com/jdx/mise/commit/a1b335539a0963dc01478d37f6c4a34b6031f369)

## [2024.9.5](https://github.com/jdx/mise/compare/v2024.9.4..v2024.9.5) - 2024-09-17

### 🔍 Other Changes

- change win -> windows by Jeff Dickey in [e45623c](https://github.com/jdx/mise/commit/e45623c88662a11f08db93068ac765efb3813855)

## [2024.9.4](https://github.com/jdx/mise/compare/v2024.9.3..v2024.9.4) - 2024-09-15

### 🚀 Features

- support for global configuration profiles by Roland Schaer in [a48f562](https://github.com/jdx/mise/commit/a48f562211cfbcc120ab8ebdcd2ad1c5e8dfd532)
- add Atmos by mtweeman in [b3705c6](https://github.com/jdx/mise/commit/b3705c6f0840b6b5f448494e2e6e59f536a2e2c2)
- add semver matching in mise templates by Erick Guan in [a2ea77f](https://github.com/jdx/mise/commit/a2ea77f2e06f0caf0e303e75caccf12a31cc4806)
- add rest of tera features for templates by Erick Guan in [146a52f](https://github.com/jdx/mise/commit/146a52fb80948b153c8377aa954e7a2223e4aa8d)

### 🐛 Bug Fixes

- fix a few tera filter error messages by Erick Guan in [c73ecd1](https://github.com/jdx/mise/commit/c73ecd1aa55f6e2c29da5d7314a207af4bb2010f)
- use "windows" instead of "win" by Jeff Dickey in [3327e8c](https://github.com/jdx/mise/commit/3327e8c5eca4dc39529790c4b830fdcca57ebe65)
- fixed release-plz by Jeff Dickey in [bc4fae3](https://github.com/jdx/mise/commit/bc4fae3f1acefdf0fb05f8b97a0ec1703a216f57)
- cannot install truffelruby by Roland Schaer in [0c88ede](https://github.com/jdx/mise/commit/0c88ede5c2fd7128cd8cc23f1373e2d7161af475)

### 📚 Documentation

- wrong version in the README example when install specific version by Roland Schaer in [d161afe](https://github.com/jdx/mise/commit/d161afe843694020f22fc981537f81e5ea7f2896)

### 🔍 Other Changes

- fix nightly lint warning by Jeff Dickey in [0a41dc6](https://github.com/jdx/mise/commit/0a41dc67aa7b1faf6301a67386eabb3ebd31ed4d)

### New Contributors

* @mtweeman made their first contribution in [#2577](https://github.com/jdx/mise/pull/2577)

## [2024.9.3](https://github.com/jdx/mise/compare/v2024.9.2..v2024.9.3) - 2024-09-12

### 🐛 Bug Fixes

- Look for `-P` or `--profile` to get mise environment. by Gary Coady in [32956cb](https://github.com/jdx/mise/commit/32956cba52ab5d5caf102af4275ad548007b4d50)
- use consistent names for tera platform information by jdx in [c6eac80](https://github.com/jdx/mise/commit/c6eac802aade4bb2d39e2c60bce9ff4ca933b800)

### 📚 Documentation

- added contributors to readme by jdx in [16cccdd](https://github.com/jdx/mise/commit/16cccdd821a2b78f6a2144ea82ea16f09cacf84f)
- pdate getting-started.md by Francesc Esplugas in [ce9e3e5](https://github.com/jdx/mise/commit/ce9e3e59d07a9bc2b6a0752230f4bcf5a03d263b)

### New Contributors

* @fesplugas made their first contribution in [#2570](https://github.com/jdx/mise/pull/2570)

## [2024.9.2](https://github.com/jdx/mise/compare/v2024.9.1..v2024.9.2) - 2024-09-11

### 🚀 Features

- implement a few tera functions for mise toml config by Erick Guan in [542a78d](https://github.com/jdx/mise/commit/542a78d8f8c233a7f4a16b60b71d7a8216f0b91b)

### 🐛 Bug Fixes

- ruby ls-remote not showing alternative implementations by Roland Schaer in [338d24f](https://github.com/jdx/mise/commit/338d24f41054a8c2573d6ec1a679e0f6ed56c457)
- cannot disable hints during Zsh completion by Roland Schaer in [0830c06](https://github.com/jdx/mise/commit/0830c06b4aca10147ecd113b26806741b834e614)

### 📚 Documentation

- Create zig.md by Albert in [b16d158](https://github.com/jdx/mise/commit/b16d158b14e66a0809609c744c3a047cbee6ff7b)

## [2024.9.1](https://github.com/jdx/mise/compare/v2024.9.0..v2024.9.1) - 2024-09-10

### 🚀 Features

- add global --env argument by Roland Schaer in [71f4036](https://github.com/jdx/mise/commit/71f4036dc9429cc68dbbb0d32a3e000afe756c85)

### 🐛 Bug Fixes

- mise plugins ls command should ignore .DS_Store file on macOS by Roland Schaer in [1d739c3](https://github.com/jdx/mise/commit/1d739c3e0b92802dd21d2e2ca045eb86d127d241)
- mise deactivate zsh does not work, but mise deactivate does by Roland Schaer in [a5af19a](https://github.com/jdx/mise/commit/a5af19a787dc501e08246fb04dbdf0bde9439273)

### 🔍 Other Changes

- ignore RUSTSEC-2024-0370 by Jeff Dickey in [2de83b1](https://github.com/jdx/mise/commit/2de83b1af9e4c408886e8d756e734fa70f62e477)

## [2024.9.0](https://github.com/jdx/mise/compare/v2024.8.15..v2024.9.0) - 2024-09-05

### 🚀 Features

- **(pipx)** add support for specifying package extras by Antonio Molner Domenech in [6730416](https://github.com/jdx/mise/commit/6730416d39a8b95418670d9853e0cc1e4ad5cafc)
- mise hints by Roland Schaer in [595c70b](https://github.com/jdx/mise/commit/595c70b469720a12b181a72343ae9c18fc461edb)

### 🐛 Bug Fixes

- **(asdf)** handle plugin URLs with trailing slash by Jeff Dickey in [4541fbe](https://github.com/jdx/mise/commit/4541fbe92700d6598a03479aa77278bfbc7035c0)
- ls-remote doesn't support @sub-X style versions by Roland Schaer in [d341e4e](https://github.com/jdx/mise/commit/d341e4e80cf7dceccef2bb53e0a64c9f13cea885)
- ensure `mise install` installs missing runtimes listed in `mise ls` by Stan Hu in [fbe5bba](https://github.com/jdx/mise/commit/fbe5bba3e1e82e0103723b42b3904ea93eef9aa9)
- Ensure dependencies are available for alternative backends by David Brownman in [0fd5c11](https://github.com/jdx/mise/commit/0fd5c11c2501b230ecd86f4459b2f7629b02d72d)
- tweak hints by Jeff Dickey in [732fc58](https://github.com/jdx/mise/commit/732fc58deda43339e5dd0e5136c5b71dab275232)
- Update fish.rs for activation of mise by Shobhit Aggarawal in [84a2929](https://github.com/jdx/mise/commit/84a29292278c26588c227e73d0bbb13b28ece381)
- resolve issue with prefixed dependencies by jdx in [1ed5997](https://github.com/jdx/mise/commit/1ed5997a660f681e262608e5d221279b87168dfe)

### 🧪 Testing

- added e2e env vars by Jeff Dickey in [585024f](https://github.com/jdx/mise/commit/585024fc882559beeef65c5a9772f40c8e1b5235)

### New Contributors

* @Shobhit0109 made their first contribution in [#2542](https://github.com/jdx/mise/pull/2542)
* @xavdid made their first contribution in [#2532](https://github.com/jdx/mise/pull/2532)
* @stanhu made their first contribution in [#2524](https://github.com/jdx/mise/pull/2524)

## [2024.8.15](https://github.com/jdx/mise/compare/v2024.8.14..v2024.8.15) - 2024-08-28

### 🚀 Features

- **(vfox)** added aliases like vfox:cmake -> vfox:version-fox/vfox-cmake by Jeff Dickey in [0654f6c](https://github.com/jdx/mise/commit/0654f6c3a4b15640fa64d5cee6cfec3f2f08a580)
- use https-only in paranoid by Jeff Dickey in [ad9f959](https://github.com/jdx/mise/commit/ad9f959ee0c7659596d8c3dc4e9ca33e82fec041)
- make use_versions_host a setting by Jeff Dickey in [d9d4d23](https://github.com/jdx/mise/commit/d9d4d23c56d1181c2ed5b7ce62475b9c469b9da4)

### 🐛 Bug Fixes

- **(pipx)** allow using uv provided by mise by Jeff Dickey in [b608a73](https://github.com/jdx/mise/commit/b608a736d94f3a97c4cd06226b194bef41b15d9d)
- **(pipx)** order pipx github releases correctly by Jeff Dickey in [054ff85](https://github.com/jdx/mise/commit/054ff85609d385ac0cd07dd9014a7bd6fe376271)
- **(vfox)** ensure plugin is installed before listing env vars by Jeff Dickey in [914d0b4](https://github.com/jdx/mise/commit/914d0b4ca78ef8144158ecde6158f7276879f4d8)
- correct aur fish completion directory by Jeff Dickey in [ff2f652](https://github.com/jdx/mise/commit/ff2f652a1419ccc7be2fd212a3275491e7f5cd49)

### 📚 Documentation

- **(readme)** remove failing green color by David Girón in [c1e6e73](https://github.com/jdx/mise/commit/c1e6e7306397b11bf0346a2e89f79ef03c83fb54)
- document vfox by Jeff Dickey in [1084fc4](https://github.com/jdx/mise/commit/1084fc4896eec08921481ba24e263cda0b760875)
- render registry with asdf and not vfox by Jeff Dickey in [cc6876e](https://github.com/jdx/mise/commit/cc6876e51534d24a485c9f07568d11954bc87f90)
- document python_venv_auto_create by Jeff Dickey in [7fc7bd8](https://github.com/jdx/mise/commit/7fc7bd8c479e23242ce9afa071a99870cda40270)
- removed some references to rtx by Jeff Dickey in [44a7d2e](https://github.com/jdx/mise/commit/44a7d2e4558f1756677785b2afe2917cff8dfe63)

### 🧪 Testing

- set RUST_BACKTRACE in e2e tests by Jeff Dickey in [e1efb7f](https://github.com/jdx/mise/commit/e1efb7fd8dca45c8a337def418f48862ef63e1c6)
- added cargo_features test by Jeff Dickey in [3aa5f57](https://github.com/jdx/mise/commit/3aa5f5784ec63ec04f0ffeb5c1d2246687a65314)
- reset test by Jeff Dickey in [131cb0a](https://github.com/jdx/mise/commit/131cb0ada079efb7865e6666a12e6bf99e4d8150)

### 🔍 Other Changes

- set DEBUG=1 for alpine to find out why it is not creating MRs by Jeff Dickey in [313a2a0](https://github.com/jdx/mise/commit/313a2a062d08128c2d04484135ce3c2a9adb41f3)
- bump vfox.rs by Jeff Dickey in [9fbc562](https://github.com/jdx/mise/commit/9fbc56274ef134ddb8e1d400fc72765868981fb5)
- apply code lint fixes by Jeff Dickey in [c18dbc2](https://github.com/jdx/mise/commit/c18dbc2428ae2e585ecf5860a5577f7f93e30fdd)

## [2024.8.14](https://github.com/jdx/mise/compare/v2024.8.13..v2024.8.14) - 2024-08-27

### 🚀 Features

- **(cargo)** allow specifying features via tool options by jdx in [b6c900a](https://github.com/jdx/mise/commit/b6c900ac7f32c342cf68e6510444d4245acd9615)
- **(zig)** make dev builds installable by jdx in [de5d26b](https://github.com/jdx/mise/commit/de5d26b053f39a8033417ee08c2c401613000b86)
- add support for using `uv tool` as a replacement for pipx by Antonio Molner Domenech in [5568bf7](https://github.com/jdx/mise/commit/5568bf7bd47b0b5fa7100de80fe03ca61b428abd)

### 🐛 Bug Fixes

- **(src/path_env.rs)** Issue 2504: Fix for JoinPathsError by Matt Callaway in [270adb8](https://github.com/jdx/mise/commit/270adb8ee23e513e5d556c6bcbd2210014190047)
- block remote versions which are not simple versions by Jeff Dickey in [ba90c3b](https://github.com/jdx/mise/commit/ba90c3bbe71bd33d628df607326da9f0cf363af1)
- npm backend not finding updates by Roland Schaer in [6897258](https://github.com/jdx/mise/commit/6897258b8199b665a7179dafb8d73cdf564a34ec)

### 🔍 Other Changes

- Update contributing.md by jdx in [e9cc129](https://github.com/jdx/mise/commit/e9cc129f703ac2949900307a3b828c3a095644ca)
- fix nightly lint warning by Jeff Dickey in [6796a46](https://github.com/jdx/mise/commit/6796a46f95227286f3337bce374e7447536e9503)

### New Contributors

* @mcallaway made their first contribution in [#2511](https://github.com/jdx/mise/pull/2511)

## [2024.8.13](https://github.com/jdx/mise/compare/v2024.8.12..v2024.8.13) - 2024-08-26

### 🐛 Bug Fixes

- add suggestion for invalid use of repo_url by jdx in [f9a417e](https://github.com/jdx/mise/commit/f9a417e837277de7cdbe4b20333c5c65aea2323f)

### 📚 Documentation

- add individual page for every CLI command by Jeff Dickey in [acea81c](https://github.com/jdx/mise/commit/acea81ca090fab76c4974a77a25c9557822d6263)
- add individual page for every CLI command by Jeff Dickey in [e379df7](https://github.com/jdx/mise/commit/e379df732bd85d77faead4fce650e388993f5999)
- add experimental badges to cli commands by Jeff Dickey in [4e50f33](https://github.com/jdx/mise/commit/4e50f330968b93b1af2ad4c93a78e82f9514324b)
- lint by Jeff Dickey in [26ebdec](https://github.com/jdx/mise/commit/26ebdec2765416c26adc1001451abb6a2ce71978)

### 🧪 Testing

- fixed render_help test by Jeff Dickey in [d39d861](https://github.com/jdx/mise/commit/d39d86152814e1f24ec8b648e79235a2e1f2bba5)

### 🔍 Other Changes

- make some gh workflows only run on jdx/mise by Chris Wesseling in [f71e3ef](https://github.com/jdx/mise/commit/f71e3ef047d91b69eb4eb009536ea820c23a2e4b)
- Update index.md by jdx in [b2c25f3](https://github.com/jdx/mise/commit/b2c25f39cd736c02174462d2e94cc0605d6c8e22)

### 📦️ Dependency Updates

- update dependency vitepress to v1.3.4 by renovate[bot] in [6b6d0b4](https://github.com/jdx/mise/commit/6b6d0b4c9e4f04951c5f4fa020953d1baab01eaa)

## [2024.8.12](https://github.com/jdx/mise/compare/v2024.8.11..v2024.8.12) - 2024-08-20

### 🐛 Bug Fixes

- vendor git2 openssl by jdx in [f78b25c](https://github.com/jdx/mise/commit/f78b25c02df2aa467682d1b595a18a2bc3309ce1)
- python-compile setting by jdx in [28fe7b8](https://github.com/jdx/mise/commit/28fe7b804cf5029741332e270de6bc34e81c4da3)

### 🧪 Testing

- reset test by Jeff Dickey in [000fdb8](https://github.com/jdx/mise/commit/000fdb8560b9994e7678924978cf1866bd58e623)
- reset test by Jeff Dickey in [2deb6ce](https://github.com/jdx/mise/commit/2deb6cef5bca37a5bb8e769293e4a665f533209e)
- reset test by Jeff Dickey in [385c09b](https://github.com/jdx/mise/commit/385c09b88013281af6a5adc9706a9d85e951ff61)

### 📦️ Dependency Updates

- update dependency vitepress to v1.3.3 by renovate[bot] in [f30a81b](https://github.com/jdx/mise/commit/f30a81b403f7a2b3cd3148a4eda294904cc1583a)

## [2024.8.11](https://github.com/jdx/mise/compare/v2024.8.10..v2024.8.11) - 2024-08-19

### 🐛 Bug Fixes

- bump xx by Jeff Dickey in [9a9d3c1](https://github.com/jdx/mise/commit/9a9d3c11e46028bcea0c7ec2fee10bf5c9b1fbe6)

## [2024.8.10](https://github.com/jdx/mise/compare/v2024.8.9..v2024.8.10) - 2024-08-18

### 🚀 Features

- python on windows by Jeff Dickey in [2d4cee2](https://github.com/jdx/mise/commit/2d4cee239f8e7d53f7be176369f6e2502f3c3032)

### 🐛 Bug Fixes

- hide non-working core plugins on windows by Jeff Dickey in [16a08fc](https://github.com/jdx/mise/commit/16a08fc0fa00fc8f9751f7a25cc4f5f5fc87b94d)
- windows compat by Jeff Dickey in [2084a37](https://github.com/jdx/mise/commit/2084a37436fd7f7af8958501adc7b6535f608816)
- vfox tweaks by Jeff Dickey in [c260ab2](https://github.com/jdx/mise/commit/c260ab220a31241eaca971d6ddf4046f1f57865b)
- remove windows warning by Jeff Dickey in [9be937e](https://github.com/jdx/mise/commit/9be937e15dece684c574bcccd6f66499361cb935)

### 📚 Documentation

- windows by Jeff Dickey in [437b63c](https://github.com/jdx/mise/commit/437b63cff94b5302a0527881d6b6e461e1e4d628)

### 🧪 Testing

- fixing tests by Jeff Dickey in [1206497](https://github.com/jdx/mise/commit/12064971a43f74cdb0f34276e07fb02aaf240096)
- reset test by Jeff Dickey in [c740cfd](https://github.com/jdx/mise/commit/c740cfddf45703444a52388d899c1deb52b73134)

### 🔍 Other Changes

- clippy by Jeff Dickey in [ee005ff](https://github.com/jdx/mise/commit/ee005ffac65093aad8949cdbfaf0761df4595851)
- fix windows build by Jeff Dickey in [28c5cb6](https://github.com/jdx/mise/commit/28c5cb64bd6506bf6db08769885d65c192fb20ce)
- set GITHUB_TOKEN in release task by Jeff Dickey in [0ae049b](https://github.com/jdx/mise/commit/0ae049baedaf2daf3056ec7d2043a8ba27f09df1)

## [2024.8.9](https://github.com/jdx/mise/compare/v2024.8.8..v2024.8.9) - 2024-08-18

### 🚀 Features

- use registry shortname for mise.toml/install dirs by jdx in [d80ed88](https://github.com/jdx/mise/commit/d80ed8870573d3b0bc26250b84f195983c3b4816)
- vfox backend by jdx in [7b458a9](https://github.com/jdx/mise/commit/7b458a96f5e88b853590413286faea96a256f504)

### 🐛 Bug Fixes

- hide file tasks starting with "." by jdx in [8babc74](https://github.com/jdx/mise/commit/8babc741653f62cb2de562c7713efeb1f067f7d8)
- mise prune removes tool versions which are in use by Roland Schaer in [88b1514](https://github.com/jdx/mise/commit/88b1514c262c0b00b8c9abcd08520e5dd6b636ca)
- cargo_binstall missing from set commands by Roland Schaer in [40b85ee](https://github.com/jdx/mise/commit/40b85ee93c995cd2157ff62ed10a4d0caa002712)
- only warn if config properties are not found by jdx in [4732d81](https://github.com/jdx/mise/commit/4732d81c7dc9e2ba7fb32b10aba235e27b051ee2)

### 🚜 Refactor

- Asdf -> AsdfBackend by jdx in [84e5d72](https://github.com/jdx/mise/commit/84e5d7261d1e54b6d4fbf1ca47ee0228c461c30a)
- backend repetition by Jeff Dickey in [d2f7f33](https://github.com/jdx/mise/commit/d2f7f33d81906aaee80ab0e333935111c7307b36)

## [2024.8.8](https://github.com/jdx/mise/compare/v2024.8.7..v2024.8.8) - 2024-08-17

### 🚜 Refactor

- split asdf into forge+plugin by jdx in [5b92a77](https://github.com/jdx/mise/commit/5b92a773ca7bc299c2c4f31197148355387858d5)

### 🧪 Testing

- fix home directory for win tests by jdx in [15fbf0c](https://github.com/jdx/mise/commit/15fbf0cae5483e724f57a9c187a900c9caf5277b)

### 📦️ Dependency Updates

- update rust crate tabled to 0.16.0 by renovate[bot] in [e03455b](https://github.com/jdx/mise/commit/e03455b31d5f5b1bbe07beb2c95299f779491fb9)

## [2024.8.7](https://github.com/jdx/mise/compare/v2024.8.6..v2024.8.7) - 2024-08-16

### 🐛 Bug Fixes

- mise treats escaped newlines in env files differently than dotenvy by Roland Schaer in [0150500](https://github.com/jdx/mise/commit/015050090a44c14684760eefc212309dd30a2421)
- wait for spawned tasks to die before exiting by jdx in [02f1423](https://github.com/jdx/mise/commit/02f14237a7651fc3957bc848b1c651c21378f2c2)

### 📦️ Dependency Updates

- update dependency vitepress to v1.3.2 by renovate[bot] in [a316c80](https://github.com/jdx/mise/commit/a316c80e74a78e3901b2949926517d5b878e2ab1)

## [2024.8.6](https://github.com/jdx/mise/compare/v2024.8.5..v2024.8.6) - 2024-08-12

### 🐛 Bug Fixes

- spm backend doesn't allow a GitHub repo name containing a dot by Roland Schaer in [062b6e3](https://github.com/jdx/mise/commit/062b6e3edf8b9043bfde053766607c597faa879d)

### 🚜 Refactor

- renamed tool_request_version to tool_request to match the class by Jeff Dickey in [76a611a](https://github.com/jdx/mise/commit/76a611ac0f3cfbc7ac58fdc87a528e86ef73507e)

### 📚 Documentation

- fix typos again by Kian-Meng Ang in [d5fbea6](https://github.com/jdx/mise/commit/d5fbea6284ce6d4ecc5e6ffed2ac2811e7775cef)
- add executable permission after installation by Kian-Meng Ang in [7ef1949](https://github.com/jdx/mise/commit/7ef1949af74a88e39e84f13fad5acfaf68e72786)

## [2024.8.5](https://github.com/jdx/mise/compare/v2024.8.4..v2024.8.5) - 2024-08-03

### 🚀 Features

- show friendly errors when not in verbose/debug mode by jdx in [f6e8d43](https://github.com/jdx/mise/commit/f6e8d43416de00866da93ea682ad37512b5e86f2)
- allow installing cargo packages with `--git` by jdx in [754e895](https://github.com/jdx/mise/commit/754e895afafaee782aee431de2630484cb2eba58)
- some ux improvements to `mise sync nvm` by jdx in [502d1ce](https://github.com/jdx/mise/commit/502d1ce3ff14909de983ad99ff6242940e9791dd)

### 🐛 Bug Fixes

- display untrusted file on error by jdx in [c29648c](https://github.com/jdx/mise/commit/c29648cf861eaf6cef335173cea42683424d4454)
- `mise trust` issue with unstable hashing by jdx in [390218b](https://github.com/jdx/mise/commit/390218bd5f51cf2866193035c9865099c2f598c1)
- use newer eza in e2e test by Jeff Dickey in [eec3989](https://github.com/jdx/mise/commit/eec3989d8602ebc10304adbd5ded0574fc2981f0)
- take out home directory paths from `mise dr` output by jdx in [89ef4d6](https://github.com/jdx/mise/commit/89ef4d6ed558f0fb5af3f163c1299af30d20d22f)

### 🔍 Other Changes

- use pub(crate) to get notified about dead code by jdx in [a0d1eb1](https://github.com/jdx/mise/commit/a0d1eb1ef71869355bb308f27a5503ed296ae2a6)

## [2024.8.4](https://github.com/jdx/mise/compare/v2024.8.3..v2024.8.4) - 2024-08-02

### 🐛 Bug Fixes

- alpine key madness by Jeff Dickey in [a7156e0](https://github.com/jdx/mise/commit/a7156e0042cf10fc3d43723ffd6a92860b4faa0a)
- alpine github key by Jeff Dickey in [a52b68d](https://github.com/jdx/mise/commit/a52b68d024a8ce9955bd84347cc591b249717312)
- alpine github key by Jeff Dickey in [ebc923f](https://github.com/jdx/mise/commit/ebc923ff3c140c6c282bb0c1a2896ad758b4a3c2)
- spm - cannot install package with null release name field by Roland Schaer in [75437b6](https://github.com/jdx/mise/commit/75437b6b597483c174dd23f1a301a3aff9b2837f)

### 🔍 Other Changes

- removed dead code by jdx in [db4470f](https://github.com/jdx/mise/commit/db4470f7c413658b83e984f1dcc3b1995b48e149)

## [2024.8.3](https://github.com/jdx/mise/compare/v2024.8.2..v2024.8.3) - 2024-08-01

### 🧪 Testing

- clean up global config test by Jeff Dickey in [c9f2ec5](https://github.com/jdx/mise/commit/c9f2ec514082c6b1816c52378ce5c29d24aa73cc)

### 🔍 Other Changes

- set extra alpine key by Jeff Dickey in [c6b152b](https://github.com/jdx/mise/commit/c6b152bd1864b49c392ad64becbff1b1722be52f)
- test alpine releases by Jeff Dickey in [08f7730](https://github.com/jdx/mise/commit/08f77301c772eb55cee376908f9d907e42c7fe4b)
- perform alpine at the very end by Jeff Dickey in [7c31e17](https://github.com/jdx/mise/commit/7c31e17cc6ff612298c8bdb335d86cab95c9473b)
- chmod by Jeff Dickey in [a3fe85b](https://github.com/jdx/mise/commit/a3fe85b7b71faecb220b33d6cc3b630884b4343a)
- added jq/gh to alpine docker by Jeff Dickey in [e1514cf](https://github.com/jdx/mise/commit/e1514cf95cc625085530c12afc6a7ceb57ff0b64)

## [2024.8.2](https://github.com/jdx/mise/compare/v2024.8.1..v2024.8.2) - 2024-08-01

### 🐛 Bug Fixes

- windows bug fixes by Jeff Dickey in [465ea89](https://github.com/jdx/mise/commit/465ea894f317eda025783e66a68f58ab10319790)
- made cmd! work on windows by Jeff Dickey in [c0cef5b](https://github.com/jdx/mise/commit/c0cef5b0941b476badfdbb4f46f24b117d72698d)
- got node to install on windows by Jeff Dickey in [e5aa94e](https://github.com/jdx/mise/commit/e5aa94ecb14c7700823ff7dd58a6e633ced5e054)
- windows shims by Jeff Dickey in [fc2cd48](https://github.com/jdx/mise/commit/fc2cd489babe834546424831a9613e1d0558aa7d)
- windows paths by Jeff Dickey in [a06bcce](https://github.com/jdx/mise/commit/a06bcce484ce405342e68a1ac5dbb667db376f5e)

### 🔍 Other Changes

- fix build by Jeff Dickey in [9d85182](https://github.com/jdx/mise/commit/9d8518249c783819a82366f8541f1ea20959e771)
- dry-run alpine releases by Jeff Dickey in [0ef2727](https://github.com/jdx/mise/commit/0ef2727905ce904e44b25cfe46c29645fd41405a)
- update bun version in e2e test by Jeff Dickey in [f4b339f](https://github.com/jdx/mise/commit/f4b339f7974dbb261e7e8a387d082f4090e01f21)
- fix bun test by Jeff Dickey in [00d7054](https://github.com/jdx/mise/commit/00d70543a5f3e0db891b7bfb505e65dacb66d8f0)

## [2024.8.1](https://github.com/jdx/mise/compare/v2024.8.0..v2024.8.1) - 2024-08-01

### 🐛 Bug Fixes

- various windows bug fixes by Jeff Dickey in [90b02eb](https://github.com/jdx/mise/commit/90b02eb49055bc7d458cd3cbfb0de00119539dfb)
- ignore PROMPT_DIRTRIM in diffing logic by Jeff Dickey in [7b5563c](https://github.com/jdx/mise/commit/7b5563cd007edf26bc17f07e6cddabacad451e00)

### 📚 Documentation

- added information on rolling alpine tokens by Jeff Dickey in [bd693b0](https://github.com/jdx/mise/commit/bd693b02fb4d1060ff7a07dcea07b4a7c5584a8b)

### 🔍 Other Changes

- mark releases as draft until they have been fully released by Jeff Dickey in [508f125](https://github.com/jdx/mise/commit/508f125dcea9c6d0457b59c36293204d25adc7ef)
- fix windows builds by Jeff Dickey in [91c90a2](https://github.com/jdx/mise/commit/91c90a2b2d373998433c64196254f7e4d0d8cd82)
- fix alpine release builds by Jeff Dickey in [a7534bb](https://github.com/jdx/mise/commit/a7534bbdd961e6a16852c947f1594d6a52034e58)
- only edit releases when not a dry run by Jeff Dickey in [2255522](https://github.com/jdx/mise/commit/2255522b5045e45ce0dea3699f6555a22a271971)

## [2024.8.0](https://github.com/jdx/mise/compare/v2024.7.5..v2024.8.0) - 2024-08-01

### 📚 Documentation

- Fix 'mise x' command snippet in the Continuous Integration section by Daniel Jankowski in [e4445de](https://github.com/jdx/mise/commit/e4445de9831b207d42a7ccfbfbb9ab2d412e3903)

### 🔍 Other Changes

- retry mise tests for docker-dev-test workflow by Jeff Dickey in [cc014dd](https://github.com/jdx/mise/commit/cc014dde3dedd1d891dab62fc37e4633dc995226)
- add BSD-2-Clause to allowed dep licenses by Jeff Dickey in [b4ea53c](https://github.com/jdx/mise/commit/b4ea53c4b2b01103ed93fc185dbca858730c3207)
- create new alpine gitlab token to replace the expired one by Jeff Dickey in [b30db04](https://github.com/jdx/mise/commit/b30db04aaa1f13ef0dcdf02e6df2f2afbdd73c94)

### New Contributors

* @mollyIV made their first contribution in [#2411](https://github.com/jdx/mise/pull/2411)

## [2024.7.5](https://github.com/jdx/mise/compare/v2024.7.4..v2024.7.5) - 2024-07-29

### 🐛 Bug Fixes

- mise use does not create a local .mise.toml anymore by Roland Schaer in [1865fb5](https://github.com/jdx/mise/commit/1865fb5d888e1c711461fecbe0f8801849db86c3)
- transform `master` to `ref:master` in ls-remote for zig by Mathew Robinson in [7de3dcc](https://github.com/jdx/mise/commit/7de3dcc345d474ed4cc7d52e12fc6cfdc03c7879)

### 📦️ Dependency Updates

- bump openssl from 0.10.64 to 0.10.66 by dependabot[bot] in [241747f](https://github.com/jdx/mise/commit/241747f1a5751a4d434988ab51bd85a3c346281c)

### New Contributors

* @chasinglogic made their first contribution in [#2409](https://github.com/jdx/mise/pull/2409)

## [2024.7.4](https://github.com/jdx/mise/compare/v2024.7.3..v2024.7.4) - 2024-07-19

### 🚀 Features

- added MISE_LIBGIT2 setting by jdx in [cba1d31](https://github.com/jdx/mise/commit/cba1d315e7e5378e4b99e86ac88818fd4218a477)

### 🐛 Bug Fixes

- keep RUBYLIB env var by jdx in [63afb43](https://github.com/jdx/mise/commit/63afb43252db52f5b9ba5011efcd14d2ffb9e4eb)

### 📦️ Dependency Updates

- update dependency vitepress to v1.3.1 by renovate[bot] in [a897efa](https://github.com/jdx/mise/commit/a897efafbd531a47888cd13d8142a6e03fcd7559)
- update docker/build-push-action action to v6 by renovate[bot] in [6062fdd](https://github.com/jdx/mise/commit/6062fdd63de1e53913b28927840daa610b00dea5)

## [2024.7.3](https://github.com/jdx/mise/compare/v2024.7.2..v2024.7.3) - 2024-07-14

### 🔍 Other Changes

- Use correct capitalization of GitHub by Jacob Hands in [f70db30](https://github.com/jdx/mise/commit/f70db302f4c245c20ecfb1044e6ce773626b6df2)
- loosen git2 requirements by jdx in [ed15fb5](https://github.com/jdx/mise/commit/ed15fb5344ed9d5c92b3a872539b4420c9496e57)

### New Contributors

* @jahands made their first contribution in [#2372](https://github.com/jdx/mise/pull/2372)

## [2024.7.2](https://github.com/jdx/mise/compare/v2024.7.1..v2024.7.2) - 2024-07-13

### 🚀 Features

- support env vars in plugin urls by Roland Schaer in [2373a38](https://github.com/jdx/mise/commit/2373a38668ea527840c07ae015fb04cc9d2c966a)

### 📦️ Dependency Updates

- update rust crate self_update to 0.41 by renovate[bot] in [7166552](https://github.com/jdx/mise/commit/7166552fcf66410fbd835bb62fe5c378c5791c3d)
- update dependency vitepress to v1.3.0 by renovate[bot] in [6531358](https://github.com/jdx/mise/commit/65313580a4aadecc8c118f2beb59c475172318c5)

## [2024.7.1](https://github.com/jdx/mise/compare/v2024.7.0..v2024.7.1) - 2024-07-08

### 🔍 Other Changes

- Fix link to Python venv activation doc section by Gregor Zurowski in [6ecf5b8](https://github.com/jdx/mise/commit/6ecf5b8957a33931bd23cce9742271313ca1f7a1)

### 📦️ Dependency Updates

- update built to 0.7.4 and git2 to 0.19.0 by Roland Schaer in [5d8c7fc](https://github.com/jdx/mise/commit/5d8c7fc3f179b117da19875ef098a142f1019e91)

### New Contributors

* @gzurowski made their first contribution in [#2353](https://github.com/jdx/mise/pull/2353)

## [2024.7.0](https://github.com/jdx/mise/compare/v2024.6.6..v2024.7.0) - 2024-07-03

### 📚 Documentation

- update actions/checkout version by light planck in [f479a46](https://github.com/jdx/mise/commit/f479a46bbd36e3d5050b3515e84dada895c3cd8e)

### New Contributors

* @light-planck made their first contribution in [#2349](https://github.com/jdx/mise/pull/2349)

## [2024.6.6](https://github.com/jdx/mise/compare/v2024.6.5..v2024.6.6) - 2024-06-20

### 🐛 Bug Fixes

- improve error message for missing plugins by jdx in [eab1def](https://github.com/jdx/mise/commit/eab1def1546e00232e94c402b8e54eb116b796fa)

### 🔍 Other Changes

- Update configuration.md by jdx in [a2f19cb](https://github.com/jdx/mise/commit/a2f19cbc655058472009d000c77d1fc8df8612fd)
- Update index.md by jdx in [d9ef467](https://github.com/jdx/mise/commit/d9ef467ee9ef026039fa2220163f21a2214ebbfc)
- Update index.md by jdx in [63739c8](https://github.com/jdx/mise/commit/63739c880dbfefdecab282736710d496d7e88dbc)

### 📦️ Dependency Updates

- bump curve25519-dalek from 4.1.2 to 4.1.3 by dependabot[bot] in [d6c5a08](https://github.com/jdx/mise/commit/d6c5a088d39828da56d1e10df933f4e619a386fc)

## [2024.6.5](https://github.com/jdx/mise/compare/v2024.6.4..v2024.6.5) - 2024-06-18

### 🔍 Other Changes

- Fixes nix flake by Zhongcheng Lao in [3564412](https://github.com/jdx/mise/commit/356441270e42a23039205cc777140c9b2d21797d)

## [2024.6.4](https://github.com/jdx/mise/compare/v2024.6.3..v2024.6.4) - 2024-06-15

### 🐛 Bug Fixes

- allow glob patterns in task outputs and sources by Adam Dickinson in [bd64408](https://github.com/jdx/mise/commit/bd64408e8b05a6b366b0cf8a383a49956f8375c3)

### New Contributors

* @adamdickinson made their first contribution in [#2286](https://github.com/jdx/mise/pull/2286)

## [2024.6.3](https://github.com/jdx/mise/compare/v2024.6.2..v2024.6.3) - 2024-06-10

### 🐛 Bug Fixes

- github API rate limiting could be handled more explicitly by Roland Schaer in [9ae8952](https://github.com/jdx/mise/commit/9ae89524a46fb73ef6e9db7470f159c3a62f20cf)
- group prefix not applied for script tasks by Roland Schaer in [3b038c7](https://github.com/jdx/mise/commit/3b038c752e55e583a5b9ddda8b7be04d1c6d1ce1)
- mise plugins ls returns error immediately after install by Roland Schaer in [2ccda67](https://github.com/jdx/mise/commit/2ccda6774c2638957b745fd98acff72f71733238)

### 📦️ Dependency Updates

- update dependency vitepress to v1.2.3 by renovate[bot] in [60111ad](https://github.com/jdx/mise/commit/60111adf45c5bbe4b485b110167ac3333dd274a5)
- update rust crate regex to v1.10.5 by renovate[bot] in [466556a](https://github.com/jdx/mise/commit/466556aebf195f81df252f58fb25783c61527469)
- update rust crate regex to v1.10.5 by renovate[bot] in [577de17](https://github.com/jdx/mise/commit/577de1757c4bb4e6421d3e281c44825a8b8788b8)

## [2024.6.2](https://github.com/jdx/mise/compare/v2024.6.1..v2024.6.2) - 2024-06-07

### 🐛 Bug Fixes

- after installing the latest version, mise rolls back to the previous one by Roland Schaer in [f239b11](https://github.com/jdx/mise/commit/f239b1119c912684cdfd9482f2f784a70b35d136)

### 📚 Documentation

- add SPM backend page by Vasiliy Kattouf in [0dd0c8f](https://github.com/jdx/mise/commit/0dd0c8f8c579dfd15a0c73417f39aa662af80448)

## [2024.6.1](https://github.com/jdx/mise/compare/v2024.6.0..v2024.6.1) - 2024-06-03

### 🚀 Features

- SPM(Swift Package Manager) backend by Vasiliy Kattouf in [8d37aab](https://github.com/jdx/mise/commit/8d37aabd08d80de0caf03e83bacc87f78afca277)

### 🐛 Bug Fixes

- mise up node fails by Roland Schaer in [f24cf88](https://github.com/jdx/mise/commit/f24cf8882ba81ac1c71fb2addc53a4424e4dca64)

### 📚 Documentation

- fixed syntax by jdx in [56083f8](https://github.com/jdx/mise/commit/56083f858a4ee28a020a414c1addf0c2bb7968af)

### 🧪 Testing

- set GITHUB_TOKEN in dev-test by Jeff Dickey in [4334313](https://github.com/jdx/mise/commit/4334313da52c13d7f87656fb0e7978e4cf1f5d2f)

### 🔍 Other Changes

- Update getting-started.md: nushell by Krzysztof Modras in [2a99aa3](https://github.com/jdx/mise/commit/2a99aa36aa8da2ce660f637a88ab556011cb6b50)

### 📦️ Dependency Updates

- update rust crate demand to v1.2.4 by renovate[bot] in [fc4ce46](https://github.com/jdx/mise/commit/fc4ce46fff8042640c2ccb8c07778ad121ea7e5a)
- update rust crate zip to v2.1.2 by renovate[bot] in [e22d5a0](https://github.com/jdx/mise/commit/e22d5a059702580e15acf42cfe5a19c56aacd00e)

### New Contributors

* @chrmod made their first contribution in [#2248](https://github.com/jdx/mise/pull/2248)

## [2024.6.0](https://github.com/jdx/mise/compare/v2024.5.28..v2024.6.0) - 2024-06-01

### 🔍 Other Changes

- bump itertools by jdx in [7552338](https://github.com/jdx/mise/commit/75523385ad308f445b76b0487e08a6e6718e4b39)
- migrate docs repo into this repo by jdx in [7f6c51d](https://github.com/jdx/mise/commit/7f6c51d4f7a7fdb797ad93b8f538cc903757749f)

## [2024.5.28](https://github.com/jdx/mise/compare/v2024.5.27..v2024.5.28) - 2024-05-31

### 🐛 Bug Fixes

- download keeps failing if it takes more than 30s by Roland Schaer in [cca3a8a](https://github.com/jdx/mise/commit/cca3a8abf58a7d851ce413a8e0c7dc99b859477f)
- settings unset does not work by Roland Schaer in [a7a90a8](https://github.com/jdx/mise/commit/a7a90a8340fe7df86a032f24d6f387e2df81b388)
- cleaner community-developed plugin warning by Jeff Dickey in [8dcf0f3](https://github.com/jdx/mise/commit/8dcf0f3a746fcae74d944412b6f0e141ded88860)
- correct `mise use` ordering by jdx in [62f20db](https://github.com/jdx/mise/commit/62f20dbecd504f9c16860b92a93e22ef79b5d473)

### 🚜 Refactor

- forge -> backend by jdx in [27ae23b](https://github.com/jdx/mise/commit/27ae23b139e37e0f94c101cf031ec9b596547aea)

### 🧪 Testing

- added reset() to more tests by Jeff Dickey in [5a6ea6a](https://github.com/jdx/mise/commit/5a6ea6afb9855827b5e6216aa20760dd45f5502f)

## [2024.5.27](https://github.com/jdx/mise/compare/v2024.5.26..v2024.5.27) - 2024-05-31

### 🚜 Refactor

- rename External plugins to Asdf by Jeff Dickey in [8e774ba](https://github.com/jdx/mise/commit/8e774ba44e933eedfb999259d1244d589fc7d847)
- split asdf into forge+plugin by jdx in [f1683a6](https://github.com/jdx/mise/commit/f1683a6ca2ef42d2cea1d477fd57baa0f7fd3af9)

### 🧪 Testing

- added reset() to more tests by Jeff Dickey in [1c76011](https://github.com/jdx/mise/commit/1c760112eef92eb51ada4ab00e45568adcf62b97)
- added reset() to more tests by Jeff Dickey in [402c5ce](https://github.com/jdx/mise/commit/402c5cee97ebdbeb42fc32d055f73794d4dfdf12)

### 🔍 Other Changes

- dont clean cache on win by Jeff Dickey in [ede6528](https://github.com/jdx/mise/commit/ede6528f5fe5e5beeabf0a007997f3abc188faa5)

## [2024.5.26](https://github.com/jdx/mise/compare/v2024.5.25..v2024.5.26) - 2024-05-30

### 🐛 Bug Fixes

- normalize remote urls by jdx in [e24eea5](https://github.com/jdx/mise/commit/e24eea54131f0be24538635ee2df1296de4eea1d)

### 🧪 Testing

- added reset() to more tests by Jeff Dickey in [f9f65b3](https://github.com/jdx/mise/commit/f9f65b39214c9341bf44ad694c6659b6a17fdf9c)

### 🔍 Other Changes

- remove armv6 targets by Jeff Dickey in [90752f4](https://github.com/jdx/mise/commit/90752f4f08a8ca4095fb464edd79a7aed2b07e54)

## [2024.5.25](https://github.com/jdx/mise/compare/v2024.5.24..v2024.5.25) - 2024-05-30

### 🚀 Features

- use all tera features by Jeff Dickey in [48ca740](https://github.com/jdx/mise/commit/48ca74043e21fe12de18a8457e4554ac2cadb17b)

### 🚜 Refactor

- turn asdf into a forge by jdx in [32cfecb](https://github.com/jdx/mise/commit/32cfecb7e21968f42d1b1863f90d812244b3e0f1)

### 🧪 Testing

- clean cwd in unit tests by jdx in [ef25cb1](https://github.com/jdx/mise/commit/ef25cb1a419a39de42a2839b4e97c56f19bfe569)
- windows by jdx in [a0d5ad8](https://github.com/jdx/mise/commit/a0d5ad8f091fdf2c566ad79d1bd8c3263bd2a2db)
- add reset() to more tests by jdx in [ed5c5a3](https://github.com/jdx/mise/commit/ed5c5a312cd2d9ddd571a4481d4a581ba9f93afb)
- added reset() to more tests by Jeff Dickey in [a22c9dd](https://github.com/jdx/mise/commit/a22c9dd1f0eb8c057046e23807abe3c5352faf66)

### 🔍 Other Changes

- fix build-tarball call by Jeff Dickey in [2a4b986](https://github.com/jdx/mise/commit/2a4b98685f0dc2c4c85c3ecee9634b08432354fc)
- **breaking** use kebab-case for backend-installs by jdx in [fa4793a](https://github.com/jdx/mise/commit/fa4793aa4d1fe77a780664fc8d699cd8b3df14f2)

## [2024.5.24](https://github.com/jdx/mise/compare/v2024.5.23..v2024.5.24) - 2024-05-28

### 🐛 Bug Fixes

- **(pipx)** version ordering by jdx in [9d37771](https://github.com/jdx/mise/commit/9d37771e2b88527034271c95d2654cb41114028c)
- **(use)** re-use mise.toml if exists by jdx in [4425cca](https://github.com/jdx/mise/commit/4425ccabea0cb2055f408a61edbe4696d178efdd)
- mise trust works incorrectly with symlinked configuration file by Roland Schaer in [bf24ef6](https://github.com/jdx/mise/commit/bf24ef62c6863c61d0bdf55831a3acf430914c25)

### 🚜 Refactor

- simplify ForgeArg building by jdx in [fcf9386](https://github.com/jdx/mise/commit/fcf9386eeeb079fd5143ff7ac4db10de72d2cd0f)

### 🔍 Other Changes

- resolve macros/derived-traits from crates w/ scopes rather than globally by Donald Guy in [9a9924e](https://github.com/jdx/mise/commit/9a9924e6473036468176014fdac5e8e49ee8123e)
- eliminate .tool-versions only used for jq by Donald Guy in [0df152b](https://github.com/jdx/mise/commit/0df152bb1a78ff1e3dcacceeb6e5b9d2ae4c5c48)

### New Contributors

* @donaldguy made their first contribution in [#2195](https://github.com/jdx/mise/pull/2195)

## [2024.5.23](https://github.com/jdx/mise/compare/v2024.5.22..v2024.5.23) - 2024-05-27

### 🐛 Bug Fixes

- **(self_update)** explicitly set target since there seems to be a bug with .identifier() by jdx in [10dd050](https://github.com/jdx/mise/commit/10dd05071706aa75140a037f2d9a8b93ce7628c6)
- minor race condition creating directories by Jeff Dickey in [23db391](https://github.com/jdx/mise/commit/23db39146c8edf7340472302e7f498f1d89cf5b4)
- vendor libgit2 for precompiled binaries by jdx in [f857f6c](https://github.com/jdx/mise/commit/f857f6c834ed88a250ac9b94f3c6233b14ed9f80)

### 🧪 Testing

- break coverage tasks up a bit by jdx in [edd8623](https://github.com/jdx/mise/commit/edd8623f8757fb892ef87e73887184dc8d9ec385)

### 🔍 Other Changes

- updated zip by jdx in [804d771](https://github.com/jdx/mise/commit/804d77166ad0e9630746cfb6cba70a0576e7d1a8)
- bump usage-lib by Jeff Dickey in [74fcd88](https://github.com/jdx/mise/commit/74fcd8863c8668f11c4886dd95fb7929f823eb14)
- Update bug_report.md by jdx in [64271ed](https://github.com/jdx/mise/commit/64271edec6e8cbf68dd0ec5f646247fdc3f158e2)
- added git debug log by Jeff Dickey in [7df466e](https://github.com/jdx/mise/commit/7df466e8c9c287ad04b0a753df65c02d64e00451)
- retry build-tarball by Jeff Dickey in [1acf037](https://github.com/jdx/mise/commit/1acf0375072dbf4ae57ddfadf0daf5eea00d5b71)

## [2024.5.22](https://github.com/jdx/mise/compare/v2024.5.21..v2024.5.22) - 2024-05-25

### 🐛 Bug Fixes

- correctly use .mise/config.$MISE_ENV.toml files by Jeff Dickey in [cace97b](https://github.com/jdx/mise/commit/cace97b9fe7697a58354b93cc1109b14c9fbd30c)
- correctly use .mise/config.$MISE_ENV.toml files by Jeff Dickey in [262fa2e](https://github.com/jdx/mise/commit/262fa2e283dbd4c2fe4f44f15d81ab6eed54b79d)

### 🔍 Other Changes

- use async reqwest by jdx in [cef35ba](https://github.com/jdx/mise/commit/cef35ba6ed4977887b89137383824e44027491d7)
- sign macos binary by Jeff Dickey in [88f43f8](https://github.com/jdx/mise/commit/88f43f8072a2a223d1be92504cd60b7191ef975b)
- use sccache by jdx in [70f4b95](https://github.com/jdx/mise/commit/70f4b95be783ee297c8ecc234446b8b08612d5f7)
- compile on windows by jdx in [b563597](https://github.com/jdx/mise/commit/b563597677f4e4e65dd4b8e6a35a48316699e7f7)
- conditionally set sccache token by jdx in [53bdd31](https://github.com/jdx/mise/commit/53bdd312dd9f05ad7be5adb87753c31ae1d641b5)

## [2024.5.21](https://github.com/jdx/mise/compare/v2024.5.20..v2024.5.21) - 2024-05-23

### 🐛 Bug Fixes

- **(git-pre-commit)** rewrite existing git hook to pre-commit.old by jdx in [2ae4203](https://github.com/jdx/mise/commit/2ae4203ea55ee1de48397891e1912821f24fd6b3)
- handle issue running `mise install` with existing tools by jdx in [54d213e](https://github.com/jdx/mise/commit/54d213e291a7768a2474c1a577e68885ef37b503)

### 🔍 Other Changes

- update kerl to 4.1.1 by Beatrix Klebe in [d7f64e2](https://github.com/jdx/mise/commit/d7f64e23f8d95a10fbd0eaccdd9075841c1936c6)

### New Contributors

* @bklebe made their first contribution in [#2173](https://github.com/jdx/mise/pull/2173)

## [2024.5.20](https://github.com/jdx/mise/compare/v2024.5.18..v2024.5.20) - 2024-05-21

### 🐛 Bug Fixes

- **(prune)** make it not install the world by Jeff Dickey in [78f4aec](https://github.com/jdx/mise/commit/78f4aeca2647c3980feb68cd3c1e299c9c56b0d6)
- allow plugins overriding core plugins by jdx in [51750b8](https://github.com/jdx/mise/commit/51750b8a031ac90134327370bf94238995164fb5)

### 🚜 Refactor

- toolset -> toolrequestset by jdx in [d39b437](https://github.com/jdx/mise/commit/d39b43796e8a3fbc7db7e7605abc9d80ad4963c1)
- toolset -> toolrequestset by jdx in [1ad7b0c](https://github.com/jdx/mise/commit/1ad7b0cb95ba9cc9bd9f6c892076095eaf0d1088)

### 📚 Documentation

- fix core plugin registry urls by Jeff Dickey in [bb1556e](https://github.com/jdx/mise/commit/bb1556ee5a9c7806c28d9bf7472bd444ab70f35e)

### 🧪 Testing

- **(pipx)** use python3 instead of python by Jeff Dickey in [0ff52da](https://github.com/jdx/mise/commit/0ff52daf026d711d5001cc3af08caef0bdb4d163)
- name cache steps by Jeff Dickey in [532fe90](https://github.com/jdx/mise/commit/532fe9032a4f61c2ffbf47d29713ee3900770b55)
- fix lint-fix job by Jeff Dickey in [6439ca4](https://github.com/jdx/mise/commit/6439ca41820c240846686f9fbe6d67d24114934e)
- reset config after local tests by Jeff Dickey in [29077af](https://github.com/jdx/mise/commit/29077af3a0d04ad004a054e16e7e85e411058be1)
- fix implode running first when shuffled by Jeff Dickey in [7b07258](https://github.com/jdx/mise/commit/7b072589d46b4279574f99385f3515b6bd181bd5)
- added test for core plugin overloading by Jeff Dickey in [9a56129](https://github.com/jdx/mise/commit/9a5612993dc59359e0c876e8f948f2fece8ce93f)
- added shebang to e2e scripts by jdx in [bed68c4](https://github.com/jdx/mise/commit/bed68c43b694ef41fccaffd65bead77ac8656701)

## [2024.5.18](https://github.com/jdx/mise/compare/v2024.5.17..v2024.5.18) - 2024-05-19

### 🚀 Features

- added plugin registry to docs by jdx in [6b5e366](https://github.com/jdx/mise/commit/6b5e3667a8048d737927eb3bc192b6843bf9d53c)
- added registry command by jdx in [09daa70](https://github.com/jdx/mise/commit/09daa7075d70769f6e00b465d705cb9366b9f18e)
- pre-commit and github action generate commands by jdx in [16278a1](https://github.com/jdx/mise/commit/16278a176bcf01e764d7cb66767b657d70b61956)

### 🐛 Bug Fixes

- raise error if resolve fails and is a CLI argument by jdx in [70d4e92](https://github.com/jdx/mise/commit/70d4e92a00a4cb0fe7200b8b24b35abaa2ed65f6)
- clean up architectures for precompiled binaries by jdx in [ea31a39](https://github.com/jdx/mise/commit/ea31a39b0de977983cb36a4e32535f4d7d3e2b21)
- add target and other configs to cache key logic by jdx in [0aaba58](https://github.com/jdx/mise/commit/0aaba587b19bb57c93917bb56964e9c71c5e568d)

### 🚜 Refactor

- remove cmd_forge by jdx in [108ef38](https://github.com/jdx/mise/commit/108ef3893953187f0f7c6fdd9e35353caece3723)

### 🧪 Testing

- separate nightly into its own job by jdx in [d40f58b](https://github.com/jdx/mise/commit/d40f58bd697d0f80ea6e4a52ffafccf651387294)
- lint in nightly job by Jeff Dickey in [b5a3d08](https://github.com/jdx/mise/commit/b5a3d0884655f884319b23924d06566d597a4abe)

## [2024.5.17](https://github.com/jdx/mise/compare/v2024.5.16..v2024.5.17) - 2024-05-18

### 🚀 Features

- allow install specific version from https://mise.run #1800 by Alexandre Marre in [8560895](https://github.com/jdx/mise/commit/8560895d9850f6b145c424678af7a07af71dfa21)
- confirm all plugins by Roland Schaer in [3e7571c](https://github.com/jdx/mise/commit/3e7571cc33bb456fb7246c0735ce26bf9f825e95)
- allow ignore missing plugin by Roland Schaer in [9f32d66](https://github.com/jdx/mise/commit/9f32d6642a372409d84b563b9b8f4c3e9fdfa19c)

### 🐛 Bug Fixes

- **(pipx)** depend on python by Jeff Dickey in [89b9c9a](https://github.com/jdx/mise/commit/89b9c9a7db4e1db624019bb760ed32a76d5a7597)

### 🚜 Refactor

- fetch transitive dependencies by jdx in [2b130cb](https://github.com/jdx/mise/commit/2b130cb0f6bcbdfb1d17a5b37765a92daf99be3d)

### 🧪 Testing

- pass MISE_LOG_LEVEL through by Jeff Dickey in [7dea795](https://github.com/jdx/mise/commit/7dea795967ee11526af6e95a55e19bf7fddb3315)
- make unit tests work shuffled by jdx in [774e7dd](https://github.com/jdx/mise/commit/774e7dd5c6afce42d42a9ba2507921ce17be3751)
- ensure tests reset by jdx in [a3f0aec](https://github.com/jdx/mise/commit/a3f0aecbddc4b6fff543d12f416fb5ebdbf97598)
- ensure tests reset by Jeff Dickey in [feeaf8f](https://github.com/jdx/mise/commit/feeaf8f072a253305df9f59d357596a87fc0da36)
- clean up .test.mise.toml file by Jeff Dickey in [c41e0a3](https://github.com/jdx/mise/commit/c41e0a3adedf5502901d5c8b5f49d2f51e4f9428)

## [2024.5.16](https://github.com/jdx/mise/compare/v2024.5.15..v2024.5.16) - 2024-05-15

### 🚀 Features

- **(registry)** map ubi -> cargo:ubi by jdx in [e3080ba](https://github.com/jdx/mise/commit/e3080baab48d21ffcc3b883475dfe3065b4b7b7b)
- **(tasks)** add --json flag by Lev Vereshchagin in [d327027](https://github.com/jdx/mise/commit/d327027220d1b1d22c8bf82d94eed2754d80df60)

### 🐛 Bug Fixes

- support "mise.toml" filename by Jeff Dickey in [035745f](https://github.com/jdx/mise/commit/035745f95f5f143b62e6d3cdc6cfbaa4a6d887e0)

### 🔍 Other Changes

- add rustfmt to release-plz by Jeff Dickey in [2d530f6](https://github.com/jdx/mise/commit/2d530f645b6263c6162380684ab7914efc3dce39)

### New Contributors

* @vrslev made their first contribution in [#2116](https://github.com/jdx/mise/pull/2116)

## [2024.5.15](https://github.com/jdx/mise/compare/v2024.5.14..v2024.5.15) - 2024-05-14

### 🚀 Features

- support non-hidden configs by jdx in [19a5ecf](https://github.com/jdx/mise/commit/19a5ecf8896734437d7d53c2a2fadce2c7370837)

### 🐛 Bug Fixes

- handle sub-0.1 in new resolving logic by Jeff Dickey in [fd943a1](https://github.com/jdx/mise/commit/fd943a184bcc64866b761514788b5a0e4be07ac0)

### 🚜 Refactor

- ToolVersionRequest -> ToolRequest by Jeff Dickey in [45caece](https://github.com/jdx/mise/commit/45caece3517792b02444620edb96c18c2d7513c2)

### 🧪 Testing

- fail-fast by Jeff Dickey in [2338376](https://github.com/jdx/mise/commit/23383760900ede666865e073acb680dced37d8fc)
- update deno version by Jeff Dickey in [71f5480](https://github.com/jdx/mise/commit/71f5480e780953e03aa97682535a58767956a927)
- check plugin dependencies with python and pipx. by Adirelle in [eeecfc1](https://github.com/jdx/mise/commit/eeecfc14c6d647596ab368103d78f7acec68e0aa)
- wait a bit longer before retrying e2e test failures by Jeff Dickey in [d098c86](https://github.com/jdx/mise/commit/d098c866a415459981a5bb770f60b51067f444ce)

### 🔍 Other Changes

- optimize imports by Jeff Dickey in [892184f](https://github.com/jdx/mise/commit/892184f5681c7f1863cbd89f07fca0cf5fa3afb2)
- optimize imports by Jeff Dickey in [54bfee6](https://github.com/jdx/mise/commit/54bfee6b435f8b1cbfba7210f73b9dfde1a3c6f1)
- automatically optimize imports by jdx in [bd13622](https://github.com/jdx/mise/commit/bd13622fb705a697f51b1db7bca61e5e0e54d8b8)
- fix release-plz with nightly rustfmt by Jeff Dickey in [0b6521a](https://github.com/jdx/mise/commit/0b6521ab620cf6c16e36d9c5d3cf56b7b0ee81eb)

## [2024.5.14](https://github.com/jdx/mise/compare/v2024.5.13..v2024.5.14) - 2024-05-14

### 🚀 Features

- **(erlang)** make erlang core plugin stable by Jeff Dickey in [d4bde6a](https://github.com/jdx/mise/commit/d4bde6a15297d693a00e7194ea3e20f399ae4184)
- **(python)** make python_compile 3-way switch by jdx in [7af4432](https://github.com/jdx/mise/commit/7af44322692cf8fd355cf8a37c32321e6127512e)
- raise warning instead if install default gems failed by jiz4oh in [83350be](https://github.com/jdx/mise/commit/83350be1976185dd2dd2f13e8f7a9ee940449d16)

### 🐛 Bug Fixes

- **(python)** correct flavor for macos-x64 by jdx in [7178414](https://github.com/jdx/mise/commit/7178414f2865a7803d8a4e05336d3d2e281b56ec)
- warn if failure installing default packages by jdx in [343f985](https://github.com/jdx/mise/commit/343f985f778fcff842b82d3b5bfd83e1c38ee82e)
- hide missing runtime warning in shim context by jdx in [063dfff](https://github.com/jdx/mise/commit/063dfff9b7336dc31a2d2d3c6d1dfa547e52856a)
- handle tool_version parse failures by jdx in [ce3de29](https://github.com/jdx/mise/commit/ce3de292b83201c87d8f968e8e04af84982e7a00)

### ⚡ Performance

- memoize `which` results by Jeff Dickey in [89291ec](https://github.com/jdx/mise/commit/89291ecaa4bc53e99d61eaf3c24040f9fee11240)

### 🔍 Other Changes

- do not fail workflow if cant post message by Jeff Dickey in [0f3bfd3](https://github.com/jdx/mise/commit/0f3bfd38c5d9a7add05499bb230577ebe849060f)

### New Contributors

* @jiz4oh made their first contribution

## [2024.5.13](https://github.com/jdx/mise/compare/v2024.5.12..v2024.5.13) - 2024-05-14

### 🚀 Features

- pass github token to UBI and cargo-binstall backends. by Adirelle in [e020b55](https://github.com/jdx/mise/commit/e020b554e8415800dc03ecb953b81762850752af)

### 🚜 Refactor

- bubble up resolve errors by jdx in [1e61d23](https://github.com/jdx/mise/commit/1e61d239e09a55464b312c3fd5fb7c609776cf9e)

### 🔍 Other Changes

- always build with git2 feature by Jeff Dickey in [fb51b57](https://github.com/jdx/mise/commit/fb51b57234e3227e00b1866f7ed93bf9d1bc90db)

## [2024.5.12](https://github.com/jdx/mise/compare/v2024.5.11..v2024.5.12) - 2024-05-13

### ⚡ Performance

- various performance tweaks by jdx in [e80ee4c](https://github.com/jdx/mise/commit/e80ee4cc94f43a4e52bf59f590b1924eb6d169c5)

### 🧪 Testing

- only set realpath for macos by Jeff Dickey in [cdd1c93](https://github.com/jdx/mise/commit/cdd1c935f335e0119a7821b22415b792cc83109a)

## [2024.5.11](https://github.com/jdx/mise/compare/v2024.5.10..v2024.5.11) - 2024-05-13

### 🐛 Bug Fixes

- **(exec)** do not default to "latest" if a version is already configured by Jeff Dickey in [f55e8ef](https://github.com/jdx/mise/commit/f55e8efccc2050cbf1a9b14f6396d7ee6fc20828)
- **(self_update)** downgrade reqwest by Jeff Dickey in [0e17a84](https://github.com/jdx/mise/commit/0e17a84ebe9ea087d27a6c825a0bf6840cfcd3ca)
- prompt to trust config files with env vars by Jeff Dickey in [55b3a4b](https://github.com/jdx/mise/commit/55b3a4bb1e394a3830f476594514216a4490de82)

### 🧪 Testing

- work with macos /private tmp dir by Jeff Dickey in [7d8ffaf](https://github.com/jdx/mise/commit/7d8ffaf2bc3341293b4884df2cdf1e14913f5eb6)

## [2024.5.10](https://github.com/jdx/mise/compare/v2024.5.9..v2024.5.10) - 2024-05-13

### 🐛 Bug Fixes

- fixed misc bugs with ubi+pipx backends by jdx in [112dea9](https://github.com/jdx/mise/commit/112dea9e939b47b6ee07cffdcb622c775095ed26)

### 🔍 Other Changes

- updated reqwest by Jeff Dickey in [d927085](https://github.com/jdx/mise/commit/d92708585b62d65a838e37c022a3796de5fefe1d)

### 📦️ Dependency Updates

- update rust crate xx to v1 by renovate[bot] in [13f16cb](https://github.com/jdx/mise/commit/13f16cb30325fa0914084b68a9b78812d74ab41a)

## [2024.5.9](https://github.com/jdx/mise/compare/v2024.5.8..v2024.5.9) - 2024-05-12

### 🐛 Bug Fixes

- `.` in `list-bin-paths` was taken as is to form `PATH` by Yinan Ding in [a88fee8](https://github.com/jdx/mise/commit/a88fee8ca822f08c34846e64e0f018dd98573385)

### 🧪 Testing

- use fd instead of find for macos compat by jdx in [ada2f04](https://github.com/jdx/mise/commit/ada2f04a738542a8e406a1503b585de118737782)
- test_java_corretto is not slow by Jeff Dickey in [92267b1](https://github.com/jdx/mise/commit/92267b1eb861357433005b26134689b0ce43a2b0)
- mark some e2e tests slow by Jeff Dickey in [99f9454](https://github.com/jdx/mise/commit/99f9454e4f062914ab4e4cd950d2f11023bd06bc)
- mark test_pipx as slow by Jeff Dickey in [ced564a](https://github.com/jdx/mise/commit/ced564ab5b8786f74d25d2a92e68c58ca488c122)
- add homebrew to e2e PATH by Jeff Dickey in [f1c7fb3](https://github.com/jdx/mise/commit/f1c7fb3434edc18787a293dc033459f78dd39514)

### 🔍 Other Changes

- add fd to e2e-linux jobs by Jeff Dickey in [9f57dae](https://github.com/jdx/mise/commit/9f57dae9298c4124352c8e7528024265a068ecc9)
- bump usage-lib by jdx in [a794537](https://github.com/jdx/mise/commit/a794537c8dc4fcb720c03b7d322f11f4a24d39f0)
- add permissions for pr comment tool by Jeff Dickey in [64cb8da](https://github.com/jdx/mise/commit/64cb8dacd1b5c39c21cafa03eab361e68ac3a1d9)

### New Contributors

* @FranklinYinanDing made their first contribution in [#2077](https://github.com/jdx/mise/pull/2077)

## [2024.5.8](https://github.com/jdx/mise/compare/v2024.5.7..v2024.5.8) - 2024-05-12

### 🐛 Bug Fixes

- use correct url for aur-bin by Jeff Dickey in [a683c15](https://github.com/jdx/mise/commit/a683c1593d3c83660a42e4e6685522edb20e0480)
- handle race condition when initializing backends with dependencies by jdx in [6ad7926](https://github.com/jdx/mise/commit/6ad7926b7c68017cb99d330fd30c35d5e562610d)

## [2024.5.7](https://github.com/jdx/mise/compare/v2024.5.6..v2024.5.7) - 2024-05-12

### 🧪 Testing

- add coverage report summary by jdx in [c2d1fe2](https://github.com/jdx/mise/commit/c2d1fe21dd28accbb9a97f004a6714f21c49f861)

### 🔍 Other Changes

- fix release job by Jeff Dickey in [a491270](https://github.com/jdx/mise/commit/a49127029b67d39f80708e47cfc20351faca941f)
- fix release job by Jeff Dickey in [90268db](https://github.com/jdx/mise/commit/90268dbdbb71f6e0ba51dbc657536029c2aac099)

## [2024.5.6](https://github.com/jdx/mise/compare/v2024.5.5..v2024.5.6) - 2024-05-12

### 🚀 Features

- add cargo-binstall as dependency for cargo backend by Jeff Dickey in [94868af](https://github.com/jdx/mise/commit/94868afcca9731c43fb48670ed0d7d4f40a4fab8)

### 🐛 Bug Fixes

- performance fix for _.file/_.path by Jeff Dickey in [76202de](https://github.com/jdx/mise/commit/76202ded1bb47ecf9c1a5a7e6f71216aca26c68e)

### 🚜 Refactor

- **(cargo)** improve cargo-binstall check by Jeff Dickey in [d1432e0](https://github.com/jdx/mise/commit/d1432e0316a1e1b335022372ef0896c5b5b7b0df)

### 🧪 Testing

- **(e2e)** fix mise path by Jeff Dickey in [f6de41a](https://github.com/jdx/mise/commit/f6de41af71e7ad03d831bf602c291f38dd6c0fd8)
- isolation of end-to-end tests by Adirelle in [ebe4917](https://github.com/jdx/mise/commit/ebe4917c1a93e1d0b368a865028fc7a6bcc42d08)
- simplify release e2e jobs by Jeff Dickey in [b97a0bb](https://github.com/jdx/mise/commit/b97a0bb563762a4de40ea49a5bccb3a74daafb8f)

### 🔍 Other Changes

- **(aur)** added usage as optional dependency by Jeff Dickey in [5280ece](https://github.com/jdx/mise/commit/5280ece4f2f2337e7dd56c17062a09fdf1e1c808)
- **(codacy)** fix codacy on forks by Jeff Dickey in [c70d567](https://github.com/jdx/mise/commit/c70d567b2529e7054a79e461114a85c2fceb457d)
- switch back to secret for codacy by Jeff Dickey in [7622cfb](https://github.com/jdx/mise/commit/7622cfbb969c9a40638855d13009a72e4dc91ac8)
- added semantic-pr check by jdx in [0c9259f](https://github.com/jdx/mise/commit/0c9259fd421f6ac3c51940505d52ef8cb34c7134)
- fix whitespace by Jeff Dickey in [3eadcb5](https://github.com/jdx/mise/commit/3eadcb548960729e7168842af18c8200b3b70863)

## [2024.5.5](https://github.com/jdx/mise/compare/v2024.5.4..v2024.5.5) - 2024-05-12

### 🐛 Bug Fixes

- **(pipx)** remove unneeded unwrap by Jeff Dickey in [273c73d](https://github.com/jdx/mise/commit/273c73d15d77d42e8ff4ed732335cc418f903e0b)
- resolve bug with backends not resolving mise-installed tools by jdx in [ef1da53](https://github.com/jdx/mise/commit/ef1da53f493ac7aa154d143cae223ad157d2aae1)

## [2024.5.4] - 2024-05-11

### 🚀 Features

- add more directory env var configs by jdx in [88f58b4](https://github.com/jdx/mise/commit/88f58b4ad32500bccc615d543ddc950fa8a0dbd5)

### 🚜 Refactor

- move opts from ToolVersion to ToolVersionRequest struct by jdx in [0cd86a7](https://github.com/jdx/mise/commit/0cd86a7ed9ea0de8b60a765ec38681affc4fe808)
- remove use of mutex by Jeff Dickey in [278d028](https://github.com/jdx/mise/commit/278d028247adcd3a166f11281f81dd7a437e5547)

### 📚 Documentation

- **(changelog)** cleaning up changelog by Jeff Dickey in [845c1af](https://github.com/jdx/mise/commit/845c1afdc58437d083f0f3d50e4733142bef2281)

### 🔍 Other Changes

- Commit from GitHub Actions (test) by mise[bot] in [695f851](https://github.com/jdx/mise/commit/695f8513c0117623ca190c052c603a6b910814ad)
- Merge pull request #2019 from jdx/release by jdx in [6bbd3d1](https://github.com/jdx/mise/commit/6bbd3d17d353eba1684eb11799f6b3684e38b578)
- include symlink error context in error message by Andrew Klotz in [ddd58fc](https://github.com/jdx/mise/commit/ddd58fc7eca72163dd0541596c5b6f06712aec28)
- Merge pull request #2040 from KlotzAndrew/aklotz/show_symlink_error by jdx in [e71a8a0](https://github.com/jdx/mise/commit/e71a8a07e3385bf9bfe0985259325febd3bcf977)
- continue git subtree on error by Jeff Dickey in [a2c590c](https://github.com/jdx/mise/commit/a2c590c7dd82ac60c22844ef7e4ef88da3c1e507)
- squash registry by Jeff Dickey in [143ea6e](https://github.com/jdx/mise/commit/143ea6e589c8232c1d8a61aa33a576815754a3f0)
- reclone registry in release-plz job by Jeff Dickey in [05848a5](https://github.com/jdx/mise/commit/05848a52ea19c27e77ebf30310e7a4753c1b8ab0)
- reclone registry in release-plz job by Jeff Dickey in [c020c1e](https://github.com/jdx/mise/commit/c020c1e60347fcf9538293d141922eff1728500a)
- updated changelog by Jeff Dickey in [0465520](https://github.com/jdx/mise/commit/0465520f4c2d1d78a5ddc0c1d955a062d6f34d3b)
- show bash trace in release-plz by Jeff Dickey in [8a322bc](https://github.com/jdx/mise/commit/8a322bc2740a1c5676574cebdeb4c02726f36358)

### New Contributors

* @KlotzAndrew made their first contribution

<!-- generated by git-cliff -->
