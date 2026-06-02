<template>
  <section
    v-if="sponsors.length || error"
    aria-labelledby="endev-sponsors-title"
    class="EndevSponsors"
  >
    <div class="EndevSponsorsInner">
      <p id="endev-sponsors-title" class="EndevSponsorsTitle">
        sponsors
      </p>
      <p v-if="error" class="EndevSponsorsError">
        Sponsor feed unavailable.
      </p>
      <div v-else class="EndevSponsorsLogos">
        <a
          v-for="sponsor in sponsors"
          :key="sponsor.url"
          class="EndevSponsorsLogo"
          :href="sponsor.url"
          rel="noopener noreferrer sponsored"
          target="_blank"
        >
          <img :alt="sponsor.name" :src="sponsor.logo" loading="lazy" decoding="async" />
        </a>
      </div>
      <a class="EndevSponsorsCta" href="https://en.dev/sponsors.html">
        View all sponsors
      </a>
    </div>
  </section>
</template>

<script setup>
import { onMounted, ref } from "vue";

const sponsors = ref([]);
const error = ref(false);
const footerTiers = new Set(["anchor", "premier", "partner"]);
const sponsorFeedTimeoutMs = 5000;

const sponsorItems = (items) => (Array.isArray(items) ? items : []);
const isSafeUrl = (url) => {
  try {
    const { protocol } = new URL(url);
    return protocol === "https:" || protocol === "http:";
  } catch {
    return false;
  }
};
const isSponsor = (sponsor) =>
  sponsor &&
  typeof sponsor === "object" &&
  typeof sponsor.name === "string" &&
  typeof sponsor.url === "string" &&
  typeof sponsor.logo === "string" &&
  isSafeUrl(sponsor.url) &&
  isSafeUrl(sponsor.logo);

onMounted(async () => {
  const controller = new AbortController();
  const timeout = window.setTimeout(() => controller.abort(), sponsorFeedTimeoutMs);

  try {
    const res = await fetch("https://en.dev/sponsors.json", {
      headers: { Accept: "application/json" },
      signal: controller.signal,
    });
    if (!res.ok) throw new Error(`Sponsor feed returned ${res.status}`);

    const payload = await res.json();
    sponsors.value = sponsorItems(payload?.sponsors).filter((sponsor) =>
      isSponsor(sponsor) && footerTiers.has(sponsor.tier),
    );
  } catch {
    error.value = true;
    sponsors.value = [];
  } finally {
    window.clearTimeout(timeout);
  }
});
</script>

<style scoped>
.EndevSponsors {
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

.EndevSponsorsError {
  color: var(--vp-c-text-3);
  font-size: 13px;
  font-weight: 500;
  margin: 0;
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
