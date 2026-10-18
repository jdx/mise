<script setup>
// The mise.toml reference as an explorer: a tree of every key on the left, grouped by
// topic, and the selected key on the right with a snippet of where it goes in a mise.toml.
// Readers without scripts, and agents, read the Markdown copy at reference.md instead.
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
import ConfigTree from "/components/config-tree.vue";
import ConfigKeyDetail from "/components/config-key-detail.vue";
import { grouped, guides } from "/components/config-reference-sections.ts";

// The page first renders each section and the keys directly in it. The rest of the
// reference loads once the page has shown.
const full = shallowRef(null);
const failed = ref(false);
const keys = computed(() => full.value ?? sections);
const groups = computed(() => grouped(keys.value));
const total = computed(() =>
  keys.value.reduce((n, key) => n + 1 + key.count, 0),
);

const entities = { amp: "&", lt: "<", gt: ">", quot: '"', "#39": "'" };

// A rendered description as text: its tags dropped and the entities Markdown writes, such as
// the `&lt;` of a `<url>` placeholder, read back.
function plain(html) {
  return html
    .replace(/<[^>]*>/g, "")
    .replace(/&(amp|lt|gt|quot|#39);/g, (_, name) => entities[name]);
}

// Each key by path, its parent, and the text the filter searches.
const index = computed(() => {
  const byPath = new Map();
  const parents = new Map();
  const entries = [];
  const visit = (key, parent) => {
    byPath.set(key.path, key);
    parents.set(key.path, parent);
    const description = plain(key.description);
    entries.push({
      key,
      text: `${key.path} ${description}`.toLowerCase(),
    });
    for (const child of key.children) visit(child, key.path);
  };
  for (const key of keys.value) visit(key, undefined);
  return { byPath, parents, entries };
});

const filter = ref("");
const open = reactive(new Set());
const selected = ref(null);
const narrow = ref(false);

// With a filter: the keys that match it, and those keys with every key above them.
const matches = computed(() => {
  const query = filter.value.trim().toLowerCase();
  if (!query) return null;
  const hits = [];
  const shown = new Set();
  for (const { key, text } of index.value.entries) {
    if (!text.includes(query)) continue;
    hits.push(key);
    for (
      let p = key.path;
      p !== undefined && !shown.has(p);
      p = index.value.parents.get(p)
    ) {
      shown.add(p);
    }
  }
  return { hits, hitPaths: new Set(hits.map((k) => k.path)), shown };
});

const selectedKey = computed(() =>
  selected.value ? index.value.byPath.get(selected.value) : undefined,
);

function ancestors(path) {
  const out = [];
  for (
    let p = index.value.parents.get(path);
    p !== undefined;
    p = index.value.parents.get(p)
  ) {
    out.unshift(p);
  }
  return out;
}

function section(path) {
  return ancestors(path)[0] ?? path;
}

function setHash(path) {
  const url = new URL(window.location.href);
  url.hash = path ? encodeURIComponent(path).replace(/%2E/g, ".") : "";
  window.history.replaceState(window.history.state, "", url);
}

// Selects `path`, opens the keys above it, and shows it in the tree.
async function select(path, { focus = false } = {}) {
  selected.value = path;
  setHash(path);
  if (!path) return;
  for (const p of ancestors(path)) open.add(p);
  await nextTick();
  const row = document.querySelector(
    `.tree-pane [data-path="${CSS.escape(path)}"]`,
  );
  row?.scrollIntoView({ block: "nearest" });
  if (focus) row?.focus();
  const pane = document.querySelector(".detail-pane");
  const top = pane?.getBoundingClientRect().top ?? 0;
  if (pane && (top < 0 || narrow.value)) {
    window.scrollBy({ top: top - 96 });
  }
}

function toggle(path) {
  if (open.has(path)) open.delete(path);
  else open.add(path);
}

// The row the tree's tab stop is on: the selected key while the tree shows it, else the
// first row shown, so collapsing or filtering away the selected key never leaves the tree
// without one.
const tabTarget = ref(grouped(sections)[0]?.keys[0]?.path);

function updateTabTarget() {
  nextTick(() => {
    const shown = rows();
    tabTarget.value =
      shown.find((r) => r.dataset.path === selected.value)?.dataset.path ??
      shown[0]?.dataset.path;
  });
}

watch([selected, matches, () => open.size, keys], updateTabTarget);

provide("configReference", {
  open,
  matches,
  selected,
  tabTarget,
  toggle,
  select,
});

function expandAll() {
  for (const { key } of index.value.entries) {
    if (key.count > 0) open.add(key.path);
  }
}

// The key `id` names, or the nearest key above it that the page lists: a key inside a tree
// the page lists once, under "Same keys as", is not on the page itself.
function listed(id) {
  const { byPath } = index.value;
  let at = id;
  while (at && !byPath.has(at)) {
    const cut = at.endsWith("[]") ? at.length - 2 : at.lastIndexOf(".");
    if (cut <= 0) return undefined;
    at = at.slice(0, cut);
  }
  return at || undefined;
}

function reveal() {
  const id = listed(decodeURIComponent(window.location.hash.slice(1)));
  if (id === selected.value) return;
  if (id && matches.value && !matches.value.shown.has(id)) filter.value = "";
  select(id ?? null);
}

// The rows the tree shows, in order, for the arrow keys.
function rows() {
  return [...document.querySelectorAll(".tree-pane [data-path]")];
}

function onTreeKey(event) {
  const list = rows();
  // the focused row, which after "Collapse all" or a filter can be another than the selected key
  const current =
    event.target.closest?.("[data-path]")?.dataset.path ?? selected.value;
  const at = list.findIndex((r) => r.dataset.path === current);
  const key = current ? index.value.byPath.get(current) : undefined;
  const move = (i) => {
    const row = list[Math.max(0, Math.min(list.length - 1, i))];
    if (row) select(row.dataset.path, { focus: true });
  };
  switch (event.key) {
    case "ArrowDown":
      move(at + 1);
      break;
    case "ArrowUp":
      move(at - 1);
      break;
    case "Home":
      move(0);
      break;
    case "End":
      move(list.length - 1);
      break;
    case "ArrowRight":
      if (!key || key.count === 0) return;
      if (!open.has(key.path)) open.add(key.path);
      else move(at + 1);
      break;
    case "ArrowLeft":
      if (key && open.has(key.path)) open.delete(key.path);
      else {
        const parent = key && index.value.parents.get(key.path);
        if (parent) select(parent, { focus: true });
      }
      break;
    default:
      return;
  }
  event.preventDefault();
}

const filterInput = ref(null);

function onFilterKey(event) {
  if (event.key === "ArrowDown" || event.key === "Enter") {
    const first = matches.value?.hits[0] ?? keys.value[0];
    if (first) select(first.path, { focus: true });
    event.preventDefault();
  } else if (event.key === "Escape") {
    filter.value = "";
  }
}

// `/` jumps to the filter, as on other docs sites. The site search binds `/` too; on this
// page the filter takes it, and Ctrl+K still opens the site search.
function onGlobalKey(event) {
  if (event.key !== "/" || event.metaKey || event.ctrlKey || event.altKey)
    return;
  const target = event.target;
  if (
    target instanceof HTMLElement &&
    (target.isContentEditable ||
      ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName))
  )
    return;
  event.preventDefault();
  event.stopImmediatePropagation();
  // on a phone the filter is in the tree, which a selected key hides
  if (narrow.value && selected.value) {
    select(null);
    nextTick(() => filterInput.value?.focus());
  } else {
    filterInput.value?.focus();
  }
}

watch(filter, (value) => {
  const url = new URL(window.location.href);
  if (value.trim()) url.searchParams.set("filter", value);
  else url.searchParams.delete("filter");
  window.history.replaceState(window.history.state, "", url);
});

let media;
const onMedia = () => (narrow.value = media.matches);

onMounted(async () => {
  media = window.matchMedia("(max-width: 767px)");
  onMedia();
  media.addEventListener("change", onMedia);
  window.addEventListener("hashchange", reveal);
  // in the capture phase, so it runs before the site search's own listener
  window.addEventListener("keydown", onGlobalKey, { capture: true });
  try {
    full.value = (await import("/config-reference.data.ts")).data;
  } catch (error) {
    failed.value = true;
    throw error;
  }
  filter.value =
    new URLSearchParams(window.location.search).get("filter") ?? "";
  reveal();
});

onBeforeUnmount(() => {
  media?.removeEventListener("change", onMedia);
  window.removeEventListener("hashchange", reveal);
  window.removeEventListener("keydown", onGlobalKey, { capture: true });
});

function firstSentence(html) {
  const text = plain(html);
  const cut = text.search(/[.;](\s|$)/);
  return cut > 0 ? text.slice(0, cut) : text;
}
</script>

<template>
  <div class="explorer" :class="{ 'has-selection': selected }">
    <div class="tree-pane">
      <div class="tree-tools">
        <input
          ref="filterInput"
          v-model="filter"
          class="filter"
          type="search"
          :placeholder="`Filter ${total.toLocaleString('en-US')} keys`"
          aria-label="Filter keys and descriptions"
          @keydown="onFilterKey"
        />
        <kbd class="slash" aria-hidden="true">/</kbd>
      </div>
      <div class="tree-actions">
        <span v-if="matches" role="status">
          {{ matches.hits.length }}
          {{ matches.hits.length === 1 ? "match" : "matches" }}
        </span>
        <span v-else class="muted">{{ keys.length }} sections</span>
        <span class="spacer"></span>
        <button type="button" :disabled="!full" @click="expandAll">
          Expand all
        </button>
        <button type="button" @click="open.clear()">Collapse all</button>
      </div>
      <p v-if="failed" class="muted" role="status">
        The rest of the reference did not load. Reload the page, or read
        <a href="https://mise.jdx.dev/configuration/reference.md"
          >the Markdown copy</a
        >.
      </p>
      <div
        class="tree"
        role="tree"
        aria-label="mise.toml keys"
        @keydown="onTreeKey"
      >
        <template v-for="group in groups" :key="group.title">
          <div
            v-show="
              !matches || group.keys.some((k) => matches.shown.has(k.path))
            "
            class="group"
          >
            <div class="group-title">{{ group.title }}</div>
            <ConfigTree :keys="group.keys" parent="" :depth="0" />
          </div>
        </template>
      </div>
    </div>

    <div class="detail-pane">
      <button v-if="selected" type="button" class="back" @click="select(null)">
        ‹ All keys
      </button>
      <ConfigKeyDetail
        v-if="selectedKey"
        :item="selectedKey"
        :index="index"
        :section="section(selectedKey.path)"
      />
      <div v-else-if="matches" class="results">
        <h2 class="pane-title">
          {{ matches.hits.length }}
          {{ matches.hits.length === 1 ? "key matches" : "keys match" }}
          <q>{{ filter.trim() }}</q>
        </h2>
        <ul>
          <li v-for="hit in matches.hits.slice(0, 200)" :key="hit.path">
            <a
              :href="`#${hit.path}`"
              class="result-path"
              @click.prevent="select(hit.path)"
              ><code>{{ hit.path }}</code></a
            >
            <code class="type">{{ hit.type }}</code>
            <div
              v-if="hit.description"
              class="result-desc"
              v-html="hit.description"
            ></div>
          </li>
        </ul>
        <p v-if="matches.hits.length > 200" class="muted">
          Showing the first 200. Narrow the filter to see the rest.
        </p>
      </div>
      <div v-else class="overview">
        <aside class="howto">
          <ul>
            <li>
              <code>&lt;name&gt;</code> stands for a name you choose, such as a
              task's name in <code>tasks.&lt;name&gt;</code>.
            </li>
            <li><code>string | string[]</code> means either form works.</li>
            <li>
              <kbd>/</kbd> filters; <kbd>↑</kbd> <kbd>↓</kbd> <kbd>←</kbd>
              <kbd>→</kbd> move through the tree.
            </li>
          </ul>
          <ul class="links">
            <li>
              How the sections fit together:
              <a href="/configuration.html">mise.toml</a>
            </li>
            <li>
              Explain your own file:
              <a href="/cli/config/explain.html"
                ><code>mise config explain</code></a
              >
              or the <a href="/configuration/playground.html">playground</a>
            </li>
            <li>
              For scripts and agents:
              <a href="https://mise.jdx.dev/configuration/reference.md"
                >reference.md</a
              >
            </li>
          </ul>
        </aside>
        <section v-for="group in groups" :key="group.title" class="ov-group">
          <h2 class="ov-title">{{ group.title }}</h2>
          <div class="cards">
            <div v-for="key in group.keys" :key="key.path" class="card">
              <div class="card-head">
                <a
                  :href="`#${key.path}`"
                  class="card-name"
                  @click.prevent="select(key.path)"
                  ><code>{{ key.path }}</code></a
                >
                <span v-if="key.experimental" class="VPBadge warning"
                  >experimental</span
                >
                <span v-if="key.count > 0" class="muted count">{{
                  key.count
                }}</span>
              </div>
              <p class="card-desc">{{ firstSentence(key.description) }}</p>
              <a
                v-if="guides[key.path]"
                class="card-guide"
                :href="guides[key.path].link"
                >{{ guides[key.path].text }} guide →</a
              >
            </div>
          </div>
        </section>
      </div>
    </div>
  </div>
</template>

<style>
/* the explorer takes the width the page's outline would have used */
.config-reference-page .VPDoc .container,
.config-reference-page .VPDoc .content,
.config-reference-page .VPDoc .content-container {
  max-width: none !important;
}
</style>

<style scoped>
.explorer {
  display: grid;
  grid-template-columns: minmax(250px, 320px) minmax(0, 1fr);
  gap: 0 32px;
  margin-top: 24px;
  align-items: start;
}

.tree-pane {
  position: sticky;
  top: calc(var(--vp-nav-height) + 16px);
  max-height: calc(100vh - var(--vp-nav-height) - 32px);
  display: flex;
  flex-direction: column;
  border: 1px solid var(--vp-c-divider);
  border-radius: 12px;
  background: var(--vp-c-bg-soft);
  overflow: hidden;
}

.tree-tools {
  position: relative;
  padding: 12px 12px 0;
}

.filter {
  width: 100%;
  padding: 7px 32px 7px 12px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 8px;
  background: var(--vp-c-bg);
  font-size: 14px;
}

.filter:focus {
  border-color: var(--vp-c-brand-1);
  outline: none;
}

.slash {
  position: absolute;
  right: 20px;
  top: 19px;
  padding: 0 6px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 4px;
  font-size: 12px;
  line-height: 18px;
  color: var(--vp-c-text-3);
}

.tree-actions {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 8px 12px;
  font-size: 12px;
  color: var(--vp-c-text-2);
  border-bottom: 1px solid var(--vp-c-divider);
}

.tree-actions .spacer {
  flex: 1;
}

.tree-actions button {
  padding: 2px 8px;
  border-radius: 6px;
  color: var(--vp-c-text-2);
}

.tree-actions button:hover:not(:disabled) {
  background: var(--vp-c-default-soft);
  color: var(--vp-c-text-1);
}

.tree-actions button:disabled {
  color: var(--vp-c-text-3);
}

.tree {
  overflow-y: auto;
  padding: 4px 6px 12px;
  overscroll-behavior: contain;
}

.group-title {
  margin: 14px 8px 4px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--vp-c-text-3);
}

