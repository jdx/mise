<script setup lang="ts">
import { computed, onUnmounted, ref } from "vue";
import { data as showreel } from "../showreel.data";

// With a rendered showreel, "Watch the demo" goes to the player under the
// hero; builds without one keep the recorded demo page.
const demoLink = showreel ? "/#showreel" : "/demo";

const examples = [
  {
    name: "Tools",
    section: "[tools]",
    lines: ['node = "24"', 'python = "3.14"'],
    command: "mise install",
    output: ["✓ installed 2 tools in 6.2s: node@24.21.0, python@3.14.8"],
    caption: "Tool versions for this project",
    link: "/dev-tools/",
  },
  {
    name: "Environments",
    section: "[env]",
    lines: [
      'DATABASE_URL = "postgres://localhost/app"',
      '_.file = ".env.local"',
    ],
    command: "mise set",
    output: [
      "key           value                    source",
      "DATABASE_URL  postgres://localhost/app ~/my-project/mise.toml",
      "API_KEY       dev-key                  ~/my-project/.env.local",
    ],
    caption: "Project environment variables",
    link: "/environments/",
  },
  {
    name: "Tasks",
    section: "[tasks.lint]",
    lines: ['run = "ruff check"'],
    command: "mise run lint",
    output: ["[lint] $ ruff check", "All checks passed!"],
    caption: "A named command to lint the project",
    link: "/tasks/",
  },
  {
    name: "Bootstrap",
    section: "[bootstrap.packages]",
    lines: ['"apt:tmux" = "latest"', '"apt:tree" = "latest"'],
    command: "mise bootstrap status",
    output: [
      "Part      Item      Current  State",
      "packages  apt:tmux           missing",
      "packages  apt:tree           missing",
    ],
    caption: "System packages for this machine",
    link: "/bootstrap.html",
  },
];
const selected = ref(0);
const active = computed(() => examples[selected.value]);
const copyState = ref("Copy");
const installCommand = "curl -fsSL https://mise.run | sh";
const installCode = ref<HTMLElement | null>(null);
let copyTimeout: ReturnType<typeof setTimeout> | undefined;

async function copyInstall() {
  let copied = false;
  try {
    await navigator.clipboard.writeText(installCommand);
    copied = true;
  } catch {
    // Keep copy working when the Clipboard API is unavailable or denied.
    const button = document.activeElement;
    const textarea = document.createElement("textarea");
    textarea.value = installCommand;
    textarea.setAttribute("readonly", "");
    textarea.style.position = "fixed";
    textarea.style.opacity = "0";
    document.body.appendChild(textarea);
    textarea.select();
    try {
      copied = document.execCommand("copy");
    } catch {
      // Select the visible command below if neither clipboard method works.
    } finally {
      textarea.remove();
      if (button instanceof HTMLElement) button.focus({ preventScroll: true });
    }
  }
  clearTimeout(copyTimeout);
  if (!installCode.value) return;
  if (copied) {
    copyState.value = "Copied!";
    copyTimeout = setTimeout(() => (copyState.value = "Copy"), 2500);
  } else {
    const selection = window.getSelection();
    if (selection) {
      installCode.value.focus({ preventScroll: true });
      const range = document.createRange();
      range.selectNodeContents(installCode.value);
      selection.removeAllRanges();
      selection.addRange(range);
      copyState.value = "Press Ctrl/Cmd+C";
    } else {
      copyState.value = "Select to copy";
    }
  }
}
onUnmounted(() => clearTimeout(copyTimeout));
</script>

<template>
  <section class="home-hero" aria-labelledby="home-title">
    <div class="hero-copy">
      <a class="hero-song" href="/mise-en-place.html">
        <span class="hero-song-play" aria-hidden="true">▶</span>
        <span
          ><strong>mise run</strong>, the theme song<span
            class="hero-song-extra"
          >
            and music video</span
          ></span
        >
        <span class="hero-song-arrow" aria-hidden="true">→</span>
      </a>
      <h1 id="home-title" class="hero-title">mise-en-place</h1>
      <p class="hero-meaning">Dev tools, env vars, and tasks in one CLI</p>
      <p class="hero-pronunciation">
        mise is pronounced <strong>“meez”</strong>
      </p>
      <p class="hero-lede">
        mise installs a project's tools, sets its environment variables, and
        runs its tasks from one <code>mise.toml</code> that works in your shell,
        editor, and CI. With <code>mise bootstrap</code>, it can also set up a
        whole machine: packages, dotfiles, and services.
      </p>
      <div class="hero-actions">
        <a class="action-btn action-btn-brand" href="/getting-started.html">
          Get started <span aria-hidden="true">→</span>
        </a>
        <a class="action-btn action-btn-alt" :href="demoLink">Watch the demo</a>
      </div>
      <div class="hero-install">
        <span class="install-prompt" aria-hidden="true">$</span>
        <code ref="installCode" tabindex="-1">{{ installCommand }}</code>
        <button
          type="button"
          aria-label="Copy mise install command"
          @click="copyInstall"
        >
          <span aria-live="polite">{{ copyState }}</span>
        </button>
      </div>
      <p class="hero-install-note">
        macOS &amp; Linux <span aria-hidden="true">·</span>
        <a href="/installing-mise.html#windows">Installing on Windows?</a>
      </p>
    </div>
    <div class="hero-workbench">
      <div class="workbench-bar">
        <span class="workbench-file"
          ><span aria-hidden="true">≡</span> mise.toml</span
        >
        <span>Example configuration</span>
      </div>
      <div
        class="workbench-select"
        role="group"
        aria-label="Explore mise features"
      >
        <button
          v-for="(example, index) in examples"
          :key="example.name"
          type="button"
          :aria-pressed="selected === index"
          aria-controls="workbench-example"
          @click="selected = index"
        >
          {{ example.name }}
        </button>
      </div>
      <div id="workbench-example" aria-live="polite" aria-atomic="true">
        <div class="workbench-config">
          <p class="workbench-comment"># {{ active.caption }}</p>
          <pre
            :aria-label="`${active.name} configuration example`"
          ><code><span class="workbench-section">{{ active.section }}</span>
<span v-for="line in active.lines" :key="line" class="workbench-line">{{ line.split(' = ')[0] }}<span class="workbench-equals"> = </span><span class="workbench-value">{{ line.split(' = ')[1] }}</span>
</span></code></pre>
        </div>
        <div class="workbench-terminal">
          <p class="workbench-terminal-label">
            Example output <span>~/my-project</span>
          </p>
          <pre><code><span class="workbench-prompt">$</span> {{ active.command }}
<span v-for="line in active.output" :key="line" class="workbench-output">{{ line }}
</span></code></pre>
        </div>
      </div>
      <a class="workbench-guide" :href="active.link"
        >Explore {{ active.name.toLowerCase() }}
        <span aria-hidden="true">→</span></a
      >
    </div>
  </section>
  <div class="hero-footnote">
    <span>Open source, MIT licensed</span>
    <ul aria-label="About mise">
      <li>macOS, Linux &amp; Windows</li>
      <li>Written in Rust</li>
      <li>Releases several times a week</li>
    </ul>
  </div>
</template>
