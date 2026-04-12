/**
 * WorkspaceAuthPanel — select and revoke the authorized local directory for a session.
 * Used inside SettingsModal > General tab.
 */
import { useState, useEffect } from 'react'
import {
  authorizeLocalDirectory,
  getAuthorizedWorkspace,
  revokeAuthorizedWorkspace,
  type AuthorizedWorkspaceRef,
} from '@/lib/tauri'

interface Props {
  sessionId: string
}

export function WorkspaceAuthPanel({ sessionId }: Props) {
  const [workspace, setWorkspace] = useState<AuthorizedWorkspaceRef | null>(null)
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (!sessionId) return
    getAuthorizedWorkspace(sessionId)
      .then(setWorkspace)
      .catch(console.error)
  }, [sessionId])

  async function handleSelect() {
    try {
      setLoading(true)
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({ directory: true, multiple: false })
      if (!selected || typeof selected !== 'string') return
      const ref = await authorizeLocalDirectory(selected, sessionId)
      setWorkspace(ref)
    } catch (e) {
      console.error(e)
    } finally {
      setLoading(false)
    }
  }

  async function handleRevoke() {
    try {
      await revokeAuthorizedWorkspace(sessionId)
      setWorkspace(null)
    } catch (e) {
      console.error(e)
    }
  }

  return (
    <div className="space-y-3">
      <div className="text-sm font-medium" style={{ color: 'var(--color-text-secondary)' }}>
        本地工作目录
      </div>
      {workspace ? (
        <div className="flex items-center gap-2">
          <span
            className="flex-1 truncate text-sm"
            style={{ color: 'var(--color-text-muted)' }}
            title={workspace.rootPath}
          >
            {workspace.displayName}
          </span>
          <button
            onClick={handleRevoke}
            className="shrink-0 text-xs hover:underline"
            style={{ color: 'var(--color-semantic-red)', background: 'none', border: 'none', cursor: 'pointer' }}
          >
            撤销授权
          </button>
        </div>
      ) : (
        <button
          onClick={handleSelect}
          disabled={loading || !sessionId}
          className="text-sm hover:underline disabled:opacity-50"
          style={{ color: 'var(--color-primary)', background: 'none', border: 'none', cursor: 'pointer' }}
        >
          {loading ? '正在授权...' : '选择工作目录'}
        </button>
      )}
      <p className="text-xs" style={{ color: 'var(--color-text-muted)' }}>
        授权后，AI 可以直接读取该目录中的文件进行分析，无需先上传。
      </p>
    </div>
  )
}