.muted {
  color: var(--vp-c-text-3);
}

.back {
  display: none;
}

/* detail pane */
.detail-pane {
  min-width: 0;
}

.results h2.pane-title {
  margin: 0 0 16px;
  padding: 0;
  border: none;
  font-size: 20px;
}

.results ul {
  list-style: none;
  padding: 0;
}

.results li {
  margin: 0 0 14px;
}

.result-path {
  text-decoration: none;
  margin-right: 8px;
}

.result-path code {
  font-weight: 600;
  color: var(--vp-c-text-1);
}

.type {
  color: var(--vp-c-text-2);
  background: none;
  padding: 0;
  font-size: 13px;
}

.result-desc {
  margin-top: 2px;
  font-size: 14px;
  color: var(--vp-c-text-2);
}

.howto {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
  gap: 4px 32px;
  margin: 0 0 28px;
  padding: 14px 18px;
  border-radius: 10px;
  background: var(--vp-c-bg-soft);
  font-size: 13px;
  line-height: 1.6;
  color: var(--vp-c-text-2);
}

.howto ul {
  margin: 0;
  padding-left: 16px;
}

.howto li {
  margin: 2px 0;
}

.howto code {
  font-size: 12px;
}

.howto kbd {
  padding: 0 5px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 4px;
  font-family: var(--vp-font-family-mono);
  font-size: 11px;
}

