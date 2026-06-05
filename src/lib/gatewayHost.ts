/**
 * Collapse a user-entered URL to its bare origin (scheme + host + port). A path
 * would get concatenated with the fixed ingress paths (e.g.
 * `/anthropic/v1/messages`) and break the URL, so paths/query/hash are dropped.
 * Returns null for unparseable input.
 */
export function normalizeGatewayHost(raw: string): string | null {
  try {
    return new URL(raw.trim()).origin
  } catch {
    return null
  }
}
