import { CheckCircle2, FolderOpen, Package, XCircle } from 'lucide-react'

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import type { ExportConversationResult } from '@/lib/tauri'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'

export type ConversationExportStatus = 'idle' | 'exporting' | 'success' | 'error'

interface ConversationExportDialogProps {
  open: boolean
  status: ConversationExportStatus
  progressStep: number
  result: ExportConversationResult | null
  error: string | null
  onOpenChange: (open: boolean) => void
  onStart: () => void
  onReveal: () => void
}

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 KB'
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

function StepRow({
  done,
  active,
  label,
}: {
  done: boolean
  active: boolean
  label: string
}) {
  return (
    <div className="flex items-center gap-3 text-sm">
      <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
        {done ? (
          <CheckCircle2 className="h-4 w-4 text-primary" aria-hidden />
        ) : active ? (
          <Spinner className="text-primary" aria-hidden />
        ) : (
          <span className="h-2 w-2 rounded-md bg-muted-foreground/40" />
        )}
      </span>
      <span className={done || active ? 'text-foreground' : 'text-muted-foreground'}>{label}</span>
    </div>
  )
}

export function ConversationExportDialog({
  open,
  status,
  progressStep,
  result,
  error,
  onOpenChange,
  onStart,
  onReveal,
}: ConversationExportDialogProps) {
  const isIdle = status === 'idle'
  const isExporting = status === 'exporting'
  const isSuccess = status === 'success' && result
  const isError = status === 'error'

  return (
    <Dialog open={open} onOpenChange={isExporting ? undefined : onOpenChange}>
      <DialogContent
        className="overflow-hidden rounded-md border border-border bg-background p-0 shadow-[var(--shadow-modal)]"
        style={{
          width: 'min(28rem, calc(100vw - 2rem))',
          maxWidth: 'calc(100vw - 2rem)',
        }}
      >
        <DialogHeader className="px-6 pt-6 text-left">
          <DialogTitle>导出对话</DialogTitle>
          <DialogDescription>
            {isSuccess
              ? '已生成一个对话文件，可以在文件夹中查看。'
              : isError
                ? '导出时遇到问题，可以稍后再试。'
                : isIdle
                  ? '将生成一个本地 zip 文件，包含当前对话和最近 24 小时运行信息。文件只会保存在本机。'
                  : '正在整理当前对话内容，请稍等。'}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 px-6 py-5">
          {isSuccess ? (
            <div className="flex items-start gap-3 overflow-hidden rounded-md border border-border bg-muted/40 p-3">
              <Package className="mt-0.5 h-5 w-5 shrink-0 text-primary" aria-hidden />
              <div className="min-w-0 flex-1 overflow-hidden">
                <div
                  className="text-sm font-medium text-foreground"
                  style={{
                    maxWidth: '100%',
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                  }}
                  title={result.fileName}
                >
                  {result.fileName}
                </div>
                <div className="mt-1 text-xs text-muted-foreground">{formatBytes(result.sizeBytes)}</div>
              </div>
            </div>
          ) : isError ? (
            <div className="flex items-start gap-3 rounded-md border border-destructive/30 bg-destructive/5 p-3">
              <XCircle className="mt-0.5 h-5 w-5 shrink-0 text-destructive" aria-hidden />
              <div className="text-sm text-foreground">{error || '导出失败。'}</div>
            </div>
          ) : isIdle ? (
            <div className="flex items-start gap-3 rounded-md border border-border bg-muted/30 p-3">
              <Package className="mt-0.5 h-5 w-5 shrink-0 text-primary" aria-hidden />
              <div className="space-y-1 text-sm">
                <div className="font-medium text-foreground">准备生成对话文件</div>
                <div className="text-muted-foreground">开始后会自动整理当前对话和最近 24 小时日志。</div>
              </div>
            </div>
          ) : (
            <div className="space-y-3 rounded-md border border-border bg-muted/30 p-3">
              <StepRow done={progressStep > 0} active={progressStep === 0} label="整理对话内容" />
              <StepRow done={progressStep > 1} active={progressStep === 1} label="收集最近 24 小时运行信息" />
              <StepRow done={false} active={progressStep >= 2} label="生成文件" />
            </div>
          )}
        </div>

        <DialogFooter
          className="gap-2 border-t border-border bg-muted/20 px-6 py-4 sm:space-x-0"
          style={{ display: 'flex', flexWrap: 'wrap', justifyContent: 'flex-end' }}
        >
          {isSuccess ? (
            <>
              <Button variant="secondary" onClick={() => onOpenChange(false)}>
                完成
              </Button>
              <Button className="min-w-0" icon={<FolderOpen className="h-4 w-4" aria-hidden />} onClick={onReveal}>
                <span className="truncate">打开所在文件夹</span>
              </Button>
            </>
          ) : isError ? (
            <Button onClick={() => onOpenChange(false)}>知道了</Button>
          ) : isIdle ? (
            <>
              <Button variant="secondary" onClick={() => onOpenChange(false)}>
                取消
              </Button>
              <Button onClick={onStart}>开始导出</Button>
            </>
          ) : (
            <Button loading disabled>
              导出中
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
