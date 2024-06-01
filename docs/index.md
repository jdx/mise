---
# https://vitepress.dev/reference/default-theme-home-page
layout: home
title: Home

hero:
  name: mise-en-place
  tagline: |
      The front-end to your dev env
      <span class="formerly">Pronounced "MEEZ ahn plahs"</span>
  actions:
    - theme: brand
      text: Getting Started
      link: /getting-started
    - theme: alt
      text: About
      link: /about
    - theme: alt
      text: GitHub
      link: https://github.com/jdx/mise
    - theme: alt
      text: Discord
      link: https://discord.gg/UBa7pJUN7Z

features:
  - title: <a href="/dev-tools/">Dev Tools</a>
    details: mise is a polyglot tool version manager. It replaces tools like <a href="https://asdf-vm.com">asdf</a>, nvm, pyenv, rbenv, etc.
  - title: <a href="/environments.html">Environments</a>
    details: mise allows you to switch sets of env vars in different project directories. It can replace <a href="https://github.com/direnv/direnv">direnv</a>.
  - title: <a href="/tasks/">Tasks</a> <Badge type="warning" text="experimental" />
    details: mise is a task runner that can replace <a href="https://www.gnu.org/software/make">make</a>, or <a href="https://docs.npmjs.com/cli/v10/using-npm-scripts">npm scripts</a>.
---

<style>
.formerly {
    font-size: 0.7em;
    color: #666;
}
</style>