.ov-group + .ov-group {
  margin-top: 28px;
}

.overview h2.ov-title {
  margin: 0 0 12px;
  padding: 0;
  border: none;
  font-size: 13px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--vp-c-text-2);
}

.cards {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
  gap: 12px;
}

.card {
  display: flex;
  flex-direction: column;
  padding: 12px 14px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 10px;
  transition: border-color 0.15s;
}

.card:hover {
  border-color: var(--vp-c-brand-1);
}

.card-head {
  display: flex;
  align-items: center;
  gap: 6px;
}

.card-name {
  text-decoration: none;
}

.card-name code {
  font-weight: 600;
  color: var(--vp-c-text-1);
}

.card-head .count {
  margin-left: auto;
  font-size: 12px;
}

.card-desc {
  flex: 1;
  margin: 6px 0 8px;
  font-size: 13px;
  line-height: 1.5;
  color: var(--vp-c-text-2);
  display: -webkit-box;
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.card-guide {
  font-size: 13px;
  text-decoration: none;
}

@media (max-width: 767px) {
  .explorer {
    grid-template-columns: minmax(0, 1fr);
  }

  .tree-pane {
    position: static;
    max-height: none;
  }

  .explorer.has-selection .tree-pane,
  .explorer:not(.has-selection) .overview {
    display: none;
  }

  .back {
    display: inline-block;
    margin-bottom: 12px;
    padding: 4px 10px;
    border: 1px solid var(--vp-c-divider);
    border-radius: 8px;
    font-size: 14px;
    color: var(--vp-c-text-2);
  }
}
</style>
