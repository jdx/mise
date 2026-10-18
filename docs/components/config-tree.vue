<script setup>
// One level of the mise.toml reference's tree. A key with keys below it opens to show them.
import { computed, inject } from "vue";

const props = defineProps({
  keys: { type: Array, required: true },
  // the path of the key these are in
  parent: { type: String, required: true },
  depth: { type: Number, required: true },
  // show every key, even with a filter: the reader opened the key they are in
  all: Boolean,
});

const { open, matches, selected, toggle, select } = inject("configReference");

const shown = computed(() =>
  !matches.value || props.all
    ? props.keys
    : props.keys.filter((key) => matches.value.shown.has(key.path)),
);

// `tasks.<name>` in `tasks`, and `[].name` in a list's items
function name(key) {
  if (!props.parent) return key.path;
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

function choose(key) {
  // a click on a closed key opens it too; a second click on the selected key closes it
  if (key.count > 0) {
    if (selected.value !== key.path && !open.has(key.path)) open.add(key.path);
    else if (selected.value === key.path) toggle(key.path);
  }
  select(key.path);
}

// the first row takes the tab stop until a key is selected
function tabStop(key) {
  if (selected.value) return selected.value === key.path ? 0 : -1;
  return props.depth === 0 && key === props.keys[0] ? 0 : -1;
}
</script>

<template>
  <ul class="level" :role="depth === 0 ? 'none' : 'group'">
    <li v-for="key in shown" :key="key.path" role="none">
      <div
        class="row"
        role="treeitem"
        :data-path="key.path"
        :tabindex="tabStop(key)"
        :aria-selected="selected === key.path"
        :aria-expanded="key.count > 0 ? expanded(key) : undefined"
        :aria-level="depth + 1"
        :class="{
          selected: selected === key.path,
          hit: matches?.hitPaths.has(key.path),
          deprecated: key.deprecated,
        }"
        :style="{ paddingLeft: `${depth * 14 + 4}px` }"
        :title="key.path"
        @click="choose(key)"
      >
        <span
          v-if="key.count > 0"
          class="chevron"
          :class="{ open: expanded(key) }"
          aria-hidden="true"
          @click.stop="toggle(key.path)"
        ></span>
        <span v-else class="chevron leaf" aria-hidden="true"></span>
        <code class="name">{{ name(key) }}</code>
        <span
          v-if="key.experimental"
          class="dot exp"
          title="experimental"
        ></span>
        <span v-if="key.sameAs" class="same" title="Same keys as another key"
          >→</span
        >
        <span v-if="key.count > 0" class="count">{{ key.count }}</span>
      </div>
      <ConfigTree
        v-if="expanded(key) && key.children.length > 0"
        :keys="key.children"
        :parent="key.path"
        :depth="depth + 1"
        :all="!matches || open.has(key.path)"
      />
    </li>
  </ul>
</template>

<style scoped>
.level {
  list-style: none;
  margin: 0;
  padding: 0;
}

.row {
  display: flex;
  align-items: center;
  gap: 4px;
  min-height: 28px;
  padding-right: 8px;
  border-radius: 6px;
  cursor: pointer;
  user-select: none;
  white-space: nowrap;
}

.row:hover {
  background: var(--vp-c-default-soft);
}

.row:focus-visible {
  outline: 2px solid var(--vp-c-brand-1);
  outline-offset: -2px;
}

.row.selected {
  background: var(--vp-c-brand-soft);
}

.row.selected .name {
  color: var(--vp-c-brand-1);
}

.chevron {
  flex: none;
  display: grid;
  place-items: center;
  width: 18px;
  height: 18px;
  border-radius: 4px;
  color: var(--vp-c-text-3);
}

.chevron:not(.leaf):hover {
  background: var(--vp-c-default-soft);
  color: var(--vp-c-text-1);
}

/* drawn with borders, so it needs no glyph from the page's fonts */
.chevron:not(.leaf)::before {
  content: "";
  width: 5px;
  height: 5px;
  border-right: 1.5px solid currentColor;
  border-bottom: 1.5px solid currentColor;
  transform: translateX(-1px) rotate(-45deg);
  transition: transform 0.15s;
}

.chevron.open::before {
  transform: translateY(-1px) rotate(45deg);
}

.name {
  overflow: hidden;
  text-overflow: ellipsis;
  padding: 0;
  background: none;
  font-size: 13px;
  color: var(--vp-c-text-1);
}

.hit .name {
  background: var(--vp-c-yellow-soft);
  border-radius: 3px;
}

.deprecated .name {
  color: var(--vp-c-text-3);
  text-decoration: line-through;
}

.dot.exp {
  flex: none;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--vp-c-warning-1);
}

.same {
  font-size: 11px;
  color: var(--vp-c-text-3);
}

.count {
  margin-left: auto;
  padding-left: 8px;
  font-size: 11px;
  color: var(--vp-c-text-3);
  font-variant-numeric: tabular-nums;
}
</style>
