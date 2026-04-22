<template>
  <footer class="EndevFooter">
    <span>MIT License</span>
    <span aria-hidden="true">·</span>
    <span>Copyright © {{ year }}</span>
    <span aria-hidden="true">·</span>
    <a class="EndevFooterLink" href="https://en.dev">
      <img
        alt=""
        class="EndevFooterLogo"
        height="28"
        src="https://github.com/endevco.png?size=96"
        width="28"
      />
      <span>en.dev</span>
    </a>
  </footer>
</template>

<script setup lang="ts">
const year = new Date().getFullYear();
</script>
