<template>
  <div class="sleeves-catalog">
    <div class="catalog-controls">
      <div class="filter-group">
        <label class="filter-label">Filter by category:</label>
        <div class="filter-buttons">
          <button
            v-for="cat in categories"
            :key="cat"
            :class="['filter-btn', { active: selectedCategory === cat }]"
            @click="selectedCategory = selectedCategory === cat ? '' : cat"
          >
            {{ cat }}
          </button>
        </div>
      </div>
    </div>

    <div class="catalog-grid">
      <div
        v-for="p in filteredProviders"
        :key="p.name"
        class="catalog-card"
        :class="{ expanded: expandedProvider === p.name }"
        @click="expandedProvider = expandedProvider === p.name ? '' : p.name"
      >
        <div class="card-header">
          <div class="card-title">
            <span class="card-name">{{ p.name }}</span>
            <span class="card-arrow">{{ expandedProvider === p.name ? '&#9650;' : '&#9660;' }}</span>
          </div>
          <div class="card-categories">
            <span v-for="c in p.categories" :key="c" class="category-tag">{{ c }}</span>
          </div>
        </div>

        <div v-if="expandedProvider === p.name" class="card-details">
          <div v-for="s in p.services" :key="s.service" class="service-block">
            <div class="service-header">
              <code class="service-cmd">mise sleeves add {{ p.name.toLowerCase() }}/{{ s.service }}</code>
              <span class="service-desc">{{ s.description }}</span>
            </div>
            <div class="tier-list">
              <div v-for="t in s.tiers" :key="t.name" class="tier-row">
                <span class="tier-name">{{ t.name }}</span>
                <span class="tier-price">{{ t.price }}</span>
                <span class="tier-features">{{ t.features.join(' · ') }}</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { ref, computed } from 'vue'

const selectedCategory = ref('')
const expandedProvider = ref('')

const providers = [
  {
    name: 'Vercel',
    categories: ['Hosting'],
    services: [{
      service: 'project',
      description: 'Frontend hosting and serverless functions',
      tiers: [
        { name: 'hobby', price: 'free', features: ['Serverless functions', 'Edge network'] },
        { name: 'pro', price: '$20/mo', features: ['Team collaboration', 'Advanced analytics', 'Password protection'] },
      ]
    }]
  },
  {
    name: 'Railway',
    categories: ['Hosting', 'Database'],
    services: [
      {
        service: 'project',
        description: 'Full-stack app hosting with built-in CI/CD',
        tiers: [
          { name: 'trial', price: 'free', features: ['500 hours/month', '1 GB RAM'] },
          { name: 'pro', price: '$5/mo + usage', features: ['Unlimited hours', '8 GB RAM'] },
        ]
      },
      {
        service: 'database',
        description: 'Managed PostgreSQL, MySQL, or Redis',
        tiers: [{ name: 'standard', price: 'usage-based', features: ['Auto-scaling', 'Daily backups'] }]
      },
    ]
  },
  {
    name: 'Supabase',
    categories: ['Database', 'Authentication'],
    services: [
      {
        service: 'database',
        description: 'Managed PostgreSQL with realtime subscriptions',
        tiers: [
          { name: 'free', price: 'free', features: ['500 MB database', '50k MAUs'] },
          { name: 'pro', price: '$25/mo', features: ['8 GB database', '100k MAUs'] },
        ]
      },
      {
        service: 'auth',
        description: 'Authentication and user management',
        tiers: [{ name: 'included', price: 'included', features: ['Social login', 'Row level security'] }]
      },
    ]
  },
  {
    name: 'Neon',
    categories: ['Database'],
    services: [{
      service: 'database',
      description: 'Serverless Postgres with branching',
      tiers: [
        { name: 'free', price: 'free', features: ['0.5 GiB storage', 'Branching'] },
        { name: 'launch', price: '$19/mo', features: ['10 GiB storage', 'Autoscaling'] },
      ]
    }]
  },
  {
    name: 'PlanetScale',
    categories: ['Database'],
    services: [{
      service: 'database',
      description: 'Serverless MySQL with branching and deploy requests',
      tiers: [
        { name: 'hobby', price: 'free', features: ['5 GB storage', '1B row reads/mo'] },
        { name: 'scaler', price: '$29/mo', features: ['10 GB storage', 'Unlimited connections'] },
      ]
    }]
  },
  {
    name: 'Turso',
    categories: ['Database'],
    services: [{
      service: 'database',
      description: 'Edge-hosted distributed SQLite (libSQL)',
      tiers: [
        { name: 'starter', price: 'free', features: ['9 GB storage', '500 databases'] },
        { name: 'scaler', price: '$29/mo', features: ['24 GB storage', '10k databases'] },
      ]
    }]
  },
  {
    name: 'Chroma',
    categories: ['Vector Database'],
    services: [{
      service: 'database',
      description: 'AI-native open-source vector database',
      tiers: [{ name: 'cloud', price: 'usage-based', features: ['Managed hosting', 'Auto-scaling'] }]
    }]
  },
  {
    name: 'Clerk',
    categories: ['Authentication'],
    services: [{
      service: 'auth',
      description: 'Drop-in authentication and user management',
      tiers: [
        { name: 'free', price: 'free', features: ['10k MAUs', 'Pre-built components'] },
        { name: 'pro', price: '$25/mo', features: ['Unlimited MAUs', 'Custom domains', 'Remove branding'] },
      ]
    }]
  },
  {
    name: 'PostHog',
    categories: ['Analytics', 'Feature Flags'],
    services: [{
      service: 'analytics',
      description: 'Product analytics, session replay, and feature flags',
      tiers: [
        { name: 'free', price: 'free', features: ['1M events/mo', 'Session replay'] },
        { name: 'paid', price: 'usage-based', features: ['Unlimited events', 'Group analytics', 'A/B testing'] },
      ]
    }]
  },
  {
    name: 'Runloop',
    categories: ['Sandboxes'],
    services: [{
      service: 'sandbox',
      description: 'Secure sandboxed execution environments',
      tiers: [{ name: 'standard', price: 'usage-based', features: ['Isolated runtimes', 'API access'] }]
    }]
  },
]

