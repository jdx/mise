<template>
  <DefaultTheme.Layout>
    <template #home-hero-info-before>
      <div class="hero-copy">
        <div class="hero-lockup">
          <img class="chef-logo chef-logo-light" src="/logo-light.svg" alt="" />
          <img class="chef-logo chef-logo-dark" src="/logo-dark.svg" alt="" />
          <span class="lockup-word">mise-en-place</span>
        </div>
        <h1>
          Every tool, env var, and task your project needs,
          <em>in one file.</em>
        </h1>
        <p class="hero-lede">
          mise reads a <code>mise.toml</code> checked into your repo, installs
          the right versions of your dev tools, loads the project's environment,
          and runs its tasks. Point it at a fresh machine and
          <code>mise bootstrap</code> sets up the rest: packages, dotfiles,
          services.
        </p>
        <div class="hero-actions">
          <button class="install-command" type="button" @click="copyInstall">
            <code>curl https://mise.run | sh</code>
            <span class="install-copy" :class="{ copied }">{{
              copied ? "copied" : "copy"
            }}</span>
          </button>
          <a class="action-btn action-btn-brand" href="/getting-started"
            >Getting started</a
          >
          <a class="action-btn action-btn-alt" href="/demo">Watch the demo</a>
        </div>
        <p class="hero-meta">
          <span>Open source, MIT</span>
          <span>macOS, Linux, Windows</span>
          <span>One static binary, no dependencies</span>
        </p>
      </div>
    </template>

    <template #layout-bottom>
      <EndevSponsors />
      <EndevFooter />
    </template>
  </DefaultTheme.Layout>
</template>

<script setup lang="ts">
import { useRoute } from "vitepress";
import DefaultTheme from "vitepress/theme";
import { nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import EndevFooter from "./EndevFooter.vue";
import EndevSponsors from "./EndevSponsors.vue";

const copied = ref(false);
const installCommand = "curl https://mise.run | sh";

// Hide the navbar brand while the big hero lockup is on screen so the
// logo appears exactly once; it fades into the header as you scroll past.
const route = useRoute();

function updateNavBrand() {
  const lockup = document.querySelector(".hero-lockup");
  const navBottom =
    document.querySelector(".VPNavBar")?.getBoundingClientRect().bottom ?? 64;
  // On narrow viewports the navbar scrolls away with the page and its
  // rect bottom goes negative — clamp so the lockup check stays sane.
  const threshold = Math.max(navBottom, 0) + 8;
  const hide = !!lockup && lockup.getBoundingClientRect().bottom > threshold;
  document.documentElement.classList.toggle("hide-nav-brand", hide);
}

watch(
  () => route.path,
  () => nextTick(updateNavBrand),
);

onMounted(() => {
  window.addEventListener("scroll", updateNavBrand, { passive: true });
  window.addEventListener("resize", updateNavBrand, { passive: true });
  updateNavBrand();
  // Hydration is done and the nav state is correct — drop the pre-paint
  // preboot classes (set by the inline head script in config.ts) after the
  // corrected state has painted, re-enabling normal transitions.
  requestAnimationFrame(() =>
    requestAnimationFrame(() =>
      document.documentElement.classList.remove("preboot", "preboot-sidebar"),
    ),
  );
});

onUnmounted(() => {
  window.removeEventListener("scroll", updateNavBrand);
  window.removeEventListener("resize", updateNavBrand);
  document.documentElement.classList.remove("hide-nav-brand");
});

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
   INSTALL COMMAND (hero)
   ═══════════════════════════════════════════ */
.install-command {
  display: inline-flex;
  align-items: center;
  gap: 16px;
  height: 48px;
  padding: 0 18px;
  background: var(--vp-c-bg-soft);
  border: 1px solid var(--vp-c-divider);
  border-radius: 8px;
  cursor: pointer;
  transition:
    border-color 0.2s ease,
    box-shadow 0.2s ease;
}

.install-command:hover {
  border-color: var(--vp-c-brand-1);
  box-shadow: 0 8px 24px -16px rgba(139, 34, 82, 0.35);
}

.install-command code {
  font-family: var(--vp-font-family-mono);
  font-size: 0.9rem;
  color: var(--vp-c-text-1);
  background: none;
  padding: 0;
  letter-spacing: -0.01em;
  white-space: nowrap;
}

.install-copy {
  font-family: var(--vp-font-family-mono);
  font-size: 0.68rem;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: var(--vp-c-text-3);
  transition: color 0.2s ease;
  user-select: none;
  min-width: 4.5em;
  text-align: right;
}

.install-copy.copied {
  color: var(--vp-c-success-1);
}

.install-command:hover .install-copy {
  color: var(--vp-c-brand-1);
}

@media (max-width: 640px) {
  .install-command {
    width: 100%;
    justify-content: space-between;
  }

  .install-command code {
    font-size: 0.82rem;
  }
}
</style>
