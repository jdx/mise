<template>
  <div class="custom-hero">
    <div class="hero-background">
      <div class="gradient-orb orb-1"></div>
      <div class="gradient-orb orb-2"></div>
      <div class="gradient-orb orb-3"></div>
    </div>
    <div class="hero-content">
      <div class="hero-chef-logo">
        <img
          class="chef-logo chef-logo-light"
          src="/logo-full-light.svg"
          alt="mise"
          width="280"
          height="136"
        />
        <img
          class="chef-logo chef-logo-dark"
          src="/logo-full-dark.svg"
          alt="mise"
          width="280"
          height="136"
        />
      </div>
      <div class="hero-logo">
        <div class="logo-subtitle">en-place</div>
      </div>
      <p class="hero-tagline">
        The front-end to your dev env
        <span class="pronunciation">Pronounced "MEEZ ahn plahs"</span>
      </p>
      <div class="hero-actions">
        <a href="/getting-started" class="action-button primary">
          <span class="button-content">
            <svg
              class="button-icon"
              width="20"
              height="20"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
            >
              <path d="M5 12h14M12 5l7 7-7 7" />
            </svg>
            Getting Started
          </span>
        </a>
        <a href="/demo" class="action-button secondary">
          <span class="button-content">
            <svg
              class="button-icon"
              width="20"
              height="20"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
            >
              <polygon points="5 3 19 12 5 21 5 3" />
            </svg>
            Watch Demo
          </span>
        </a>
      </div>
      <div class="hero-stats">
        <div class="stat">
          <div class="stat-value">50+</div>
          <div class="stat-label">Languages</div>
        </div>
        <div class="stat">
          <div class="stat-value">Fast</div>
          <div class="stat-label">Performance</div>
        </div>
        <div class="stat">
          <div class="stat-value">Simple</div>
          <div class="stat-label">Configuration</div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.custom-hero {
  position: relative;
  padding: 64px 24px 96px;
  overflow: hidden;
  min-height: 600px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.hero-background {
  position: absolute;
  inset: 0;
  pointer-events: none;
  overflow: hidden;
}

.gradient-orb {
  position: absolute;
  border-radius: 50%;
  filter: blur(100px);
  opacity: 0.35;
  animation: float 20s infinite ease-in-out;
}

.orb-1 {
  width: 600px;
  height: 600px;
  background: linear-gradient(135deg, #8B2252, #C75B7A);
  top: -200px;
  left: -100px;
  animation-delay: 0s;
}

.orb-2 {
  width: 500px;
  height: 500px;
  background: linear-gradient(135deg, #C5975B, #D4A76A);
  bottom: -150px;
  right: -150px;
  animation-delay: 5s;
}

.orb-3 {
  width: 400px;
  height: 400px;
  background: linear-gradient(135deg, #8FA86E, #6B7F4E);
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  animation-delay: 10s;
}

@keyframes float {
  0%,
  100% {
    transform: translate(0, 0) scale(1);
  }
  33% {
    transform: translate(30px, -30px) scale(1.1);
  }
  66% {
    transform: translate(-20px, 20px) scale(0.9);
  }
}

.hero-content {
  position: relative;
  z-index: 1;
  text-align: center;
  max-width: 800px;
  margin: 0 auto;
}

.hero-chef-logo {
  margin-bottom: 16px;
  animation: fadeInDown 0.8s ease-out;
}

.chef-logo {
  max-width: 280px;
  height: auto;
}

/* Show/hide based on color mode */
.chef-logo-dark {
  display: none;
}

.dark .chef-logo-light {
  display: none;
}

.dark .chef-logo-dark {
  display: inline;
}

.hero-logo {
  margin-bottom: 24px;
  animation: fadeInDown 0.8s ease-out 0.1s both;
}

.logo-subtitle {
  font-size: clamp(1.5rem, 5vw, 2.5rem);
  font-weight: 300;
  color: var(--vp-c-text-2);
  margin-top: -8px;
  letter-spacing: 0.1em;
  opacity: 0.8;
}

.hero-tagline {
  font-size: clamp(1rem, 3vw, 1.35rem);
  color: var(--vp-c-text-2);
  margin-bottom: 32px;
  line-height: 1.6;
  animation: fadeInUp 0.8s ease-out 0.2s both;
}

.pronunciation {
  display: block;
  font-size: 0.85em;
  color: var(--vp-c-text-3);
  margin-top: 8px;
  font-style: italic;
}

.hero-actions {
  display: flex;
  gap: 16px;
  justify-content: center;
  margin-bottom: 48px;
  flex-wrap: wrap;
  animation: fadeInUp 0.8s ease-out 0.4s both;
}

.action-button {
  display: inline-flex;
  align-items: center;
  padding: 14px 28px;
  border-radius: 12px;
  font-weight: 600;
  text-decoration: none;
  transition: all 0.3s ease;
  font-size: 1rem;
  position: relative;
  overflow: hidden;
}

.action-button::before {
  content: "";
  position: absolute;
  top: 0;
  left: -100%;
  width: 100%;
  height: 100%;
  background: linear-gradient(
    90deg,
    transparent,
    rgba(255, 255, 255, 0.2),
    transparent
  );
  transition: left 0.5s;
}

.action-button:hover::before {
  left: 100%;
}

.button-content {
  display: flex;
  align-items: center;
  gap: 8px;
  position: relative;
  z-index: 1;
}

.button-icon {
  transition: transform 0.3s ease;
}

.action-button:hover .button-icon {
  transform: translateX(4px);
}

.action-button.primary {
  background: linear-gradient(135deg, #8B2252, #722F37);
  color: white;
  box-shadow: 0 4px 20px rgba(139, 34, 82, 0.3);
}

.action-button.primary:hover {
  transform: translateY(-2px);
  box-shadow: 0 8px 30px rgba(139, 34, 82, 0.4);
}

.action-button.secondary {
  background: var(--vp-c-bg-soft);
  color: var(--vp-c-text-1);
  border: 2px solid var(--vp-c-divider);
}

.action-button.secondary:hover {
  background: var(--vp-c-bg-mute);
  border-color: var(--vp-c-brand-1);
  color: var(--vp-c-brand-1);
  transform: translateY(-2px);
}

.hero-stats {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(120px, 1fr));
  gap: 24px;
  animation: fadeInUp 0.8s ease-out 0.6s both;
}

.stat {
  padding: 20px;
  background: var(--vp-c-bg-soft);
  border-radius: 12px;
  border: 1px solid var(--vp-c-divider);
  transition: all 0.3s ease;
}

.stat:hover {
  transform: translateY(-4px);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.1);
  border-color: var(--vp-c-brand-1);
}

.stat-value {
  font-size: 1.75rem;
  font-weight: 700;
  background: linear-gradient(135deg, #8B2252, #C5975B);
  -webkit-background-clip: text;
  background-clip: text;
  -webkit-text-fill-color: transparent;
  margin-bottom: 4px;
}

.stat-label {
  font-size: 0.875rem;
  color: var(--vp-c-text-3);
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

@keyframes fadeInDown {
  from {
    opacity: 0;
    transform: translateY(-20px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

@keyframes fadeInUp {
  from {
    opacity: 0;
    transform: translateY(20px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

@media (max-width: 768px) {
  .custom-hero {
    padding: 48px 16px 64px;
  }

  .chef-logo {
    max-width: 200px;
  }

  .hero-actions {
    flex-direction: column;
    align-items: center;
  }

  .action-button {
    width: 100%;
    max-width: 280px;
    justify-content: center;
  }

  .gradient-orb {
    filter: blur(80px);
  }
}

/* Dark mode adjustments */
.dark .stat {
  background: rgba(255, 255, 255, 0.03);
  border-color: rgba(255, 255, 255, 0.1);
}

.dark .stat:hover {
  background: rgba(255, 255, 255, 0.05);
}

.dark .stat-value {
  background: linear-gradient(135deg, #C75B7A, #D4A76A);
  -webkit-background-clip: text;
  background-clip: text;
  -webkit-text-fill-color: transparent;
}
</style>
