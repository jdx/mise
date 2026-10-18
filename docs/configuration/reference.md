---
description: "Every key mise reads from mise.toml, with its type and description, generated from the mise.toml schema."
outline: 2
---

# mise.toml reference

<script setup>
import ConfigReference from '/components/config-reference.vue';
</script>

This page lists every key mise reads from a `mise.toml`, with its type and
what it does. It is generated from
[`schema/mise.json`](https://mise.jdx.dev/schema/mise.json), the same schema
editors use to complete and check a config, so it always matches the version
of mise these docs describe. For how the sections fit together, start with
[mise.toml](/configuration.html); [settings](/configuration/settings.html) have
their own reference.

Names in angle brackets, such as `tasks.<name>`, stand for a key you choose. A
type such as `string | string[]` means either form works. Open a key to see the
keys inside it, or filter by name or description. A key that has the same keys
as another links to it instead of listing them again. The reference is also
plain Markdown, one file per section, starting at
[reference.md](https://mise.jdx.dev/configuration/reference.md), for reading without this page's
scripts.

To see what each key in one of your own files does, run
[`mise config explain`](/cli/config/explain.html).

<ConfigReference />
