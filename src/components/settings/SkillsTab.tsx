import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/common/Button'
import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import {
  listCustomSkills, initSkillTemplate, packSkill,
  reloadSkill, startSkillWatch, stopSkillWatch, syncBuiltinSkills, TAURI_EVENTS,
} from '@/lib/tauri'
import { listen } from '@tauri-apps/api/event'
import type { CustomSkillInfo } from '@/lib/tauri'
import { message } from '@tauri-apps/plugin-dialog'
import { downloadDir, join } from '@tauri-apps/api/path'
import { useNotificationStore } from '@/stores/notificationStore'
import { useAuthStore } from '@/stores/authStore'
import { useSkillStore } from '@/stores/skillStore'
import { uploadWithOverwriteConfirm } from '@/features/skill-center/uploadWithOverwriteConfirm'
import { SkillMarketplace } from './SkillMarketplace'

type SubTab = 'installed' | 'marketplace'

interface SkillsTabProps {
  onRequestClose?: () => void
}

/**
 * Reduce an arbitrary skill id / name to a value safe for use as a file name
 * across macOS / Windows / Linux. Mirrors the backend's `safe_filename`
 * guarantees (no path separators, no reserved Windows names, no trailing
 * dots or whitespace, length ≤ 200). Falls back to "skill" when the input
 * collapses to empty.
 */
