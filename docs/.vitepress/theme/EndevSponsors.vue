<template>
  <section
    v-if="sponsors.length"
    aria-labelledby="endev-sponsors-title"
    class="EndevSponsors"
  >
    <div class="EndevSponsorsInner">
      <p id="endev-sponsors-title" class="EndevSponsorsTitle">
        Company sponsors
      </p>
      <div class="EndevSponsorsLogos">
        <a
          v-for="sponsor in sponsors"
          :key="sponsor.name"
          :aria-label="sponsor.name"
          class="EndevSponsorsLogo"
          :href="sponsor.url"
          rel="noopener noreferrer"
          target="_blank"
        >
          <img :alt="sponsor.name" :src="sponsor.logo" />
        </a>
      </div>
      <a class="EndevSponsorsCta" href="https://en.dev/#contact">
        Sponsor the work
      </a>
    </div>
  </section>
</template>

<script setup>
import { onMounted, ref } from "vue";

const sponsors = ref([]);

onMounted(async () => {
  try {
    const res = await fetch("https://en.dev/sponsors.json", {
      headers: { Accept: "application/json" },
    });
    if (!res.ok) return;

    const payload = await res.json();
    sponsors.value = (Array.isArray(payload.sponsors) ? payload.sponsors : [])
      .filter((sponsor) =>
        sponsor?.kind !== "infrastructure" &&
        sponsor?.name &&
        sponsor?.url &&
        sponsor?.logo
      );
  } catch {
    sponsors.value = [];
  }
});
</script>

<style scoped>
.EndevSponsors {
  border-top: 1px solid var(--vp-c-divider);
  padding: 22px 24px;
}

.EndevSponsorsInner {
  align-items: center;
  display: flex;
  flex-wrap: wrap;
  gap: 12px 18px;
  justify-content: center;
  margin: 0 auto;
  max-width: 960px;
}

.EndevSponsorsTitle {
  color: var(--vp-c-text-2);
  font-size: 13px;
  font-weight: 600;
  margin: 0;
  text-transform: uppercase;
}

.EndevSponsorsLogos {
  align-items: center;
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  justify-content: center;
}

.EndevSponsorsLogo {
  align-items: center;
  border: 1px solid var(--vp-c-divider);
  border-radius: 8px;
  display: inline-flex;
  height: 40px;
  justify-content: center;
  padding: 8px 12px;
  transition: border-color 0.2s ease, background-color 0.2s ease;
}

.EndevSponsorsLogo:hover {
  background: var(--vp-c-bg-soft);
  border-color: var(--vp-c-brand-1);
}

.EndevSponsorsLogo img {
  display: block;
  max-height: 22px;
  max-width: 120px;
}

.EndevSponsorsCta {
  color: var(--vp-c-text-2);
  font-size: 13px;
  font-weight: 500;
  text-decoration: none;
  transition: color 0.2s ease;
}

.EndevSponsorsCta:hover {
  color: var(--vp-c-brand-1);
}
</style>
