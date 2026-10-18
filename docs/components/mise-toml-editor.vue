<script setup lang="ts">
// An editor for a mise.toml that knows the schema: hover a key or value to see what it is,
// type for completions, and see problems as they are made. The editor itself loads after the
// page, so until then, and without scripts, the config shows as plain text.
import { onBeforeUnmount, onMounted, ref } from "vue";

const props = defineProps<{
  /** The config to start with. */
  code?: string;
  /** Show the config without letting it be edited; hover still works. */
  readonly?: boolean;
  /** Keep the config in the page's URL, so a link shares it. */
  share?: boolean;
}>();

const host = ref<HTMLElement>();
const ready = ref(false);
const failed = ref(false);
const copied = ref("");
let view: import("@codemirror/view").EditorView | undefined;
let current = props.code ?? "";

const PARAM = "config";

// The config in the URL is base64url, so a link survives being pasted anywhere.
function encode(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary)
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

function decode(text: string): string | undefined {
  try {
    const binary = atob(text.replace(/-/g, "+").replace(/_/g, "/"));
    return new TextDecoder().decode(
      Uint8Array.from(binary, (c) => c.charCodeAt(0)),
    );
  } catch {
    return undefined;
  }
}

onMounted(async () => {
  let doc = props.code ?? "";
  if (props.share) {
    const shared = new URLSearchParams(window.location.search).get(PARAM);
    doc = (shared && decode(shared)) ?? doc;
  }
  current = doc;
  try {
    const { createEditor } = await import("../.vitepress/mise-toml/editor.ts");
    view = await createEditor(host.value!, {
      doc,
      readonly: props.readonly,
      onChange: (text) => {
        current = text;
        copied.value = "";
      },
    });
    ready.value = true;
  } catch (error) {
    failed.value = true;
    throw error;
  }
});

onBeforeUnmount(() => view?.destroy());

async function copy(text: string, what: string) {
  try {
    await navigator.clipboard.writeText(text);
    copied.value = what;
  } catch {
    copied.value = "";
  }
}

function copyConfig() {
  void copy(current, "config");
}

function copyLink() {
  const url = new URL(window.location.href);
  url.searchParams.set(PARAM, encode(current));
  url.hash = "";
  window.history.replaceState(window.history.state, "", url);
  void copy(url.toString(), "link");
}

function reset() {
  if (!view) return;
  view.dispatch({
    changes: { from: 0, to: view.state.doc.length, insert: props.code ?? "" },
  });
}
</script>

<template>
  <div class="mise-toml-editor" :class="{ ready }">
    <pre v-if="!ready" class="mte-fallback"><code>{{ code }}</code></pre>
    <div ref="host" class="mte-host" />
    <div v-if="ready" class="mte-toolbar">
      <span class="mte-hint">
        {{
          readonly
            ? "Hover a key for what it does"
            : "Hover a key for what it does; Ctrl-Space suggests keys"
        }}
      </span>
      <span class="mte-actions">
        <button type="button" @click="copyConfig">
          {{ copied === "config" ? "Copied" : "Copy" }}
        </button>
        <button v-if="share" type="button" @click="copyLink">
          {{ copied === "link" ? "Link copied" : "Copy link" }}
        </button>
        <button v-if="!readonly" type="button" @click="reset">Reset</button>
      </span>
    </div>
    <p v-if="failed" class="mte-hint">
      The editor did not load; the config above is plain text.
    </p>
  </div>
</template>

<style>
.mise-toml-editor {
  --mte-heading: var(--vp-c-brand-1);
  --mte-key: var(--vp-c-indigo-1);
  --mte-string: var(--vp-c-green-1);
  --mte-literal: var(--vp-c-yellow-1);
  --mte-comment: var(--vp-c-text-3);
  --mte-punctuation: var(--vp-c-text-2);
  margin: 16px 0;
}

.mise-toml-editor .mte-fallback {
  margin: 0;
  padding: 20px 24px;
  background: var(--vp-code-block-bg);
  border-radius: 8px;
  overflow-x: auto;
  font-family: var(--vp-font-family-mono);
  font-size: var(--vp-code-font-size);
  line-height: 1.7;
}

.mise-toml-editor .mte-host .cm-editor {
  max-height: 32em;
}

.mise-toml-editor .mte-toolbar {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 12px;
  justify-content: space-between;
  align-items: center;
  margin-top: 6px;
  font-size: 13px;
}

.mise-toml-editor .mte-hint {
  color: var(--vp-c-text-3);
  font-size: 13px;
}

.mise-toml-editor .mte-actions {
  display: flex;
  gap: 6px;
}

.mise-toml-editor .mte-actions button {
  padding: 2px 10px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 6px;
  color: var(--vp-c-text-2);
}

.mise-toml-editor .mte-actions button:hover {
  border-color: var(--vp-c-brand-1);
  color: var(--vp-c-text-1);
}

/* The hover card: its tooltip lives inside the editor, so these apply there. */
.mte-card {
  max-width: min(32em, 80vw);
  padding: 8px 12px;
  font-size: 14px;
  line-height: 1.5;
}

.mte-card .mte-head {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 4px 8px;
}

.mte-card .mte-path {
  font-weight: 600;
  overflow-wrap: anywhere;
}

.mte-card .mte-type {
  color: var(--vp-c-text-2);
  font-family: var(--vp-font-family-mono);
  font-size: 13px;
}

.mte-card .mte-badge {
  padding: 0 6px;
  border-radius: 8px;
  font-size: 12px;
  font-weight: 500;
}

.mte-card .mte-deprecated {
  background: var(--vp-c-danger-soft);
  color: var(--vp-c-danger-1);
}

.mte-card .mte-experimental {
  background: var(--vp-c-warning-soft);
  color: var(--vp-c-warning-1);
}

.mte-card .mte-desc,
.mte-card .mte-meta {
  margin-top: 4px;
  color: var(--vp-c-text-2);
}

.mte-card code,
.cm-completionInfo code {
  padding: 1px 4px;
  border-radius: 4px;
  background: var(--vp-c-default-soft);
  font-family: var(--vp-font-family-mono);
  font-size: 0.9em;
}

.mte-card code.mte-chosen {
  background: var(--vp-c-brand-soft);
  color: var(--vp-c-brand-1);
}

.mte-card .mte-ref {
  display: inline-block;
  margin-top: 6px;
  font-size: 13px;
  color: var(--vp-c-brand-1);
}
</style>
