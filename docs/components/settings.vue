<script setup>
import { data } from "/settings.data.ts";
import Setting from "/components/setting.vue";
const { child } = defineProps(["child", "level"]);

const settings = child
  ? (data.find((f) => f.key === child)?.settings ?? [])
  : data;

// Check if there are any settings to display
const hasSettings = settings.some((f) => f.type) || settings.some((f) => !f.type);
</script>

<!--  <ul>-->
<!--    <li v-for="setting in settings">-->
<!--      <a v-if="!settings.settings" :href="`#${ setting.key }`"><code>{{ setting.key }}</code></a>-->
<!--      <ul v-if="setting.settings">-->
<!--        <li v-for="child in setting.settings">-->
<!--          <a :href="`#${ child.key }`"><code>{{ child.key }}</code></a>-->
<!--        </li>-->
<!--      </ul>-->
<!--    </li>-->
<!--  </ul>-->

<template>
  <div v-if="!hasSettings" class="no-settings">
    <p>No settings available.</p>
  </div>

  <template v-else>
    <Setting
      v-for="setting in settings.filter((f) => f.type)"
      :setting="setting"
      :key="setting.key"
      :level="level"
    />

    <div v-for="child in settings.filter((f) => !f.type)">
      <h2 :id="child.key">
        <code>{{ child.key }}</code>
        <a :href="`#${child.key}`" class="header-anchor"></a>
      </h2>
      <Setting
        v-for="setting in child.settings"
        :setting="setting"
        :key="setting.key"
        :level="level + 1"
      />
    </div>
  </template>
</template>
