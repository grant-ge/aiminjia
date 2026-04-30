export function parseEnvVars(raw: string): Record<string, string> | undefined {
  if (!raw.trim()) return undefined

  const entries = raw
    .trim()
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const [key, ...rest] = line.split('=')
      return [key?.trim() ?? '', rest.join('=').trim()] as const
    })
    .filter(([key, value]) => key && value)

  return entries.length > 0 ? Object.fromEntries(entries) : undefined
}
