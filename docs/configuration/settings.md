# Settings

<script setup>
import Settings from '/components/settings.vue';
</script>

The following is a list of all of mise's settings. These can be set via `mise settings set`,
by directly modifying `~/.config/mise/config.toml` or local config, or via environment variables.

Some of them also can be set via global CLI flags.

<Settings :level="2" />
