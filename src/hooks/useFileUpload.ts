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
/** Maximum number of files per conversation. */
const MAX_FILES = 10
/** Maximum total file size in bytes (50 MB). */
const MAX_TOTAL_SIZE = 50 * 1024 * 1024

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
  /**
   * Open the native file dialog (supports multiple selection),
   * upload each file, and return metadata array.
   *
   * @param existingFiles - already pending files (for count/size limit checks)
   */
  const selectAndUploadFiles = useCallback(async (
    existingFiles: UploadedFile[] = [],
  ): Promise<UploadedFile[]> => {
    let conversationId = useChatStore.getState().activeConversationId

    // Auto-create a conversation if none is active
    if (!conversationId) {
      try {
        const { createConversation } = await import('@/lib/tauri')
        const newId = await createConversation()
        const now = new Date().toISOString()
        const store = useChatStore.getState()
        store.setConversations([
          { id: newId, title: '新对话', createdAt: now, updatedAt: now, isArchived: false },
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
        return []
      }
    }

    // Check file count limit before opening dialog
    const remainingSlots = MAX_FILES - existingFiles.length
    if (remainingSlots <= 0) {
      notifications.push({
        level: 'warning',
        title: 'File Limit Reached',
        message: `Maximum ${MAX_FILES} files per conversation.`,
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
      return []
    }

    setIsUploading(true)

    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({
        multiple: true,
        filters: [{
          name: DIALOG_FILTER_NAME,
          extensions: ALLOWED_EXTENSIONS,
        }],
      })

      if (!selected) return []

      // Normalize to array (single selection returns string, multiple returns string[])
      const paths = Array.isArray(selected) ? selected : [selected]
      if (paths.length === 0) return []

      // Enforce file count limit
      const pathsToUpload = paths.slice(0, remainingSlots)

      const { uploadFile } = await import('@/lib/tauri')
      const uploaded: UploadedFile[] = []
      let totalSize = existingFiles.reduce((sum, f) => sum + f.fileSize, 0)

      for (const filePath of pathsToUpload) {
        try {
          const result = await uploadFile(filePath, conversationId)

          // Check total size limit
          totalSize += result.fileSize
          if (totalSize > MAX_TOTAL_SIZE) {
            notifications.push({
              level: 'warning',
              title: 'Size Limit Exceeded',
              message: 'Total file size exceeds 50 MB. Some files were skipped.',
              actions: [],
              dismissible: true,
              autoHide: 4,
              context: 'toast',
            })
            break
          }

          const fileName = filePath.split('/').pop() ?? filePath
          const fileType = detectFileType(filePath)
          uploaded.push({ id: result.fileId, fileName, fileType, fileSize: result.fileSize })
        } catch (err) {
          console.error(`[useFileUpload] Failed to upload ${filePath}:`, err)
          // Continue with remaining files
        }
      }

      return uploaded
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

      return []
    } finally {
      setIsUploading(false)
    }
  }, [notifications])

  return {
    isUploading,
    selectAndUploadFiles,
  } as const
}
