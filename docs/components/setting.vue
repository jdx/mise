<script setup>
defineProps(["setting", "level"]);
</script>

<template>
  <h2 v-if="level === 2">
    <code>{{ setting.key }}</code>
    <span v-if="setting.deprecated" class="VPBadge warning">deprecated</span>
  </h2>
  <h3 v-if="level === 3">
    <code>{{ setting.key }}</code>
    <span v-if="setting.deprecated" class="VPBadge warning">deprecated</span>
  </h3>

  <ul>
    <li>
      Type: <code>{{ setting.type }}</code>
      <span v-if="setting.optional">(optional)</span>
    </li>
    <li>
      Env: <code>{{ setting.env }}</code>
    </li>
    <li>
      Default: <code>{{ setting.default }}</code>
    </li>
    <li v-if="setting.deprecated">Deprecated: {{ setting.deprecated }}</li>
    <li v-if="setting.enum">
      Choices:
      <ul>
        <li v-for="choice in setting.enum">
          <code>{{ choice[0] }}</code> – {{ choice[1] }}
        </li>
      </ul>
    </li>
  </ul>

  <span v-html="setting.docs"></span>
</template>
