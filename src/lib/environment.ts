/**
 * Environment — the backend origins the frontend builds URLs against.
 *
 * Mirrors `src-tauri/src/environment.rs`: an environment bundles the `tenant`
 * gateway origin and the `ops` portal origin, which move together on a dev
 * switch. The authoritative value lives in Rust; this is a synchronous,
 * process-level mirror because render-path callers (e.g. the branding logo
 * proxy in `brandingStore`) need an origin without awaiting an IPC round-trip.
 *
 * Seeded at startup (`main.tsx`) and updated on the dev switch
 * (`TitleBarEnvSwitcher`). In release builds nothing seeds it, so it stays
 * pinned to production.
 */

/** Production tenant gateway origin. Mirrors `PROD_TENANT` in `environment.rs`. */
const PROD_TENANT = 'https://ai-tenant.renlijia.com'
/** Production ops-portal origin. Mirrors `PROD_OPS` in `environment.rs`. */
const PROD_OPS = 'https://ai-ops.renlijia.com'

export interface Environment {
  tenant: string
  ops: string
}

let current: Environment = { tenant: PROD_TENANT, ops: PROD_OPS }

/**
 * Collapse a user-entered URL to its bare origin (scheme + host + port). A path
 * would get concatenated with fixed API paths (e.g. `/aijia/v2/ai/responses`)
 * and break the URL, so paths/query/hash are dropped. Returns null for
 * unparseable input.
 */
export function normalizeOrigin(raw: string): string | null {
  try {
    return new URL(raw.trim()).origin
  } catch {
    return null
  }
}

/** The tenant gateway origin currently in effect (no trailing slash). */
export function tenantHost(): string {
  return current.tenant
}

/** The ops-portal origin currently in effect (no trailing slash). */
export function opsHost(): string {
  return current.ops
}

/**
 * Update the cached environment. Each origin that fails to parse falls back to
 * its production default, so a partial/invalid update never leaves a broken host.
 */
export function setEnvironmentCache(env: Partial<Environment>): void {
  current = {
    tenant: (env.tenant && normalizeOrigin(env.tenant)) || PROD_TENANT,
    ops: (env.ops && normalizeOrigin(env.ops)) || PROD_OPS,
  }
}
