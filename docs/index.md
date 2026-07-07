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
      <h2>One file for your whole dev environment.</h2>
      <p>
        In a kitchen, <em>mise en place</em> means everything is prepped before
        cooking starts. mise applies the same idea to your dev environment.
      </p>
      <p>
        It installs the tools your project needs, loads its env vars, and runs
        its tasks — all configured in a single <code>mise.toml</code> checked
        into your repo, so every machine gets the same setup.
      </p>
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
      <h2>What mise does.</h2>
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
    <p>mise installs and manages 900+ tools</p>
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
      <h2>aube: a fast Node.js package manager.</h2>
      <p>
        From the author of mise. aube works with your existing lockfile — no
        migration needed.
      </p>
    </div>
  </a>

  <div class="landing-section landing-quickstart">
    <div>
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
          <pre><code>$ curl https://mise.run | sh<br /><br />$ mise --version<br />2026.7.0 linux-x64</code></pre>
        </div>
        <div class="recipe-panel recipe-panel-2">
          <pre><code>$ mise use node@24 python@3.13<br />mise node@24.18.0 ✓ installed<br />mise python@3.13.14 ✓ installed<br />mise ./mise.toml tools: node@24.18.0, python@3.13.14</code></pre>
        </div>
        <div class="recipe-panel recipe-panel-3">
          <pre><code>$ cat .env.local<br />DATABASE_URL=postgres://localhost/orders<br /><br />$ mise env -s bash<br />export DATABASE_URL='postgres://localhost/orders'</code></pre>
        </div>
        <div class="recipe-panel recipe-panel-4">
          <pre><code>$ mise run test<br />[test] $ pytest<br />42 passed in 1.02s</code></pre>
        </div>
      </div>
    </div>
  </div>

  <div class="landing-section landing-cta">
    <h2>Get started.</h2>
    <div class="landing-mini-install"><code>curl https://mise.run | sh</code></div>
    <div class="landing-links">
      <a href="/getting-started">Getting started</a>
      <a href="/demo">Run the demo</a>
    </div>
  </div>
</section>
