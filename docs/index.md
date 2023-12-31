---
# https://vitepress.dev/reference/default-theme-home-page
layout: home
title: Home

hero:
  name: "rtx documentation"
  tagline: "The front-end to your dev env."
  actions:
    - theme: brand
      text: Getting Started
      link: /getting-started.html
    - theme: alt
      text: GitHub
      link: https://github.com/jdx/rtx
    - theme: alt
      text: Discord
      link: https://discord.gg/UBa7pJUN7Z

features:
  - title: Dev Tools
    details: rtx is a polyglot tool version manager. It replaces tools like <a href="https://asdf-vm.com">asdf</a>, nvm, pyenv, rbenv, etc.
  - title: Environments
    details: rtx allows you to switch sets of env vars in different project directories. It can replace <a href="https://github.com/direnv/direnv">direnv</a>.
  - title: <a href="/tasks">Tasks</a> <Badge type="warning" text="experimental" />
    details: rtx is a task runner that can replace <a href="https://www.gnu.org/software/make">make</a>, or <a href="https://docs.npmjs.com/cli/v10/using-npm/scripts">npm scripts</a>.
---
