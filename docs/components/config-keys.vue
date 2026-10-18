<script setup>
// One level of the mise.toml reference: each key with its type and
// description, and the keys below it.
defineProps({ keys: { type: Array, required: true } });
</script>

<template>
  <ul class="config-keys">
    <li v-for="key in keys" :key="key.path">
      <code>{{ key.path }}</code>
      <span v-if="key.deprecated" class="VPBadge danger">deprecated</span>
      <span v-if="key.experimental" class="VPBadge warning">experimental</span>
      — <code>{{ key.type }}</code>
      <template v-if="key.description">
        — <span v-html="key.description"></span
      ></template>
      <template v-if="key.choices.length > 0">
        Choices:
        <template v-for="(choice, i) in key.choices" :key="choice"
          ><template v-if="i > 0">, </template
          ><code>{{ choice }}</code></template
        >.
      </template>
      <template v-if="key.default">
        Default: <code>{{ key.default }}</code
        >.
      </template>
      <template v-if="key.link">
        See <a :href="key.link">its reference</a>.
      </template>
      <ConfigKeys v-if="key.children.length > 0" :keys="key.children" />
    </li>
  </ul>
</template>
