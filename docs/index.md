---
layout: home
title: Home

# The custom HomeHero renders the hero. These values supply the llms.txt header.
hero:
  name: mise-en-place
  tagline: Dev tools, env vars, and tasks in one CLI
---

<section class="landing-page" aria-label="mise overview">
  <div class="landing-section landing-stations">
    <p class="landing-kicker"><span>01</span> The essentials</p>
    <h2>Start with tools. Add what you need.</h2>
    <div class="stations-grid">
      <a class="station pillar-tools" href="/dev-tools/">
        <p class="station-cmd">$ mise use node@24</p>
        <h3>Dev tools</h3>
        <p>
          Install hundreds of tools, select versions per project, and switch
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
          Apply the machine setup you declare: OS packages, dotfiles,
          repos, services, macOS defaults, and development tools.
        </p>
        <span class="card-link">Bootstrap</span>
      </a>
    </div>
    <p class="landing-note migration-note">Coming from asdf? Your <code>.tool-versions</code> already works. <a href="/configuration.html#idiomatic-version-files">Enable files like .nvmrc, too</a>.</p>
  </div>

  <div class="landing-section landing-switch">
    <p class="landing-kicker"><span>02</span> Day to day</p>
    <div class="landing-switch-grid">
      <div>
        <h2>Change directory. <em>Everything follows.</em></h2>
        <p class="landing-lede">
          Activate mise in your shell once. From then on, entering a project
          puts its installed tool versions on your <code>PATH</code> and loads
          its environment variables. Leave the project and mise restores the
          environment for your new directory.
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
    <p class="landing-kicker"><span>03</span> New machine</p>
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
          Declare the packages, repositories, dotfiles, and services your
          machine needs, then apply them with <code>mise bootstrap</code>.
          Preview declarative resource changes with
          <code>mise bootstrap plan</code>. Add shell activation and
          platform-specific settings to keep machine setup alongside your tools.
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
    <p class="landing-kicker"><span>04</span> Quickstart</p>
    <h2>Run your first project task.</h2>
    <ol class="recipe">
      <li class="recipe-row">
        <div class="recipe-text">
          <span class="recipe-num">Step 1</span>
          <h3>Install mise</h3>
          <p>On macOS or Linux, use the installer below. See <a href="/installing-mise">Windows and package manager instructions</a> for other options.</p>
        </div>
        <div class="recipe-code terminal-lines">
          <div><span class="prompt">$</span> curl https://mise.run | sh</div>
          <div><span class="prompt">$</span> ~/.local/bin/mise --version</div>
        </div>
      </li>
      <li class="recipe-row">
        <div class="recipe-text">
          <span class="recipe-num">Step 2</span>
          <h3>Activate your shell</h3>
          <p>For zsh, add this line and restart your shell. Follow the <a href="/getting-started#activate-mise">activation guide</a> for other shells. You can also skip activation and use <code>~/.local/bin/mise exec</code> or <code>~/.local/bin/mise run</code>.</p>
        </div>
        <div class="recipe-code terminal-lines">
          <div><span class="prompt">$</span> echo 'eval "$(~/.local/bin/mise activate zsh)"' &gt;&gt; ~/.zshrc</div>
          <div><span class="dim"># Restart your shell before continuing.</span></div>
        </div>
      </li>
      <li class="recipe-row">
        <div class="recipe-text">
          <span class="recipe-num">Step 3</span>
          <h3>Add tools</h3>
          <p>From your project directory, <code>mise use</code> installs a tool and saves its version request in <code>mise.toml</code>.</p>
        </div>
        <div class="recipe-code terminal-lines">
          <div><span class="prompt">$</span> mkdir mise-example</div>
          <div><span class="prompt">$</span> cd mise-example</div>
          <div><span class="prompt">$</span> mise use node@24</div>
        </div>
      </li>
      <li class="recipe-row">
        <div class="recipe-text">
          <span class="recipe-num">Step 4</span>
          <h3>Add env vars and tasks</h3>
          <p>Save a task alongside its environment, then run it. Commit <code>mise.toml</code> to share the setup. See <a href="/getting-started#set-up-a-project">the complete example</a> to print both the tool version and environment.</p>
        </div>
        <div class="recipe-code terminal-lines">
          <div><span class="prompt">$</span> mise set NODE_ENV=development</div>
          <div><span class="prompt">$</span> mise tasks add hello -- node -p process.env.NODE_ENV</div>
          <div><span class="prompt">$</span> mise run hello</div>
          <div>development</div>
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
      <a href="/demo">Watch the demo</a>
      <a href="https://github.com/jdx/mise">GitHub</a>
    </div>
  </div>
</section>
