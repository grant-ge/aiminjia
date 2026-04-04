import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/common/Button'
import { listCustomSkills, installCustomSkill, uninstallCustomSkill, initSkillTemplate, packSkill } from '@/lib/tauri'
import type { CustomSkillInfo } from '@/lib/tauri'

export function SkillsTab() {
  const { t } = useTranslation()
  const [skills, setSkills] = useState<CustomSkillInfo[]>([])
  const [loading, setLoading] = useState(true)

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

  useEffect(() => { loadSkills() }, [])

  const handleInstall = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({ directory: true, title: t('settings.skills.selectFolder') })
      if (selected) {
        const msg = await installCustomSkill(selected)
        await loadSkills()
        alert(msg)
      }
    } catch (e) {
      alert(String(e))
    }
  }

  const handleUninstall = async (id: string, name: string) => {
    if (!confirm(t('settings.skills.confirmUninstall', { name }))) return
    try {
      await uninstallCustomSkill(id)
      await loadSkills()
    } catch (e) {
      alert(String(e))
    }
  }

  const handleCreateNew = async () => {
    try {
      const skillId = prompt(t('settings.skills.skillIdPlaceholder'))
      if (!skillId) return
      const skillName = prompt(t('settings.skills.skillNamePlaceholder'))
      if (!skillName) return

      const { open } = await import('@tauri-apps/plugin-dialog')
      const targetDir = await open({ directory: true, title: t('settings.skills.selectTargetDir') })
      if (!targetDir) return

      const createdPath = await initSkillTemplate(targetDir, skillId, skillName)
      alert(t('settings.skills.created', { path: createdPath }))
    } catch (e) {
      alert(String(e))
    }
  }

  const handlePackSkill = async (skillPath: string) => {
    try {
      const outputPath = await packSkill(skillPath)
      alert(t('settings.skills.packSuccess', { path: outputPath }))
    } catch (e) {
      alert(String(e))
    }
  }

  const handlePackStandalone = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({ directory: true, title: t('settings.skills.selectFolder') })
      if (selected) {
        const outputPath = await packSkill(selected)
        alert(t('settings.skills.packSuccess', { path: outputPath }))
      }
    } catch (e) {
      alert(String(e))
    }
  }

  return (
    <div>
      <div className="mb-4 flex items-center justify-between">
        <p style={{ color: 'var(--color-text-secondary)', fontSize: '0.85rem' }}>
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
              <div>
                <div className="font-medium" style={{ color: 'var(--color-text-primary)' }}>
                  {skill.name || skill.id}
                </div>
                <div style={{ color: 'var(--color-text-secondary)', fontSize: '0.8rem' }}>
                  {skill.description || skill.id}
                </div>
              </div>
              <div className="flex gap-2">
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
  )
}
