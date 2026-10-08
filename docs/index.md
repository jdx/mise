---
layout: home
title: Dev tools, env vars, and tasks in one CLI
description: Install a project's tools, set its environment variables, and run its tasks from one mise.toml that works in your shell, editor, and CI.
socialDescription: Install tools, set environment variables, and run tasks from one mise.toml.

# The custom HomeHero renders the hero. These values supply the llms.txt header.
hero:
  name: mise-en-place
  tagline: Dev tools, env vars, and tasks in one CLI
---

<script setup>
import ProjectSwitchDiagram from "./.vitepress/theme/ProjectSwitchDiagram.vue";
import { data as showreel } from "./.vitepress/showreel.data";
</script>

<section class="landing-page" aria-label="mise overview">
  <div class="landing-section landing-stations">
    <h2>What mise manages</h2>
    <div class="stations-grid">
      <a class="station pillar-tools" href="/dev-tools/">
        <p class="station-cmd">$ mise use node@24</p>
        <h3>Dev tools</h3>
        <p>
          Install any of 1,000+ tools, choose a version for each project, and
          switch automatically as you move between directories.
        </p>
        <span class="card-link">Read the dev tools guide</span>
      </a>
      <a class="station pillar-env" href="/environments/">
        <p class="station-cmd">$ mise env</p>
        <h3>Environments</h3>
        <p>
          Load a project's environment variables from <code>mise.toml</code>,
          .env files, and secrets when you enter its directory.
        </p>
        <span class="card-link">Read the environments guide</span>
      </a>
      <a class="station pillar-tasks" href="/tasks/">
        <p class="station-cmd">$ mise run test</p>
        <h3>Tasks</h3>
        <p>
          Define build, test, lint, and deploy commands next to the tools and
          environment variables they need, with dependencies and parallel runs.
        </p>
        <span class="card-link">Read the tasks guide</span>
      </a>
      <a class="station pillar-boot" href="/bootstrap.html">
        <p class="station-cmd">$ mise bootstrap</p>
        <h3>Bootstrap</h3>
        <p>
          Set up a whole machine: system packages, dotfiles, repositories,
          services, macOS defaults, and tools.
        </p>
        <span class="card-link">Read the bootstrap guide</span>
      </a>
    </div>
    <p class="landing-note version-files-note">Already have <code>.tool-versions</code>, <code>.nvmrc</code>, or <code>.python-version</code> files? mise reads <code>.tool-versions</code> as it is and <a href="/dev-tools/versions.html#idiomatic-version-files">reads the others once you enable them</a>.</p>
  </div>

  <div class="landing-section landing-switch">
    <div class="landing-switch-grid">
      <div>
        <h2>Switch between project environments</h2>
        <p class="landing-lede">
          Activate mise in your shell once. From then on, entering a project
          puts its installed tool versions on your <code>PATH</code> and loads
          its environment variables. Leave the project and mise restores the
          environment for your new directory.
        </p>
        <ul class="landing-checklist">
          <li>Shell activation for bash, zsh, fish, Nushell, PowerShell, and <a href="/shell-setup.html">more</a></li>
          <li>Shims for editors and scripts that never read your shell startup file</li>
          <li><code>mise exec</code> for scripts and containers, and a <a href="/continuous-integration.html">GitHub Action</a> for CI</li>
        </ul>
      </div>
      <ProjectSwitchDiagram />
    </div>
  </div>

  <div class="landing-section landing-machine">
    <div class="landing-machine-grid">
      <figure class="bootstrap-diagram" aria-label="mise bootstrap applies one configuration to packages, repositories, dotfiles, and services">
        <div class="bootstrap-source">
          <span class="bootstrap-label">Declare your setup</span>
          <strong>mise.toml</strong>
          <code>[bootstrap.packages]<br>[bootstrap.repos]<br>[dotfiles]<br>[bootstrap.services]</code>
        </div>
        <div class="bootstrap-connector"><span aria-hidden="true">↓</span> <code>mise bootstrap</code></div>
        <div class="bootstrap-resources">
          <div><strong>Packages</strong><span>brew · apt · scoop · winget</span></div>
          <div><strong>Repositories</strong><span>Project checkouts</span></div>
          <div><strong>Dotfiles</strong><span>Track · link · template</span></div>
          <div><strong>Services</strong><span>Background processes</span></div>
        </div>
        <figcaption><code>mise bootstrap --dry-run</code> shows what will change before you apply it.</figcaption>
      </figure>
      <div>
        <h2>Set up a machine with mise bootstrap</h2>
        <p class="landing-lede">
          Declare the packages, repositories, dotfiles, and services a machine
          needs, then apply them with <code>mise bootstrap</code>. Run
          <code>mise bootstrap --dry-run</code> first to see what will change. This
          setup belongs to the machine, not to a project, so it usually lives
          in your global config or in a repository of your own.
        </p>
        <ul class="landing-checklist">
          <li><a href="/bootstrap/packages/">Packages</a> from apt, dnf, pacman, AUR, zypper, apk, nix, flatpak, Homebrew formulae and casks, direct macOS app downloads, mas, scoop, winget, and package manager plugins</li>
          <li>Dotfiles tracked in place with history you can roll back and sync between machines, or applied as symlinks, copies, templates, and line or block edits</li>
          <li>Remote hosts over SSH with <code>mise bootstrap remote</code></li>
        </ul>
        <p class="landing-note">Set up a new machine from a repository that holds your <code>mise.toml</code>:</p>
        <div class="landing-inline-cmd"><code>mise bootstrap --from git@github.com:you/dotfiles.git</code></div>
        <p class="landing-note"><a href="/bootstrap.html">Read the bootstrap guide</a> or the <a href="/dotfiles.html">dotfiles guide</a>.</p>
      </div>
    </div>
  </div>

  <div class="landing-pantry" aria-label="Supported tools">
    <div class="landing-pantry-inner">
      <div class="pantry-head">
        <p class="landing-kicker"><span>—</span> Tool registry</p>
        <p class="pantry-stat">1,000+<small>tools in the registry, from node to terraform</small></p>
      </div>
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
        <a href="https://mise-versions.jdx.dev/tools/erlang">erlang</a>
        <a href="https://mise-versions.jdx.dev/tools/dotnet">dotnet</a>
        <a href="https://mise-versions.jdx.dev/tools/pnpm">pnpm</a>
        <a href="https://mise-versions.jdx.dev/tools/uv">uv</a>
        <a href="https://mise-versions.jdx.dev/tools/awscli">awscli</a>
        <a href="https://mise-versions.jdx.dev/tools/gh">gh</a>
        <a href="https://mise-versions.jdx.dev/tools/jq">jq</a>
        <a href="https://mise-versions.jdx.dev/tools/ripgrep">ripgrep</a>
        <a class="more" href="/registry.html">browse the registry</a>
      </div>
      <p class="pantry-backends">
        Sourced from
        <a href="/dev-tools/backends/packslip.html">packslip</a>,
        <a href="/dev-tools/backends/aqua.html">aqua</a>,
        <a href="/dev-tools/backends/github.html">GitHub releases</a>,
        <a href="/dev-tools/backends/cargo.html">cargo</a>,
        <a href="/dev-tools/backends/npm.html">npm</a>,
        <a href="/dev-tools/backends/pypi.html">pypi</a>,
        <a href="/dev-tools/backends/go.html">go</a>,
        <a href="/dev-tools/backends/gem.html">gem</a>,
        <a href="/dev-tools/backends/http.html">http</a>,
        <a href="/dev-tools/backends/vfox.html">vfox</a>, and
        <a href="/dev-tools/backends/">more</a>.
      </p>
    </div>
  </div>

  <div class="landing-section landing-recipe">
    <h2>Run your first project task</h2>
    <ol class="recipe">
      <li class="recipe-row">
        <div class="recipe-text">
          <span class="recipe-num">Step 1</span>
          <h3>Install mise</h3>
          <p>On macOS or Linux, run the installer. It puts mise in <code>~/.local/bin</code>; if that is not on your <code>PATH</code>, type <code>~/.local/bin/mise</code> in the next two steps. On Windows, run <code>winget install jdx.mise</code>. See <a href="/installing-mise.html">Installing mise</a> for other methods.</p>
        </div>
        <div class="recipe-code terminal-lines">
          <div><span class="prompt">$</span> curl -fsSL https://mise.run | sh</div>
          <div><span class="prompt">$</span> ~/.local/bin/mise --version</div>
        </div>
      </li>
      <li class="recipe-row">
        <div class="recipe-text">
          <span class="recipe-num">Step 2</span>
          <h3>Try a tool</h3>
          <p><code>mise exec</code> installs Node.js 24 if needed and runs one command with it. Your shell and config files stay as they were.</p>
        </div>
        <div class="recipe-code terminal-lines">
          <div><span class="prompt">$</span> mise exec node@24 -- node --version</div>
          <div>v24.x.x</div>
        </div>
      </li>
      <li class="recipe-row">
        <div class="recipe-text">
          <span class="recipe-num">Step 3</span>
          <h3>Set up a project</h3>
          <p>These commands write <code>[tools]</code>, <code>[env]</code>, and <code>[tasks.hello]</code> to the project's <code>mise.toml</code>. The task runs with the project's Node.js and <code>NODE_ENV</code>. Commit <code>mise.toml</code> so teammates and CI get the same setup.</p>
        </div>
        <div class="recipe-code terminal-lines">
          <div><span class="prompt">$</span> mkdir my-project &amp;&amp; cd my-project</div>
          <div><span class="prompt">$</span> mise use node@24</div>
          <div><span class="dim">mise ~/my-project/mise.toml tools: node@24.x.x</span></div>
          <div><span class="prompt">$</span> mise set NODE_ENV=development</div>
          <div><span class="prompt">$</span> mise tasks add hello -- node -p process.env.NODE_ENV</div>
          <div><span class="prompt">$</span> mise run hello</div>
          <div><span class="dim">[hello] $ node -p process.env.NODE_ENV</span></div>
          <div>development</div>
        </div>
      </li>
      <li class="recipe-row">
        <div class="recipe-text">
          <span class="recipe-num">Step 4 (optional)</span>
          <h3>Activate mise in your shell</h3>
          <p>Activation puts the project's tools and environment variables in your shell when you enter the project, so <code>node</code> works without <code>mise exec</code>. Run the commands for your shell once, then open a new shell. For PowerShell, Nushell, and others, see <a href="/shell-setup.html">Shell setup</a>.</p>
        </div>
        <div class="recipe-code terminal-lines">
          <div><span class="dim"># bash</span></div>
          <div><span class="prompt">$</span> echo 'eval "$(~/.local/bin/mise activate bash)"' &gt;&gt; ~/.bashrc</div>
          <div><span class="dim"># zsh</span></div>
          <div><span class="prompt">$</span> echo 'eval "$(~/.local/bin/mise activate zsh)"' &gt;&gt; "${ZDOTDIR:-$HOME}/.zshrc"</div>
          <div><span class="dim"># fish</span></div>
          <div><span class="prompt">$</span> mkdir -p ~/.config/fish</div>
          <div><span class="prompt">$</span> echo '~/.local/bin/mise activate fish | source' &gt;&gt; ~/.config/fish/config.fish</div>
        </div>
      </li>
    </ol>
  </div>

  <div class="landing-cta">
    <p class="landing-kicker"><span>—</span> Install mise</p>
    <h2><em>Allez.</em> Prep your station.</h2>
    <div class="landing-mini-install"><code>curl -fsSL https://mise.run | sh</code></div>
    <div class="landing-links">
      <a href="/getting-started.html">Getting started</a>
      <a :href="showreel ? '/#showreel' : '/demo.html'">Watch the demo</a>
      <a href="https://github.com/jdx/mise">GitHub</a>
    </div>
  </div>

  <a class="landing-special landing-related" href="https://mr-boxington.jdx.dev/" aria-label="Try Mr Boxington">
    <div>
      <p class="landing-kicker"><span>—</span> Related project</p>
      <h2>Share Cargo builds with Mr Boxington</h2>
      <p>Give every Cargo checkout one shared, self-pruning compilation cache, locally and in CI.</p>
    </div>
    <span class="card-link">mr-boxington.jdx.dev</span>
  </a>
</section>
