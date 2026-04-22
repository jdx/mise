---
layout: home
title: Home

hero:
  name: mise-en-place
  tagline: The front-end to your dev env
---

<section class="landing-page" aria-label="mise overview">
  <div class="landing-section landing-metaphor">
    <div>
      <p class="landing-kicker">The Idea</p>
      <h2>Everything in its place, <em>before</em> you code.</h2>
      <p>
        In a professional kitchen, <em>mise en place</em> is the ritual of prep:
        knives sharpened, onions diced, stock warm, station clean. The work
        before the work.
      </p>
      <p>
        mise does the same for your dev env. It activates the right language
        versions, loads the right env vars, and wires up the right tasks for
        the commands you run.
      </p>
    </div>
    <div class="landing-definition">
      <div class="definition-word">mise en place <span>/meez ahn plahs/</span></div>
      <p>1. the gathering and arrangement of ingredients and tools before cooking.</p>
      <p>2. a polyglot tool that keeps your project tools, env, and tasks in one place.</p>
    </div>
  </div>

  <div class="landing-section landing-menu">
    <div class="landing-section-heading">
      <p class="landing-kicker">The Menu</p>
      <h2>Three dishes, one kitchen.</h2>
      <a href="/getting-started" class="landing-small-button">All docs</a>
    </div>
    <div class="landing-feature-grid">
      <a href="/dev-tools/" class="landing-feature-card">
        <span class="card-number">— 01</span>
        <span class="card-icon">🔪</span>
        <h3>Dev Tools</h3>
        <p>A polyglot tool version manager. Replaces asdf, nvm, pyenv, rbenv, and more — one tool for every language.</p>
        <span class="card-link">read more →</span>
      </a>
      <a href="/environments/" class="landing-feature-card">
        <span class="card-number">— 02</span>
        <span class="card-icon">🫕</span>
        <h3>Environments</h3>
        <p>Switch sets of environment variables per project directory. A smarter, simpler replacement for direnv.</p>
        <span class="card-link">read more →</span>
      </a>
      <a href="/tasks/" class="landing-feature-card">
        <span class="card-number">— 03</span>
        <span class="card-icon">🍳</span>
        <h3>Tasks</h3>
        <p>A powerful task runner that replaces make and npm scripts. Define, compose, and run with ease.</p>
        <span class="card-link">read more →</span>
      </a>
    </div>
  </div>

  <div class="landing-tools" aria-label="Supported tools">
    <p>— pantry · 900+ tools, 1 toml file —</p>
    <div class="landing-tools-track">
      <a href="https://mise-versions.jdx.dev/tools/node">node</a><a href="https://mise-versions.jdx.dev/tools/python">python</a><a href="https://mise-versions.jdx.dev/tools/ruby">ruby</a><a href="https://mise-versions.jdx.dev/tools/go">go</a><a href="https://mise-versions.jdx.dev/tools/rust">rust</a><a href="https://mise-versions.jdx.dev/tools/java">java</a><a href="https://mise-versions.jdx.dev/tools/deno">deno</a><a href="https://mise-versions.jdx.dev/tools/bun">bun</a><a href="https://mise-versions.jdx.dev/tools/terraform">terraform</a><a href="https://mise-versions.jdx.dev/tools/kubectl">kubectl</a><a href="https://mise-versions.jdx.dev/tools/zig">zig</a><a href="https://mise-versions.jdx.dev/tools/swift">swift</a><a href="https://mise-versions.jdx.dev/tools/php">php</a><a href="https://mise-versions.jdx.dev/tools/elixir">elixir</a><a href="https://mise-versions.jdx.dev/tools/node">node</a><a href="https://mise-versions.jdx.dev/tools/python">python</a><a href="https://mise-versions.jdx.dev/tools/ruby">ruby</a><a href="https://mise-versions.jdx.dev/tools/go">go</a><a href="https://mise-versions.jdx.dev/tools/rust">rust</a><a href="https://mise-versions.jdx.dev/tools/java">java</a><a href="https://mise-versions.jdx.dev/tools/deno">deno</a><a href="https://mise-versions.jdx.dev/tools/bun">bun</a><a href="https://mise-versions.jdx.dev/tools/terraform">terraform</a><a href="https://mise-versions.jdx.dev/tools/kubectl">kubectl</a><a href="https://mise-versions.jdx.dev/tools/zig">zig</a><a href="https://mise-versions.jdx.dev/tools/swift">swift</a><a href="https://mise-versions.jdx.dev/tools/php">php</a><a href="https://mise-versions.jdx.dev/tools/elixir">elixir</a>
    </div>
  </div>

  <a class="landing-aube" href="https://aube.en.dev/" aria-label="Try aube">
    <div>
      <p class="landing-kicker">Chef's Special</p>
      <h2>Meet <em>aube</em>, a fast Node.js package manager.</h2>
      <p>
        New from en.dev by @jdx. aube uses your existing lockfile and is ready
        to try in beta.
      </p>
    </div>
    <div class="aube-ticket" aria-hidden="true">
      <code>$ aube</code>
    </div>
  </a>

  <div class="landing-section landing-quickstart">
    <div>
      <p class="landing-kicker">The Recipe</p>
      <h2>Four steps to a prepped station.</h2>
    </div>
    <div class="landing-recipe-grid">
      <input class="recipe-tab-input" type="radio" name="recipe-tab" id="recipe-tab-1" checked />
      <input class="recipe-tab-input" type="radio" name="recipe-tab" id="recipe-tab-2" />
      <input class="recipe-tab-input" type="radio" name="recipe-tab" id="recipe-tab-3" />
      <input class="recipe-tab-input" type="radio" name="recipe-tab" id="recipe-tab-4" />
      <div class="recipe-steps" aria-label="Recipe steps">
        <label class="recipe-step recipe-step-1" for="recipe-tab-1"><span>01</span> Install mise</label>
        <label class="recipe-step recipe-step-2" for="recipe-tab-2"><span>02</span> Add and install tools</label>
        <label class="recipe-step recipe-step-3" for="recipe-tab-3"><span>03</span> Load env vars</label>
        <label class="recipe-step recipe-step-4" for="recipe-tab-4"><span>04</span> Define tasks</label>
      </div>
      <div class="landing-code">
        <div class="recipe-panel recipe-panel-1">
          <pre><code>$ curl https://mise.run | sh<br />✓ mise installed<br /><br />$ mise doctor<br />✓ mise is ready</code></pre>
        </div>
        <div class="recipe-panel recipe-panel-2">
          <pre><code>$ mise use node@24 python@3.13 terraform@1<br />✓ wrote mise.toml<br /><br />$ mise install<br />✓ installed 3 tools</code></pre>
        </div>
        <div class="recipe-panel recipe-panel-3">
          <pre><code>$ cat .env.local<br />DATABASE_URL=postgres://localhost/orders<br /><br />$ mise env -s bash<br />export DATABASE_URL=postgres://localhost/orders</code></pre>
        </div>
        <div class="recipe-panel recipe-panel-4">
          <pre><code>$ mise run test<br />→ lint · typecheck · unit · e2e<br />✓ 4 tasks complete<br /><br />$ mise run deploy<br />✓ shipped</code></pre>
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
