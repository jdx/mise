<script setup>
// The selected key of the mise.toml reference: what it is, where it goes in a mise.toml,
// and the keys inside it.
import { computed, inject, ref } from "vue";
import { guides } from "/components/config-reference-sections.ts";
import { highlight, snippet } from "/components/config-reference-toml.ts";

const props = defineProps({
  item: { type: Object, required: true },
  // { byPath, parents } of the whole reference
  index: { type: Object, required: true },
  // the top-level section the key is in
  section: { type: String, required: true },
});

const { select } = inject("configReference");

// The key whose keys this one has, when it lists them there instead of here.
const same = computed(() =>
  props.item.sameAs ? props.index.byPath.get(props.item.sameAs) : undefined,
);
const children = computed(() => same.value?.children ?? props.item.children);
const childParent = computed(() => same.value?.path ?? props.item.path);

const crumbs = computed(() => {
  const out = [];
  for (
    let p = props.index.parents.get(props.item.path);
    p !== undefined;
    p = props.index.parents.get(p)
  ) {
    out.unshift(p);
  }
  return out.map((path, i) => ({
    path,
    label: i === 0 ? path : relative(out[i - 1], path),
  }));
});

function relative(parent, path) {
  return path.slice(parent.length).replace(/^\./, "");
}

const title = computed(() => {
  const parent = props.index.parents.get(props.item.path);
  return parent ? relative(parent, props.item.path) : props.item.path;
});

// Keys elsewhere that list this key's keys by pointing here.
const usedBy = computed(() =>
  [...props.index.byPath.values()]
    .filter((k) => k.sameAs === props.item.path)
    .map((k) => k.path),
);

const toml = computed(() =>
  snippet({ ...props.item, children: children.value }),
);
const tomlHtml = computed(() => highlight(toml.value));
const guide = computed(() => guides[props.section]);

const copied = ref("");
async function copy(what, text) {
  try {
    await navigator.clipboard.writeText(text);
    copied.value = what;
    setTimeout(() => (copied.value = ""), 1500);
  } catch {
    // no clipboard access: nothing to show
  }
}

function link(path) {
  return `#${path}`;
}
</script>

<template>
  <article class="detail">
    <nav v-if="crumbs.length > 0" class="crumbs" aria-label="Keys above">
      <template v-for="crumb in crumbs" :key="crumb.path">
        <a :href="link(crumb.path)" @click.prevent="select(crumb.path)"
          ><code>{{ crumb.label }}</code></a
        >
        <span class="sep" aria-hidden="true">›</span>
      </template>
    </nav>
    <div class="head">
      <h2 class="title">
        <code>{{ title }}</code>
      </h2>
      <span v-if="item.deprecated" class="VPBadge danger">deprecated</span>
      <span v-if="item.experimental" class="VPBadge warning">experimental</span>
      <span class="spacer"></span>
      <button
        type="button"
        class="ghost"
        :title="`Copy ${item.path}`"
        @click="copy('path', item.path)"
      >
        {{ copied === "path" ? "Copied" : "Copy path" }}
      </button>
    </div>
    <dl class="facts">
      <div>
        <dt>Type</dt>
        <dd>
          <code>{{ item.type }}</code>
        </dd>
      </div>
      <div v-if="item.default">
        <dt>Default</dt>
        <dd>
          <code>{{ item.default }}</code>
        </dd>
      </div>
      <div v-if="item.choices.length > 0">
        <dt>Choices</dt>
        <dd class="choices">
          <code v-for="choice in item.choices" :key="choice">{{ choice }}</code>
        </dd>
      </div>
      <div v-if="guide">
        <dt>Guide</dt>
        <dd>
          <a :href="guide.link">{{ guide.text }} →</a>
        </dd>
      </div>
    </dl>

    <p v-if="item.description" class="desc" v-html="item.description"></p>
    <p v-else class="desc muted">No description yet.</p>

    <p v-if="item.link" class="note">
      Every key is documented in
      <a :href="item.link">the settings reference</a>.
    </p>

    <div class="snippet">
      <div class="snippet-head">
        <span>mise.toml</span>
        <button type="button" class="ghost" @click="copy('toml', toml)">
          {{ copied === "toml" ? "Copied" : "Copy" }}
        </button>
      </div>
      <pre><code v-html="tomlHtml"></code></pre>
    </div>

    <section v-if="children.length > 0" class="keys">
      <h3 class="keys-title">
        Keys
        <span class="muted">{{ children.length }}</span>
      </h3>
      <p v-if="same" class="note">
        The same keys as
        <a :href="link(same.path)" @click.prevent="select(same.path)"
          ><code>{{ same.path }}</code></a
        >, listed there.
      </p>
      <table>
        <tbody>
          <tr v-for="child in children" :key="child.path">
            <td class="k">
              <a :href="link(child.path)" @click.prevent="select(child.path)"
                ><code>{{ relative(childParent, child.path) }}</code></a
              >
              <span v-if="child.deprecated" class="VPBadge danger"
                >deprecated</span
              >
              <span v-if="child.experimental" class="VPBadge warning"
                >experimental</span
              >
            </td>
            <td class="t">
              <code>{{ child.type }}</code>
            </td>
            <td class="d">
              <span v-if="child.description" v-html="child.description"></span>
              <span v-if="child.count > 0" class="muted">
                {{ child.description ? " · " : "" }}{{ child.count }}
                {{ child.count === 1 ? "key" : "keys" }}</span
              >
            </td>
          </tr>
        </tbody>
      </table>
    </section>

    <p v-if="usedBy.length > 0" class="note">
      Also the keys of
      <template v-for="(path, i) in usedBy" :key="path"
        ><template v-if="i > 0">, </template
        ><a :href="link(path)" @click.prevent="select(path)"
          ><code>{{ path }}</code></a
        ></template
      >.
    </p>
  </article>
