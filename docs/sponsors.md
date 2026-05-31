---
title: Sponsors
description: Companies supporting the en.dev open source project family.
---

<script setup>
import { computed, onMounted, ref } from "vue";

const feed = ref(null);
const error = ref("");
const tiers = [
  ["anchor", "Anchor"],
  ["premier", "Premier"],
  ["partner", "Partner"],
  ["backer", "Backer"],
];

onMounted(async () => {
  try {
    const res = await fetch("https://en.dev/sponsors.json");
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    feed.value = await res.json();
  } catch (err) {
    error.value = err?.message || "Unable to load sponsors";
  }
});

const paidSponsors = computed(() =>
  (feed.value?.paid || feed.value?.sponsors || []).filter((s) => s.name && s.url),
);

const sponsorsByTier = computed(() =>
  tiers.map(([id, label]) => ({
    id,
    label,
    sponsors: paidSponsors.value.filter((s) => s.tier === id),
  })),
);

const otherSponsors = computed(() =>
  paidSponsors.value.filter((s) => !tiers.some(([id]) => id === s.tier)),
);

const infrastructureSponsors = computed(() =>
  (feed.value?.infrastructure || []).filter((s) => s.name && s.url),
);
</script>

# Sponsors

These companies support the en.dev open source project family.

<div v-if="error" class="sponsors-note">
  Sponsor data could not be loaded. Visit <a href="https://en.dev/sponsor.html">en.dev sponsors</a>.
</div>

<div v-else-if="!feed" class="sponsors-note">Loading sponsors...</div>

<div v-else class="sponsors-page">
  <section v-for="tier in sponsorsByTier" :key="tier.id" class="sponsor-tier">
    <h2>{{ tier.label }}</h2>
    <div v-if="tier.sponsors.length" class="sponsor-grid">
      <a
        v-for="sponsor in tier.sponsors"
        :key="sponsor.name"
        class="sponsor-card"
        :href="sponsor.url"
        target="_blank"
        rel="noopener noreferrer"
      >
        <img v-if="sponsor.logo" :src="sponsor.logo" :alt="sponsor.name">
        <span>{{ sponsor.name }}</span>
      </a>
    </div>
    <p v-else class="sponsors-note">No sponsors at this tier yet.</p>
  </section>

  <section v-if="otherSponsors.length" class="sponsor-tier">
    <h2>Other Sponsors</h2>
    <div class="sponsor-grid">
      <a
        v-for="sponsor in otherSponsors"
        :key="sponsor.name"
        class="sponsor-card"
        :href="sponsor.url"
        target="_blank"
        rel="noopener noreferrer"
      >
        <img v-if="sponsor.logo" :src="sponsor.logo" :alt="sponsor.name">
        <span>{{ sponsor.name }}</span>
      </a>
    </div>
  </section>

  <section v-if="infrastructureSponsors.length" class="sponsor-tier">
    <h2>Infrastructure Partners</h2>
    <div class="sponsor-grid">
      <a
        v-for="sponsor in infrastructureSponsors"
        :key="sponsor.name"
        class="sponsor-card"
        :href="sponsor.url"
        target="_blank"
        rel="noopener noreferrer"
      >
        <img v-if="sponsor.logo" :src="sponsor.logo" :alt="sponsor.name">
        <span>{{ sponsor.name }}</span>
        <small v-if="sponsor.note">{{ sponsor.note }}</small>
      </a>
    </div>
  </section>
</div>

Want to support the work? [Become a sponsor](https://en.dev/sponsor.html).

<style>
.sponsors-note {
  color: var(--vp-c-text-2);
}

.sponsor-tier {
  margin-top: 2rem;
}

.sponsor-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
  gap: 0.75rem;
}

.sponsor-card {
  min-height: 96px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 8px;
  padding: 1rem;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  justify-content: center;
  gap: 0.55rem;
  color: var(--vp-c-text-1);
  background: var(--vp-c-bg-soft);
  text-decoration: none;
}

.sponsor-card:hover {
  border-color: var(--vp-c-brand-1);
  text-decoration: none;
}

.sponsor-card img {
  max-width: 150px;
  max-height: 36px;
  object-fit: contain;
}

.sponsor-card span {
  font-weight: 600;
}

.sponsor-card small {
  color: var(--vp-c-text-2);
}
</style>
