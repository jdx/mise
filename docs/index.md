---
layout: home
title: Home

hero: {}
---

<section class="landing-page" aria-label="mise overview">
  <div class="hero-card" aria-label="One mise.toml drives tools, env vars, and tasks">
    <div class="card-file">
      <div class="card-bar"><strong>mise.toml</strong><span>checked into your repo</span></div>
      <div class="card-toml" aria-label="Example mise.toml">
        <div class="row row-tools"><span class="tk-section">[tools]</span></div>
        <div class="row row-tools"><span class="tk-key">node</span><span class="tk-op"> = </span><span class="tk-str">"24"</span></div>
        <div class="row row-tools"><span class="tk-key">python</span><span class="tk-op"> = </span><span class="tk-str">"3.13"</span></div>
        <div class="row row-tools"><span class="tk-key">terraform</span><span class="tk-op"> = </span><span class="tk-str">"1.13"</span></div>
        <div class="row row-blank"></div>
        <div class="row row-env"><span class="tk-section">[env]</span></div>
        <div class="row row-env"><span class="tk-key">DATABASE_URL</span><span class="tk-op"> = </span><span class="tk-str">"postgres://localhost/orders"</span></div>
        <div class="row row-env"><span class="tk-key">_.file</span><span class="tk-op"> = </span><span class="tk-str">".env.local"</span></div>
        <div class="row row-blank"></div>
        <div class="row row-tasks"><span class="tk-section">[tasks.test]</span></div>
        <div class="row row-tasks"><span class="tk-key">depends</span><span class="tk-op"> = [</span><span class="tk-str">"build"</span><span class="tk-op">]</span></div>
        <div class="row row-tasks"><span class="tk-key">run</span><span class="tk-op"> = </span><span class="tk-str">"pytest"</span></div>
        <div class="row row-blank"></div>
        <div class="row row-boot"><span class="tk-section">[bootstrap.packages]</span></div>
        <div class="row row-boot"><span class="tk-key">"brew:postgresql@17"</span><span class="tk-op"> = </span><span class="tk-str">"latest"</span></div>
        <div class="row row-boot"><span class="tk-key">"apt:build-essential"</span><span class="tk-op"> = </span><span class="tk-str">"latest"</span></div>
        <div class="row row-blank"></div>
        <div class="row row-boot"><span class="tk-section">[dotfiles]</span></div>
        <div class="row row-boot"><span class="tk-key">"~/.config/mise/config.toml"</span><span class="tk-op"> = { </span><span class="tk-key">source</span><span class="tk-op"> = </span><span class="tk-str">"config.toml"</span><span class="tk-op">, </span><span class="tk-key">mode</span><span class="tk-op"> = </span><span class="tk-str">"symlink"</span><span class="tk-op"> }</span></div>
      </div>
    </div>
    <div class="card-outputs">
      <div class="card-output pillar-tools">
        <div class="card-output-head"><span class="pillar-dot"></span><span>Dev tools</span><code>$ mise install</code></div>
        <div class="terminal-lines">
          <div><span class="dim">mise</span> node@24.18.0 <span class="ok">✓ installed</span></div>
          <div><span class="dim">mise</span> python@3.13.14 <span class="ok">✓ installed</span></div>
          <div><span class="dim">mise</span> terraform@1.13.2 <span class="ok">✓ installed</span></div>
        </div>
      </div>
      <div class="card-output pillar-env">
        <div class="card-output-head"><span class="pillar-dot"></span><span>Environments</span><code>$ mise env</code></div>
        <div class="terminal-lines">
          <div>export DATABASE_URL=postgres://localhost/orders</div>
          <div>export STRIPE_KEY=sk_test_51H… <span class="dim"># from .env.local</span></div>
        </div>
      </div>
      <div class="card-output pillar-tasks">
        <div class="card-output-head"><span class="pillar-dot"></span><span>Tasks</span><code>$ mise run test</code></div>
        <div class="terminal-lines">
          <div><span class="key">[build]</span> $ npm run build</div>
          <div><span class="key">[test]</span> $ pytest</div>
          <div><span class="ok">42 passed</span> in 1.02s</div>
        </div>
      </div>
      <div class="card-output pillar-boot">
        <div class="card-output-head"><span class="pillar-dot"></span><span>Bootstrap</span><code>$ mise bootstrap</code></div>
        <div class="terminal-lines">
          <div><span class="dim">mise bootstrap:</span> system packages</div>
          <div>brew:postgresql@17 <span class="ok">✓ installed</span></div>
          <div><span class="dim">mise bootstrap:</span> dotfiles</div>
          <div>~/.zshrc <span class="ok">✓ symlinked</span></div>
        </div>
      </div>
    </div>
  </div>

  <div class="landing-section landing-why">
    <p class="landing-kicker"><span>01</span> Why mise</p>
    <div class="landing-why-grid">
      <div>
        <h2>Clear the counter.</h2>
        <p class="landing-lede">
          Most projects carry a drawer of version managers, dotfiles, and a
          README full of setup steps. Every new machine adds a Brewfile, a
          dotfiles manager, and a playbook. mise replaces them with one file
          that's checked in, so a fresh laptop is <code>git clone</code> and
          <code>mise bootstrap</code>.
        </p>
        <p class="landing-note">
          Coming from asdf? mise reads <code>.tool-versions</code> as-is. Files
          like <code>.nvmrc</code> work too, once you
          <a href="/configuration.html#idiomatic-version-files">enable them</a>.
        </p>
      </div>
      <div class="landing-ledger" aria-label="Before and after mise">
        <div class="ledger-col ledger-before">
          <p class="ledger-head">Before</p>
          <ul>
            <li>nvm, pyenv, rbenv <span>.nvmrc, .python-version</span></li>
            <li>direnv <span>.envrc</span></li>
            <li>make, just <span>Makefile</span></li>
            <li>Homebrew <span>Brewfile</span></li>
            <li>chezmoi <span>dotfiles repo</span></li>
            <li>Ansible <span>playbook.yml</span></li>
            <li>README <span>"Setup", 14 steps</span></li>
          </ul>
        </div>
        <div class="ledger-col ledger-after">
          <p class="ledger-head">After</p>
          <ul>
            <li><strong>mise</strong> <span>mise.toml</span></li>
          </ul>
          <div class="ledger-pillars">
            <span class="pillar-tools">tools</span>
            <span class="pillar-env">env</span>
            <span class="pillar-tasks">tasks</span>
            <span class="pillar-boot">bootstrap</span>
          </div>
        </div>
      </div>
    </div>
  </div>

  <div class="landing-section landing-stations">
    <p class="landing-kicker"><span>02</span> What it does</p>
    <h2>Four stations, one line.</h2>
    <div class="stations-grid">
      <a class="station pillar-tools" href="/dev-tools/">
        <p class="station-cmd">$ mise use node@24</p>
        <h3>Dev tools</h3>
        <p>
          Install any of 1000+ tools, pin versions per project, and switch
          automatically as you move between directories.
        </p>
        <span class="card-link">Dev tools</span>
      </a>
      <a class="station pillar-env" href="/environments/">
        <p class="station-cmd">$ mise env</p>
        <h3>Environments</h3>
        <p>
          Per-project env vars from <code>mise.toml</code>, .env files,
          secrets, and shell commands. Set when you enter, gone when you leave.
        </p>
        <span class="card-link">Environments</span>
      </a>
      <a class="station pillar-tasks" href="/tasks/">
        <p class="station-cmd">$ mise run test</p>
        <h3>Tasks</h3>
        <p>
          Build, test, lint, and deploy commands defined next to the tools and
          env they need, with dependencies and parallel runs.
        </p>
        <span class="card-link">Tasks</span>
      </a>
      <a class="station pillar-boot" href="/bootstrap">
        <p class="station-cmd">$ mise bootstrap</p>
        <h3>Bootstrap</h3>
        <p>
          Set up a whole machine from the same config: OS packages, dotfiles,
          repos, services, macOS defaults, then your tools.
        </p>
        <span class="card-link">Bootstrap</span>
      </a>
    </div>
  </div>

  <div class="landing-section landing-switch">
    <p class="landing-kicker"><span>03</span> Day to day</p>
    <div class="landing-switch-grid">
      <div>
        <h2>Change directory. <em>Everything follows.</em></h2>
        <p class="landing-lede">
          Activate mise in your shell once. From then on, entering a project
          puts its tool versions on your <code>PATH</code> and its env vars in
          your shell. Leaving takes them away again.
        </p>
        <ul class="landing-checklist">
          <li>Shell hooks for bash, zsh, fish, nushell, PowerShell, and more</li>
          <li>Shims for editors and scripts that never source your shell rc</li>
          <li><code>mise exec</code> and <code>mise-action</code> for Docker and CI</li>
        </ul>
      </div>
      <div class="landing-terminal" aria-label="Switching between two projects">
        <div class="terminal-bar"><span>~/work</span><span>zsh</span></div>
        <div class="terminal-lines">
          <div><span class="prompt">$</span> cd api</div>
          <div><span class="prompt">$</span> node --version</div>
          <div>v22.12.0</div>
          <div><span class="prompt">$</span> echo $DATABASE_URL</div>
          <div>postgres://localhost/api</div>
          <div>&nbsp;</div>
          <div><span class="prompt">$</span> cd ../dashboard</div>
          <div><span class="prompt">$</span> node --version</div>
          <div>v24.18.0</div>
          <div><span class="prompt">$</span> mise ls --current</div>
          <div><span class="dim">Tool&nbsp;&nbsp;&nbsp;Version&nbsp;&nbsp;&nbsp;Source</span></div>
          <div>bun&nbsp;&nbsp;&nbsp;&nbsp;1.2.20&nbsp;&nbsp;&nbsp;&nbsp;~/work/dashboard/mise.toml</div>
          <div>node&nbsp;&nbsp;&nbsp;24.18.0&nbsp;&nbsp;&nbsp;~/work/dashboard/mise.toml</div>
        </div>
      </div>
    </div>
  </div>

  <div class="landing-section landing-machine">
    <p class="landing-kicker"><span>04</span> New machine</p>
    <div class="landing-machine-grid">
      <div class="landing-config-card" aria-label="Example bootstrap config">
        <div class="card-bar"><strong>mise.toml</strong><span>~/.config/mise</span></div>
        <div class="card-toml">
          <div class="row row-boot"><span class="tk-section">[bootstrap.packages]</span></div>
          <div class="row row-boot"><span class="tk-key">"brew:postgresql@17"</span><span class="tk-op"> = </span><span class="tk-str">"latest"</span></div>
          <div class="row row-boot"><span class="tk-key">"apt:build-essential"</span><span class="tk-op"> = </span><span class="tk-str">"latest"</span></div>
          <div class="row row-blank"></div>
          <div class="row row-boot"><span class="tk-section">[bootstrap.repos]</span></div>
          <div class="row row-boot"><span class="tk-key">"~/src/notes"</span><span class="tk-op"> = { </span><span class="tk-key">url</span><span class="tk-op"> = </span><span class="tk-str">"git@github.com:me/notes.git"</span><span class="tk-op"> }</span></div>
          <div class="row row-blank"></div>
          <div class="row row-boot"><span class="tk-section">[dotfiles]</span></div>
          <div class="row row-boot"><span class="tk-key">"~/.config/mise/config.toml"</span><span class="tk-op"> = { </span><span class="tk-key">source</span><span class="tk-op"> = </span><span class="tk-str">"config.toml"</span><span class="tk-op">, </span><span class="tk-key">mode</span><span class="tk-op"> = </span><span class="tk-str">"symlink"</span><span class="tk-op"> }</span></div>
          <div class="row row-boot"><span class="tk-key">"~/.gitconfig"</span><span class="tk-op"> = { </span><span class="tk-key">source</span><span class="tk-op"> = </span><span class="tk-str">"dotfiles/gitconfig"</span><span class="tk-op">, </span><span class="tk-key">mode</span><span class="tk-op"> = </span><span class="tk-str">"template"</span><span class="tk-op"> }</span></div>
          <div class="row row-boot"><span class="tk-key">"~/.config/nvim"</span><span class="tk-op"> = { </span><span class="tk-key">source</span><span class="tk-op"> = </span><span class="tk-str">"dotfiles/nvim"</span><span class="tk-op">, </span><span class="tk-key">mode</span><span class="tk-op"> = </span><span class="tk-str">"symlink"</span><span class="tk-op"> }</span></div>
          <div class="row row-blank"></div>
          <div class="row row-boot"><span class="tk-section">[bootstrap.macos.dock]</span></div>
          <div class="row row-boot"><span class="tk-key">autohide</span><span class="tk-op"> = </span><span class="tk-str">true</span></div>
          <div class="row row-blank"></div>
          <div class="row row-boot"><span class="tk-section">[bootstrap.mise_shell_activate]</span></div>
          <div class="row row-boot"><span class="tk-key">zshrc</span><span class="tk-op"> = </span><span class="tk-str">"activate"</span></div>
        </div>
      </div>
      <div>
        <h2>One config for the <em>whole machine.</em></h2>
        <p class="landing-lede">
          <code>mise bootstrap</code> sets up a new computer from the same
          file: OS packages, git repos, dotfiles, shell activation, macOS
          defaults, and services, then your tools. Run it again and mise skips
          anything that's already set up. mise has its own Homebrew
          implementation, so it installs formulae and casks without requiring
          Homebrew. It replaces what you'd otherwise assemble from a Brewfile,
          chezmoi, and an Ansible playbook.
        </p>
        <ul class="landing-checklist">
          <li>Packages through brew, apt, dnf, pacman, apk, and mas</li>
          <li>Dotfiles as symlinks, copies, or templates, plus single-line edits</li>
          <li>Remote hosts over SSH with <code>mise bootstrap remote</code></li>
        </ul>
        <div class="landing-inline-cmd"><code>mise bootstrap --from git@github.com:you/dotfiles.git</code></div>
        <p class="landing-note"><a href="/bootstrap">Read the bootstrap guide</a></p>
      </div>
    </div>
  </div>

  <div class="landing-pantry" aria-label="Supported tools">
    <div class="landing-pantry-inner">
      <div class="pantry-head">
        <p class="landing-kicker"><span>—</span> The pantry</p>
        <p class="pantry-stat">1000+<small>tools in the registry, from node to terraform</small></p>
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
        <a class="more" href="/registry">browse the registry</a>
      </div>
      <p class="pantry-backends">
        Sourced from
        <a href="/dev-tools/backends/aqua">aqua</a>,
        <a href="/dev-tools/backends/github">GitHub releases</a>,
        <a href="/dev-tools/backends/cargo">cargo</a>,
        <a href="/dev-tools/backends/npm">npm</a>,
        <a href="/dev-tools/backends/pipx">pipx</a>,
        <a href="/dev-tools/backends/go">go</a>,
        <a href="/dev-tools/backends/gem">gem</a>,
        <a href="/dev-tools/backends/http">http</a>,
        <a href="/dev-tools/backends/asdf">asdf</a>,
        <a href="/dev-tools/backends/vfox">vfox</a>, and
        <a href="/dev-tools/backends/">more</a>.
      </p>
    </div>
  </div>

  <a class="landing-special" href="https://mr-boxington.jdx.dev/" aria-label="Try Mr Boxington">
    <div>
      <p class="landing-kicker"><span>—</span> Chef's special</p>
      <h2>Mr Boxington: fix your target/.</h2>
      <p>Give every Cargo checkout one shared, self-pruning compilation cache, locally and in CI.</p>
    </div>
    <span class="card-link">mr-boxington.jdx.dev</span>
  </a>

  <div class="landing-section landing-recipe">
    <p class="landing-kicker"><span>05</span> Quickstart</p>
    <h2>Set up in four steps.</h2>
    <ol class="recipe">
      <li class="recipe-row">
        <div class="recipe-text">
          <span class="recipe-num">Step 1</span>
          <h3>Install mise</h3>
          <p>One command, one static binary. <a href="/installing-mise">Homebrew, apt, cargo, and more</a> also work.</p>
        </div>
        <div class="recipe-code terminal-lines">
          <div><span class="prompt">$</span> curl https://mise.run | sh</div>
          <div><span class="prompt">$</span> mise --version</div>
          <div>2026.9.1 linux-x64</div>
        </div>
      </li>
      <li class="recipe-row">
        <div class="recipe-text">
          <span class="recipe-num">Step 2</span>
          <h3>Hook into your shell</h3>
          <p>Optional, but this is what makes tools and env vars switch as you <code>cd</code>. <a href="/installing-mise#shells">Other shells</a> are one line too.</p>
        </div>
        <div class="recipe-code terminal-lines">
          <div><span class="prompt">$</span> echo 'eval "$(mise activate zsh)"' &gt;&gt; ~/.zshrc</div>
          <div><span class="dim"># bash: ~/.bashrc · fish: config.fish · pwsh: $PROFILE</span></div>
        </div>
      </li>
      <li class="recipe-row">
        <div class="recipe-text">
          <span class="recipe-num">Step 3</span>
          <h3>Add tools</h3>
          <p><code>mise use</code> installs the tool and pins it in <code>mise.toml</code> in one go. Commit the file.</p>
        </div>
        <div class="recipe-code terminal-lines">
          <div><span class="prompt">$</span> mise use node@24 python@3.13</div>
          <div><span class="dim">mise</span> node@24.18.0 <span class="ok">✓ installed</span></div>
          <div><span class="dim">mise</span> python@3.13.14 <span class="ok">✓ installed</span></div>
          <div><span class="dim">mise</span> ./mise.toml tools: node@24.18.0, python@3.13.14</div>
        </div>
      </li>
      <li class="recipe-row">
        <div class="recipe-text">
          <span class="recipe-num">Step 4</span>
          <h3>Add env vars and tasks</h3>
          <p>They live in the same file, next to the tools they depend on. Teammates run <code>mise install</code>; a new laptop runs <code>mise bootstrap</code>.</p>
        </div>
        <div class="recipe-code terminal-lines">
          <div><span class="prompt">$</span> mise set DATABASE_URL=postgres://localhost/orders</div>
          <div><span class="prompt">$</span> mise tasks add test -- pytest</div>
          <div><span class="prompt">$</span> mise run test</div>
          <div><span class="key">[test]</span> $ pytest</div>
          <div><span class="ok">42 passed</span> in 1.02s</div>
        </div>
      </li>
    </ol>
  </div>

  <div class="landing-cta">
    <p class="landing-kicker"><span>—</span> Ready when you are</p>
    <h2><em>Allez.</em> Prep your station.</h2>
    <div class="landing-mini-install"><code>curl https://mise.run | sh</code></div>
    <div class="landing-links">
      <a href="/getting-started">Getting started</a>
      <a href="/demo">Run the demo</a>
      <a href="https://github.com/jdx/mise">GitHub</a>
    </div>
  </div>
</section>
