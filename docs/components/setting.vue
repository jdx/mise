<script setup>
defineProps(["setting", "level"]);
</script>

<template>
  <h2 v-if="level === 2" :id="setting.key">
    <code>{{ setting.key }}</code
    ><a :href="`#${setting.key}`" class="header-anchor"></a>
    <span v-if="setting.deprecated" class="VPBadge warning">deprecated</span>
  </h2>
  <h3 v-if="level === 3" :id="setting.key">
    <code>{{ setting.key }}</code
    ><a :href="`#${setting.key}`" class="header-anchor"></a>
    <span v-if="setting.deprecated" class="VPBadge warning">deprecated</span>
  </h3>
  <h4 v-if="level === 4" :id="setting.key">
    <code>{{ setting.key }}</code
    ><a :href="`#${setting.key}`" class="header-anchor"></a>
    <span v-if="setting.deprecated" class="VPBadge warning">deprecated</span>
  </h4>

  <ul>
    <li>
      Type: <code>{{ setting.type }}</code>
      <span v-if="setting.optional">(optional)</span>
    </li>
    <li v-if="setting.env">
      Env: <code>{{ setting.env }}</code>
      <span v-if="setting.parseEnv">({{ setting.parseEnv }} separated)</span>
    </li>
    <li>
      Default: <code>{{ setting.default }}</code>
    </li>
    <li v-if="setting.deprecated">Deprecated: {{ setting.deprecated }}</li>
    <li v-if="setting.enum">
      Choices:
      <ul>
        <li v-for="choice in setting.enum">
          <template v-if="typeof choice === 'object' && choice !== null && 'value' in choice">
            <code>{{ choice.value }}</code><template v-if="choice.description"> – {{ choice.description }}</template>
          </template>
          <template v-else>
            <code>{{ choice }}</code>
          </template>
        </li>
      </ul>
    </li>
  </ul>

  <span v-html="setting.docs"></span>
</template>
