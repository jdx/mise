---
layout: home
title: Home

hero:
  name: mise-en-place
  tagline: Dev tools, env vars, and tasks in one CLI
---

<section class="landing-page" aria-label="mise overview">
  <div class="landing-section landing-metaphor">
    <div>
      <p class="landing-kicker">The Idea</p>
      <h2>Everything in its place, <em>before</em> you code.</h2>
      <p>
        It installs the tools your project needs, loads its env vars, and runs
        its tasks — all configured in a single <code>mise.toml</code> checked
        into your repo, so every machine gets the same setup.
      </p>
      <div class="landing-definition">
        <div class="definition-word">mise en place <span>/meez ahn plahs/</span></div>
        <p>1. the gathering and arrangement of ingredients and tools before cooking.</p>
        <p>2. a polyglot tool that keeps your project tools, env, and tasks in one place.</p>
      </div>
    </div>
    <div class="landing-config" aria-label="Example mise.toml">

```toml
# mise.toml
[tools]
node = "24"
python = "3.13"

[env]
_.file = ".env.local"

[tasks.test]
run = "pytest"
```

</div>
  </div>

  <div class="landing-section landing-menu">
    <div class="landing-section-heading">
      <div>
        <p class="landing-kicker">The Menu</p>
        <h2>What mise does.</h2>
      </div>
      <a href="/getting-started" class="landing-small-button">All docs</a>
    </div>
    <div class="landing-feature-grid">
      <a href="/dev-tools/" class="landing-feature-card">
        <h3>Dev Tools</h3>
        <p>Install project tools, pin versions, and switch automatically as you move between directories.</p>
        <span class="card-link">Learn more</span>
      </a>
      <a href="/environments/" class="landing-feature-card">
        <h3>Environments</h3>
        <p>Load project-specific environment variables from <code>mise.toml</code>, <code>.env</code> files, shell commands, and more.</p>
        <span class="card-link">Learn more</span>
      </a>
      <a href="/tasks/" class="landing-feature-card">
        <h3>Tasks</h3>
        <p>Define build, test, lint, and deploy commands next to the tools and env vars they need.</p>
        <span class="card-link">Learn more</span>
      </a>
    </div>
  </div>

  <div class="landing-tools" aria-label="Supported tools">
    <p>The pantry · 900+ tools, one config file</p>
    <div class="landing-tools-list">
      <a href="https://mise-versions.jdx.dev/tools/node">node</a>
      <a href="https://mise-versions.jdx.dev/tools/python">python</a>
      <a href="https://mise-versions.jdx.dev/tools/ruby">ruby</a>
      <a href="https://mise-versions.jdx.dev/tools/go">go</a>
      <a href="https://mise-versions.jdx.dev/tools/rust">rust</a>
      <a href="https://mise-versions.jdx.dev/tools/java">java</a>
      <a href="https://mise-versions.jdx.dev/tools/deno">deno</a>
      <a href="https://mise-versions.jdx.dev/tools/bun">bun</a>
      <a href="https://mise-versions.jdx.dev/tools/terraform">terraform</a>
      <a href="https://mise-versions.jdx.dev/tools/kubectl">kubectl</a>
      <a href="https://mise-versions.jdx.dev/tools/zig">zig</a>
      <a href="https://mise-versions.jdx.dev/tools/swift">swift</a>
      <a href="https://mise-versions.jdx.dev/tools/php">php</a>
      <a href="https://mise-versions.jdx.dev/tools/elixir">elixir</a>
      <a href="/registry">…and 900+ more</a>
    </div>
  </div>

  <a class="landing-aube" href="https://aube.jdx.dev/" aria-label="Try aube">
    <div>
      <p class="landing-kicker">Chef's Special</p>
      <h2>aube: a fast Node.js package manager.</h2>
      <p>
        From the author of mise. aube works with your existing lockfile — no
        migration needed.
      </p>
    </div>
  </a>

  <div class="landing-section landing-quickstart">
    <div>
      <p class="landing-kicker">The Recipe</p>
      <h2>Get set up in four steps.</h2>
    </div>
    <div class="landing-recipe-grid">
      <input class="recipe-tab-input" type="radio" name="recipe-tab" id="recipe-tab-1" checked />
      <input class="recipe-tab-input" type="radio" name="recipe-tab" id="recipe-tab-2" />
      <input class="recipe-tab-input" type="radio" name="recipe-tab" id="recipe-tab-3" />
      <input class="recipe-tab-input" type="radio" name="recipe-tab" id="recipe-tab-4" />
      <div class="recipe-steps" aria-label="Quickstart steps">
        <label class="recipe-step recipe-step-1" for="recipe-tab-1"><span>01</span> Install mise</label>
        <label class="recipe-step recipe-step-2" for="recipe-tab-2"><span>02</span> Add and install tools</label>
        <label class="recipe-step recipe-step-3" for="recipe-tab-3"><span>03</span> Load env vars</label>
        <label class="recipe-step recipe-step-4" for="recipe-tab-4"><span>04</span> Define tasks</label>
      </div>
      <div class="landing-code">
        <div class="recipe-panel recipe-panel-1">

```shellsession
$ curl https://mise.run | sh

$ mise --version
2026.7.0 linux-x64
```

</div>
        <div class="recipe-panel recipe-panel-2">

```shellsession
$ mise use node@24 python@3.13
mise node@24.18.0 ✓ installed
mise python@3.13.14 ✓ installed
mise ./mise.toml tools: node@24.18.0, python@3.13.14
```

</div>
        <div class="recipe-panel recipe-panel-3">

```shellsession
$ cat .env.local
DATABASE_URL=postgres://localhost/orders

$ mise env -s bash
export DATABASE_URL='postgres://localhost/orders'
```

</div>
        <div class="recipe-panel recipe-panel-4">

```shellsession
$ mise run test
[test] $ pytest
42 passed in 1.02s
```

</div>
      </div>
    </div>
  </div>

  <div class="landing-section landing-cta">
    <p class="landing-kicker">Ready When You Are</p>
    <h2><em>Allez,</em> prep your station.</h2>
    <div class="landing-mini-install"><code>curl https://mise.run | sh</code></div>
    <div class="landing-links">
      <a href="/getting-started">Getting started</a>
      <a href="/demo">Run the demo</a>
    </div>
  </div>
</section>
