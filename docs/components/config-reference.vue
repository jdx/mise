<script setup>
import { data } from "/config-reference.data.ts";
import ConfigKeys from "/components/config-keys.vue";
</script>

<template>
  <template v-for="key in data" :key="key.path">
    <h2 :id="key.path">
      <code>{{ key.path }}</code>
      <a
        :href="`#${key.path}`"
        class="header-anchor"
        :aria-label="`Permalink to ${key.path}`"
      ></a>
      <span v-if="key.deprecated" class="VPBadge danger">deprecated</span>
      <span v-if="key.experimental" class="VPBadge warning">experimental</span>
    </h2>
    <p v-if="key.description" v-html="key.description"></p>
    <ul>
      <li>
        Type: <code>{{ key.type }}</code>
      </li>
      <li v-if="key.choices.length > 0">
        Choices:
        <template v-for="(choice, i) in key.choices" :key="choice"
          ><template v-if="i > 0">, </template
          ><code>{{ choice }}</code></template
        >
      </li>
      <li v-if="key.default">
        Default: <code>{{ key.default }}</code>
      </li>
      <li v-if="key.link">
        Documented in <a :href="key.link">its own reference</a>.
      </li>
    </ul>
    <ConfigKeys v-if="key.children.length > 0" :keys="key.children" />
  </template>
</template>
