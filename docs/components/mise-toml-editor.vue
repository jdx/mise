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
  /** A bare code block that happens to have hover: no toolbar, and loaded when scrolled into view. */
  compact?: boolean;
  /** The file name shown above the config, from a ```toml [name] fence. */
  title?: string;
}>();

const host = ref<HTMLElement>();
const root = ref<HTMLElement>();
const ready = ref(false);
const failed = ref(false);
const copied = ref("");
let view: import("@codemirror/view").EditorView | undefined;
// Set when the page leaves while the editor is still loading, so it is not attached afterwards.
let unmounted = false;
// A fenced block ends with a newline that a code block hides but an editor shows as a last blank line.
const initial = props.compact
  ? (props.code ?? "").replace(/\n+$/, "")
  : (props.code ?? "");
let current = initial;

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

// Pages show hundreds of examples, so a compact one loads its editor only once it is near the screen.
function whenVisible(): Promise<void> {
  // The whole block, not the host below it: a tall example is in view before its host is.
  const el = root.value;
  if (!props.compact || !el || !("IntersectionObserver" in window)) {
    return Promise.resolve();
  }
  return new Promise((resolve) => {
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          observer.disconnect();
          resolve();
        }
      },
      { rootMargin: "400px" },
    );
    observer.observe(el);
    stopObserving = () => observer.disconnect();
  });
}
let stopObserving = () => {};

onMounted(async () => {
  let doc = initial;
  if (props.share) {
    const shared = new URLSearchParams(window.location.search).get(PARAM);
    doc = (shared && decode(shared)) ?? doc;
  }
  current = doc;
  try {
    await whenVisible();
    if (unmounted) return;
    const { createEditor } = await import("../.vitepress/mise-toml/editor.ts");
    if (unmounted) return;
    const created = await createEditor(host.value!, {
      doc,
      readonly: props.readonly,
      onChange: (text) => {
        current = text;
        copied.value = "";
      },
    });
    if (unmounted) {
      created.destroy();
      return;
    }
    view = created;
    ready.value = true;
  } catch (error) {
    failed.value = true;
    throw error;
  }
});

onBeforeUnmount(() => {
  unmounted = true;
  stopObserving();
  view?.destroy();
});

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

// The playground takes the config from the URL, the same as a shared link.
function openPlayground() {
  const url = new URL("/configuration/playground.html", window.location.origin);
  url.searchParams.set(PARAM, encode(props.code ?? ""));
  window.open(url, "_blank", "noopener");
}

function reset() {
  if (!view) return;
  view.dispatch({
    changes: { from: 0, to: view.state.doc.length, insert: props.code ?? "" },
  });
}
</script>

<template>
  <div
    ref="root"
    class="mise-toml-editor"
    :class="{ ready, compact, titled: !!title }"
  >
    <div v-if="title" class="mte-title">{{ title }}</div>
    <pre v-if="!ready" class="mte-fallback"><code>{{ initial }}</code></pre>
    <div ref="host" class="mte-host" />
    <span v-if="compact" class="mte-corner">
      <button
        type="button"
        aria-label="Edit in the playground"
        title="Edit in the playground"
        @click="openPlayground"
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path
            d="M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"
          />
        </svg>
      </button>
      <button
        type="button"
        :aria-label="copied === 'config' ? 'Copied' : 'Copy code'"
        :title="copied === 'config' ? 'Copied' : 'Copy code'"
        @click="copyConfig"
      >
        <svg v-if="copied === 'config'" viewBox="0 0 24 24" aria-hidden="true">
          <path d="M5 12.5l4.5 4.5L19 7.5" />
        </svg>
        <svg v-else viewBox="0 0 24 24" aria-hidden="true">
          <path
            d="M9 9h10a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H9a1 1 0 0 1-1-1V10a1 1 0 0 1 1-1zM16 9V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3"
          />
        </svg>
      </button>
    </span>
    <div v-if="ready && !compact" class="mte-toolbar">
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

.mise-toml-editor.compact {
  position: relative;
  margin: 16px 0;
}

/* Like the copy button on a plain code block: in the corner, shown on hover or focus. */
.mise-toml-editor .mte-corner {
  position: absolute;
  top: 8px;
  right: 8px;
  z-index: 5;
  display: flex;
  gap: 6px;
  opacity: 0;
  transition: opacity 0.25s;
}

.mise-toml-editor .mte-corner button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border: 1px solid var(--vp-code-copy-code-border-color, var(--vp-c-divider));
  border-radius: 4px;
  background: var(--vp-code-copy-code-bg, var(--vp-code-block-bg));
  color: var(--vp-c-text-2);
}

.mise-toml-editor .mte-corner svg {
  width: 16px;
  height: 16px;
  fill: none;
  stroke: currentColor;
  stroke-width: 2;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.mise-toml-editor.titled .mte-corner {
  top: 48px;
}

/* A code group's tab shows the title, so the block has no title bar to clear. */
.vp-code-group .mise-toml-editor.titled .mte-corner {
  top: 8px;
}

.mise-toml-editor.compact:hover .mte-corner,
.mise-toml-editor .mte-corner:focus-within {
  opacity: 1;
}

/* Touch screens have no hover to reveal them, and a pointer cannot move away to uncover the
   code, so the buttons get a row of their own between the title and the code. */
@media (hover: none) {
  .mise-toml-editor.compact {
    display: flex;
    flex-direction: column;
  }

  .mise-toml-editor.compact .mte-title {
    order: 0;
  }

  .mise-toml-editor.compact .mte-corner,
  .mise-toml-editor.titled .mte-corner,
  .vp-code-group .mise-toml-editor.titled .mte-corner {
    position: static;
    order: 1;
    justify-content: flex-end;
    padding: 6px 8px 0;
    background: var(--vp-code-block-bg);
    border-radius: 8px 8px 0 0;
    opacity: 1;
  }

  .mise-toml-editor.compact .mte-fallback,
  .mise-toml-editor.compact .mte-host {
    order: 2;
  }

  .mise-toml-editor.compact .mte-hint {
    order: 3;
  }
}

.mise-toml-editor .mte-corner button:hover {
  border-color: var(--vp-c-brand-1);
  color: var(--vp-c-text-1);
}

.vp-code-group .mise-toml-editor .mte-title {
  display: none;
}

.mise-toml-editor .mte-title {
  display: flex;
  align-items: center;
  box-sizing: border-box;
  height: 40px;
  padding: 0 24px;
  background: var(--vp-code-tab-bg);
  border-radius: 8px 8px 0 0;
  border-bottom: 1px solid var(--vp-code-tab-divider);
  color: var(--vp-code-tab-text-color);
  font-family: var(--vp-font-family-mono);
  font-size: 13px;
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

.mise-toml-editor:not(.compact) .mte-host .cm-editor {
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

/* Scoped to the editor: it sits in a code block, and `.vp-doc [class*='language-'] code`
   would make each of these a full-width block with the block's padding. */
.mise-toml-editor .mte-card code,
.mise-toml-editor .cm-completionInfo code {
  display: inline;
  width: auto;
  min-width: 0;
  padding: 1px 4px;
  border-radius: 4px;
  background: var(--vp-c-default-soft);
  color: inherit;
  font-family: var(--vp-font-family-mono);
  font-size: 0.9em;
  line-height: inherit;
}

.mise-toml-editor .mte-card code.mte-chosen {
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
