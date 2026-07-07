<template>
  <DefaultTheme.Layout>
    <template #home-hero-info-before>
      <div class="hero-copy">
        <div class="hero-eyebrow">mise · pronounced "meez"</div>
        <div class="hero-lockup">
          <img
            class="chef-logo chef-logo-light"
            src="/logo-full-light.svg"
            alt="mise-en-place"
          />
          <img
            class="chef-logo chef-logo-dark"
            src="/logo-full-dark.svg"
            alt="mise-en-place"
          />
        </div>
        <h1>The front-end to your dev env</h1>
        <p>One tool that manages dev tools, env vars, and tasks per project.</p>
        <div class="hero-actions">
          <a class="action-btn action-btn-brand" href="/getting-started"
            >Getting Started</a
          >
          <a class="action-btn action-btn-alt" href="/demo">Demo</a>
        </div>
      </div>
    </template>

    <template #home-hero-info-after>
      <div class="hero-right">
        <div class="hero-terminal" aria-label="mise terminal example">
          <div class="terminal-bar">
            <span></span>
            <span></span>
            <span></span>
            <strong>~/projects/orders · zsh</strong>
          </div>
          <div class="terminal-body">
            <div>
              <span class="prompt">$</span> mise use node@24 python@3.13
            </div>
            <div>
              <span class="dim">mise</span> node@24.18.0
              <span class="ok">✓ installed</span>
            </div>
            <div>
              <span class="dim">mise</span> python@3.13.14
              <span class="ok">✓ installed</span>
            </div>
            <div>
              <span class="dim">mise</span> ./mise.toml tools: node@24.18.0,
              python@3.13.14
            </div>
            <div><span class="prompt">$</span> node --version</div>
            <div>v24.18.0</div>
            <div><span class="prompt">$</span> mise run build</div>
            <div><span class="key">[build]</span> $ tsc</div>
          </div>
        </div>
        <div class="hero-install">
          <button class="install-command" type="button" @click="copyInstall">
            <code>curl https://mise.run | sh</code>
            <span class="install-copy" :class="{ copied }">{{
              copied ? "copied" : "copy"
            }}</span>
          </button>
          <a class="install-alt" href="/installing-mise"
            >More install methods</a
          >
        </div>
      </div>
    </template>

    <template #layout-bottom>
      <EndevSponsors />
      <EndevFooter />
    </template>
  </DefaultTheme.Layout>
</template>

<script setup lang="ts">
import DefaultTheme from "vitepress/theme";
import { ref } from "vue";
import EndevFooter from "./EndevFooter.vue";
import EndevSponsors from "./EndevSponsors.vue";

const copied = ref(false);
const installCommand = "curl https://mise.run | sh";

async function copyInstall() {
  if (await copyText(installCommand)) {
    copied.value = true;
    setTimeout(() => (copied.value = false), 2000);
  }
}

async function copyText(text: string) {
  try {
    await navigator.clipboard?.writeText(text);
    if (navigator.clipboard) return true;
  } catch {
    // Fall back to the temporary textarea path below.
  }

  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  const copiedText = document.execCommand("copy");
  document.body.removeChild(textarea);
  return copiedText;
}
</script>

<style>
/* ═══════════════════════════════════════════
   INSTALL COMMAND
   ═══════════════════════════════════════════ */
.hero-install {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  margin-top: 0;
}

.install-command {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 14px 20px;
  background: var(--vp-c-bg-soft);
  border: 1px solid var(--vp-c-divider);
  border-radius: 10px;
  cursor: pointer;
  transition: all 0.25s ease;
  position: relative;
}

.install-command:hover {
  border-color: var(--vp-c-brand-1);
  box-shadow: 0 4px 20px rgba(139, 34, 82, 0.1);
  transform: translateY(-1px);
}

.dark .install-command:hover {
  box-shadow: 0 4px 20px rgba(199, 91, 122, 0.1);
}

.install-command code {
  font-family: "JetBrains Mono", var(--vp-font-family-mono);
  font-size: 0.95rem;
  color: var(--vp-c-text-1);
  background: none;
  padding: 0;
  letter-spacing: -0.01em;
}

.install-copy {
  font-size: 1.1rem;
  color: var(--vp-c-text-3);
  transition: all 0.2s ease;
  user-select: none;
  min-width: 1.2em;
  text-align: center;
}

.install-copy.copied {
  color: var(--vp-c-success-1);
}

.install-command:hover .install-copy {
  color: var(--vp-c-brand-1);
}

.install-alt {
  margin-top: 10px;
  font-size: 0.85rem;
  font-family: "Roc Grotesk", sans-serif;
  color: var(--vp-c-text-3);
  text-decoration: none;
  transition: color 0.2s ease;
}

.install-alt:hover {
  color: var(--vp-c-brand-1);
}

/* ═══════════════════════════════════════════
   RESPONSIVE
   ═══════════════════════════════════════════ */
@media (max-width: 768px) {
  .install-command code {
    font-size: 0.85rem;
  }
}
</style>
