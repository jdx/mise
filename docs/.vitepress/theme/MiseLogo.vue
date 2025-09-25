<template>
  <svg
    :width="width"
    :height="height"
    viewBox="0 0 120 120"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
    class="mise-logo"
    :class="{ animated }"
  >
    <!-- Background circle with gradient -->
    <defs>
      <linearGradient id="logoGradient" x1="0%" y1="0%" x2="100%" y2="100%">
        <stop offset="0%" style="stop-color: #00d9ff; stop-opacity: 1" />
        <stop offset="50%" style="stop-color: #52e892; stop-opacity: 1" />
        <stop offset="100%" style="stop-color: #ff9100; stop-opacity: 1" />
      </linearGradient>

      <linearGradient id="logoGradientDark" x1="0%" y1="0%" x2="100%" y2="100%">
        <stop offset="0%" style="stop-color: #00b8d9; stop-opacity: 1" />
        <stop offset="50%" style="stop-color: #3dd979; stop-opacity: 1" />
        <stop offset="100%" style="stop-color: #e68200; stop-opacity: 1" />
      </linearGradient>

      <!-- Filter for glow effect -->
      <filter id="glow">
        <feGaussianBlur stdDeviation="3" result="coloredBlur" />
        <feMerge>
          <feMergeNode in="coloredBlur" />
          <feMergeNode in="SourceGraphic" />
        </feMerge>
      </filter>
    </defs>

    <!-- Outer hexagon shape (mise plate concept) -->
    <g class="logo-base">
      <path
        d="M60 10 L100 35 L100 85 L60 110 L20 85 L20 35 Z"
        stroke="url(#logoGradient)"
        stroke-width="3"
        fill="none"
        opacity="0.8"
        class="hexagon-outer"
      />

      <!-- Inner hexagon -->
      <path
        d="M60 20 L90 40 L90 80 L60 100 L30 80 L30 40 Z"
        fill="url(#logoGradient)"
        opacity="0.1"
        class="hexagon-inner"
      />
    </g>

    <!-- Terminal/CLI representation -->
    <g class="terminal-group">
      <!-- Terminal window -->
      <rect
        x="35"
        y="45"
        width="50"
        height="30"
        rx="3"
        fill="none"
        stroke="url(#logoGradient)"
        stroke-width="2"
        opacity="0.9"
      />

      <!-- Terminal prompt -->
      <text
        x="40"
        y="58"
        font-family="JetBrains Mono, monospace"
        font-size="12"
        font-weight="bold"
        fill="url(#logoGradient)"
      >
        &gt;_
      </text>

      <!-- Command lines -->
      <rect
        x="55"
        y="52"
        width="25"
        height="2"
        fill="url(#logoGradient)"
        opacity="0.7"
      />
      <rect
        x="40"
        y="60"
        width="20"
        height="2"
        fill="url(#logoGradient)"
        opacity="0.5"
      />
      <rect
        x="40"
        y="68"
        width="30"
        height="2"
        fill="url(#logoGradient)"
        opacity="0.3"
      />
    </g>

    <!-- Lightning bolt for speed -->
    <path
      d="M60 25 L50 50 L57 50 L50 70 L70 40 L63 40 L70 25 Z"
      fill="url(#logoGradient)"
      opacity="0"
      class="lightning"
      filter="url(#glow)"
    />

    <!-- Animated dots representing tools/packages -->
    <g class="dots">
      <circle cx="25" cy="35" r="3" fill="#00d9ff" class="dot dot-1" />
      <circle cx="95" cy="35" r="3" fill="#52e892" class="dot dot-2" />
      <circle cx="95" cy="85" r="3" fill="#ff9100" class="dot dot-3" />
      <circle cx="25" cy="85" r="3" fill="#00d9ff" class="dot dot-4" />
      <circle cx="60" cy="15" r="3" fill="#52e892" class="dot dot-5" />
      <circle cx="60" cy="105" r="3" fill="#ff9100" class="dot dot-6" />
    </g>
  </svg>
</template>

<script setup lang="ts">
interface Props {
  width?: number | string;
  height?: number | string;
  animated?: boolean;
}

withDefaults(defineProps<Props>(), {
  width: 120,
  height: 120,
  animated: false,
});
</script>

<style scoped>
.mise-logo {
  display: inline-block;
}

.mise-logo:hover .hexagon-outer {
  animation: rotate 20s linear infinite;
  transform-origin: center;
  transform-box: fill-box;
}

.mise-logo:hover .hexagon-inner {
  animation: pulse 3s ease-in-out infinite;
}

.mise-logo:hover .lightning {
  animation: lightning-strike 0.5s ease-out forwards;
}

.mise-logo:hover .dot {
  animation: orbit 6s ease-in-out infinite;
}

.dot-1 {
  animation-delay: 0s;
}
.dot-2 {
  animation-delay: 1s;
}
.dot-3 {
  animation-delay: 2s;
}
.dot-4 {
  animation-delay: 3s;
}
.dot-5 {
  animation-delay: 4s;
}
.dot-6 {
  animation-delay: 5s;
}

@keyframes rotate {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}

@keyframes pulse {
  0%,
  100% {
    opacity: 0.1;
  }
  50% {
    opacity: 0.2;
  }
}

@keyframes lightning-strike {
  0% {
    opacity: 0;
    transform: translateY(-5px);
  }
  50% {
    opacity: 1;
    transform: translateY(0);
  }
  100% {
    opacity: 0;
    transform: translateY(5px);
  }
}

@keyframes orbit {
  0%,
  100% {
    transform: scale(1);
    opacity: 0.8;
  }
  50% {
    transform: scale(1.5);
    opacity: 1;
  }
}

/* Dark mode adjustments */
.dark .mise-logo path,
.dark .mise-logo rect,
.dark .mise-logo text {
  stroke: url(#logoGradientDark);
  fill: url(#logoGradientDark);
}
</style>
