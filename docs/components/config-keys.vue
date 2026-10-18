<script setup>
// One level of the mise.toml reference: each key with its type and
// description. A key with keys below it opens to show them.
import { computed, inject } from "vue";

const props = defineProps({
  keys: { type: Array, required: true },
  // the path of the key these are in
  parent: { type: String, required: true },
  // show every key, even with a filter: the reader opened the key they are in
  all: Boolean,
});

const { open, matches, toggle } = inject("configReference");

const shown = computed(() =>
  !matches.value || props.all
    ? props.keys
    : props.keys.filter((key) => matches.value.shown.has(key.path)),
);

// `tasks.<name>` in `tasks`, and `[].name` in a list's items
function name(key) {
  return key.path.slice(props.parent.length).replace(/^\./, "");
}

// Open when the reader opened it, or when a key below it matches the filter.
function expanded(key) {
  return (
    open.has(key.path) ||
    (!!matches.value &&
      key.children.some((child) => matches.value.shown.has(child.path)))
  );
}
</script>

<template>
  <ul class="config-keys">
    <li
      v-for="key in shown"
      :id="key.path"
      :key="key.path"
      :class="{ hit: matches?.hits.has(key.path) }"
    >
      <div class="key">
        <button
          v-if="key.count > 0"
          type="button"
          class="toggle"
          :aria-expanded="expanded(key)"
          :aria-label="`${expanded(key) ? 'Close' : 'Open'} ${key.path}`"
          @click="toggle(key.path)"
        ></button>
        <span v-else class="toggle"></span>
        <a :href="`#${key.path}`" class="name" :title="key.path"
          ><code>{{ name(key) }}</code></a
        >
        <code class="type">{{ key.type }}</code>
        <span v-if="key.deprecated" class="VPBadge danger">deprecated</span>
        <span v-if="key.experimental" class="VPBadge warning"
          >experimental</span
        >
        <span v-if="key.count > 0 && !expanded(key)" class="count"
          >{{ key.count }} {{ key.count === 1 ? "key" : "keys" }}</span
        >
      </div>
      <div
        v-if="
          key.description ||
          key.choices.length > 0 ||
          key.default ||
          key.link ||
          key.sameAs
        "
        class="about"
      >
        <span v-if="key.description" v-html="key.description"></span>
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
        <template v-if="key.sameAs">
          Same keys as
          <a :href="`#${key.sameAs}`"
            ><code>{{ key.sameAs }}</code></a
          >.
        </template>
      </div>
      <ConfigKeys
        v-if="expanded(key) && key.children.length > 0"
        :keys="key.children"
        :parent="key.path"
        :all="!matches || open.has(key.path)"
      />
    </li>
  </ul>
</template>

<style scoped>
.config-keys {
  list-style: none;
  padding-left: 0;
  margin: 12px 0;
}

.config-keys .config-keys {
  margin: 4px 0 4px 9px;
  padding-left: 16px;
  border-left: 1px solid var(--vp-c-divider);
}

.config-keys li {
  margin: 6px 0;
  scroll-margin-top: calc(var(--vp-nav-height) + 24px);
}

.key {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 4px 8px;
}

.toggle {
  flex: none;
  width: 18px;
  height: 18px;
  align-self: center;
  border-radius: 4px;
  color: var(--vp-c-text-2);
}

/* drawn with borders, so it needs no glyph from the page's fonts */
button.toggle::before {
  content: "";
  display: block;
  width: 6px;
  height: 6px;
  margin: 0 auto;
  border-right: 1.5px solid currentColor;
  border-bottom: 1.5px solid currentColor;
  transform: translateX(-1px) rotate(-45deg);
  transition: transform 0.15s;
}

button.toggle[aria-expanded="true"]::before {
  transform: translateY(-1px) rotate(45deg);
}

button.toggle:hover {
  background: var(--vp-c-default-soft);
  color: var(--vp-c-text-1);
}

.name {
  min-width: 0;
  overflow-wrap: anywhere;
  text-decoration: none;
}

.name code {
  font-weight: 600;
  color: var(--vp-c-text-1);
}

.hit > .key .name code {
  background: var(--vp-c-brand-soft);
}

.type {
  color: var(--vp-c-text-2);
  background: none;
  padding: 0;
}

.count {
  color: var(--vp-c-text-3);
  font-size: 13px;
}

.about {
  margin: 2px 0 0 26px;
  color: var(--vp-c-text-2);
  font-size: 14px;
  line-height: 1.6;
}
</style>
