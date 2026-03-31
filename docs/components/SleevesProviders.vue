<template>
  <div class="sleeves-providers">
    <div class="provider-grid">
      <div v-for="p in providers" :key="p.name" class="provider-card">
        <div class="provider-header">
          <span class="provider-name">{{ p.name }}</span>
          <span class="provider-categories">{{ p.categories.join(', ') }}</span>
        </div>
        <div class="provider-services">
          <div v-for="s in p.services" :key="s.service" class="service-badge">
            {{ s.service }}
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
const providers = [
  { name: 'Vercel', categories: ['Hosting'], services: [{ service: 'project' }] },
  { name: 'Railway', categories: ['Hosting', 'Database', 'Storage'], services: [{ service: 'project' }, { service: 'database' }] },
  { name: 'Supabase', categories: ['Database', 'Auth', 'Storage'], services: [{ service: 'database' }, { service: 'auth' }] },
  { name: 'Neon', categories: ['Database', 'Auth'], services: [{ service: 'database' }] },
  { name: 'PlanetScale', categories: ['Database'], services: [{ service: 'database' }] },
  { name: 'Turso', categories: ['Database'], services: [{ service: 'database' }] },
  { name: 'Chroma', categories: ['Vector Database'], services: [{ service: 'database' }] },
  { name: 'Clerk', categories: ['Authentication'], services: [{ service: 'auth' }] },
  { name: 'PostHog', categories: ['Analytics', 'Feature Flags'], services: [{ service: 'analytics' }] },
  { name: 'Runloop', categories: ['Sandboxes', 'Hosting'], services: [{ service: 'sandbox' }] },
]
</script>

<style scoped>
.sleeves-providers {
  margin: 1.5rem 0;
}

.provider-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 1rem;
}

.provider-card {
  border: 1px solid var(--vp-c-divider);
  border-radius: 8px;
  padding: 1rem;
  transition: border-color 0.25s, box-shadow 0.25s;
}

.provider-card:hover {
  border-color: var(--vp-c-brand-1);
  box-shadow: 0 2px 12px rgba(139, 34, 82, 0.08);
}

.provider-header {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  margin-bottom: 0.75rem;
}

.provider-name {
  font-weight: 600;
  font-size: 1.1rem;
  color: var(--vp-c-text-1);
}

.provider-categories {
  font-size: 0.8rem;
  color: var(--vp-c-text-3);
}

.provider-services {
  display: flex;
  flex-wrap: wrap;
  gap: 0.4rem;
}

.service-badge {
  background: var(--vp-c-bg-soft);
  border: 1px solid var(--vp-c-divider);
  border-radius: 4px;
  padding: 0.15rem 0.5rem;
  font-size: 0.75rem;
  font-family: var(--vp-font-family-mono);
  color: var(--vp-c-text-2);
}
</style>
