<template>
  <a
    href="https://github.com/jdx/mise"
    target="_blank"
    rel="noopener"
    class="github-stars"
    aria-label="GitHub repository stars"
  >
    <svg
      class="github-icon"
      role="img"
      viewBox="0 0 24 24"
      xmlns="http://www.w3.org/2000/svg"
    >
      <path
        d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12"
      />
    </svg>
    <span class="star-badge">
      <span class="star-icon">⭐</span>
      <span class="star-count">{{ stars }}</span>
    </span>
  </a>
</template>

<script setup lang="ts">
import { data } from "../stars.data";
const { stars } = data;
</script>

<style scoped>
.github-stars {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 6px 12px;
  border-radius: 20px;
  text-decoration: none;
  background: var(--vp-c-bg-soft);
  border: 1px solid var(--vp-c-divider);
  transition: all 0.3s ease;
  margin-right: 8px;
  font-size: 0.875rem;
}

.github-stars:hover {
  background: var(--vp-c-bg-mute);
  border-color: var(--vp-c-brand-1);
  transform: translateY(-1px);
  box-shadow: 0 2px 8px rgba(0, 217, 255, 0.15);
}

.github-icon {
  width: 18px;
  height: 18px;
  fill: currentColor;
}

.star-badge {
  display: flex;
  align-items: center;
  gap: 4px;
  color: var(--vp-c-text-1);
  font-weight: 600;
}

.star-icon {
  font-size: 0.9rem;
  filter: saturate(1.5);
}

.star-count {
  font-family: var(--vp-font-family-mono);
  letter-spacing: -0.02em;
}

/* Animation on hover */
.github-stars:hover .star-icon {
  animation: twinkle 0.5s ease;
}

@keyframes twinkle {
  0%, 100% {
    transform: scale(1) rotate(0deg);
  }
  50% {
    transform: scale(1.2) rotate(180deg);
  }
}

/* Mobile responsiveness */
@media (max-width: 768px) {
  .github-stars {
    padding: 4px 8px;
    font-size: 0.8rem;
  }

  .github-icon {
    width: 16px;
    height: 16px;
  }
}
</style>