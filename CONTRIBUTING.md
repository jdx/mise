# Contributing

## Contributing a new Plugin

1. Clone this repo
   ```bash
   git clone https://github.com/asdf-vm/asdf-plugins
   ```
1. Install repo dependencies
   ```bash
   asdf install
   ```
1. Add the plugin to the repository `README.md` _Plugin List_ table.
1. Create a file with the shortname you wish to be used by asdf in `plugins/<name>`. The contents should be `repository = <your_repo>`. Eg:
   ```bash
   printf "repository = https://github.com/asdf-vm/asdf-nodejs.git\n" > plugins/nodejs
   ```
1. Test your code
   ```bash
   scripts/test_plugin.bash --file plugins/<name>
   ```
1. Format your code & this README:
   ```bash
   scripts/format.bash
   ```
1. Create a PR following the instructions in the PR template.
   1. Make sure you use [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/), both in the commit messages and the pull request title

## Fixing an existing plugin

If you see a plugin has an incorrect Build status link, please PR a fix to the correct link.

If you see a plugin which is no longer maintained by the repository owner, please reach out to them on their repo before PRing a removal of the plugin. Sometimes code not actively maintained functions perfectly fine. If you PR a removal, link to the thread showing you attempted to communicate with the owner/author.

---

Thanks for contributing!
