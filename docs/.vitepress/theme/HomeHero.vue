<script setup lang="ts">
import { computed, onUnmounted, ref } from "vue";

const examples = [
  {
    name: "Tools",
    section: "[tools]",
    lines: ['node = "24"', 'python = "3.13"', 'terraform = "1.13"'],
    command: "mise install",
    output: [
      "✓ node, python, terraform installed",
      "Tool versions ready for this project.",
    ],
    caption: "The right versions, in every project.",
    link: "/dev-tools/",
  },
  {
    name: "Environments",
    section: "[env]",
    lines: [
      'DATABASE_URL = "postgres://localhost/app"',
      '_.file = ".env.local"',
    ],
    command: "mise env",
    output: ["export DATABASE_URL=postgres://localhost/app"],
    caption: "Your environment, ready when you cd.",
    link: "/environments/",
  },
  {
    name: "Tasks",
    section: "[tasks.test]",
    lines: ['run = "python -m unittest"'],
    command: "mise run test",
    output: ["[test] $ python -m unittest", "Ran 42 tests", "OK"],
    caption: "Project commands, without the guesswork.",
    link: "/tasks/",
  },
  {
    name: "Bootstrap",
    section: "[bootstrap.packages]",
    lines: ['"brew:jq" = "latest"', '"apt:build-essential" = "latest"'],
    command: "mise bootstrap",
    output: ["✓ System packages installed", "✓ Dev tools ready"],
    caption: "A fresh machine. A familiar setup.",
    link: "/bootstrap",
  },
];
const selected = ref(0);
const active = computed(() => examples[selected.value]);
const copyState = ref("Copy");
const installCommand = "curl https://mise.run | sh";
let copyTimeout: ReturnType<typeof setTimeout> | undefined;

async function copyInstall() {
  try {
    await navigator.clipboard.writeText(installCommand);
    copyState.value = "Copied!";
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
      copyState.value = document.execCommand("copy")
        ? "Copied!"
        : "Select to copy";
    } catch {
      copyState.value = "Select to copy";
    } finally {
      textarea.remove();
      if (button instanceof HTMLElement) button.focus({ preventScroll: true });
    }
  }
  clearTimeout(copyTimeout);
  copyTimeout = setTimeout(() => (copyState.value = "Copy"), 2500);
}
onUnmounted(() => clearTimeout(copyTimeout));
</script>

<template>
  <section class="home-hero" aria-labelledby="home-title">
    <div class="hero-copy">
      <p class="hero-eyebrow">mise-en-place / everything in its place</p>
      <h1 id="home-title">Your dev setup.<br /><em>In good order.</em></h1>
      <p class="hero-lede">
        Declare your tool versions, environment variables, and commands in
        <code>mise.toml</code>. Use them in your shell, editor, and CI. Add
        machine setup with <code>mise bootstrap</code> when you need system
        packages, dotfiles, or services.
      </p>
      <div class="hero-actions">
        <a class="action-btn action-btn-brand" href="/getting-started">
          Get started <span aria-hidden="true">→</span>
        </a>
        <a class="action-btn action-btn-alt" href="/demo">Watch the demo</a>
      </div>
      <div class="hero-install">
        <span class="install-prompt" aria-hidden="true">$</span>
        <code>{{ installCommand }}</code>
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
        <a href="/getting-started#installing-mise-cli"
          >Installing on Windows?</a
        >
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
            Illustrative output <span>~/my-project</span>
          </p>
          <pre><code><span class="workbench-prompt">$</span> {{ active.command }}
<span v-for="line in active.output" :key="line" class="workbench-output">{{ line }}
</span></code></pre>
        </div>
      </div>
      <a class="workbench-guide" :href="active.link"
        >Explore {{ active.name.toLowerCase() }}
        <span aria-hidden="true">↗</span></a
      >
    </div>
  </section>
  <div class="hero-footnote">
    <span>A comfortable home for your development workflow.</span>
    <ul aria-label="About mise">
      <li>Open source &amp; MIT licensed</li>
      <li>macOS, Linux &amp; Windows</li>
      <li>Single CLI</li>
    </ul>
  </div>
</template>
