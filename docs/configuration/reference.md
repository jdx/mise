---
description: "Every key mise reads from mise.toml, with its type, description and where it goes, generated from the mise.toml schema."
aside: false
sidebar: false
pageClass: config-reference-page
head:
  - - link
    - rel: alternate
      type: text/markdown
      href: /configuration/reference.md
---

# mise.toml reference

<script setup>
import ConfigReference from '/components/config-reference.vue';
</script>

Every key mise reads from a `mise.toml`: its type, what it does and where it
goes, generated from [`schema/mise.json`](https://mise.jdx.dev/schema/mise.json)
so it matches this version of mise.

<ConfigReference />
