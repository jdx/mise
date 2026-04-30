<template>
  <DefaultTheme.Layout>
    <!-- Atmospheric background layer behind the entire hero -->
    <template #home-hero-before>
      <div class="hero-atmosphere" aria-hidden="true">
        <div class="hero-glow hero-glow-1"></div>
        <div class="hero-glow hero-glow-2"></div>
        <div class="hero-glow hero-glow-3"></div>
        <div class="hero-grain"></div>
      </div>
    </template>

    <!-- Landing hero content from the design handoff -->
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
        <h1>Your dev env,<br /><em>already prepped.</em></h1>
        <p>
          One tool to manage languages, env vars, and tasks per project,
          reproducibly.
        </p>
        <div class="hero-actions">
          <a class="action-btn action-btn-brand" href="/getting-started"
            >Getting Started</a
          >
          <a class="action-btn action-btn-alt" href="/demo">Demo</a>
        </div>
      </div>
    </template>

    <!-- Right column: terminal-forward proof of the workflow -->
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
            <div><span class="prompt">$</span> cd ~/projects/orders</div>
            <div>
              <span class="dim"
                ># mise picks up mise.toml and updates the shell</span
              >
            </div>
            <div><span class="ok">✓</span> node@24 active</div>
            <div><span class="ok">✓</span> python@3.13 active</div>
            <div><span class="ok">✓</span> terraform@1 active</div>
            <div>
              <span class="ok">✓</span> DATABASE_URL loaded from .env.local
            </div>
            <div><span class="prompt">$</span> mise run deploy</div>
            <div>
              <span class="key">→</span> running task "deploy" (4 steps)
            </div>
            <div>
              <span class="dim"> build · test · migrate · ship ...</span>
            </div>
            <div><span class="ok">✓</span> done in 42.1s</div>
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
      <EndevFooter />
    </template>
  </DefaultTheme.Layout>
</template>

<script setup lang="ts">
import DefaultTheme from "vitepress/theme";
import { ref } from "vue";
import EndevFooter from "./EndevFooter.vue";

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
   HERO ATMOSPHERE — radial glows + grain
   ═══════════════════════════════════════════ */
.VPHero {
  position: relative;
  overflow: hidden;
}

.hero-atmosphere {
  position: absolute;
  inset: 0;
  pointer-events: none;
  z-index: 0;
  overflow: clip;
  contain: paint;
}

.hero-glow {
  position: absolute;
  border-radius: 50%;
  filter: blur(80px);
  opacity: 0.18;
}

.hero-glow-1 {
  width: 600px;
  height: 600px;
  top: -200px;
  left: -100px;
  background: radial-gradient(circle, #8b2252 0%, transparent 70%);
  animation: glowDrift1 12s ease-in-out infinite;
}

.hero-glow-2 {
  width: 500px;
  height: 500px;
  top: -100px;
  right: -80px;
  background: radial-gradient(circle, #d4a76a 0%, transparent 70%);
  animation: glowDrift2 15s ease-in-out infinite;
}

.hero-glow-3 {
  width: 400px;
  height: 400px;
  bottom: -100px;
  left: 30%;
  background: radial-gradient(circle, #6b7f4e 0%, transparent 70%);
  animation: glowDrift3 18s ease-in-out infinite;
}

/* Dark mode: brighter, moodier glows */
.dark .hero-glow {
  opacity: 0.12;
}
.dark .hero-glow-1 {
  background: radial-gradient(circle, #c75b7a 0%, transparent 70%);
}
.dark .hero-glow-2 {
  background: radial-gradient(circle, #d4a76a 0%, transparent 70%);
}
.dark .hero-glow-3 {
  background: radial-gradient(circle, #8fa86e 0%, transparent 70%);
}

/* Subtle film grain texture */
.hero-grain {
  position: absolute;
  inset: 0;
  opacity: 0.03;
  background-image: url("data:image/svg+xml,%3Csvg viewBox='0 0 256 256' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='noise'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='4' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23noise)'/%3E%3C/svg%3E");
  background-repeat: repeat;
  background-size: 256px 256px;
}

.dark .hero-grain {
  opacity: 0.04;
}

@keyframes glowDrift1 {
  0%,
  100% {
    transform: translate(0, 0) scale(1);
  }
  50% {
    transform: translate(40px, 30px) scale(1.1);
  }
}

@keyframes glowDrift2 {
  0%,
  100% {
    transform: translate(0, 0) scale(1);
  }
  50% {
    transform: translate(-30px, 40px) scale(1.15);
  }
}

@keyframes glowDrift3 {
  0%,
  100% {
    transform: translate(0, 0) scale(1);
  }
  50% {
    transform: translate(20px, -30px) scale(1.05);
  }
}

/* ═══════════════════════════════════════════
   INSTALL COMMAND — signature dish
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

@keyframes heroFadeUp {
  from {
    opacity: 0;
    transform: translateY(16px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

/* ═══════════════════════════════════════════
   RESPONSIVE
   ═══════════════════════════════════════════ */
@media (max-width: 768px) {
  .hero-glow-1 {
    width: 350px;
    height: 350px;
  }
  .hero-glow-2 {
    width: 300px;
    height: 300px;
  }
  .hero-glow-3 {
    width: 250px;
    height: 250px;
  }

  .install-command code {
    font-size: 0.85rem;
  }
}
</style>
