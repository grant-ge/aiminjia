import { useCallback, useEffect, useRef, useState } from 'react'
import { ExternalLink, FileText, Loader2, X } from 'lucide-react'

import { AssistantMarkdown } from '@/components/chat-scene/AssistantMarkdown'
import { Button } from '@/components/ui/button'
import { getFilePreview, getLocalFilePreview, openLocalFile, type FilePreview } from '@/lib/tauri'
import type { PreviewTarget } from './generatedFileActions'

interface FilePreviewPaneProps {
  target: PreviewTarget | null
  onOpenExternal?: (target: PreviewTarget) => void
  onClosePreview?: () => void
}

type PreviewState =
  | { status: 'success'; key: string; preview: FilePreview }
  | { status: 'error'; key: string; error: string }
  | null

export function FilePreviewPane({ target, onOpenExternal, onClosePreview }: FilePreviewPaneProps) {
  const [previewState, setPreviewState] = useState<PreviewState>(null)
  const [retryToken, setRetryToken] = useState(0)
  const requestIdRef = useRef(0)

  const retryPreview = useCallback(() => {
    setRetryToken((current) => current + 1)
  }, [])

  const targetFileId = target?.fileId
  const targetConversationId = target?.conversationId
  const targetLocalPath = target?.localPath
  const previewKey = (targetLocalPath || (targetFileId && targetConversationId))
    ? `${targetConversationId ?? '-'}:${targetLocalPath ?? targetFileId}:${retryToken}`
    : null

  useEffect(() => {
    if (!previewKey) {
      requestIdRef.current += 1
      return
    }

    const requestId = requestIdRef.current + 1
    requestIdRef.current = requestId

    const promise = targetLocalPath
      ? getLocalFilePreview(targetLocalPath)
      : (targetFileId && targetConversationId
          ? getFilePreview(targetFileId, targetConversationId)
          : Promise.reject(new Error('No file selected')))

    promise
      .then((nextPreview) => {
        if (requestIdRef.current === requestId) {
          setPreviewState({ status: 'success', key: previewKey, preview: nextPreview })
        }
      })
      .catch((err: unknown) => {
        if (requestIdRef.current === requestId) {
          setPreviewState({
            status: 'error',
            key: previewKey,
            error: err instanceof Error ? err.message : '无法预览文件',
          })
        }
      })

    return () => {
      if (requestIdRef.current === requestId) {
        requestIdRef.current += 1
      }
    }
  }, [targetFileId, targetConversationId, targetLocalPath, previewKey])

  const handleOpenExternal = useCallback(() => {
    if (!target) return
    if (target.localPath) {
      void openLocalFile(target.localPath)
      return
    }
    onOpenExternal?.(target)
  }, [target, onOpenExternal])

  if (!target) {
    return (
      <div className="flex h-full flex-1 items-center justify-center bg-muted/20 px-6 text-center">
        <p className="text-sm text-muted-foreground">选择一个产物进行预览</p>
      </div>
    )
  }

  const isCurrentPreviewState = previewState?.key === previewKey

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col bg-background">
      <div data-testid="file-preview-header" className="flex items-center justify-between gap-3 border-b border-border px-4 py-2">
        <div className="flex min-w-0 items-center gap-2">
          <FileText className="h-4 w-4 shrink-0 text-muted-foreground" />
          <h2 className="truncate text-sm font-semibold text-foreground">{target.fileName}</h2>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="gap-1.5"
            onClick={handleOpenExternal}
          >
            <ExternalLink className="h-3.5 w-3.5" />
            用默认应用打开
          </Button>
          {onClosePreview && (
            <Button
              type="button"
              variant="ghost"
              size="icon"
              aria-label="Close preview"
              onClick={onClosePreview}
              className="h-8 w-8 text-muted-foreground hover:text-foreground"
            >
              <X className="h-4 w-4" />
            </Button>
          )}
        </div>
      </div>
      <div className={previewState?.status === 'success' && previewState.preview.kind === 'html'
        ? 'flex-1 overflow-auto'
        : 'flex-1 overflow-auto p-6'}
      >
        {!isCurrentPreviewState ? (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            正在加载预览
          </div>
        ) : previewState.status === 'error' ? (
          <div className="space-y-3 rounded-xl border border-destructive/20 bg-destructive/5 p-4 text-sm text-destructive">
            <p>{previewState.error}</p>
            <Button type="button" variant="outline" size="sm" onClick={retryPreview}>
              重试
            </Button>
          </div>
        ) : (
          <PreviewContent preview={previewState.preview} />
        )}
      </div>
    </div>
  )
}

function PreviewContent({ preview }: { preview: FilePreview }) {
  switch (preview.kind) {
    case 'markdown':
      return <AssistantMarkdown text={preview.content} />
    case 'html':
      return (
        <iframe
          title={preview.fileName}
          sandbox=""
          srcDoc={preview.content}
          className="h-full min-h-[520px] w-full bg-background"
        />
      )
    case 'image':
      return (
        <div className="flex h-full min-h-[520px] items-center justify-center rounded-xl bg-muted/30 p-4">
          <img
            src={preview.dataUrl}
            alt={preview.fileName}
            className="max-h-full max-w-full object-contain"
          />
        </div>
      )
    case 'json':
    case 'csv':
    case 'text':
      return (
        <pre className="whitespace-pre-wrap rounded-xl bg-muted p-4 text-xs leading-6 text-foreground">
          {preview.content}
        </pre>
      )
    case 'unsupported':
      return (
        <div className="rounded-xl border border-border bg-muted/40 p-4 text-sm text-muted-foreground">
          {preview.reason}
        </div>
      )
  }
}
