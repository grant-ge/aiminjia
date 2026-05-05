import { useState } from 'react'

import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'

interface CronEditDialogProps {
  open: boolean
  initial: string | null
  onSubmit: (cron: string | null) => void
  onCancel: () => void
}

const PRESETS: { label: string; cron: string }[] = [
  { label: '每个工作日 09:00', cron: '0 9 * * 1-5' },
  { label: '每个工作日 08:30', cron: '30 8 * * 1-5' },
  { label: '每周一 09:00', cron: '0 9 * * 1' },
  { label: '每天 09:00', cron: '0 9 * * *' },
  { label: '每天 21:00', cron: '0 21 * * *' },
]

export function CronEditDialog({ open, initial, onSubmit, onCancel }: CronEditDialogProps) {
  const [value, setValue] = useState<string>(initial ?? '')

  return (
    <Dialog open={open} onOpenChange={(o) => { if (!o) onCancel() }}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="text-base">修改触发时间</DialogTitle>
        </DialogHeader>

        <div className="flex flex-col gap-3">
          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-medium text-muted-foreground">常用预设</label>
            <div className="flex flex-wrap gap-1.5">
              {PRESETS.map((p) => (
                <button
                  key={p.cron}
                  type="button"
                  onClick={() => setValue(p.cron)}
                  className={`rounded-md px-2 py-1 text-xs ${
                    value === p.cron
                      ? 'bg-primary text-primary-foreground'
                      : 'bg-accent hover:bg-accent/80'
                  }`}
                >
                  {p.label}
                </button>
              ))}
            </div>
          </div>

          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-medium text-muted-foreground">cron 表达式（高级）</label>
            <Input
              value={value}
              onChange={(e) => setValue(e.target.value)}
              placeholder="0 9 * * 1-5"
              className="font-mono text-xs"
            />
            <p className="text-xs text-muted-foreground/70">
              5 字段：分钟 小时 日 月 周。留空则关闭定时（保留员工，可手动派活）。
            </p>
          </div>

          <div className="flex items-center justify-between pt-2">
            <Button variant="ghost" onClick={() => onSubmit(null)} className="text-destructive">
              清除定时
            </Button>
            <div className="flex items-center gap-2">
              <Button variant="ghost" onClick={onCancel}>取消</Button>
              <Button onClick={() => onSubmit(value.trim() || null)}>保存</Button>
            </div>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
