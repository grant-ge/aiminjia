/**
 * PersonaTab — persona management UI in settings modal.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { usePersonaStore } from '@/stores/personaStore'
import { Button } from '@/components/common/Button'
import { useNotificationStore } from '@/stores/notificationStore'
import type { Persona } from '@/lib/tauri'
import { getPersona } from '@/lib/tauri'

export function PersonaTab() {
  const { t } = useTranslation()
  const { personas, activePersona, reload, setActive, save: savePersona, delete: deletePersona } = usePersonaStore()
  const notifications = useNotificationStore()
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [editing, setEditing] = useState(false)
  const [editingPersona, setEditingPersona] = useState<Persona | null>(null)

  useEffect(() => {
    reload()
  }, [reload])

  const handleSelect = (id: string) => {
    setSelectedId(id)
    setEditing(false)
  }

  const handleSetActive = async (id: string) => {
    try {
      await setActive(id)
      notifications.push({
        level: 'success',
        title: t('persona.switched'),
        message: t('persona.currentPersona', { name: personas.find(p => p.id === id)?.name }),
        actions: [],
        dismissible: true,
        autoHide: 3,
        context: 'toast',
      })
    } catch (err) {
      notifications.push({
        level: 'error',
        title: t('persona.switchFailed'),
        message: err instanceof Error ? err.message : t('common.unknownError'),
        actions: [],
        dismissible: true,
        autoHide: 5,
        context: 'toast',
      })
    }
  }

  const handleEdit = async (id: string) => {
    try {
      const fullPersona = await getPersona(id)
      setEditingPersona(fullPersona)
      setEditing(true)
    } catch (err) {
      console.error('Failed to load persona:', err)
    }
  }

  const handleSave = async () => {
    if (!editingPersona) return
    try {
      await savePersona(editingPersona)
      setEditing(false)
      setEditingPersona(null)
      notifications.push({
        level: 'success',
        title: t('persona.saveSuccess'),
        message: t('persona.personaUpdated'),
        actions: [],
        dismissible: true,
        autoHide: 3,
        context: 'toast',
      })
    } catch (err) {
      notifications.push({
        level: 'error',
        title: t('persona.saveFailed'),
        message: err instanceof Error ? err.message : t('common.unknownError'),
        actions: [],
        dismissible: true,
        autoHide: 5,
        context: 'toast',
      })
    }
  }

  const handleDelete = async (id: string) => {
    const persona = personas.find(p => p.id === id)
    if (!persona) return
    if (persona.builtin) {
      notifications.push({
        level: 'error',
        title: t('persona.cannotDelete'),
        message: t('persona.builtinCannotDelete'),
        actions: [],
        dismissible: true,
        autoHide: 3,
        context: 'toast',
      })
      return
    }
    if (!confirm(t('persona.confirmDelete', { name: persona.name }))) return
    try {
      await deletePersona(id)
      if (selectedId === id) setSelectedId(null)
      notifications.push({
        level: 'success',
        title: t('persona.deleted'),
        message: t('persona.personaDeleted', { name: persona.name }),
        actions: [],
        dismissible: true,
        autoHide: 3,
        context: 'toast',
      })
    } catch (err) {
      notifications.push({
        level: 'error',
        title: t('persona.deleteFailed'),
        message: err instanceof Error ? err.message : t('common.unknownError'),
        actions: [],
        dismissible: true,
        autoHide: 5,
        context: 'toast',
      })
    }
  }

  const handleExport = async (_id: string) => {
    // TODO: Implement export (requires plugin-fs or clipboard API)
    notifications.push({
      level: 'info',
      title: t('persona.featureInDev'),
      message: t('persona.exportComingSoon'),
      actions: [],
      dismissible: true,
      autoHide: 3,
      context: 'toast',
    })
  }

  const handleImport = async () => {
    // TODO: Implement import (requires plugin-fs or clipboard API)
    notifications.push({
      level: 'info',
      title: t('persona.featureInDev'),
      message: t('persona.importComingSoon'),
      actions: [],
      dismissible: true,
      autoHide: 3,
      context: 'toast',
    })
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <p className="text-sm" style={{ color: 'var(--color-text-muted)' }}>
          {t('persona.description')}
        </p>
        <Button onClick={handleImport} variant="secondary" size="sm">
          {t('persona.import')}
        </Button>
      </div>

      <div className="grid grid-cols-4 gap-3">
        {personas.map((p) => (
          <button
            key={p.id}
            type="button"
            className="flex flex-col items-center gap-2 rounded-lg p-3 text-center transition-all"
            style={{
              background: selectedId === p.id ? 'var(--color-bg-elevated)' : 'transparent',
              border: `1px solid ${selectedId === p.id ? 'var(--color-accent)' : 'var(--color-border-subtle)'}`,
            }}
            onClick={() => handleSelect(p.id)}
          >
            <span className="text-2xl">{p.icon}</span>
            <span className="text-xs font-medium" style={{ color: 'var(--color-text-primary)' }}>
              {t(`personas.${p.id}`, p.name)}
            </span>
            {activePersona?.id === p.id && (
              <span className="text-xs" style={{ color: 'var(--color-accent)' }}>
                {t('persona.current')}
              </span>
            )}
          </button>
        ))}
      </div>

      {selectedId && (
        <div className="flex gap-2">
          <Button
            onClick={() => handleSetActive(selectedId)}
            disabled={activePersona?.id === selectedId}
            size="sm"
          >
            {t('persona.setAsCurrent')}
          </Button>
          <Button onClick={() => handleEdit(selectedId)} variant="secondary" size="sm">
            {t('common.edit')}
          </Button>
          <Button onClick={() => handleExport(selectedId)} variant="secondary" size="sm">
            {t('common.export')}
          </Button>
          {!personas.find(p => p.id === selectedId)?.builtin && (
            <Button onClick={() => handleDelete(selectedId)} variant="secondary" size="sm">
              {t('common.delete')}
            </Button>
          )}
        </div>
      )}

      {editing && editingPersona && (
        <div className="space-y-3 rounded-lg border p-4" style={{ borderColor: 'var(--color-border)' }}>
          <h3 className="font-medium" style={{ color: 'var(--color-text-primary)' }}>
            {t('persona.editPersona')}
          </h3>
          <div className="space-y-2">
            <label className="block text-sm" style={{ color: 'var(--color-text-muted)' }}>
              {t('persona.name')}
              <input
                type="text"
                value={editingPersona.name}
                onChange={(e) => setEditingPersona({ ...editingPersona, name: e.target.value })}
                className="mt-1 w-full rounded border px-3 py-2"
                style={{
                  background: 'var(--color-bg-elevated)',
                  borderColor: 'var(--color-border)',
                  color: 'var(--color-text-primary)',
                }}
              />
            </label>
            <label className="block text-sm" style={{ color: 'var(--color-text-muted)' }}>
              {t('persona.icon')}
              <input
                type="text"
                value={editingPersona.icon}
                onChange={(e) => setEditingPersona({ ...editingPersona, icon: e.target.value })}
                className="mt-1 w-full rounded border px-3 py-2"
                style={{
                  background: 'var(--color-bg-elevated)',
                  borderColor: 'var(--color-border)',
                  color: 'var(--color-text-primary)',
                }}
              />
            </label>
            <label className="block text-sm" style={{ color: 'var(--color-text-muted)' }}>
              {t('persona.descriptionLabel')}
              <textarea
                value={editingPersona.description}
                onChange={(e) => setEditingPersona({ ...editingPersona, description: e.target.value })}
                rows={2}
                className="mt-1 w-full rounded border px-3 py-2"
                style={{
                  background: 'var(--color-bg-elevated)',
                  borderColor: 'var(--color-border)',
                  color: 'var(--color-text-primary)',
                }}
              />
            </label>
            <label className="block text-sm" style={{ color: 'var(--color-text-muted)' }}>
              {t('persona.identity')}
              <textarea
                value={editingPersona.identity}
                onChange={(e) => setEditingPersona({ ...editingPersona, identity: e.target.value })}
                rows={3}
                className="mt-1 w-full rounded border px-3 py-2"
                style={{
                  background: 'var(--color-bg-elevated)',
                  borderColor: 'var(--color-border)',
                  color: 'var(--color-text-primary)',
                }}
              />
            </label>
          </div>
          <div className="flex gap-2">
            <Button onClick={handleSave} size="sm">
              {t('common.save')}
            </Button>
            <Button onClick={() => setEditing(false)} variant="secondary" size="sm">
              {t('common.cancel')}
            </Button>
          </div>
        </div>
      )}
    </div>
  )
}
