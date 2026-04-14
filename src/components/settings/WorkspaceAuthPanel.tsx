/**
 * WorkspaceAuthPanel — select, replace, or revoke the authorized local
 * directory for a session. Used inside SettingsModal > General tab.
 */
import { useEffect, useState, type FormEvent } from 'react'
import {
  authorizeLocalDirectory,
  pickLocalDirectory,
  revokeAuthorizedWorkspace,
} from '@/lib/tauri'
import {
  emitAuthorizedWorkspaceChanged,
  useAuthorizedWorkspace,
} from '@/hooks/useAuthorizedWorkspace'

interface Props {
  sessionId: string
}

export function WorkspaceAuthPanel({ sessionId }: Props) {
  const { workspace, loading, refresh } = useAuthorizedWorkspace(sessionId)
  const [mutating, setMutating] = useState(false)
  const [manualPath, setManualPath] = useState('')
  const [errorMessage, setErrorMessage] = useState<string | null>(null)

  useEffect(() => {
    if (workspace?.rootPath) {
      setManualPath(workspace.rootPath)
    }
  }, [workspace?.rootPath])

  async function authorizePath(path: string, source: 'dialog' | 'manual') {
    const normalized = path.trim()
    if (!normalized) {
      setErrorMessage('请输入本地目录绝对路径。')
      return
    }
    if (!sessionId) {
      setErrorMessage('当前没有激活会话，请先进入一个聊天会话后再授权目录。')
      return
    }

    try {
      setMutating(true)
      setErrorMessage(null)
      console.info('[WorkspaceAuthPanel] authorizing workspace', {
        sessionId,
        path: normalized,
        source,
      })
      const authorized = await authorizeLocalDirectory(normalized, sessionId)
      console.info('[WorkspaceAuthPanel] workspace authorized', {
        sessionId,
        rootPath: authorized.rootPath,
        source,
      })
      setManualPath(authorized.rootPath)
      emitAuthorizedWorkspaceChanged(sessionId)
      await refresh()
    } catch (e) {
      console.error('[WorkspaceAuthPanel] Failed to authorize workspace:', e)
      const message = e instanceof Error
        ? e.message
        : '目录授权失败，请检查路径是否存在且当前应用有权限访问。'
      setErrorMessage(message)
    } finally {
      setMutating(false)
    }
  }

  async function handleSelect() {
    try {
      setErrorMessage(null)
      const selectedPath = await pickLocalDirectory({
        defaultPath: manualPath || workspace?.rootPath,
        title: '选择本地工作目录',
      })

      console.info('[WorkspaceAuthPanel] directory picker result', {
        sessionId,
        selectedPath,
      })

      if (!selectedPath) {
        setErrorMessage('未选择目录。若系统目录选择器无法确认，可直接在下方粘贴本地目录路径后授权。')
        return
      }

      await authorizePath(selectedPath, 'dialog')
    } catch (e) {
      console.error('[WorkspaceAuthPanel] Failed to open directory picker:', e)
      const message = e instanceof Error
        ? e.message
        : '打开系统目录选择器失败，请改用手动输入路径授权。'
      setErrorMessage(message)
    }
  }

  async function handleRevoke() {
    try {
      setMutating(true)
      setErrorMessage(null)
      await revokeAuthorizedWorkspace(sessionId)
      emitAuthorizedWorkspaceChanged(sessionId)
      await refresh()
      setManualPath('')
    } catch (e) {
      console.error('[WorkspaceAuthPanel] Failed to revoke workspace:', e)
      const message = e instanceof Error
        ? e.message
        : '撤销授权失败，请稍后重试。'
      setErrorMessage(message)
    } finally {
      setMutating(false)
    }
  }

  async function handleManualSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    await authorizePath(manualPath, 'manual')
  }

  const pending = loading || mutating

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
            disabled={pending}
            className="shrink-0 text-xs hover:underline"
            style={{ color: 'var(--color-semantic-red)', background: 'none', border: 'none', cursor: pending ? 'default' : 'pointer' }}
          >
            撤销授权
          </button>
        </div>
      ) : (
        <button
          onClick={handleSelect}
          disabled={pending || !sessionId}
          className="text-sm hover:underline disabled:opacity-50"
          style={{ color: 'var(--color-primary)', background: 'none', border: 'none', cursor: 'pointer' }}
        >
          {pending ? '正在授权...' : '选择工作目录'}
        </button>
      )}
      {workspace && (
        <button
          onClick={handleSelect}
          disabled={pending || !sessionId}
          className="text-sm hover:underline disabled:opacity-50"
          style={{ color: 'var(--color-primary)', background: 'none', border: 'none', cursor: 'pointer' }}
        >
          {pending ? '正在授权...' : '重新选择目录'}
        </button>
      )}
      <form className="space-y-2" onSubmit={handleManualSubmit}>
        <div
          className="rounded-lg border p-3"
          style={{
            borderColor: 'var(--color-border-secondary)',
            background: 'var(--color-bg-secondary)',
          }}
        >
          <div className="mb-2 text-xs" style={{ color: 'var(--color-text-muted)' }}>
            如果系统目录选择器无法确认，可直接粘贴本地目录绝对路径完成授权。
          </div>
          <div className="flex items-center gap-2">
            <input
              value={manualPath}
              onChange={(event) => setManualPath(event.target.value)}
              disabled={pending || !sessionId}
              placeholder="/Users/you/Documents/project"
              className="flex-1 rounded-md border px-3 py-2 text-sm"
              style={{
                borderColor: 'var(--color-border-secondary)',
                background: 'var(--color-bg-primary)',
                color: 'var(--color-text-primary)',
              }}
            />
            <button
              type="submit"
              disabled={pending || !sessionId || !manualPath.trim()}
              className="shrink-0 rounded-md px-3 py-2 text-sm disabled:opacity-50"
              style={{
                background: 'var(--color-primary)',
                color: 'white',
                border: 'none',
                cursor: pending ? 'default' : 'pointer',
              }}
            >
              {pending ? '处理中...' : workspace ? '更新授权' : '手动授权'}
            </button>
          </div>
        </div>
      </form>
      {errorMessage && (
        <p className="text-xs" style={{ color: 'var(--color-semantic-red)' }}>
          {errorMessage}
        </p>
      )}
      <p className="text-xs" style={{ color: 'var(--color-text-muted)' }}>
        授权后，AI 可以直接读取该目录中的文件进行分析，无需先上传。
      </p>
    </div>
  )
}
