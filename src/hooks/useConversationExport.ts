import { useEffect, useRef, useState } from 'react'

import type { ConversationExportStatus } from '@/features/chat/ConversationExportDialog'
import {
  exportConversation,
  revealExportInFolder,
  type ExportConversationResult,
} from '@/lib/tauri'
import { useNotificationStore } from '@/stores/notificationStore'

export function useConversationExport(conversationId: string) {
  const pushNotification = useNotificationStore((s) => s.push)
  const [open, setOpen] = useState(false)
  const [status, setStatus] = useState<ConversationExportStatus>('idle')
  const [progressStep, setProgressStep] = useState(0)
  const [result, setResult] = useState<ExportConversationResult | null>(null)
  const [error, setError] = useState<string | null>(null)
  const currentConversationIdRef = useRef(conversationId)
  const requestSeqRef = useRef(0)

  const openExportDialog = () => {
    if (status === 'exporting') return
    setOpen(true)
    setStatus('idle')
    setProgressStep(0)
    setResult(null)
    setError(null)
  }

  const startExport = async () => {
    if (status === 'exporting') return
    const requestSeq = requestSeqRef.current + 1
    requestSeqRef.current = requestSeq
    const requestConversationId = conversationId
    setOpen(true)
    setStatus('exporting')
    setProgressStep(0)
    setResult(null)
    setError(null)

    try {
      const exportResult = await exportConversation(requestConversationId)
      if (
        requestSeqRef.current !== requestSeq ||
        currentConversationIdRef.current !== requestConversationId
      ) {
        return
      }
      setProgressStep(2)
      setResult(exportResult)
      setStatus('success')
    } catch (err) {
      if (
        requestSeqRef.current !== requestSeq ||
        currentConversationIdRef.current !== requestConversationId
      ) {
        return
      }
      const message = err instanceof Error ? err.message : '导出失败。'
      setError(message)
      setStatus('error')
      pushNotification({
        level: 'error',
        title: '导出失败',
        message,
        actions: [],
        dismissible: true,
        context: 'toast',
      })
    }
  }

  const revealExport = async () => {
    if (!result) return
    try {
      await revealExportInFolder(result.zipPath)
    } catch (err) {
      pushNotification({
        level: 'error',
        title: '无法打开文件夹',
        message: err instanceof Error ? err.message : '打开导出文件夹失败。',
        actions: [],
        dismissible: true,
        context: 'toast',
      })
    }
  }

  useEffect(() => {
    if (status !== 'exporting') return undefined
    const timers = [
      window.setTimeout(() => setProgressStep(1), 300),
      window.setTimeout(() => setProgressStep(2), 900),
    ]
    return () => timers.forEach(window.clearTimeout)
  }, [status])

  useEffect(() => {
    currentConversationIdRef.current = conversationId
    requestSeqRef.current += 1
    setOpen(false)
    setStatus('idle')
    setProgressStep(0)
    setResult(null)
    setError(null)
  }, [conversationId])

  return {
    openExportDialog,
    dialogProps: {
      open,
      status,
      progressStep,
      result,
      error,
      onOpenChange: setOpen,
      onStart: () => void startExport(),
      onReveal: () => void revealExport(),
    },
  }
}
