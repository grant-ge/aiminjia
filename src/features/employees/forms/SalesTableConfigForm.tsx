import { Button } from '@/components/ui/button'

interface SalesTableConfigFormProps {
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  initial: Record<string, unknown>
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  onSubmit: (next: Record<string, unknown>) => void
  onCancel: () => void
}

export function SalesTableConfigForm({ onCancel }: SalesTableConfigFormProps) {
  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm leading-relaxed text-muted-foreground">
        小销需要绑定钉钉 AI 表格作为客户库，并配置字段映射（owner / stage / last_contact / next_action 等）。
        该配置流程将在后续版本中提供。
      </p>
      <p className="text-xs text-muted-foreground/70">
        在此之前，小销将以"未配置"状态存在于卡片栏，派活时会提示需要先完成配置。
      </p>
      <div className="flex items-center justify-end pt-2">
        <Button variant="ghost" onClick={onCancel}>
          关闭
        </Button>
      </div>
    </div>
  )
}
