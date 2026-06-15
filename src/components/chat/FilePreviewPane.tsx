import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import * as ContextMenuPrimitive from '@radix-ui/react-context-menu'
import { Download, ExternalLink, FileText, Loader2, X } from 'lucide-react'

import { AssistantMarkdown } from '@/components/chat-scene/AssistantMarkdown'
import { getFilePreview, getLocalFilePreview, openLocalFile, type FilePreview } from '@/lib/tauri'
import type { PreviewTarget } from './generatedFileActions'
import { Button } from '@/components/ui/button'

interface FilePreviewPaneProps {
  target: PreviewTarget | null
  onOpenExternal?: (target: PreviewTarget) => void
  onDownload?: (target: PreviewTarget) => void
  onClosePreview?: () => void
}

type PreviewState =
  | { status: 'success'; key: string; preview: FilePreview }
  | { status: 'error'; key: string; error: string }
  | null

export function FilePreviewPane({ target, onOpenExternal, onDownload, onClosePreview }: FilePreviewPaneProps) {
  const { t } = useTranslation()
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
            error: err instanceof Error ? err.message : t('filePreview.cannotPreview'),
          })
        }
      })

    return () => {
      if (requestIdRef.current === requestId) {
        requestIdRef.current += 1
      }
    }
  }, [targetFileId, targetConversationId, targetLocalPath, previewKey, t])

  const handleOpenExternal = useCallback(() => {
    if (!target) return
    if (target.localPath) {
      void openLocalFile(target.localPath)
      return
    }
    onOpenExternal?.(target)
  }, [target, onOpenExternal])

  const handleDownload = useCallback(() => {
    if (!target) return
    onDownload?.(target)
  }, [target, onDownload])

  if (!target) {
    return (
      <div className="flex h-full flex-1 items-center justify-center bg-muted/20 px-6 text-center">
        <p className="text-sm text-muted-foreground">{t('filePreview.selectArtifact')}</p>
      </div>
    )
  }

  const isCurrentPreviewState = previewState?.key === previewKey

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col bg-background">
      <div data-testid="file-preview-header" data-aijia-file-preview-header className="flex h-12 shrink-0 items-center justify-between gap-3 border-b border-border px-4">
        <div className="flex min-w-0 items-center gap-2">
          <FileText className="h-4 w-4 shrink-0 text-muted-foreground" />
          <h2 className="truncate text-sm font-semibold text-foreground">{target.fileName}</h2>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {onDownload && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="gap-1.5"
              onClick={handleDownload}
            >
              <Download className="h-3.5 w-3.5" />
              {t('filePreview.download')}
            </Button>
          )}
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="gap-1.5"
            onClick={handleOpenExternal}
          >
            <ExternalLink className="h-3.5 w-3.5" />
            {t('filePreview.openWithDefault')}
          </Button>
          {onClosePreview && (
            <Button
              type="button"
              variant="ghost"
              size="icon"
              aria-label="Close preview"
              onClick={onClosePreview}
            >
              <X className="h-4 w-4" />
            </Button>
          )}
        </div>
      </div>
      <div data-aijia-file-preview-body className={previewState?.status === 'success' && previewState.preview.kind === 'html'
        ? 'flex-1 overflow-auto'
        : 'flex-1 overflow-auto p-6'}
      >
        {!isCurrentPreviewState ? (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            {t('filePreview.loadingPreview')}
          </div>
        ) : previewState.status === 'error' ? (
          <div className="space-y-3 rounded-md border border-destructive/20 bg-destructive/5 p-4 text-sm text-destructive">
            <p>{previewState.error}</p>
            <Button type="button" variant="outline" size="sm" onClick={retryPreview}>
              {t('filePreview.retry')}
            </Button>
          </div>
        ) : (
          <PreviewContent
            preview={previewState.preview}
            downloadLabel={t('filePreview.download')}
            onDownload={onDownload ? handleDownload : undefined}
          />
        )}
      </div>
    </div>
  )
}

function PreviewContent({
  preview,
  downloadLabel,
  onDownload,
}: {
  preview: FilePreview
  downloadLabel: string
  onDownload?: () => void
}) {
  switch (preview.kind) {
    case 'markdown':
      return <AssistantMarkdown text={preview.content} />
    case 'html':
      return (
        <iframe
          title={preview.fileName}
          sandbox="allow-scripts"
          srcDoc={preview.content}
          className="h-full min-h-[520px] w-full bg-background"
        />
      )
    case 'image': {
      const imagePreview = (
        <div className="flex h-full min-h-[520px] items-center justify-center rounded-md bg-muted/30 p-4">
          <img
            src={preview.dataUrl}
            alt={preview.fileName}
            className="max-h-full max-w-full object-contain"
          />
        </div>
      )
      if (!onDownload) return imagePreview
      return (
        <ContextMenuPrimitive.Root>
          <ContextMenuPrimitive.Trigger asChild>{imagePreview}</ContextMenuPrimitive.Trigger>
          <ContextMenuPrimitive.Portal>
            <ContextMenuPrimitive.Content className="z-50 min-w-[10rem] overflow-hidden rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-[var(--shadow-popover)]">
              <ContextMenuPrimitive.Item
                onSelect={onDownload}
                className="flex cursor-default select-none items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-none data-[highlighted]:bg-accent data-[highlighted]:text-accent-foreground"
              >
                <Download className="h-3.5 w-3.5 shrink-0" />
                <span>{downloadLabel}</span>
              </ContextMenuPrimitive.Item>
            </ContextMenuPrimitive.Content>
          </ContextMenuPrimitive.Portal>
        </ContextMenuPrimitive.Root>
      )
    }
    case 'json':
    case 'csv':
    case 'text':
      return (
        <pre className="whitespace-pre-wrap rounded-md bg-muted p-4 text-xs leading-6 text-foreground">
          {preview.content}
        </pre>
      )
    case 'unsupported':
      return (
        <div className="rounded-md border border-border bg-muted/40 p-4 text-sm text-muted-foreground">
          {preview.reason}
        </div>
      )
  }
}
