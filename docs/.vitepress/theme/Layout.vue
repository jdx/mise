<template>
  <DefaultTheme.Layout>
    <template #home-hero-info><span /></template>
    <template #home-hero-before><HomeHero /></template>
    <!-- Under the hero; left out of builds without a render. -->
    <template #home-hero-after><HomeShowreel /></template>
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
import HomeShowreel from "./HomeShowreel.vue";

onMounted(() => {
  // Drop the pre-paint sidebar layout after hydration has painted.
  requestAnimationFrame(() =>
    requestAnimationFrame(() =>
      document.documentElement.classList.remove("preboot", "preboot-sidebar"),
    ),
  );
});
</script>
