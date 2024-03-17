# Changelog

All notable changes to this project will be documented in this file.

---
## [2024.3.6](https://github.com/jdx/mise/compare/v2024.3.2..v2024.3.6) - 2024-03-17

### 🚀 Features

- very basic dependency support (#1788) - ([7a53a44](https://github.com/jdx/mise/commit/7a53a44c5bbbea7eed281536d869ec4f39de2527)) - jdx

### 🐛 Bug Fixes

- update shorthand for rabbitmq (#1784) - ([d232859](https://github.com/jdx/mise/commit/d232859b5334462a84df8f1f0b4189576712f571)) - roele
- display error message from calling usage (#1786) - ([63fc69b](https://github.com/jdx/mise/commit/63fc69bc751e6ed182243a6021995821d5f4611e)) - jdx
- automatically trust config files in CI (#1791) - ([80b340d](https://github.com/jdx/mise/commit/80b340d8f4a548caa71685a6fca925e2657345dc)) - jdx

### 🚜 Refactor

- move lint tasks from just to mise - ([4f78a8c](https://github.com/jdx/mise/commit/4f78a8cb648246e3f204b426c57662076cc17d5d)) - jdx

### 📚 Documentation

- **(changelog)** use github handles - ([b5ef2f7](https://github.com/jdx/mise/commit/b5ef2f7976e04bf11889062181fc32574eff834a)) - jdx

### 🎨 Styling

- add mise tasks to editorconfig - ([dae8ece](https://github.com/jdx/mise/commit/dae8ece2d891100f86cecea5920bc423e0f4d053)) - jdx
- run lint-fix which has changed slightly - ([6e8dd2f](https://github.com/jdx/mise/commit/6e8dd2fe24adf6d44a17a460c1054738e58f4306)) - jdx
- apply editorconfig changes - ([962bed0](https://github.com/jdx/mise/commit/962bed061ab9218f679f20aa5c53e905981133e0)) - jdx
- new git-cliff format - ([854a4fa](https://github.com/jdx/mise/commit/854a4fae9255968887dc0b0647c993f633666442)) - jdx
- ignore CHANGELOG.md style - ([790cb91](https://github.com/jdx/mise/commit/790cb91a210f5d1d37f4c933798c1802583db753)) - jdx

### 🧪 Testing

- **(mega-linter)** do not use js-standard linter - ([6b63346](https://github.com/jdx/mise/commit/6b63346bdd985964bc824eff03973d2d58d1ad28)) - jdx
- **(mega-linter)** ignore CHANGELOG.md - ([b63b3ac](https://github.com/jdx/mise/commit/b63b3aca3c597ee95db80613b2ea8ca19f0e74c3)) - jdx

### ⚙️ Miscellaneous Tasks

- **(release-plz)** removed some debugging logic - ([f7d7bea](https://github.com/jdx/mise/commit/f7d7bea616c13b31318f2e7da287aa71face8e57)) - jdx
- **(release-plz)** show actual version in PR body - ([e1ef708](https://github.com/jdx/mise/commit/e1ef708745e79bd019c77740820daefca5491b2e)) - jdx
- **(release-plz)** tweaking logic to prevent extra PR - ([8673000](https://github.com/jdx/mise/commit/86730008cd2f60d2767296f97175805225c83951)) - jdx
- **(release-plz)** make logic work for calver - ([890c919](https://github.com/jdx/mise/commit/890c919081f984f3d506c2b1d2712c8cff6f5e6b)) - jdx
- **(release-plz)** make logic work for calver - ([bb5a178](https://github.com/jdx/mise/commit/bb5a178b0642416d0e3dac8a9162a9f0732cf146)) - jdx
- **(release-plz)** fix git diffs - ([6c7e779](https://github.com/jdx/mise/commit/6c7e77944a24b289aaba887f64b7f3c63cb9e5ab)) - jdx
- **(release-plz)** create gh release - ([f9ff369](https://github.com/jdx/mise/commit/f9ff369eb1176e31044fc463fdca08397def5a81)) - jdx
- **(release-plz)** fixing gpg key - ([8286ded](https://github.com/jdx/mise/commit/8286ded8297b858e7136831e75e4c37fa49e6186)) - jdx
- **(release-plz)** fixing gpg key - ([abb1dfe](https://github.com/jdx/mise/commit/abb1dfed78e49cf2bee4a137e92879ffd7f2fb03)) - jdx
- **(release-plz)** do not publish a new release PR immediately - ([b3ae753](https://github.com/jdx/mise/commit/b3ae753fdde1fef17b4f13a1ecc8b23cb1da575c)) - jdx
- **(release-plz)** prefix versions with "v" - ([3354b55](https://github.com/jdx/mise/commit/3354b551adab7082d5cc533e5d9d0bfe272958b4)) - jdx
- **(test)** cache mise installed tools - ([0e433b9](https://github.com/jdx/mise/commit/0e433b975a5d8c28ae5c0cbd86d3b19e03146a83)) - jdx
- cargo update - ([6391239](https://github.com/jdx/mise/commit/639123930eec8e057de7da790cb71d4a2b9e17a2)) - jdx
- install tools before unit tests - ([f7456eb](https://github.com/jdx/mise/commit/f7456ebc539a4b27ec067bc480bc0aba1466e55b)) - jdx
- added git-cliff - ([0ccdf36](https://github.com/jdx/mise/commit/0ccdf36df153ddc3ac1a2714ee9b4a2116dfc918)) - jdx
- ensure `mise install` is run before lint-fix - ([e8a172f](https://github.com/jdx/mise/commit/e8a172f98ebc837619f3766777e489f3b99f36f4)) - jdx
- added release-plz workflow (#1787) - ([83fe1ec](https://github.com/jdx/mise/commit/83fe1ecc266caf094fc1cfb251ef1c0cc35afe1b)) - jdx
- set gpg key - ([467097f](https://github.com/jdx/mise/commit/467097f925053a27f0ede2a506e894562d191a09)) - jdx
- temporarily disable self-update test - ([5cb39a4](https://github.com/jdx/mise/commit/5cb39a4259f332e5bccec082f1d7cd6127da5f55)) - jdx

### Outdated

- add --json flag (#1785) - ([ec8dbdf](https://github.com/jdx/mise/commit/ec8dbdf0659a73ba64ca8a5bd1bf0e021fce0b4b)) - jdx

---
## [2024.3.2](https://github.com/jdx/mise/compare/v2024.3.1..v2024.3.2) - 2024-03-15

### 🚀 Features

- **(task)** add option to show hidden tasks in dependency tree (#1756) - ([b90ffea](https://github.com/jdx/mise/commit/b90ffea2dc2ee6628e78da84b4118572a3cb9938)) - roele

### 🐛 Bug Fixes

- **(go)** go backend supports versions prefixed with 'v' (#1753) - ([668acc3](https://github.com/jdx/mise/commit/668acc3e6431fdd6734f8a0f726d5d8a0d4ce687)) - roele
- **(npm)** mise use -g npm:yarn@latest installs wrong version (#1752) - ([b7a9067](https://github.com/jdx/mise/commit/b7a90677507b5d5bd8aec1a677cf61adc5288cad)) - roele
- **(task)** document task.hide (#1754) - ([ac829f0](https://github.com/jdx/mise/commit/ac829f093d62875e2715ef4c1c5c134ffdad7932)) - roele
- watch env._.source files (#1770) - ([5863a19](https://github.com/jdx/mise/commit/5863a191fbf8a25b60632e71a120395256ac8933)) - nicolas-geniteau
- prepend virtualenv path rather than append (#1751) - ([5c9e82e](https://github.com/jdx/mise/commit/5c9e82ececcf5e5e0965b093cd45f46b9267e06f)) - kalvinnchau

---
## [2024.3.1](https://github.com/jdx/mise/compare/v2024.2.19..v2024.3.1) - 2024-03-04

### 🐛 Bug Fixes

- **(java)** sdkmanrc zulu JVMs are missing in mise (#1719) - ([4a529c0](https://github.com/jdx/mise/commit/4a529c02824392fe54b2618f3f740d01876bd4b3)) - roele

---
## [2024.2.19](https://github.com/jdx/mise/compare/v2024.2.18..v2024.2.19) - 2024-02-28

### Release

- use normal mise data dir in justfile (#1718) - ([1014d82](https://github.com/jdx/mise/commit/1014d820a451ab19cc32d552ffbc750fc9fab47f)) - jdx

---
## [2024.2.18](https://github.com/jdx/mise/compare/v2024.2.17..v2024.2.18) - 2024-02-24

### 📚 Documentation

- make README logo link to site (#1695) - ([4adac60](https://github.com/jdx/mise/commit/4adac60c41767bb18b479ce2532324bf33d1c946)) - booniepepper

### Release

- auto-install plugins - ([3b665e2](https://github.com/jdx/mise/commit/3b665e238baad818aef8f66c74733d6c4e518312)) - jdx

---
## [2024.2.17](https://github.com/jdx/mise/compare/v2024.2.16..v2024.2.17) - 2024-02-22

### 🐛 Bug Fixes

- **(bun)** install bunx symlink (#1688) - ([28d4154](https://github.com/jdx/mise/commit/28d4154daa35015dc4e38fad1804301c3a2704ce)) - booniepepper
- **(go)** reflect on proper path for `GOROOT` (#1661) - ([aed9563](https://github.com/jdx/mise/commit/aed9563a15e8107b61697a69aa2dff6252624faa)) - wheinze
- allow go forge to install SHA versions when no tagged versions present (#1683) - ([0958953](https://github.com/jdx/mise/commit/095895346e01b77b89454b95f538c1bb53b7aa98)) - Ajpantuso

### 🚜 Refactor

- auto-try miseprintln macro - ([1d0fb78](https://github.com/jdx/mise/commit/1d0fb78377720fac356171ebd8d6cbf29a2f0ad6)) - jdx

### 📚 Documentation

- add missing alt text (#1691) - ([0c7e69b](https://github.com/jdx/mise/commit/0c7e69b0a8483f218236f3e58a949f48c375940c)) - wheinze
- improve formatting/colors - ([5c6e4dc](https://github.com/jdx/mise/commit/5c6e4dc79828b96e5cfb35865a9176670c8f6737)) - jdx
- revamped output (#1694) - ([54a5620](https://github.com/jdx/mise/commit/54a56208b3b8d4bac1d2e544d11e5a3d86685b17)) - jdx

### 🧪 Testing

- **(integration)** introduce rust based integration suite (#1612) - ([6c656f8](https://github.com/jdx/mise/commit/6c656f8ce447bd41aa8d08ce5e1ed14bd0031490)) - Ajpantuso

---
## [2024.2.16](https://github.com/jdx/mise/compare/v2024.2.15..v2024.2.16) - 2024-02-15

### Compeltions

- use dash compatible syntax - ([10dbf54](https://github.com/jdx/mise/commit/10dbf54650b9ed90eb4a9ba86fe5499db23357d8)) - jdx

---
## [2024.2.8](https://github.com/jdx/mise/compare/v2024.2.7..v2024.2.8) - 2024-02-09

### Go

- GOROOT/GOBIN/GOPATH changes (#1641) - ([786220c](https://github.com/jdx/mise/commit/786220c6178625980bdcc61403c32db19d51360f)) - jdx

### Tasks

- ignore non-executable tasks (#1642) - ([a334924](https://github.com/jdx/mise/commit/a3349240efb5a281a3895a9883f6ddc20d1af315)) - jdx

---
## [2024.2.7](https://github.com/jdx/mise/compare/v2024.2.6..v2024.2.7) - 2024-02-08

### Fish

- fix command not found handler - ([a30842b](https://github.com/jdx/mise/commit/a30842b5062caca6d07b68307d66ebf376ff01c8)) - jdx

### Ls

- add installed/active flags (#1629) - ([d8efa0e](https://github.com/jdx/mise/commit/d8efa0e49a8b30e46905aacc1592d35ce0364acb)) - jdx

### Tasks

- support global file tasks (#1627) - ([f288b40](https://github.com/jdx/mise/commit/f288b409c56a7fb0160de3c0d60075576dcf5995)) - jdx

---
## [2024.2.6](https://github.com/jdx/mise/compare/v2024.2.5..v2024.2.6) - 2024-02-07

### Fish

- reuse existing command_not_found handler (#1624) - ([521c31e](https://github.com/jdx/mise/commit/521c31eb2877d5fdb7f7460f7d9006321a09a097)) - jdx

---
## [2024.2.5](https://github.com/jdx/mise/compare/v2024.2.4..v2024.2.5) - 2024-02-06

### 📚 Documentation

- add some info (#1614) - ([6e8a97f](https://github.com/jdx/mise/commit/6e8a97f2e10f81f3c3546bd4dce45ac4718f5382)) - jdx
- cli help - ([6a004a7](https://github.com/jdx/mise/commit/6a004a723d93cc3a253321ab9b83058dea6c6c89)) - jdx

### Env-file

- add dotenv paths to watch files (#1615) - ([d15ea44](https://github.com/jdx/mise/commit/d15ea44c8146429ee655b5404c94fa1c5c0e1d9e)) - jdx

### Tasks

- support "false" env vars (#1603) - ([d959790](https://github.com/jdx/mise/commit/d9597906d796900f751a1dc01a39b3942655ddcd)) - jdx

---
## [2024.2.4](https://github.com/jdx/mise/compare/v2024.2.3..v2024.2.4) - 2024-02-03

### 🐛 Bug Fixes

- **(tasks)** fix parsing of alias attribute (#1596) - ([a43f40b](https://github.com/jdx/mise/commit/a43f40bdf9b9898789db0125e139df8b29045021)) - Ajpantuso

---
## [2024.2.3](https://github.com/jdx/mise/compare/v2024.2.2..v2024.2.3) - 2024-02-02

### Tasks

- skip running glob if no patterns - ([0eae892](https://github.com/jdx/mise/commit/0eae892c67598c788b7ca6311aaaac075279717b)) - jdx

---
## [2024.2.2](https://github.com/jdx/mise/compare/v2024.2.1..v2024.2.2) - 2024-02-02

### Plugins

- ui tweak - ([d3748ef](https://github.com/jdx/mise/commit/d3748efb24bb7b7894c5a877e4d49aff1738c0b8)) - jdx

### Python

- minor UI tweak - ([fbe2578](https://github.com/jdx/mise/commit/fbe2578e8770c8913e6bb029ea08ce7b18e6db4a)) - jdx

### Release

- clear cache on mise.run - ([1d00fbd](https://github.com/jdx/mise/commit/1d00fbdb904ce83737898e4dc2f8ba5edbf2a568)) - jdx

---
## [2024.2.1](https://github.com/jdx/mise/compare/v2024.2.0..v2024.2.1) - 2024-02-01

### 📚 Documentation

- add "dr" alias - ([67e9e30](https://github.com/jdx/mise/commit/67e9e302c979ca16e8e1160e3a7123f08dd1ab82)) - jdx

### ⚙️ Miscellaneous Tasks

- use m1 macs - ([98a6d1f](https://github.com/jdx/mise/commit/98a6d1f2441a8fb839f65a5a66d7053bdffef36b)) - jdx

### Settings

- improve set/ls commands (#1579) - ([dc0e793](https://github.com/jdx/mise/commit/dc0e793d5584461809bcdc799662184964427b4a)) - jdx

---
## [2024.2.0](https://github.com/jdx/mise/compare/v2024.1.35..v2024.2.0) - 2024-02-01

### 🚀 Features

- **(tasks)** make script task dirs configurable (#1571) - ([90c35ab](https://github.com/jdx/mise/commit/90c35ab8885759c570a31fe73f8fec458d92a7ef)) - Ajpantuso

### 🐛 Bug Fixes

- **(tasks)** prevent dependency cycles (#1575) - ([08429bb](https://github.com/jdx/mise/commit/08429bbee21d2400282d584cca2c26fc1f469226)) - Ajpantuso

### 📚 Documentation

- fix github action - ([9adc718](https://github.com/jdx/mise/commit/9adc7186b86a539e6f3e6a358d5822834e8be8fa)) - jdx
- fix github action - ([3849cdb](https://github.com/jdx/mise/commit/3849cdb8d0d4396e32fa9f555d03662efb2c41ab)) - jdx
- skip cargo-msrv - ([ff3a555](https://github.com/jdx/mise/commit/ff3a5559dde35bd47ed072704bf2bc67478ce307)) - jdx
- fix test runner - ([779c484](https://github.com/jdx/mise/commit/779c48491dfc223c2a7c8c80b8396ba9050ec54d)) - jdx
- fix dev test - ([b92566f](https://github.com/jdx/mise/commit/b92566ffc2ccf2336fafddff3bb5dd62536b1f5f)) - jdx

### ⚙️ Miscellaneous Tasks

- skip checkout for homebrew bump - ([de5e5b6](https://github.com/jdx/mise/commit/de5e5b6b33063e577f53ceb8f8de14b5035c1c4d)) - jdx

### Status

- make missing tool warning more granular (#1577) - ([6c6afe1](https://github.com/jdx/mise/commit/6c6afe194872030ec0fc3be7f8ffacd9ca71de25)) - jdx

### Tasks

- refactor to use BTreeMap instead of sorting - ([438e6a4](https://github.com/jdx/mise/commit/438e6a4dec10e17b0cffca1d921acedf7d6db324)) - jdx

---
## [2024.1.35](https://github.com/jdx/mise/compare/v2024.1.34..v2024.1.35) - 2024-01-31

### Shims

- use activate_agressive setting - ([c8837fe](https://github.com/jdx/mise/commit/c8837fea7605167c9be2e964acbb29a6ba4e48aa)) - jdx

---
## [2024.1.33](https://github.com/jdx/mise/compare/v2024.1.32..v2024.1.33) - 2024-01-30

### Shims

- treat anything not rtx/mise as a shim - ([fae51a7](https://github.com/jdx/mise/commit/fae51a7ef38890fbf3f864957e0c0c6f1be0cf65)) - jdx

---
## [2024.1.32](https://github.com/jdx/mise/compare/v2024.1.31..v2024.1.32) - 2024-01-30

### Poetry

- use compiled python - ([d3020cc](https://github.com/jdx/mise/commit/d3020cc26575864a38dbffd530ad1f7ebff64f64)) - jdx

### Python

- fix settings env vars - ([b122c19](https://github.com/jdx/mise/commit/b122c19935297a3220c438607798fc7fe52df1c1)) - jdx

---
## [2024.1.31](https://github.com/jdx/mise/compare/v2024.1.30..v2024.1.31) - 2024-01-30

### 🚀 Features

- **(tasks)** add task timing to run command (#1536) - ([6a16dc0](https://github.com/jdx/mise/commit/6a16dc0fe0beea743ed474eee7f29239887f418d)) - Ajpantuso

### 🐛 Bug Fixes

- properly handle executable shims when getting diffs (#1545) - ([add7253](https://github.com/jdx/mise/commit/add725381b2e798e6efbdf40ac356e4f02a17dbd)) - Ajpantuso

### Go

- clean up e2e tests - ([2660406](https://github.com/jdx/mise/commit/2660406a4744e789ab39a58e1732f880dcd26b4d)) - jdx

### Python

- only show precompiled warning if going to use precompiled - ([74fd185](https://github.com/jdx/mise/commit/74fd1852bef8244f2cb4c51b58f11116d10d0c11)) - jdx
- fix linux precompiled (#1559) - ([d885c66](https://github.com/jdx/mise/commit/d885c6693f1a6fd4260a6a4313396cd953d9da80)) - jdx

---
## [2024.1.27](https://github.com/jdx/mise/compare/v2024.1.26..v2024.1.27) - 2024-01-26

### 🚀 Features

- **(run)** match tasks to run with glob patterns (#1528) - ([7b3ae2e](https://github.com/jdx/mise/commit/7b3ae2e7a6f42f23d79586cd7a2e6ddc1f9efa89)) - Ajpantuso
- **(tasks)** unify glob strategy for tasks and dependencies (#1533) - ([6be2c83](https://github.com/jdx/mise/commit/6be2c83c2ef2d0eccef77b3315033a2613ec8fb3)) - Ajpantuso

### 📚 Documentation

- display missing/extra shims (#1529) - ([a4b6418](https://github.com/jdx/mise/commit/a4b641825f28cf6511321c1d28bb997c73b77402)) - jdx

### Env

- resolve env vars in order (#1519) - ([7dce359](https://github.com/jdx/mise/commit/7dce359a31f06e7f32366ee75c1f975d667000d7)) - jdx

---
## [2024.1.26](https://github.com/jdx/mise/compare/v2024.1.25..v2024.1.26) - 2024-01-25

### 🚀 Features

- **(doctor)** identify missing/extra shims (#1524) - ([0737239](https://github.com/jdx/mise/commit/07372390fdc6336856d6f3f6fb18efe03f099715)) - Ajpantuso
- **(tasks)** infer bash task topics from folder structure (#1520) - ([2d63b59](https://github.com/jdx/mise/commit/2d63b59fd4f4c2a0cecd357f0b25cec3397fff61)) - Ajpantuso

### 🚜 Refactor

- env parsing (#1515) - ([a5573cc](https://github.com/jdx/mise/commit/a5573ccd5a78f5fed1f449f5c4135ed168c03d51)) - jdx

### Bun|python

- use target_feature to use correct precompiled runtimes (#1512) - ([578ff24](https://github.com/jdx/mise/commit/578ff24321c6254acadaed4b91498dc03a03911b)) - jdx

### Config

- do not follow symbolic links for trusted paths (#1513) - ([032e325](https://github.com/jdx/mise/commit/032e325f9f44b80e920c9e4698c17233c7011ca7)) - jdx
- refactor min_version logic (#1516) - ([7ce6d3f](https://github.com/jdx/mise/commit/7ce6d3fe52cf5bc3df66748e16703a0a0e5bcbc5)) - jdx

### Env

- sort env vars coming back from exec-env (#1518) - ([278878e](https://github.com/jdx/mise/commit/278878e69bb4a85e8219fb74aab51e55be651f0a)) - jdx
- order flags in docs - ([1018b56](https://github.com/jdx/mise/commit/1018b5622c3bda4d0d9fa36b4fa9c1143aabd676)) - jdx

---
## [2024.1.25](https://github.com/jdx/mise/compare/v2024.1.24..v2024.1.25) - 2024-01-24

### 🚀 Features

- **(config)** support arrays of env tables (#1503) - ([12d87c2](https://github.com/jdx/mise/commit/12d87c215fc292df84484de810ff1975477e2513)) - Ajpantuso
- **(template)** add join_path filter (#1508) - ([9341810](https://github.com/jdx/mise/commit/9341810203d3e66dd6498400900ad6d6e1eb7c14)) - Ajpantuso
- add other arm targets for cargo-binstall (#1510) - ([6845239](https://github.com/jdx/mise/commit/6845239648dbd08d097064a519250c32650a60ea)) - yossydev

### 🐛 Bug Fixes

- **(tasks)** prevent implicit globbing of sources/outputs (#1509) - ([9ac1435](https://github.com/jdx/mise/commit/9ac14357c7f23c00c29da1ada37644609df85234)) - Ajpantuso

### Cargo

- allow cargo-binstall from mise itself (#1507) - ([651ec02](https://github.com/jdx/mise/commit/651ec029c52fdcddb00f8f8c13dbbaa2f08426aa)) - jdx

---
## [2024.1.24](https://github.com/jdx/mise/compare/v2024.1.23..v2024.1.24) - 2024-01-20

### Activate

- added --shims (#1483) - ([73b9b72](https://github.com/jdx/mise/commit/73b9b7244060b0fd32470c9b31f153b1a7ee6a45)) - jdx

### Aur

- fix conflicts - ([729de0c](https://github.com/jdx/mise/commit/729de0cb6c27646e30ee7be99d2f478f3431258c)) - jdx

### Fish_completion

- use `sort -r` instead of `tac` (#1486) - ([334ee48](https://github.com/jdx/mise/commit/334ee48138448bc5ba320da45c8d60e9cdcec2c2)) - jdx

### Runtime_symlinks

- do not fail if version parsing fails - ([8d39995](https://github.com/jdx/mise/commit/8d39995e615527ba7187b3d25369a506bcb21e0c)) - jdx

---
## [2024.1.23](https://github.com/jdx/mise/compare/v2024.1.22..v2024.1.23) - 2024-01-18

### Plugins

- improve post-plugin-update script (#1479) - ([383600c](https://github.com/jdx/mise/commit/383600cc7631663fdaae6db9e2ab033db36a3bb8)) - jdx

### Tasks

- only show select if no task specified (#1481) - ([8667bc5](https://github.com/jdx/mise/commit/8667bc51dd7af25966e423b4d84992dc8ff4fccf)) - jdx
- show cursor on ctrl-c - ([ebc5fe7](https://github.com/jdx/mise/commit/ebc5fe78bc97ecf99251438e6f305908bb134833)) - jdx
- fix project_root when using .config/mise.toml or .mise/config.toml (#1482) - ([f0965ad](https://github.com/jdx/mise/commit/f0965ad57faa36f14adf1809535eae6738f6578c)) - jdx

---
## [2024.1.22](https://github.com/jdx/mise/compare/v2024.1.21..v2024.1.22) - 2024-01-17

### 🐛 Bug Fixes

- no panic on missing current dir (#1462) - ([9c4b7fb](https://github.com/jdx/mise/commit/9c4b7fb652cab04864841b02d59ccd7581a1e805)) - tamasfe
- always load global configs (#1466) - ([fd9da12](https://github.com/jdx/mise/commit/fd9da129e093332113ca10098e14bf21660017db)) - tamasfe

### Tasks

- support array of commands directly (#1474) - ([62679b3](https://github.com/jdx/mise/commit/62679b3b25281b53710f195d698269a2883c8626)) - jdx

---
## [2024.1.21](https://github.com/jdx/mise/compare/v2024.1.20..v2024.1.21) - 2024-01-15

### 🐛 Bug Fixes

- bail out of task suggestion if there are no tasks (#1460) - ([d52d2ca](https://github.com/jdx/mise/commit/d52d2ca064f3ceed70ed96db3912cda909d02c23)) - roele

---
## [2024.1.20](https://github.com/jdx/mise/compare/v2024.1.19..v2024.1.20) - 2024-01-14

### 🚀 Features

- add command to print task dependency tree (#1440) - ([ef2cc0c](https://github.com/jdx/mise/commit/ef2cc0c9e536838e0cf89cc1cc2b67b017517cdb)) - roele
- add completions for task deps command (#1456) - ([e0ba235](https://github.com/jdx/mise/commit/e0ba235d8127a488f29f74dd07a714489ed6bab3)) - roele
- add interactive selection for tasks if task was not found (#1459) - ([6a93748](https://github.com/jdx/mise/commit/6a93748572e61c18ec1a798e8e658a72a574ae50)) - roele

### ⚙️ Miscellaneous Tasks

- re-enable standalone test - ([7e4e79b](https://github.com/jdx/mise/commit/7e4e79bcdcc541027bc3ea2fccc11fb0f0c07a5d)) - jdx

### Tasks

- enable stdin under interleaved - ([b6dfb31](https://github.com/jdx/mise/commit/b6dfb311e412e119e137186d6143644d018a6cfc)) - jdx

---
## [2024.1.19](https://github.com/jdx/mise/compare/v2024.1.18..v2024.1.19) - 2024-01-13

### 🚜 Refactor

- remove PluginName type alias - ([dedb762](https://github.com/jdx/mise/commit/dedb7624ad4708ce0434a963737a17754075d3a0)) - jdx
- rename Plugin trait to Forge - ([ec4efea](https://github.com/jdx/mise/commit/ec4efea054626f9451bb54831abdd95ff98c64d1)) - jdx
- clean up arg imports - ([5091fc6](https://github.com/jdx/mise/commit/5091fc6b04fd1e4795bbd636772c30432b825ef3)) - jdx
- clean up arg imports (#1451) - ([5e36828](https://github.com/jdx/mise/commit/5e368289e5a80913aa000564bb500e69d6b3008f)) - jdx

### Config

- allow using "env._.file|env._.path" instead of "env.mise.file|env.mise.path" - ([cf93693](https://github.com/jdx/mise/commit/cf936931201d6597ad556bd17556d47dc3d125c6)) - jdx

### Npm

- testing - ([2ee66cb](https://github.com/jdx/mise/commit/2ee66cb91837fde144bf7acbb1028372c1cd7d9a)) - jdx

---
## [2024.1.18](https://github.com/jdx/mise/compare/v2024.1.17..v2024.1.18) - 2024-01-12

### Release

- fix mise-docs publishing - ([1dcac6d](https://github.com/jdx/mise/commit/1dcac6d4e05c80b56d1371f434776057d3ca9dc7)) - jdx
- temporarily disable standalone test - ([d4f54ad](https://github.com/jdx/mise/commit/d4f54adbbf840599aeb4229c9330262569b563b5)) - jdx

---
## [2024.1.17](https://github.com/jdx/mise/compare/v2024.1.16..v2024.1.17) - 2024-01-12

### Activate

- use less aggressive PATH modifications by default - ([07e1921](https://github.com/jdx/mise/commit/07e19212053bdaf4ea2ca3968e3f3559d6f49668)) - jdx

### Settings

- remove warning about moving to settings.toml - ([750141e](https://github.com/jdx/mise/commit/750141eff2721e2fbe4ab116952d04b67d2ee187)) - jdx
- read from config.toml (#1439) - ([cdfda7d](https://github.com/jdx/mise/commit/cdfda7d7e94f82f091bf394d50f28aaa6139dbf2)) - jdx

---
## [2024.1.16](https://github.com/jdx/mise/compare/v2024.1.15..v2024.1.16) - 2024-01-11

### Env-vars

- improvements (#1435) - ([f386503](https://github.com/jdx/mise/commit/f386503d54bd32726e9ded773360abd5d8d00ab8)) - jdx

### Python

- do not panic if precompiled arch/os is not supported (#1434) - ([3d12e5a](https://github.com/jdx/mise/commit/3d12e5aeac333e6a98425ec6031016dfd792ac6e)) - jdx

---
## [2024.1.15](https://github.com/jdx/mise/compare/v2024.1.14..v2024.1.15) - 2024-01-10

### 🐛 Bug Fixes

- **(python)** fixes #1419 (#1420) - ([2003c6b](https://github.com/jdx/mise/commit/2003c6b045559421be756db0ca403b1a6d76f64b)) - gasuketsu

### Python

- fix some precompiled issues (#1431) - ([ffb6489](https://github.com/jdx/mise/commit/ffb6489c1b0e54f0caa2e6ca4ddf855469950809)) - jdx

---
## [2024.1.12](https://github.com/jdx/mise/compare/v2024.1.11..v2024.1.12) - 2024-01-07

### Python

- fixed python_compile and all_compile settings - ([5ddbf68](https://github.com/jdx/mise/commit/5ddbf68af1f32abbf8cff406a6d17d0898d4c81f)) - jdx

---
## [2024.1.11](https://github.com/jdx/mise/compare/v2024.1.10..v2024.1.11) - 2024-01-07

### Settings.toml

- add to doctor and fix warning - ([fcf9173](https://github.com/jdx/mise/commit/fcf91739bc0241114242afb9e8de6bdf819cd7ba)) - jdx

### Toml

- check min_version field - ([8de42a0](https://github.com/jdx/mise/commit/8de42a0be94098c722ba8b9eef8eca505f5838c2)) - jdx

---
## [2024.1.10](https://github.com/jdx/mise/compare/v2024.1.9..v2024.1.10) - 2024-01-07

### 🐛 Bug Fixes

- nix flake build errors (#1390) - ([f42759d](https://github.com/jdx/mise/commit/f42759d1cafaa206357e2eeaf3b1843cb80f65fb)) - nokazn

---
## [2024.1.9](https://github.com/jdx/mise/compare/v2024.1.8..v2024.1.9) - 2024-01-07

### Python

- add support for precompiled binaries (#1388) - ([128142f](https://github.com/jdx/mise/commit/128142f545f79d23c581eba3f2c0fcc122764134)) - jdx

---
## [2024.1.8](https://github.com/jdx/mise/compare/v2024.1.7..v2024.1.8) - 2024-01-06

### 🐛 Bug Fixes

- **(java)** enable macOS integration hint for Zulu distribution (#1381) - ([3bfb33e](https://github.com/jdx/mise/commit/3bfb33e2b6ea00c461ccfe32b4f72fc43769b80b)) - roele

---
## [2024.1.6](https://github.com/jdx/mise/compare/v2024.1.5..v2024.1.6) - 2024-01-04

### 🧪 Testing

- fixed elixir test case - ([9b596c6](https://github.com/jdx/mise/commit/9b596c6dadcf0f54b3637d10e1885281e1a1b534)) - jdx

### Tasks

- set CLICOLOR_FORCE=1 and FORCE_COLOR=1 (#1364) - ([3d2e132](https://github.com/jdx/mise/commit/3d2e132f1df5aa20e9d712df697746ddeea6c465)) - jdx
- set --interleaved if graph is linear (#1365) - ([fb2b218](https://github.com/jdx/mise/commit/fb2b218da96a09b1f9db3984aa217c1b11e1a3de)) - jdx

---
## [2024.1.5](https://github.com/jdx/mise/compare/v2024.1.4..v2024.1.5) - 2024-01-04

### 🐛 Bug Fixes

- remove comma from conflicts (#1353) - ([38381a6](https://github.com/jdx/mise/commit/38381a69d46a7fa4afd8d3254b2290bc5a28019b)) - pdecat

### Env

- use `mise.file`/`mise.path` config (#1361) - ([fb8a9df](https://github.com/jdx/mise/commit/fb8a9dfbb052ecb770e0ef7ffd4f811f7de522b7)) - jdx

### Logging

- prevent loading multiple times (#1358) - ([01a20ad](https://github.com/jdx/mise/commit/01a20ad0dd8bb073ac200b5b4459994c77512020)) - jdx

### Migrate

- skip ruby installs - ([c23e467](https://github.com/jdx/mise/commit/c23e467717105e34ac805638dfeb5fcac3f991a2)) - jdx

### Not-found

- use "[" instead of "test" (#1355) - ([ee6a18c](https://github.com/jdx/mise/commit/ee6a18c1416d51202e046b8703891184daee772e)) - jdx

---
## [2024.1.4](https://github.com/jdx/mise/compare/v2024.1.3..v2024.1.4) - 2024-01-04

### 🐛 Bug Fixes

- **(java)** use tar.gz archives to enable symlink support (#1343) - ([fd3ecdf](https://github.com/jdx/mise/commit/fd3ecdfa1b8198e3c79883afc9f984c49c3aa3a0)) - roele

### Aur

- add "replaces" field (#1345) - ([581a1fe](https://github.com/jdx/mise/commit/581a1fec088fdbf90c38dc9e79fc0449df2218a5)) - jdx

### Install

- docs - ([eb73edf](https://github.com/jdx/mise/commit/eb73edfab75d8a2b5bd58be71b2ccbd172b92413)) - jdx

### Plugin-install

- fix ssh urls (#1349) - ([9e252d0](https://github.com/jdx/mise/commit/9e252d0b97a2a6649beff42884dbc5cd4e799c19)) - jdx

---
## [2024.1.3](https://github.com/jdx/mise/compare/v2024.1.2..v2024.1.3) - 2024-01-03

### ⚙️ Miscellaneous Tasks

- use mise docker containers - ([d5d2d39](https://github.com/jdx/mise/commit/d5d2d39aa1a44a6421dff150da42083c4247cff9)) - jdx
- skip committing docs if no changes - ([7f6545c](https://github.com/jdx/mise/commit/7f6545c2630a1f54b864903851c24e68b3da3d2f)) - jdx

### Standalone

- use ~/.local/bin/mise instead of ~/.local/share/mise/bin/mise - ([cd2045d](https://github.com/jdx/mise/commit/cd2045d793c76b9dcf7d26c567cf163a6138f408)) - jdx

---
## [2024.1.2](https://github.com/jdx/mise/compare/v2024.1.1..v2024.1.2) - 2024-01-03

### Python

- fix venv python path - ([e2d50a2](https://github.com/jdx/mise/commit/e2d50a2f25c0c64c207f82e957e691671d52ddbd)) - jdx

---
## [2024.1.1](https://github.com/jdx/mise/compare/v2024.1.0..v2024.1.1) - 2024-01-03

### 📚 Documentation

- tweak cli reference - ([ba5f610](https://github.com/jdx/mise/commit/ba5f6108b1b91952295e4871f63c559ff01c7c64)) - jdx
- fixed reading settings from config - ([a30a5f1](https://github.com/jdx/mise/commit/a30a5f104da41794aa8a2813919f046945ed9ae6)) - jdx

### Use

- fix MISE_ASDF_COMPAT=1 (#1340) - ([edbdc7c](https://github.com/jdx/mise/commit/edbdc7c448e1db522d1304c004aa36ed0e99f0c4)) - jdx

---
## [2024.1.0](https://github.com/jdx/mise/compare/v2024.0.0..v2024.1.0) - 2024-01-02

### ⚙️ Miscellaneous Tasks

- Configure Renovate (#1307) - ([0f980b2](https://github.com/jdx/mise/commit/0f980b22382b4da002336f6b456d5181416bf75b)) - renovate[bot]
- disable auto package updates - ([e00fb1f](https://github.com/jdx/mise/commit/e00fb1fde649ecc85aa40ac8846f71316d679e54)) - jdx

### Env

- added RTX_ENV_FILE config (#1305) - ([484806f](https://github.com/jdx/mise/commit/484806fd980d6c39aaa76e4066b18f54edd35137)) - jdx

### Env-vars

- added "ev" alias - ([8d98b91](https://github.com/jdx/mise/commit/8d98b9158b6dc4d6c36332a5f52061e81cc87d91)) - jdx
- added "ev" alias - ([4bfe580](https://github.com/jdx/mise/commit/4bfe580eef8a8192f621ea729c8013ef141dacf3)) - jdx

### Renovate

- ignore asdf/nodejs - ([acc9a68](https://github.com/jdx/mise/commit/acc9a6803d6d3087a847529baa7d7e341ef46cc2)) - jdx
- ignore nodenv - ([4d921c7](https://github.com/jdx/mise/commit/4d921c7608e4807ae765383253e100763d04bd75)) - jdx
- tuck away - ([4361f03](https://github.com/jdx/mise/commit/4361f0385a82da470cfe47a5044a00ca783c9ddc)) - jdx
- disable dashboard - ([2c569fc](https://github.com/jdx/mise/commit/2c569fc01a77987e6823dc749eb917f1fe5a0cf0)) - jdx
- disable dashboard - ([400ac0a](https://github.com/jdx/mise/commit/400ac0a0ff64cf5a6846f662df5dc432237e87b2)) - jdx

---
## [2024.0.0] - 2023-12-31

### 📚 Documentation

- reference new docs url - ([5532b2a](https://github.com/jdx/mise/commit/5532b2a1c2824537e6e03928f5dafd559bd46455)) - jdx

### ⚙️ Miscellaneous Tasks

- Release rtx-cli version 2024.0.0 - ([adc8160](https://github.com/jdx/mise/commit/adc8160213e55c10b0842a4e68fc223daad27d41)) - jdx

### Node

- remove node-build support (#1304) - ([be5226f](https://github.com/jdx/mise/commit/be5226fb7eed5c0d2e5fa34c88021d11e9702448)) - jdx

### Task

- read RTX_TASK_OUTPUT as lowercase (#1288) - ([1186a6b](https://github.com/jdx/mise/commit/1186a6bc430483bc55b4b904c298b8c462e0cceb)) - jdx

<!-- generated by git-cliff -->
