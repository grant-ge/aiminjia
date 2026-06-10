import { getSettings, updateSettings } from '@/lib/tauri'
import { useSettingsStore } from '@/stores/settingsStore'

function parseCollapsedProjects(raw: string | undefined): Record<string, boolean> {
  if (!raw) return {}
  try {
    const parsed = JSON.parse(raw)
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {}
    return Object.fromEntries(
      Object.entries(parsed).filter(
        (entry): entry is [string, boolean] =>
          typeof entry[0] === 'string' && typeof entry[1] === 'boolean',
      ),
    )
  } catch {
    return {}
  }
}

export function loadCollapsedProjects(): Record<string, boolean> {
  return parseCollapsedProjects(useSettingsStore.getState().uiSidebarCollapsedProjects)
}

export function saveCollapsedProjects(next: Record<string, boolean>): void {
  const serialized = JSON.stringify(next)
  useSettingsStore.setState({ uiSidebarCollapsedProjects: serialized })

  void (async () => {
    try {
      const current = await getSettings()
      await updateSettings({
        ...current,
        uiSidebarCollapsedProjects: serialized,
      })
    } catch (err) {
      console.warn('[sidebarProjectPrefs] persist failed:', err)
    }
  })()
}
