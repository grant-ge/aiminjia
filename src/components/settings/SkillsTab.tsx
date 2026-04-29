import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/common/Button'
import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import {
  listCustomSkills, uninstallCustomSkill, initSkillTemplate, packSkill,
  reloadSkill, startSkillWatch, stopSkillWatch, TAURI_EVENTS,
} from '@/lib/tauri'
import { listen } from '@tauri-apps/api/event'
import type { CustomSkillInfo } from '@/lib/tauri'
import { message } from '@tauri-apps/plugin-dialog'
import { useNotificationStore } from '@/stores/notificationStore'
import { useAuthStore } from '@/stores/authStore'
import { useSkillStore } from '@/stores/skillStore'
import { uploadWithOverwriteConfirm } from '@/features/skill-center/uploadWithOverwriteConfirm'
import { SkillMarketplace } from './SkillMarketplace'

type SubTab = 'installed' | 'marketplace'

interface SkillsTabProps {
  onRequestClose?: () => void
}

export function SkillsTab(_props: SkillsTabProps = {}) {
  const { t } = useTranslation()
  const isLoggedIn = useAuthStore((s) => s.isLoggedIn)
  const [subTab, setSubTab] = useState<SubTab>('installed')
  const [skills, setSkills] = useState<CustomSkillInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [devWatchPath, setDevWatchPath] = useState<string | null>(null)
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
      const selected = await open({ directory: true, title: t('settings.skills.selectFolder') })
      if (!selected || Array.isArray(selected)) return

      const result = await uploadWithOverwriteConfirm((force) =>
        useSkillStore.getState().upload(selected, force),
      )
      if (result === 'installed') {
        await loadSkills()
        pushNotification({
          level: 'success',
          title: '技能已安装',
          message: '',
          actions: [],
          dismissible: true,
          autoHide: 4,
          context: 'toast',
        })
      }
      // 'cancelled' — silent
    } catch (e) {
      const errStr = String(e)
      if (errStr.startsWith('conflict:')) {
        const skillId = errStr.slice(9)
        const overwrite = await ask(
          t('settings.skills.conflictMessage', { name: skillId }),
          { title: 'AI小家', kind: 'warning' }
        )
        if (overwrite) {
          try {
            await uninstallCustomSkill(skillId)
            const { open: reOpen } = await import('@tauri-apps/plugin-dialog')
            const reSelected = await reOpen({ directory: true, title: t('settings.skills.selectFolder') })
            if (reSelected) {
              const msg = await installCustomSkill(reSelected)
              await loadSkills()
              await message(msg, { title: 'AI小家' })
            }
          } catch (e2) {
            await message(String(e2), { title: 'AI小家', kind: 'error' })
          }
        }
      } else {
        await message(errStr, { title: 'AI小家', kind: 'error' })
      }
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
      await uninstallCustomSkill(id)
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

  const handlePackSkill = async (skillPath: string) => {
    try {
      const outputPath = await packSkill(skillPath)
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
      if (selected) {
        const outputPath = await packSkill(selected)
        await message(t('settings.skills.packSuccess', { path: outputPath }), { title: 'AI小家' })
      }
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
            <div className="flex gap-2">
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
              className="rounded-lg border p-6 text-center"
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
                  className="flex items-center justify-between rounded-lg border px-4 py-3"
                  style={{ borderColor: 'var(--color-border)', background: 'var(--color-bg-main)' }}
                >
                  <div className="flex items-center gap-2">
                    <div>
                      <div className="font-medium" style={{ color: 'var(--color-text-primary)' }}>
                        {skill.name || skill.id}
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
                  <div className="flex gap-2">
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
                      onClick={() => handlePackSkill(skill.path)}
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
