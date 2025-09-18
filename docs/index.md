---
# https://vitepress.dev/reference/default-theme-home-page
layout: home
title: Home

hero:
  name: mise-en-place
  tagline: |
    The front-end to your dev env
    <span class="formerly">Pronounced "MEEZ ahn plahs"</span>
  actions:
    - theme: brand
      text: Getting Started
      link: /getting-started
    - theme: alt
      text: Demo
      link: /demo
    - theme: alt
      text: About
      link: /about

---

<div class="quick-setup">
  <div class="setup-header">
    <span class="setup-icon">🚀</span>
    <h3>Get Started in Seconds</h3>
  </div>
  <div class="setup-steps">
    <div class="setup-step">
      <span class="step-number">1</span>
      <div class="step-content">
        <p class="step-label">Install mise</p>
        <div class="code-box">
          <code>curl https://mise.run | sh</code>
          <button class="copy-btn" onclick="navigator.clipboard.writeText('curl https://mise.run | sh')">
            <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
              <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
            </svg>
          </button>
        </div>
      </div>
    </div>
    <div class="setup-step">
      <span class="step-number">2</span>
      <div class="step-content">
        <p class="step-label">Run any tool instantly</p>
        <div class="code-box">
          <code>mise x node -- node --version</code>
          <button class="copy-btn" onclick="navigator.clipboard.writeText('mise x node -- node --version')">
            <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <rect x="9" y="9" width="13" height="13" rx="2" ry="2"></rect>
              <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
            </svg>
          </button>
        </div>
        <p class="step-output">→ v22.11.0</p>
      </div>
    </div>
  </div>
</div>

---

features:
  - title: Dev Tools
    link: /dev-tools/
    icon: 🛠️
    details: mise is a polyglot tool version manager. It replaces tools like asdf, nvm, pyenv, rbenv, etc.
  - title: Environments
    details: mise allows you to switch sets of env vars in different project directories. It can replace direnv.
    icon: ⚙
    link: /environments/
  - title: Tasks
    link: /tasks/
    details: mise is a task runner that can replace make, or npm scripts.
    icon: ⚡
---

<style>
.formerly {
    font-size: 0.7em;
    color: #666;
}

.quick-setup {
    max-width: 800px;
    margin: 3rem auto 4rem;
    padding: 2rem;
    background: var(--vp-c-bg-soft);
    border-radius: 16px;
    border: 2px solid var(--vp-c-divider);
    position: relative;
    overflow: hidden;
}

.quick-setup::before {
    content: '';
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 3px;
    background: linear-gradient(90deg, #00d9ff 0%, #52e892 50%, #ff9100 100%);
    animation: shimmer 3s ease-in-out infinite;
}

.setup-header {
    text-align: center;
    margin-bottom: 2rem;
}

.setup-icon {
    font-size: 2.5rem;
    display: block;
    margin-bottom: 0.5rem;
    filter: drop-shadow(0 4px 8px rgba(0, 217, 255, 0.3));
}

.setup-header h3 {
    font-size: 1.5rem;
    font-weight: 700;
    color: var(--vp-c-text-1);
    margin: 0;
    font-family: var(--vp-font-family-heading);
    letter-spacing: -0.02em;
}

.setup-steps {
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
}

.setup-step {
    display: flex;
    gap: 1rem;
    align-items: flex-start;
}

.step-number {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: linear-gradient(135deg, #00d9ff, #52e892);
    color: white;
    font-weight: 700;
    font-size: 1rem;
    flex-shrink: 0;
}

.step-content {
    flex: 1;
}

.step-label {
    font-weight: 600;
    color: var(--vp-c-text-1);
    margin: 0 0 0.5rem 0;
    font-size: 0.9rem;
    text-transform: uppercase;
    letter-spacing: 0.02em;
}

.code-box {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    background: var(--vp-c-code-block-bg);
    border: 1px solid var(--vp-c-divider);
    border-radius: 8px;
    padding: 0.75rem 1rem;
    position: relative;
    transition: all 0.3s ease;
}

.code-box:hover {
    border-color: var(--vp-c-brand-1);
    box-shadow: 0 2px 8px rgba(0, 217, 255, 0.15);
}

.code-box code {
    flex: 1;
    font-family: var(--vp-font-family-mono);
    font-size: 0.9rem;
    color: var(--vp-c-text-1);
    background: none;
    padding: 0;
}

.copy-btn {
    background: transparent;
    border: none;
    color: var(--vp-c-text-3);
    cursor: pointer;
    padding: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.2s ease;
}

.copy-btn:hover {
    color: var(--vp-c-brand-1);
    transform: scale(1.1);
}

.copy-btn:active {
    transform: scale(0.95);
}

.step-output {
    margin: 0.5rem 0 0 0;
    color: var(--vp-c-success-1);
    font-family: var(--vp-font-family-mono);
    font-size: 0.85rem;
    font-weight: 600;
}

/* Dark mode adjustments */
.dark .quick-setup {
    background: rgba(255, 255, 255, 0.03);
    border-color: rgba(255, 255, 255, 0.1);
}

.dark .code-box {
    background: rgba(0, 0, 0, 0.3);
}

/* Mobile responsiveness */
@media (max-width: 768px) {
    .quick-setup {
        margin: 2rem 1rem 3rem;
        padding: 1.5rem;
    }

    .setup-icon {
        font-size: 2rem;
    }

    .setup-header h3 {
        font-size: 1.25rem;
    }

    .code-box code {
        font-size: 0.8rem;
    }
}
</style>
