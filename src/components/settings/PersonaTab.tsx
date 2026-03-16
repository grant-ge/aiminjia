/**
 * PersonaTab — persona management UI in settings modal.
 */
import { useEffect, useState } from 'react'
import { usePersonaStore } from '@/stores/personaStore'
import { Button } from '@/components/common/Button'
import { useNotificationStore } from '@/stores/notificationStore'
import type { Persona } from '@/lib/tauri'
import { getPersona } from '@/lib/tauri'

export function PersonaTab() {
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
        title: '已切换角色',
        message: `当前角色：${personas.find(p => p.id === id)?.name}`,
        actions: [],
        dismissible: true,
        autoHide: 3,
        context: 'toast',
      })
    } catch (err) {
      notifications.push({
        level: 'error',
        title: '切换失败',
        message: err instanceof Error ? err.message : '未知错误',
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
        title: '保存成功',
        message: '角色已更新',
        actions: [],
        dismissible: true,
        autoHide: 3,
        context: 'toast',
      })
    } catch (err) {
      notifications.push({
        level: 'error',
        title: '保存失败',
        message: err instanceof Error ? err.message : '未知错误',
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
        title: '无法删除',
        message: '内置角色不能删除',
        actions: [],
        dismissible: true,
        autoHide: 3,
        context: 'toast',
      })
      return
    }
    if (!confirm(`确定删除角色"${persona.name}"吗？`)) return
    try {
      await deletePersona(id)
      if (selectedId === id) setSelectedId(null)
      notifications.push({
        level: 'success',
        title: '已删除',
        message: `角色"${persona.name}"已删除`,
        actions: [],
        dismissible: true,
        autoHide: 3,
        context: 'toast',
      })
    } catch (err) {
      notifications.push({
        level: 'error',
        title: '删除失败',
        message: err instanceof Error ? err.message : '未知错误',
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
      title: '功能开发中',
      message: '导出功能即将上线',
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
      title: '功能开发中',
      message: '导入功能即将上线',
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
          选择或自定义你的工作角色,AI 会根据角色调整专业能力和记忆重点
        </p>
        <Button onClick={handleImport} variant="secondary" size="sm">
          导入角色
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
              {p.name}
            </span>
            {activePersona?.id === p.id && (
              <span className="text-xs" style={{ color: 'var(--color-accent)' }}>
                当前
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
            设为当前角色
          </Button>
          <Button onClick={() => handleEdit(selectedId)} variant="secondary" size="sm">
            编辑
          </Button>
          <Button onClick={() => handleExport(selectedId)} variant="secondary" size="sm">
            导出
          </Button>
          {!personas.find(p => p.id === selectedId)?.builtin && (
            <Button onClick={() => handleDelete(selectedId)} variant="secondary" size="sm">
              删除
            </Button>
          )}
        </div>
      )}

      {editing && editingPersona && (
        <div className="space-y-3 rounded-lg border p-4" style={{ borderColor: 'var(--color-border)' }}>
          <h3 className="font-medium" style={{ color: 'var(--color-text-primary)' }}>
            编辑角色
          </h3>
          <div className="space-y-2">
            <label className="block text-sm" style={{ color: 'var(--color-text-muted)' }}>
              名称
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
              图标 (emoji)
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
              描述
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
              角色设定 (注入系统提示词)
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
              保存
            </Button>
            <Button onClick={() => setEditing(false)} variant="secondary" size="sm">
              取消
            </Button>
          </div>
        </div>
      )}
    </div>
  )
}