const categories = computed(() => {
  const cats = new Set()
  providers.forEach(p => p.categories.forEach(c => cats.add(c)))
  return Array.from(cats).sort()
})

const filteredProviders = computed(() => {
  if (!selectedCategory.value) return providers
  return providers.filter(p =>
    p.categories.some(c => c === selectedCategory.value)
  )
})
</script>

<style scoped>
.sleeves-catalog {
  margin: 1.5rem 0;
}

.catalog-controls {
  margin-bottom: 1.5rem;
}

.filter-label {
  display: block;
  font-size: 0.85rem;
  font-weight: 500;
  color: var(--vp-c-text-2);
  margin-bottom: 0.5rem;
}

.filter-buttons {
  display: flex;
  flex-wrap: wrap;
  gap: 0.4rem;
}

.filter-btn {
  background: var(--vp-c-bg-soft);
  border: 1px solid var(--vp-c-divider);
  border-radius: 16px;
  padding: 0.25rem 0.75rem;
  font-size: 0.8rem;
  color: var(--vp-c-text-2);
  cursor: pointer;
  transition: all 0.2s;
}

.filter-btn:hover {
  border-color: var(--vp-c-brand-1);
  color: var(--vp-c-brand-1);
}

.filter-btn.active {
  background: var(--vp-c-brand-1);
  border-color: var(--vp-c-brand-1);
  color: white;
}

.catalog-grid {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.catalog-card {
  border: 1px solid var(--vp-c-divider);
  border-radius: 8px;
  padding: 1rem 1.25rem;
  cursor: pointer;
  transition: border-color 0.25s, box-shadow 0.25s;
}

.catalog-card:hover {
  border-color: var(--vp-c-brand-1);
}

.catalog-card.expanded {
  border-color: var(--vp-c-brand-1);
  box-shadow: 0 2px 16px rgba(139, 34, 82, 0.06);
}

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.card-title {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.card-name {
  font-weight: 600;
  font-size: 1.1rem;
  color: var(--vp-c-text-1);
}

.card-arrow {
  font-size: 0.7rem;
  color: var(--vp-c-text-3);
}

.card-categories {
  display: flex;
  gap: 0.4rem;
}

.category-tag {
  background: var(--vp-c-bg-soft);
  border-radius: 4px;
  padding: 0.1rem 0.5rem;
  font-size: 0.75rem;
  color: var(--vp-c-text-3);
}

.card-details {
  margin-top: 1rem;
  padding-top: 1rem;
  border-top: 1px solid var(--vp-c-divider);
}

.service-block {
  margin-bottom: 1rem;
}

.service-block:last-child {
  margin-bottom: 0;
}

.service-header {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  margin-bottom: 0.5rem;
}

.service-cmd {
  font-size: 0.85rem;
  background: var(--vp-c-bg-soft);
  padding: 0.2rem 0.5rem;
  border-radius: 4px;
  display: inline-block;
  width: fit-content;
}

.service-desc {
  font-size: 0.85rem;
  color: var(--vp-c-text-2);
}

.tier-list {
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
}

.tier-row {
  display: flex;
  align-items: baseline;
  gap: 0.75rem;
  padding: 0.35rem 0.5rem;
  background: var(--vp-c-bg-soft);
  border-radius: 4px;
  font-size: 0.8rem;
}

.tier-name {
  font-weight: 600;
  min-width: 60px;
  color: var(--vp-c-text-1);
}

.tier-price {
  min-width: 80px;
  color: var(--vp-c-brand-1);
  font-weight: 500;
}

.tier-features {
  color: var(--vp-c-text-3);
  flex: 1;
}

@media (max-width: 640px) {
  .card-header {
    flex-direction: column;
    align-items: flex-start;
  }

  .tier-row {
    flex-direction: column;
    gap: 0.15rem;
  }
}
</style>
