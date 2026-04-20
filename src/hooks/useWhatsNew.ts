import { useState, useEffect } from 'react'
import { getVersion } from '@tauri-apps/api/app'
import { resolveResource } from '@tauri-apps/api/path'
import { readTextFile } from '@tauri-apps/plugin-fs'

interface ChangelogEntry {
  product: string
  version: string
  date: string
  changes: { zh: string[]; en: string[] }
}

interface ChangelogData {
  versions: ChangelogEntry[]
}

const LAST_SEEN_KEY = 'aijia-last-seen-version'

export function useWhatsNew() {
  const [showWhatsNew, setShowWhatsNew] = useState(false)
  const [currentVersion, setCurrentVersion] = useState('')
  const [changes, setChanges] = useState<string[]>([])
  const [allDesktopVersions, setAllDesktopVersions] = useState<ChangelogEntry[]>([])

  useEffect(() => {
    let cancelled = false

    async function check() {
      try {
        const version = await getVersion()
        if (cancelled) return
        setCurrentVersion(version)

        // Load changelog from bundled resource
        const resourcePath = await resolveResource('changelog.json')
        const text = await readTextFile(resourcePath)
        const data: ChangelogData = JSON.parse(text)
        const desktopVersions = data.versions.filter(v => v.product === 'desktop')

        if (cancelled) return
        setAllDesktopVersions(desktopVersions)

        // Check if version changed since last seen
        const lastSeen = localStorage.getItem(LAST_SEEN_KEY)
        if (lastSeen === null) {
          // First install — don't show popup, just record current version
          localStorage.setItem(LAST_SEEN_KEY, version)
          return
        }

        if (lastSeen !== version) {
          // Version changed — find matching entry and show popup
          const lang = localStorage.getItem('i18nextLng') || 'en'
          const langKey = lang.startsWith('zh') ? 'zh' : 'en'
          const entry = desktopVersions.find(v => v.version === version)
          if (entry) {
            setChanges(entry.changes[langKey] || entry.changes['en'] || [])
            setShowWhatsNew(true)
          }
        }
      } catch (err) {
        // Silently fail — changelog is non-critical
        console.warn('Failed to load changelog:', err)
      }
    }

    check()
    return () => { cancelled = true }
  }, [])

  function dismissWhatsNew() {
    setShowWhatsNew(false)
    localStorage.setItem(LAST_SEEN_KEY, currentVersion)
  }

  return {
    showWhatsNew,
    dismissWhatsNew,
    currentVersion,
    changes,
    allDesktopVersions,
  }
}
