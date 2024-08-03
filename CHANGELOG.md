# Changelog

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

* @mollyIV made their first contribution in [#2411](https://github.com/jdx/mise/pull/2411)

## [2024.7.5](https://github.com/jdx/mise/compare/v2024.7.4..v2024.7.5) - 2024-07-29

### 🐛 Bug Fixes

- mise use does not create a local .mise.toml anymore by [@roele](https://github.com/roele) in [#2406](https://github.com/jdx/mise/pull/2406)
- transform `master` to `ref:master` in ls-remote for zig by [@chasinglogic](https://github.com/chasinglogic) in [#2409](https://github.com/jdx/mise/pull/2409)

### 📦️ Dependency Updates

- bump openssl from 0.10.64 to 0.10.66 by [@dependabot[bot]](https://github.com/dependabot[bot]) in [#2397](https://github.com/jdx/mise/pull/2397)

### New Contributors

* @chasinglogic made their first contribution in [#2409](https://github.com/jdx/mise/pull/2409)

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

### New Contributors

* @jahands made their first contribution in [#2372](https://github.com/jdx/mise/pull/2372)

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

* @gzurowski made their first contribution in [#2353](https://github.com/jdx/mise/pull/2353)

## [2024.7.0](https://github.com/jdx/mise/compare/v2024.6.6..v2024.7.0) - 2024-07-03

### 📚 Documentation

- update actions/checkout version by [@light-planck](https://github.com/light-planck) in [#2349](https://github.com/jdx/mise/pull/2349)

### New Contributors

* @light-planck made their first contribution in [#2349](https://github.com/jdx/mise/pull/2349)

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

### New Contributors

* @laozc made their first contribution in [#2305](https://github.com/jdx/mise/pull/2305)

## [2024.6.4](https://github.com/jdx/mise/compare/v2024.6.3..v2024.6.4) - 2024-06-15

### 🐛 Bug Fixes

- allow glob patterns in task outputs and sources by [@adamdickinson](https://github.com/adamdickinson) in [#2286](https://github.com/jdx/mise/pull/2286)

### New Contributors

* @adamdickinson made their first contribution in [#2286](https://github.com/jdx/mise/pull/2286)

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

* @chrmod made their first contribution in [#2248](https://github.com/jdx/mise/pull/2248)

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

* @donaldguy made their first contribution in [#2195](https://github.com/jdx/mise/pull/2195)

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

* @bklebe made their first contribution in [#2173](https://github.com/jdx/mise/pull/2173)

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
