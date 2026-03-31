# Sleeves CLI Reference

Complete reference for all `mise sleeves` commands.

## init

Create a project and initialize AlteredCarbon Sleeves.

```bash
mise sleeves init [name]
```

**Arguments:**
- `name` — Project name (defaults to current directory name)

**Flags:**
- `--json` — Return structured JSON output

**What it does:**
- Creates `.projects/state.json` and `.projects/state.local.json`
- Fails if a project is already initialized in the current directory

**Example:**
```bash
$ mise sleeves init my-app
Initialized project 'my-app'
  State written to .projects/state.json
  Run `mise sleeves add <provider>/<service>` to provision resources.
```

---

## status

View project name, services, tiers, and health.

```bash
mise sleeves status [--json]
```

**Example:**
```bash
$ mise sleeves status
Project: my-app

Provider Accounts:
  vercel (vercel account)
  clerk (clerk account)

Services:
  clerk/auth — tier: pro — status: active
  posthog/analytics — tier: free — status: active

Health:
  [ok] clerk-auth is active
  [ok] posthog-analytics is active
```

---

## catalog

List available providers, categories, and services.

```bash
mise sleeves catalog [filter] [--json]
```

**Arguments:**
- `filter` — Filter by provider name or category (optional)

**Examples:**
```bash
# Show all providers
mise sleeves catalog

# Show a specific provider
mise sleeves catalog clerk

# Show a category
mise sleeves catalog database
```

---

## add

Add a service to your project. Provisions a resource and syncs credentials to `.env`.

```bash
mise sleeves add <provider>/<service> [--json]
```

**Arguments:**
- `provider/service` — Service in `provider/service` format (e.g., `clerk/auth`)

If the provider is not yet linked, it is linked automatically.

**Example:**
```bash
$ mise sleeves add clerk/auth
Added clerk/auth (tier: free)
Resource: clerk-auth (res_y43M7h_auth)

Environment variables synced to .env:
  CLERK_SECRET_KEY
  NEXT_PUBLIC_CLERK_PUBLISHABLE_KEY
```

---

## link

Connect a provider to your project without provisioning a resource.

```bash
mise sleeves link <provider> [--json]
```

Useful for establishing the connection before provisioning, especially in agent-driven workflows.

**Example:**
```bash
$ mise sleeves link vercel
Linked provider 'vercel' (account: acct_vercel_ZtIR1OPU)
```

---

## remove

Remove a service from your project.

```bash
mise sleeves remove <identifier> [--json] [--auto-confirm]
```

**Arguments:**
- `identifier` — Either `provider/service` format or resource name

**Example:**
```bash
$ mise sleeves remove clerk/auth
Removed service 'clerk-auth'
Environment variables updated in .env
```

---

## rotate

Rotate credentials for a service and update `.env`.

```bash
mise sleeves rotate <identifier> [--json]
```

**Example:**
```bash
$ mise sleeves rotate clerk/auth
Rotated credentials for 'clerk-auth'
Updated environment variables:
  CLERK_SECRET_KEY
  NEXT_PUBLIC_CLERK_PUBLISHABLE_KEY
```

---

## upgrade

Change the tier of a service.

```bash
mise sleeves upgrade <identifier> [--tier <tier>] [--json]
```

**Arguments:**
- `identifier` — Either `provider/service` format or resource name
- `--tier` — Target tier name (defaults to `pro`)

**Example:**
```bash
$ mise sleeves upgrade supabase/database --tier pro
Upgraded 'supabase-database' to tier 'pro'
```

---

## open

Open a provider's dashboard in your default browser.

```bash
mise sleeves open <provider>
```

**Example:**
```bash
$ mise sleeves open clerk
Opening clerk dashboard: https://dashboard.clerk.com
```

---

## env

List or sync project environment variables.

```bash
# List variables (values masked)
mise sleeves env [--json]

# Sync to .env
mise sleeves env --pull [--json]
```

**Examples:**
```bash
$ mise sleeves env
  CLERK_SECRET_KEY = sk_l•••••••• (from clerk/auth)
  POSTHOG_PROJECT_API_KEY = phc_•••••••• (from posthog/analytics)

$ mise sleeves env --pull
Synced 4 environment variables to .env
```

---

## billing

Manage billing and payment methods.

### billing show

```bash
mise sleeves billing show [--json]
```

### billing add

```bash
mise sleeves billing add [--json]
```

---

## llm-context

Generate a combined LLM context file from project and provider data.

```bash
mise sleeves llm-context [--json]
```

Writes to `.projects/llm-context.md` and outputs the content. Useful for providing coding agents with project context.

**Example:**
```bash
$ mise sleeves llm-context
# Project Context

Project: my-app
Providers: vercel, clerk, posthog
Resources: 2

## clerk/auth (tier: pro)
Environment variables:
  - CLERK_SECRET_KEY
  - NEXT_PUBLIC_CLERK_PUBLISHABLE_KEY

Context written to .projects/llm-context.md
```
