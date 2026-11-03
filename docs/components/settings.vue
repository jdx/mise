<script setup>
import { computed } from "vue";
import { data } from "/settings.data.ts";
import Setting from "/components/setting.vue";

// child: render one group's settings, such as "node", without a group heading.
// prefix: render top-level settings and groups whose key starts with the
//   prefix; the group named exactly like the prefix has no heading.
// keys: limit the output to these settings, by full key ("dotnet.registry_url")
//   or, with child, by the part after the group ("registry_url").
// level: heading level of each setting; a group heading uses the same level
//   and its settings one level deeper.
// index: list the groups above the settings, for the full reference.
// intro: say how to set the settings; on by default with child or prefix.
// A child, prefix or key that matches nothing fails the build.
const props = defineProps({
  child: String,
  prefix: String,
  keys: Array,
  level: { type: [Number, String], default: 2 },
  index: Boolean,
  intro: { type: Boolean, default: undefined },
});

const level = computed(() =>
  Math.min(Math.max(Number(props.level) || 2, 2), 5),
);

const items = computed(() => {
  let result;
  if (props.child) {
    const group = data.find((f) => !f.type && f.key === props.child);
    if (!group) {
      throw new Error(
        `<Settings child="${props.child}"> matches no settings group`,
      );
    }
    result = group.settings;
  } else if (props.prefix) {
    const p = props.prefix;
    result = data.filter(
      (f) =>
        f.key === p || f.key.startsWith(`${p}_`) || f.key.startsWith(`${p}.`),
    );
  } else {
    result = data;
  }
  if (props.keys) {
    const full = (k) =>
      props.child && !k.startsWith(`${props.child}.`)
        ? `${props.child}.${k}`
        : k;
    const wanted = new Set(props.keys.map(full));
    result = result
      .map((f) =>
        f.type
          ? f
          : { ...f, settings: f.settings.filter((s) => wanted.has(s.key)) },
      )
      .filter((f) => (f.type ? wanted.has(f.key) : f.settings.length > 0));
    const found = new Set(
      result.flatMap((f) => (f.type ? [f.key] : f.settings.map((s) => s.key))),
    );
    const missing = [...wanted].filter((k) => !found.has(k));
    if (missing.length > 0) {
      throw new Error(
        `<Settings keys> matches no setting: ${missing.join(", ")}`,
      );
    }
  }
  if (result.length === 0) {
    throw new Error(`<Settings prefix="${props.prefix}"> matches no settings`);
  }
  return result;
});

const groups = computed(() => items.value.filter((f) => !f.type));
const restricted = computed(() =>
  items.value.some((f) =>
    f.type ? f.scope.length > 0 : f.settings.some((s) => s.scope.length > 0),
  ),
);
const showIntro = computed(() =>
  props.intro === undefined
    ? Boolean(props.child || props.prefix)
    : props.intro,
);
</script>

<template>
  <p v-if="showIntro">
    Set these under <code>[settings]</code> in a config file, with
    <a href="/cli/settings/set.html"><code>mise settings set</code></a
    >, or with a setting's environment variable where it has one.<template
      v-if="restricted"
    >
      A "Set in" line under a setting narrows where it takes effect.</template
    >
    See
    <a href="/configuration/settings.html#which-value-wins">which value wins</a>
    when a setting is set in more than one place.
  </p>

  <p v-if="index && groups.length > 0">
    Groups:
    <template v-for="(group, i) in groups" :key="group.key"
      ><template v-if="i > 0">, </template
      ><a :href="`#${group.key}`"
        ><code>{{ group.key }}</code></a
      ></template
    >
  </p>

  <template v-for="item in items" :key="item.key">
    <Setting v-if="item.type" :setting="item" :level="level" />
    <template v-else>
      <component v-if="item.key !== prefix" :is="`h${level}`" :id="item.key">
        <code>{{ item.key }}</code>
        <a
          :href="`#${item.key}`"
          class="header-anchor"
          :aria-label="`Permalink to ${item.key}`"
        ></a>
      </component>
      <Setting
        v-for="setting in item.settings"
        :key="setting.key"
        :setting="setting"
        :level="item.key === prefix ? level : level + 1"
      />
    </template>
  </template>
</template>
