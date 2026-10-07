<script setup>
import { computed } from "vue";

const props = defineProps(["setting", "level"]);
const tag = computed(
  () => `h${Math.min(Math.max(Number(props.level) || 2, 2), 6)}`,
);
</script>

<template>
  <component :is="tag" :id="setting.key">
    <code>{{ setting.key }}</code>
    <a
      :href="`#${setting.key}`"
      class="header-anchor"
      :aria-label="`Permalink to ${setting.key}`"
    ></a>
    <span v-if="setting.deprecated" class="VPBadge danger">deprecated</span>
    <span v-if="setting.experimental" class="VPBadge warning"
      >experimental</span
    >
    <span
      v-for="badge in setting.badges"
      :key="badge.text"
      class="VPBadge info"
      :title="badge.title"
      >{{ badge.text }}</span
    >
  </component>

  <p v-if="setting.description" v-html="setting.description"></p>

  <ul>
    <li>
      Type: <code>{{ setting.type }}</code
      ><template v-if="setting.typeHint">
        (<span v-html="setting.typeHint"></span>)</template
      >
    </li>
    <li>Default: <span v-html="setting.default"></span></li>
    <li v-if="setting.env">
      Environment variable: <code>{{ setting.env }}</code
      ><template v-if="setting.parseEnv">
        (<span v-html="setting.parseEnv"></span>)</template
      >
    </li>
    <li v-for="text in setting.scope" :key="text">
      Set in: <span v-html="text"></span>
    </li>
    <li v-if="setting.deprecated">
      Deprecated: <span v-html="setting.deprecated"></span>
    </li>
    <li v-if="setting.enum">
      Choices:
      <ul>
        <li v-for="choice in setting.enum" :key="String(choice.value)">
          <code>{{ choice.value }}</code
          ><template v-if="choice.description"
            >: <span v-html="choice.description"></span
          ></template>
        </li>
      </ul>
    </li>
  </ul>

  <div v-if="setting.docs" v-html="setting.docs"></div>
</template>
