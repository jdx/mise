---
description: "A timeline of every mise release, with the number of changes and resolved issues in each."
editLink: false
---

# Releases

<script setup>
import Releases from '/components/releases.vue';
</script>

mise ships a release almost every day. Each bar below is one release, oldest on
the left, and its height is the number of changes in that release's
[changelog](https://github.com/jdx/mise/blob/main/CHANGELOG.md). Hover or focus
a bar to read it. Release notes are on the
[GitHub releases page](https://github.com/jdx/mise/releases).

A change is one changelog entry: a feature, a fix, a registry addition, a
dependency update, and so on. New-contributor thanks and the upstream Aqua
registry updates mise vendors are not counted.

<Releases />
