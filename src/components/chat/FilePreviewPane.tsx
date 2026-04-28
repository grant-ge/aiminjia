import { ExternalLink, FileText } from 'lucide-react'

import { Button } from '@/components/ui/button'
import type { PreviewTarget } from './generatedFileActions'

interface FilePreviewPaneProps {
  target: PreviewTarget | null
  onOpenExternal?: (target: PreviewTarget) => void
}

export function FilePreviewPane({ target, onOpenExternal }: FilePreviewPaneProps) {
  if (!target) {
    return (
      <div className="flex h-full flex-1 items-center justify-center bg-muted/20 px-6 text-center">
        <p className="text-sm text-muted-foreground">选择一个产物进行预览</p>
      </div>
    )
  }

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col bg-background">
      <div className="flex items-center justify-between gap-3 border-b border-border px-4 py-3">
        <div className="flex min-w-0 items-center gap-2">
          <FileText className="h-4 w-4 shrink-0 text-muted-foreground" />
          <h2 className="truncate text-sm font-semibold text-foreground">{target.fileName}</h2>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="shrink-0 gap-1.5"
          onClick={() => onOpenExternal?.(target)}
        >
          <ExternalLink className="h-3.5 w-3.5" />
          Open with default app
        </Button>
      </div>
      <div className="flex flex-1 items-center justify-center px-6 text-center">
        <p className="text-sm text-muted-foreground">预览内容加载能力将在下一阶段接入</p>
      </div>
    </div>
  )
}
