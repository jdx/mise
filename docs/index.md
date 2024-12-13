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
  - title: Dev Tools
    link: /dev-tools/
    icon:  🛠️
    details: mise is a polyglot tool version manager. It replaces tools like asdf, nvm, pyenv, rbenv, etc.
  - title: Environments
    details: mise allows you to switch sets of env vars in different project directories. It can replace direnv.
    icon: ⚙
    link: /environments/
  - title: Tasks
    link: /tasks/
    details: mise is a task runner that can replace make, or npm scripts.
    icon: ⚡
---

<style>
.formerly {
    font-size: 0.7em;
    color: #666;
}
</style>
