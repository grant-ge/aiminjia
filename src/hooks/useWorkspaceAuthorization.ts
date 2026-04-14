/**
 * useWorkspaceAuthorization — Connect a local directory to the current chat
 * session without copying files into the Lotus workspace.
 */
import { useCallback, useState } from 'react'
import { useNotificationStore } from '@/stores/notificationStore'
import { useChatStore } from '@/stores/chatStore'
import { emitAuthorizedWorkspaceChanged } from '@/hooks/useAuthorizedWorkspace'
import type { AuthorizedWorkspaceRef } from '@/lib/tauri'

export function useWorkspaceAuthorization() {
  const [isAuthorizingDirectory, setIsAuthorizingDirectory] = useState(false)
  const notifications = useNotificationStore()

  const selectAndAuthorizeDirectory = useCallback(async (
    defaultPath?: string,
  ): Promise<AuthorizedWorkspaceRef | null> => {
    let conversationId = useChatStore.getState().activeConversationId

    if (!conversationId) {
      try {
        const { createConversation } = await import('@/lib/tauri')
        const newId = await createConversation()
        const now = new Date().toISOString()
        const store = useChatStore.getState()
        store.setConversations([
          { id: newId, title: 'New Conversation', createdAt: now, updatedAt: now, isArchived: false },
          ...store.conversations,
        ])
        store.setActiveConversation(newId)
        store.setMessages([])
        conversationId = newId
      } catch (err) {
        console.error('[useWorkspaceAuthorization] Failed to create conversation:', err)
        notifications.push({
          level: 'error',
          title: '连接目录失败',
          message: '无法为目录授权创建会话。',
          actions: [],
          dismissible: true,
          autoHide: 6,
          context: 'toast',
        })
        return null
      }
    }

    setIsAuthorizingDirectory(true)

    try {
      const { pickLocalDirectory, authorizeLocalDirectory } = await import('@/lib/tauri')
      const selectedPath = await pickLocalDirectory({
        defaultPath,
        title: '连接本地目录',
      })
      if (!selectedPath) return null

      const authorized = await authorizeLocalDirectory(selectedPath, conversationId)
      emitAuthorizedWorkspaceChanged(conversationId)

      notifications.push({
        level: 'success',
        title: '已连接本地目录',
        message: `AI 现在会直接读取 ${authorized.displayName}，不会先复制到工作目录。`,
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })

      return authorized
    } catch (err) {
      console.error('[useWorkspaceAuthorization] Authorization failed:', err)
      notifications.push({
        level: 'error',
        title: '连接目录失败',
        message: err instanceof Error ? err.message : '连接本地目录时发生未知错误。',
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
      return null
    } finally {
      setIsAuthorizingDirectory(false)
    }
  }, [notifications])

  return {
    isAuthorizingDirectory,
    selectAndAuthorizeDirectory,
  } as const
}
