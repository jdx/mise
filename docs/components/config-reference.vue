<script setup>
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  provide,
  reactive,
  ref,
  shallowRef,
  watch,
} from "vue";
import { data as sections } from "/config-sections.data.ts";
import ConfigKeys from "/components/config-keys.vue";

// The page first renders each section and the keys directly in it. The rest of
// the reference loads once the page has shown, for the filter and for opening a
// key.
const full = shallowRef(null);
const failed = ref(false);
const keys = computed(() => full.value ?? sections);

// Each key's parent, and the text the filter searches.
const index = computed(() => {
  const parents = new Map();
  const entries = [];
  const visit = (key, parent) => {
    parents.set(key.path, parent);
    const description = key.description.replace(/<[^>]*>/g, "");
    entries.push({
      path: key.path,
      count: key.count,
      text: `${key.path} ${description}`.toLowerCase(),
    });
    for (const child of key.children) visit(child, key.path);
  };
  for (const key of keys.value) visit(key, undefined);
  return { parents, entries };
});

const filter = ref("");
const open = reactive(new Set());

// With a filter: the keys that match it, and those keys with every key above
// them.
const matches = computed(() => {
  const query = filter.value.trim().toLowerCase();
  if (!query) return null;
  const hits = new Set();
  const shown = new Set();
  for (const { path, text } of index.value.entries) {
    if (!text.includes(query)) continue;
    hits.add(path);
    for (
      let p = path;
      p !== undefined && !shown.has(p);
      p = index.value.parents.get(p)
    ) {
      shown.add(p);
    }
  }
  return { hits, shown };
});

function toggle(path) {
  if (open.has(path)) open.delete(path);
  else open.add(path);
}

provide("configReference", { open, matches, toggle });

function expandAll() {
  for (const { path, count } of index.value.entries) {
    if (count > 0) open.add(path);
  }
}

// Opens the keys above the one the URL names, then scrolls to it.
async function reveal() {
  const id = decodeURIComponent(window.location.hash.slice(1));
  const { parents } = index.value;
  if (!parents.has(id)) return;
  if (matches.value && !matches.value.shown.has(id)) filter.value = "";
  for (let p = parents.get(id); p !== undefined; p = parents.get(p)) {
    open.add(p);
  }
  await nextTick();
  document.getElementById(id)?.scrollIntoView();
}

watch(filter, (value) => {
  const url = new URL(window.location.href);
  if (value.trim()) url.searchParams.set("filter", value);
  else url.searchParams.delete("filter");
  window.history.replaceState(window.history.state, "", url);
});

onMounted(async () => {
  window.addEventListener("hashchange", reveal);
  try {
    full.value = (await import("/config-reference.data.ts")).data;
  } catch (error) {
    failed.value = true;
    throw error;
  }
  filter.value =
    new URLSearchParams(window.location.search).get("filter") ?? "";
  await reveal();
});

onBeforeUnmount(() => window.removeEventListener("hashchange", reveal));
</script>

<template>
  <div class="toolbar">
    <input
      v-model="filter"
      class="filter"
      type="search"
      placeholder="Filter keys and descriptions, such as platforms or cache"
      aria-label="Filter keys"
    />
    <button type="button" :disabled="!full" @click="expandAll">
      Expand all
    </button>
    <button type="button" @click="open.clear()">Collapse all</button>
  </div>
  <p v-if="failed" class="summary" role="status">
    The rest of the reference did not load. Reload the page, or read
    <a href="https://mise.jdx.dev/configuration/reference.md"
      >the Markdown copy</a
    >.
  </p>
  <p v-if="matches" class="summary" role="status">
    {{ matches.hits.size }}
    {{ matches.hits.size === 1 ? "key matches" : "keys match" }}
  </p>
  <section
    v-for="key in keys"
    v-show="!matches || matches.shown.has(key.path)"
    :key="key.path"
  >
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
      <li v-if="key.sameAs">
        Same keys as
        <a :href="`#${key.sameAs}`"
          ><code>{{ key.sameAs }}</code></a
        >.
      </li>
    </ul>
    <ConfigKeys
      v-if="key.children.length > 0"
      :keys="key.children"
      :parent="key.path"
    />
  </section>
</template>

<style scoped>
.toolbar {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin: 24px 0 8px;
}

.filter {
  flex: 1 1 240px;
  min-width: 0;
  padding: 6px 12px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 8px;
  background: var(--vp-c-bg-soft);
  font-size: 14px;
}

.filter:focus {
  border-color: var(--vp-c-brand-1);
  outline: none;
}

.toolbar button {
  padding: 6px 12px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 8px;
  font-size: 14px;
  color: var(--vp-c-text-1);
}

.toolbar button:hover:not(:disabled) {
  border-color: var(--vp-c-brand-1);
}

.toolbar button:disabled {
  color: var(--vp-c-text-3);
  cursor: default;
}

.summary {
  color: var(--vp-c-text-2);
  font-size: 14px;
}
</style>
