import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Switch } from '@/components/common/Switch'
import { useDevSettingsStore } from '@/stores/devSettingsStore'

interface DevControlPanelProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function DevControlPanel({ open, onOpenChange }: DevControlPanelProps) {
  const showToolErrorIcon = useDevSettingsStore((s) => s.showToolErrorIcon)
  const setShowToolErrorIcon = useDevSettingsStore((s) => s.setShowToolErrorIcon)
  const showRawSkillContent = useDevSettingsStore((s) => s.showRawSkillContent)
  const setShowRawSkillContent = useDevSettingsStore((s) => s.setShowRawSkillContent)

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="!flex h-[90vh] w-[90vh] max-h-[calc(100vh-32px)] max-w-[calc(100vw-32px)] flex-col"
        onInteractOutside={(event) => event.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle>控制面板</DialogTitle>
          <DialogDescription>隐藏功能和高级操作入口，不会出现在常规设置中。</DialogDescription>
        </DialogHeader>
        <div className="mt-6 flex-1 overflow-y-auto">
          <div className="space-y-8">
            <section aria-labelledby="dev-control-display-title" className="space-y-3">
              <h3 id="dev-control-display-title" className="text-xs font-medium text-muted-foreground">
                显示
              </h3>
              <div className="divide-y divide-border/70 border-y border-border/70">
                <div className="flex items-center justify-between gap-4 py-4">
                  <div className="min-w-0 pr-4">
                    <div className="text-sm font-medium text-foreground">显示工具失败图标</div>
                    <p className="mt-1 text-xs leading-5 text-muted-foreground">
                      关闭后，工具摘要和展开行都不显示失败诊断；开启后用于排查问题。
                    </p>
                  </div>
                  <Switch
                    aria-label="显示工具失败图标"
                    checked={showToolErrorIcon}
                    onCheckedChange={setShowToolErrorIcon}
                  />
                </div>
                <div className="flex items-center justify-between gap-4 py-4">
                  <div className="min-w-0 pr-4">
                    <div className="text-sm font-medium text-foreground">显示技能原始内容</div>
                    <p className="mt-1 text-xs leading-5 text-muted-foreground">
                      开启后，技能详情页会显示完整 SKILL.md 原文和调试参数；默认隐藏给普通使用视图。
                    </p>
                  </div>
                  <Switch
                    aria-label="显示技能原始内容"
                    checked={showRawSkillContent}
                    onCheckedChange={setShowRawSkillContent}
                  />
                </div>
              </div>
            </section>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
