/**
 * 通用 "即将上线" 占位面板
 */
import { Sparkles } from 'lucide-react'

interface PlaceholderPanelProps {
  title: string
}

export function PlaceholderPanel({ title }: PlaceholderPanelProps) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 rounded-[14px] bg-secondary py-12 text-center">
      <Sparkles className="h-5 w-5 text-muted-foreground" />
      <div className="text-sm font-semibold text-foreground">{title} · 即将上线</div>
      <div className="max-w-[420px] text-[13px] text-muted-foreground">
        当前版本暂未提供该模块的可视化配置入口，下个迭代会按 design.pen 完整接入。
      </div>
    </div>
  )
}
