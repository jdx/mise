# Settings

<script setup>
import { data } from '/settings.data.ts';
import Setting from '/components/setting.vue';
</script>

The following is a list of all of mise's settings. These can be set via `mise settings set`,
by directly modifying `~/.config/mise/config.toml` or local config, or via environment variables.

Some of them also can be set via global CLI flags.

<Setting v-for="setting in data.filter(f => f.type)" :setting="setting" :key="setting.key" :level="2" />

<div v-for="child in data.filter(f => !f.type)">
  <h2><code>{{ child.key }}</code></h2>
  <Setting v-for="setting in child.settings" :setting="setting" :key="setting.key" :level="3" />
</div>
