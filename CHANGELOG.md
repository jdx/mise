# Changelog

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