</template>

<style scoped>
.crumbs {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 2px 6px;
  font-size: 13px;
}

.crumbs a {
  text-decoration: none;
}

.crumbs code {
  font-size: 12px;
  color: var(--vp-c-text-2);
}

.crumbs a:hover code {
  color: var(--vp-c-brand-1);
}

.sep {
  color: var(--vp-c-text-3);
}

.head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  margin: 6px 0 12px;
}

.head h2.title {
  margin: 0;
  padding: 0;
  border: none;
  font-size: 26px;
  line-height: 1.3;
  overflow-wrap: anywhere;
}

.title code {
  padding: 0;
  background: none;
  font-size: inherit;
  font-weight: 700;
  color: var(--vp-c-text-1);
}

.spacer {
  flex: 1;
}

.ghost {
  padding: 3px 10px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 6px;
  font-size: 12px;
  color: var(--vp-c-text-2);
}

.ghost:hover {
  border-color: var(--vp-c-brand-1);
  color: var(--vp-c-text-1);
}

.facts {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 28px;
  margin: 0 0 16px;
}

.facts dt {
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--vp-c-text-3);
}

.facts dd {
  margin: 2px 0 0;
  font-size: 14px;
}

.facts .choices {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.desc {
  margin: 0 0 16px;
  font-size: 16px;
  line-height: 1.7;
}

.muted {
  color: var(--vp-c-text-3);
}

.note {
  font-size: 14px;
  color: var(--vp-c-text-2);
}

.snippet {
  margin: 16px 0 24px;
  border-radius: 10px;
  background: var(--vp-code-block-bg);
  overflow: hidden;
}

.snippet-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 10px 6px 16px;
  border-bottom: 1px solid var(--vp-c-divider);
  font-size: 12px;
  color: var(--vp-c-text-3);
}

.snippet pre {
  margin: 0;
  padding: 14px 16px;
  overflow-x: auto;
  font-family: var(--vp-font-family-mono);
  font-size: 13px;
  line-height: 1.7;
}

.snippet pre code {
  padding: 0;
  background: none;
  font-size: inherit;
  color: var(--vp-c-text-1);
}

.snippet :deep(.t-header) {
  color: var(--vp-c-purple-1);
  font-weight: 600;
}

.snippet :deep(.t-key) {
  color: var(--vp-c-indigo-1);
}

.snippet :deep(.t-string) {
  color: var(--vp-c-green-1);
}

.snippet :deep(.t-literal) {
  color: var(--vp-c-yellow-1);
}

.snippet :deep(.t-comment) {
  color: var(--vp-c-text-3);
  font-style: italic;
}

.keys h3.keys-title {
  margin: 0 0 8px;
  font-size: 16px;
}

.keys table {
  display: table;
  width: 100%;
  margin: 8px 0 0;
  font-size: 14px;
}

.keys td {
  padding: 8px 12px;
  vertical-align: top;
}

.keys tr:nth-child(2n) {
  background: none;
}

.keys .k {
  white-space: nowrap;
}

.keys .k a {
  text-decoration: none;
}

.keys .k code {
  font-weight: 600;
  color: var(--vp-c-text-1);
}

.keys .k a:hover code {
  color: var(--vp-c-brand-1);
}

.keys .t code {
  padding: 0;
  background: none;
  font-size: 12px;
  color: var(--vp-c-text-2);
}

.keys .d {
  color: var(--vp-c-text-2);
}

@media (max-width: 767px) {
  .keys table,
  .keys tbody,
  .keys tr,
  .keys td {
    display: block;
  }

  .keys tr {
    padding: 8px 0;
    border-top: 1px solid var(--vp-c-divider);
  }

  .keys td {
    padding: 2px 0;
    border: none;
  }
}
</style>