function safeSkillFilename(raw: string): string {
  const stripped = raw
    .normalize('NFKC')
    .replace(/[\\/<>:"|?*\x00-\x1f]/g, '_')
    .replace(/^[._\s]+|[._\s]+$/g, '')
    .slice(0, 200)
  const reservedWindows = /^(CON|PRN|AUX|NUL|COM[0-9]|LPT[0-9])$/i
  if (!stripped || reservedWindows.test(stripped)) return 'skill'
  return stripped
}

export function SkillsTab(_props: SkillsTabProps = {}) {
  const { t } = useTranslation()
  const isLoggedIn = useAuthStore((s) => s.isLoggedIn)
  const [subTab, setSubTab] = useState<SubTab>('installed')
  const [skills, setSkills] = useState<CustomSkillInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [devWatchPath, setDevWatchPath] = useState<string | null>(null)
  const [syncing, setSyncing] = useState(false)
  const pushNotification = useNotificationStore((s) => s.push)

  // Debounce timer for file change events
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const loadSkills = async () => {
    setLoading(true)
    try {
      const list = await listCustomSkills()
      setSkills(list)
    } catch {
      setSkills([])
    }
    setLoading(false)
  }

  // Handle debounced reload when files change
  const handleFileChanged = useCallback((changedPath: string) => {
    if (debounceRef.current) clearTimeout(debounceRef.current)
    debounceRef.current = setTimeout(async () => {
      try {
        await reloadSkill(changedPath)
        pushNotification({
          level: 'success',
          title: t('settings.skills.devReloaded'),
          message: '',
          actions: [],
          dismissible: true,
          autoHide: 3,
          context: 'toast',
        })
      } catch (e) {
        pushNotification({
          level: 'error',
          title: t('settings.skills.devReloadFailed'),
          message: String(e),
          actions: [],
          dismissible: true,
          autoHide: 5,
          context: 'toast',
        })
      }
    }, 500)
  }, [pushNotification, t])

  // Subscribe to file change events when dev mode is active
  useEffect(() => {
    if (!devWatchPath) return

    let unlisten: (() => void) | null = null
    const setup = async () => {
      unlisten = await listen<string>(TAURI_EVENTS.SKILL_FILE_CHANGED, (event) => handleFileChanged(event.payload))
    }
    setup()

    return () => {
      unlisten?.()
      if (debounceRef.current) clearTimeout(debounceRef.current)
    }
  }, [devWatchPath, handleFileChanged])

  useEffect(() => { loadSkills() }, [])

  const handleInstall = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      // Accept either a packaged skill file (.aijia-skill / .zip / .md)
      // or a directory containing SKILL.md. Falls back to a directory
      // picker when the user cancels the file picker (some users hand
      // out raw folders from skill-smith).
      const picked = await open({
        multiple: false,
        title: t('settings.skills.selectFolder'),
        filters: [
          {
            name: 'Skill',
            extensions: ['aijia-skill', 'zip', 'md'],
          },
        ],
      })
      let selected: string | null = Array.isArray(picked) ? picked[0] ?? null : picked
      if (!selected) {
        const dir = await open({
          directory: true,
          title: t('settings.skills.selectFolder'),
        })
        if (!dir || Array.isArray(dir)) return
        selected = dir
      }

      const result = await uploadWithOverwriteConfirm((force) =>
        useSkillStore.getState().upload(selected!, force),
      )
      if (result === 'installed') {
        await loadSkills()
        pushNotification({
          level: 'success',
          title: t('settings.skills.installedToast'),
          message: '',
          actions: [],
          dismissible: true,
          autoHide: 4,
          context: 'toast',
        })
      }
      // 'cancelled' — silent
    } catch (e) {
      await message(String(e), { title: 'AI小家', kind: 'error' })
    }
  }

  const handleSyncBuiltin = async () => {
    if (syncing) return
    setSyncing(true)
    try {
      const result = await syncBuiltinSkills()
      const added = result.installed.length
      await loadSkills()
      pushNotification({
        level: 'success',
        title:
          added > 0
            ? t('settings.skills.syncSuccess', { count: added })
            : t('settings.skills.syncNoChange'),
        message: '',
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    } catch (e) {
      pushNotification({
        level: 'error',
        title: t('settings.skills.syncFailed'),
        message: String(e),
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
    } finally {
      setSyncing(false)
    }
  }

  const handleUninstall = async (id: string, name: string) => {
    const confirmed = await requestConfirm({
      title: t('settings.skills.uninstall'),
      description: t('settings.skills.confirmUninstall', { name }),
      confirmLabel: t('settings.skills.uninstall'),
      variant: 'destructive',
    })
    if (!confirmed) return
    try {
      // Go through the store so its in-memory skills cache (consumed by
      // SkillPopoverPanel etc) drops the entry too. The raw
      // `uninstallCustomSkill` IPC removes the on-disk dir + refreshes
      // the backend SkillRegistry, but the frontend store still held
      // the stale row — causing the "deleted skill still searchable"
      // bug reported 2026-05-15.
      await useSkillStore.getState().uninstall(id)
      await loadSkills()
    } catch (e) {
      await message(String(e), { title: 'AI小家', kind: 'error' })
    }
  }

  const handleCreateNew = async () => {
    try {
      // TODO: replace with custom input dialog (no native Tauri text input dialog available)
      const skillId = window.prompt(t('settings.skills.skillIdPlaceholder'))
      if (!skillId) return
      // TODO: replace with custom input dialog (no native Tauri text input dialog available)
      const skillName = window.prompt(t('settings.skills.skillNamePlaceholder'))
      if (!skillName) return

      const { open } = await import('@tauri-apps/plugin-dialog')
      const targetDir = await open({ directory: true, title: t('settings.skills.selectTargetDir') })
      if (!targetDir) return

      const createdPath = await initSkillTemplate(targetDir, skillId, skillName)
      await message(t('settings.skills.created', { path: createdPath }), { title: 'AI小家' })
    } catch (e) {
      await message(String(e), { title: 'AI小家', kind: 'error' })
    }
  }

  const pickPackDest = async (defaultBasename: string): Promise<string | null> => {
    const { save } = await import('@tauri-apps/plugin-dialog')
    // Default extension is .zip — the backend treats .aijia-skill / .zip
    // identically (both produce a real zip archive containing SKILL.md +
    // scripts/ + references/), but .zip is the one users + Finder /
    // Explorer recognize as a compressed file they can open directly.
    let defaultPath: string
    try {
      defaultPath = await join(await downloadDir(), `${defaultBasename}.zip`)
    } catch {
      defaultPath = `${defaultBasename}.zip`
    }
    const dest = await save({
      title: t('settings.skills.savePackTitle'),
      defaultPath,
      filters: [
        { name: 'Skill Package (zip)', extensions: ['zip'] },
        { name: 'AIjia Skill', extensions: ['aijia-skill'] },
        { name: 'SKILL.md', extensions: ['md'] },
      ],
    })
    return dest ?? null
  }

  const handlePackSkill = async (skill: CustomSkillInfo) => {
    try {
      const base = safeSkillFilename(skill.id || skill.name || 'skill')
      const dest = await pickPackDest(base)
      if (!dest) return
      const outputPath = await packSkill(skill.path, dest)
      await message(t('settings.skills.packSuccess', { path: outputPath }), { title: 'AI小家' })
    } catch (e) {
      await message(String(e), { title: 'AI小家', kind: 'error' })
    }
  }

  const handleToggleDevMode = async (skillPath: string) => {
    if (devWatchPath === skillPath) {
      // Turn off dev mode
      try {
        await stopSkillWatch()
        setDevWatchPath(null)
      } catch (e) {
        await message(String(e), { title: 'AI小家', kind: 'error' })
      }
    } else {
      // Turn on dev mode (stops any previous watcher)
      try {
        await startSkillWatch(skillPath)
        setDevWatchPath(skillPath)
      } catch (e) {
        await message(String(e), { title: 'AI小家', kind: 'error' })
      }
    }
  }

  const handlePackStandalone = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({ directory: true, title: t('settings.skills.selectFolder') })
      if (!selected || Array.isArray(selected)) return
      const base = safeSkillFilename(selected.split(/[\\/]/).pop() ?? 'skill')
      const dest = await pickPackDest(base)
      if (!dest) return
      const outputPath = await packSkill(selected, dest)
      await message(t('settings.skills.packSuccess', { path: outputPath }), { title: 'AI小家' })
    } catch (e) {
      await message(String(e), { title: 'AI小家', kind: 'error' })
    }
  }

  return (
    <div>
      {/* Sub-tab switcher */}
      <div className="mb-4 flex items-center gap-1 rounded-lg p-0.5" style={{ background: 'var(--color-bg-main)' }}>
        <button
          onClick={() => setSubTab('installed')}
          className="rounded-md px-3 py-1.5 text-sm font-medium transition-colors cursor-pointer"
          style={{
            background: subTab === 'installed' ? 'var(--color-bg-card)' : 'transparent',
            color: subTab === 'installed' ? 'var(--color-text-primary)' : 'var(--color-text-secondary)',
            boxShadow: subTab === 'installed' ? '0 1px 2px rgba(0,0,0,0.06)' : 'none',
            border: 'none',
          }}
        >
          {t('settings.skills.installed')}
        </button>
        {isLoggedIn && (
          <button
            onClick={() => setSubTab('marketplace')}
            className="rounded-md px-3 py-1.5 text-sm font-medium transition-colors cursor-pointer"
            style={{
              background: subTab === 'marketplace' ? 'var(--color-bg-card)' : 'transparent',
              color: subTab === 'marketplace' ? 'var(--color-text-primary)' : 'var(--color-text-secondary)',
              boxShadow: subTab === 'marketplace' ? '0 1px 2px rgba(0,0,0,0.06)' : 'none',
              border: 'none',
            }}
          >
            {t('settings.skills.marketplace')}
          </button>
        )}
      </div>

      {/* Sub-tab content */}
      {subTab === 'installed' ? (
        <div>
          <div className="mb-4 flex items-center justify-between">
            <p className="text-sm" style={{ color: 'var(--color-text-secondary)' }}>
              {t('settings.skills.description')}
            </p>
            <div className="flex items-center gap-2">
              {isLoggedIn && (
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={handleSyncBuiltin}
                  disabled={syncing}
                  data-testid="skills-sync-builtin"
                >
                  {syncing
                    ? t('settings.skills.syncing')
                    : t('settings.skills.sync')}
                </Button>
              )}
              <Button variant="secondary" size="sm" onClick={handleCreateNew}>
                {t('settings.skills.createNew')}
              </Button>
              <Button variant="secondary" size="sm" onClick={handlePackStandalone}>
                {t('settings.skills.packStandalone')}
              </Button>
              <Button variant="primary" size="sm" onClick={handleInstall}>
                {t('settings.skills.install')}
              </Button>
            </div>
          </div>

          {loading ? (
            <p style={{ color: 'var(--color-text-secondary)' }}>{t('common.loading')}</p>
          ) : skills.length === 0 ? (
            <div
              className="rounded-lg border p-6 text-center border-border"
              style={{ borderColor: 'var(--color-border)', color: 'var(--color-text-secondary)' }}
            >
              <p className="mb-1">{t('settings.skills.empty')}</p>
              <p style={{ fontSize: '0.8rem' }}>{t('settings.skills.emptyHint')}</p>
            </div>
          ) : (
            <div className="space-y-2">
              {skills.map((skill) => (
                <div
                  key={skill.id}
                  className="flex items-center justify-between rounded-lg border px-4 py-3 border-border"
                  style={{ borderColor: 'var(--color-border)', background: 'var(--color-bg-main)' }}
                >
                  <div className="flex items-center gap-2">
                    <div>
                      <div className="flex items-center gap-2">
                        <span
                          className="font-medium"
                          style={{ color: 'var(--color-text-primary)' }}
                        >
                          {skill.name || skill.id}
                        </span>
                        {skill.version ? (
                          <span
                            data-testid="skill-version-badge"
                            className="rounded-full px-2 py-0.5 text-xs font-mono"
                            style={{
                              background: 'var(--color-bg-card)',
                              color: 'var(--color-text-secondary)',
                              border: '1px solid var(--color-border)',
                            }}
                            title={t('settings.skills.version')}
                          >
                            v{skill.version}
                          </span>
                        ) : null}
                      </div>
                      <div style={{ color: 'var(--color-text-secondary)', fontSize: '0.8rem' }}>
                        {skill.description || skill.id}
                      </div>
                    </div>
                    {devWatchPath === skill.path && (
                      <span
                        className="ml-2 inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium"
                        style={{ background: 'var(--color-semantic-green-bg, rgba(52,199,89,0.15))', color: 'var(--color-semantic-green, #34C759)' }}
                      >
                        <span
                          className="inline-block h-1.5 w-1.5 rounded-full"
                          style={{ background: 'var(--color-semantic-green, #34C759)' }}
                        />
                        {t('settings.skills.devWatching')}
                      </span>
                    )}
                  </div>
                  <div className="flex items-center gap-2">
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={() => handleToggleDevMode(skill.path)}
                    >
                      {devWatchPath === skill.path
                        ? t('settings.skills.devModeOff')
                        : t('settings.skills.devMode')}
                    </Button>
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={() => handlePackSkill(skill)}
                    >
                      {t('settings.skills.pack')}
                    </Button>
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={() => handleUninstall(skill.id, skill.name || skill.id)}
                    >
                      {t('settings.skills.uninstall')}
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      ) : (
        <SkillMarketplace onInstalled={loadSkills} />
      )}
    </div>
  )
}
