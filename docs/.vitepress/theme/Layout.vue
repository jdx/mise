<template>
  <DefaultTheme.Layout>
    <template #home-hero-info><span /></template>
    <template #home-hero-before><HomeHero /></template>
    <template #layout-bottom>
      <EndevSponsors />
      <EndevFooter />
    </template>
  </DefaultTheme.Layout>
</template>

<script setup lang="ts">
import DefaultTheme from "vitepress/theme";
import { onMounted } from "vue";
import EndevFooter from "./EndevFooter.vue";
import EndevSponsors from "./EndevSponsors.vue";
import HomeHero from "./HomeHero.vue";

onMounted(() => {
  // Drop the pre-paint sidebar layout after hydration has painted.
  requestAnimationFrame(() =>
    requestAnimationFrame(() =>
      document.documentElement.classList.remove("preboot", "preboot-sidebar"),
    ),
  );
});
</script>
