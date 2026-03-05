/**
 * useFileUpload — Handles file selection and upload through Tauri.
 *
 * Uses the Tauri dialog plugin (`@tauri-apps/plugin-dialog`) to open a native
 * file picker and then calls the `uploadFile` IPC command to upload the
 * selected file to the backend workspace.
 */
import { useState, useCallback } from 'react'
import { useNotificationStore } from '@/stores/notificationStore'
import { useChatStore } from '@/stores/chatStore'
import type { FileAttachment } from '@/types/message'

/** File types the upload dialog will accept. */
const ALLOWED_EXTENSIONS = ['xlsx', 'xls', 'csv', 'pdf', 'docx', 'doc', 'json']

/** Human-readable filter label shown in the native dialog. */
const DIALOG_FILTER_NAME = 'Supported Files'

/** Detect FileAttachment.fileType from file extension. */
function detectFileType(path: string): FileAttachment['fileType'] {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  switch (ext) {
    case 'xlsx': case 'xls': return 'excel'
    case 'docx': case 'doc': return 'word'
    case 'pdf': return 'pdf'
    case 'json': return 'json'
    default: return 'csv'
  }
}

/** Upload result returned to the caller. */
export interface UploadedFile {
  id: string
  fileName: string
  fileType: FileAttachment['fileType']
  fileSize: number
}

/**
 * Provides file-upload state and actions.
 */
export function useFileUpload() {
  const [isUploading, setIsUploading] = useState(false)
  const notifications = useNotificationStore()

  /**
   * Open the native file dialog, upload the file, and return metadata.
   *
   * If no active conversation exists, one is created automatically.
   */
  const selectAndUploadFile = useCallback(async (): Promise<UploadedFile | null> => {
    let conversationId = useChatStore.getState().activeConversationId

    // Auto-create a conversation if none is active
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
        console.error('[useFileUpload] Failed to create conversation:', err)
        notifications.push({
          level: 'error',
          title: 'Upload Failed',
          message: 'Could not create a conversation for the upload.',
          actions: [],
          dismissible: true,
          autoHide: 6,
          context: 'toast',
        })
        return null
      }
    }

    setIsUploading(true)

    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const filePath = await open({
        multiple: false,
        filters: [{
          name: DIALOG_FILTER_NAME,
          extensions: ALLOWED_EXTENSIONS,
        }],
      })

      if (!filePath || Array.isArray(filePath)) return null

      const { uploadFile } = await import('@/lib/tauri')
      const result = await uploadFile(filePath, conversationId)

      const fileName = filePath.split('/').pop() ?? filePath
      const fileType = detectFileType(filePath)

      return { id: result.fileId, fileName, fileType, fileSize: result.fileSize }
    } catch (err) {
      console.error('[useFileUpload] Upload failed:', err)

      notifications.push({
        level: 'error',
        title: 'Upload Failed',
        message: err instanceof Error ? err.message : 'An unknown error occurred during file upload.',
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })

      return null
    } finally {
      setIsUploading(false)
    }
  }, [notifications])

  return {
    isUploading,
    selectAndUploadFile,
  } as const
}
