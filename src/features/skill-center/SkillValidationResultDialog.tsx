import { Check, X } from 'lucide-react'

import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import type { SkillValidationKind } from '@/stores/skillStore'
import { Button } from '@/components/ui/button'

interface SkillValidationResultDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  failure: { kind: SkillValidationKind; detail?: string } | null
  onRetry: () => void
}

type RuleId = 'name' | 'skillMd' | 'frontmatter'

const RULES: Array<{ id: RuleId; label: string }> = [
  { id: 'name', label: '目录名合法（小写字母或数字开头；仅 a-z 0-9 - _，长度 ≤ 64）' },
  { id: 'skillMd', label: '目录根下存在 SKILL.md 文件' },
  { id: 'frontmatter', label: 'SKILL.md 的 frontmatter 可解析（含 name、description）' },
]

function failedRule(kind: SkillValidationKind): RuleId | null {
  switch (kind) {
    case 'invalidName':
      return 'name'
    case 'missingSkillMd':
      return 'skillMd'
    case 'parseFailed':
      return 'frontmatter'
    case 'io':
      return null
  }
}

function detailLine(kind: SkillValidationKind, detail?: string): string {
  switch (kind) {
    case 'invalidName':
      return detail ? `目录名 "${detail}" 不符合命名规则。` : '目录名不符合命名规则。'
    case 'missingSkillMd':
      return '所选目录下未找到 SKILL.md 文件。'
    case 'parseFailed':
      return detail ? `SKILL.md 解析失败：${detail}` : 'SKILL.md 解析失败。'
    case 'io':
      return detail ?? '读取目录时发生 IO 错误。'
  }
}

export function SkillValidationResultDialog({
  open,
  onOpenChange,
  failure,
  onRetry,
}: SkillValidationResultDialogProps) {
  if (!failure) return null
  const failed = failedRule(failure.kind)

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>技能目录不符合规范</DialogTitle>
          <DialogDescription>
            技能目录需要满足以下规则。请按提示修改后重新选择目录。
          </DialogDescription>
        </DialogHeader>

        <DialogBody data-testid="skill-validation-dialog-body" className="flex flex-col gap-4">
          <ul className="flex flex-col gap-2 text-sm">
            {RULES.map((rule) => {
              const isFailed = rule.id === failed
              return (
                <li key={rule.id} className="flex items-start gap-2">
                  {isFailed ? (
                    <X className="mt-0.5 h-4 w-4 shrink-0 text-destructive" aria-label="未通过" />
                  ) : (
                    <Check className="mt-0.5 h-4 w-4 shrink-0 text-primary" aria-label="已通过" />
                  )}
                  <span className={isFailed ? 'text-foreground' : 'text-muted-foreground'}>{rule.label}</span>
                </li>
              )
            })}
          </ul>

          <div className="rounded-md border border-destructive/40 bg-destructive/5 p-3 text-sm text-destructive">
            {detailLine(failure.kind, failure.detail)}
          </div>
        </DialogBody>

        <DialogFooter data-testid="skill-validation-dialog-footer">
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button onClick={onRetry}>
            重新选择目录
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
