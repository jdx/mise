# Service Catalog

Browse all available providers, services, and pricing tiers. Use `mise sleeves catalog` in your terminal for the same information.

<SleevesProviderCatalog />

## Provider Details

### Vercel
**Categories:** Hosting

Frontend hosting and serverless functions with global edge network. Deploy from Git with zero configuration.

```bash
mise sleeves add vercel/project
```

### Railway
**Categories:** Hosting, Database, Storage

Full-stack app hosting with built-in CI/CD pipeline and managed databases.

```bash
mise sleeves add railway/project
mise sleeves add railway/database
```

### Supabase
**Categories:** Database, Authentication, Storage

Open-source Firebase alternative with managed PostgreSQL, realtime subscriptions, and built-in auth.

```bash
mise sleeves add supabase/database
mise sleeves add supabase/auth
```

### Neon
**Categories:** Database, Authentication

Serverless Postgres with database branching for development workflows.

```bash
mise sleeves add neon/database
```

### PlanetScale
**Categories:** Database

Serverless MySQL platform with branching and non-blocking schema changes.

```bash
mise sleeves add planetscale/database
```

### Turso
**Categories:** Database

Edge-hosted distributed SQLite (libSQL) for ultra-low-latency reads.

```bash
mise sleeves add turso/database
```

### Chroma
**Categories:** Vector Database

AI-native open-source vector database for embedding storage and retrieval.

```bash
mise sleeves add chroma/database
```

### Clerk
**Categories:** Authentication

Drop-in authentication with pre-built UI components and user management.

```bash
mise sleeves add clerk/auth
```

### PostHog
**Categories:** Analytics, Feature Flags

Open-source product analytics with session replay, feature flags, and A/B testing.

```bash
mise sleeves add posthog/analytics
```

### Runloop
**Categories:** Sandboxes, Hosting

Secure sandboxed execution environments with API access.

```bash
mise sleeves add runloop/sandbox
```

## Request a Provider

Want to see a provider that isn't listed? Open an issue or contact provider-request@alteredcarbon.com.

<script setup>
import SleevesProviderCatalog from '../components/SleevesProviderCatalog.vue'
</script>
