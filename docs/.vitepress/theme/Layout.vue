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

    <!-- Chef logo as the commanding centerpiece -->
    <template #home-hero-info-before>
      <div class="hero-chef-logo">
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
    </template>

    <!-- Right column: install + action buttons -->
    <template #home-hero-info-after>
      <div class="hero-right">
        <a class="hero-aube-banner" href="https://aube.en.dev/">
          <span class="aube-kicker">New from en.dev by @jdx</span>
          <span class="aube-message">Try aube, a fast Node.js package manager that uses your existing lockfile. Now in beta.</span>
        </a>
        <div class="hero-install">
          <div class="install-label">Install</div>
          <div class="install-command" @click="copyInstall">
            <code>curl https://mise.run | sh</code>
            <span class="install-copy" :class="{ copied }">
              {{ copied ? '✓' : '⎘' }}
            </span>
          </div>
          <div class="install-alt">
            <a href="/installing-mise.html">More install methods →</a>
          </div>
        </div>
        <div class="hero-actions">
          <a class="action-btn action-btn-brand" href="/getting-started.html">Getting Started</a>
          <a class="action-btn action-btn-alt" href="/demo.html">Demo</a>
          <a class="action-btn action-btn-alt" href="/about.html">About</a>
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

function copyInstall() {
  navigator.clipboard.writeText("curl https://mise.run | sh");
  copied.value = true;
  setTimeout(() => (copied.value = false), 2000);
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
  background: radial-gradient(circle, #8B2252 0%, transparent 70%);
  animation: glowDrift1 12s ease-in-out infinite;
}

.hero-glow-2 {
  width: 500px;
  height: 500px;
  top: -100px;
  right: -80px;
  background: radial-gradient(circle, #D4A76A 0%, transparent 70%);
  animation: glowDrift2 15s ease-in-out infinite;
}

.hero-glow-3 {
  width: 400px;
  height: 400px;
  bottom: -100px;
  left: 30%;
  background: radial-gradient(circle, #6B7F4E 0%, transparent 70%);
  animation: glowDrift3 18s ease-in-out infinite;
}

/* Dark mode: brighter, moodier glows */
.dark .hero-glow { opacity: 0.12; }
.dark .hero-glow-1 {
  background: radial-gradient(circle, #C75B7A 0%, transparent 70%);
}
.dark .hero-glow-2 {
  background: radial-gradient(circle, #D4A76A 0%, transparent 70%);
}
.dark .hero-glow-3 {
  background: radial-gradient(circle, #8FA86E 0%, transparent 70%);
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
  0%, 100% { transform: translate(0, 0) scale(1); }
  50% { transform: translate(40px, 30px) scale(1.1); }
}

@keyframes glowDrift2 {
  0%, 100% { transform: translate(0, 0) scale(1); }
  50% { transform: translate(-30px, 40px) scale(1.15); }
}

@keyframes glowDrift3 {
  0%, 100% { transform: translate(0, 0) scale(1); }
  50% { transform: translate(20px, -30px) scale(1.05); }
}

/* ═══════════════════════════════════════════
   CHEF LOGO — commanding centerpiece
   ═══════════════════════════════════════════ */
.hero-chef-logo {
  display: flex;
  justify-content: flex-start;
  margin-bottom: 12px;
}

.hero-chef-logo .chef-logo {
  width: 420px;
  max-width: 90vw;
  height: auto;
  filter: drop-shadow(0 4px 20px rgba(139, 34, 82, 0.15));
  transition: filter 0.4s ease;
}

.hero-chef-logo .chef-logo:hover {
  filter: drop-shadow(0 8px 32px rgba(139, 34, 82, 0.25));
}

/* Light mode: show light (black) logo, hide dark */
.hero-chef-logo .chef-logo-dark {
  display: none;
}

/* Dark mode: swap logos */
.dark .hero-chef-logo .chef-logo-light {
  display: none;
}

.dark .hero-chef-logo .chef-logo-dark {
  display: inline;
}

.dark .hero-chef-logo .chef-logo {
  filter: drop-shadow(0 4px 24px rgba(199, 91, 122, 0.2));
}

.dark .hero-chef-logo .chef-logo:hover {
  filter: drop-shadow(0 8px 40px rgba(199, 91, 122, 0.35));
}

@keyframes heroLogoIn {
  from {
    opacity: 0;
    transform: translateY(-30px) scale(0.95);
  }
  to {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
}

/* ═══════════════════════════════════════════
   AUBE PROMO — compact cross-project banner
   ═══════════════════════════════════════════ */
.hero-aube-banner {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 2px;
  width: 100%;
  padding: 8px 12px;
  color: var(--vp-c-text-1);
  background:
    linear-gradient(90deg, rgba(107, 127, 78, 0.12), rgba(139, 34, 82, 0.08)),
    var(--vp-c-bg-soft);
  border: 1px solid rgba(107, 127, 78, 0.35);
  border-radius: 8px;
  text-decoration: none;
  box-shadow: 0 8px 24px rgba(26, 18, 16, 0.06);
  transition:
    border-color 0.2s ease,
    box-shadow 0.2s ease,
    transform 0.2s ease;
}

.hero-aube-banner:hover {
  border-color: var(--vp-c-brand-1);
  box-shadow: 0 12px 32px rgba(139, 34, 82, 0.12);
  transform: translateY(-1px);
}

.dark .hero-aube-banner {
  background:
    linear-gradient(90deg, rgba(143, 168, 110, 0.14), rgba(199, 91, 122, 0.1)),
    var(--vp-c-bg-soft);
  border-color: rgba(143, 168, 110, 0.35);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.16);
}

.aube-kicker {
  font-family: "Roc Grotesk", sans-serif;
  font-size: 0.72rem;
  line-height: 1;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}

.aube-kicker {
  color: #6B7F4E;
}

.dark .aube-kicker {
  color: #8FA86E;
}

.aube-message {
  min-width: 0;
  font-size: 0.78rem;
  line-height: 1.25;
  color: var(--vp-c-text-2);
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

.install-label {
  font-family: "Roc Grotesk", sans-serif;
  font-weight: 400;
  font-size: 0.75rem;
  letter-spacing: 0.15em;
  text-transform: uppercase;
  color: var(--vp-c-text-3);
  margin-bottom: 8px;
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
}

.install-alt a {
  color: var(--vp-c-text-3);
  text-decoration: none;
  transition: color 0.2s ease;
}

.install-alt a:hover {
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
  .hero-chef-logo .chef-logo {
    max-width: 280px;
  }

  .hero-aube-banner {
    align-items: center;
    text-align: center;
  }

  .aube-message {
    font-size: 0.85rem;
  }

  .hero-glow-1 { width: 350px; height: 350px; }
  .hero-glow-2 { width: 300px; height: 300px; }
  .hero-glow-3 { width: 250px; height: 250px; }

  .install-command code {
    font-size: 0.85rem;
  }
}

@media (max-width: 480px) {
  .hero-chef-logo .chef-logo {
    max-width: 220px;
  }
}
</style>
