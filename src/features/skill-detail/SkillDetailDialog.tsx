import { useEffect, useMemo, useState } from 'react'

import { AssistantMarkdown } from '@/components/chat-scene/AssistantMarkdown'
import {
  Dialog,
  DialogBody,
  DialogBodyViewport,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import {
  getSkillAvatarNode,
  getSkillCardAvatarClass,
  getSkillIconComponent,
} from '@/components/skills/skillVisual'
import {
  isBuiltinSkill,
  isMarketSkill,
} from '@/lib/skillAvailability'
import { localizeSkill } from '@/lib/skillLocalization'
import { getSkillDetail, previewMarketplaceSkill, type MarketplaceSkillItem, type SkillInfo } from '@/lib/tauri'
import { useDevSettingsStore } from '@/stores/devSettingsStore'

import { formatSkillUpdatedAt } from './formatSkillUpdatedAt'

interface SkillDetailDialogProps {
  open: boolean
  skill?: SkillInfo | null
  marketplaceItem?: MarketplaceSkillItem | null
  installing?: boolean
  onOpenChange: (open: boolean) => void
  onInstall?: (item: MarketplaceSkillItem) => void
  onUse?: (skill: SkillInfo) => void
}

function normalizeRawSkillMarkdown(value: string): string {
  const trimmed = value.trim()
  const frontmatterMatch = /^---\n([\s\S]*?)\n---\n?([\s\S]*)$/.exec(trimmed)
  if (!frontmatterMatch) return trimmed
  const [, frontmatter, body] = frontmatterMatch
  return ['```yaml', frontmatter.trim(), '```', body.trim()].filter(Boolean).join('\n\n')
}

function getSourceLabel(skill: SkillInfo | null | undefined, item: MarketplaceSkillItem | null | undefined) {
  if (item && !skill) return '市场'
  if (!skill) return '未知'
  if (isBuiltinSkill(skill)) return '内置'
  if (isMarketSkill(skill)) return '市场'
  return '自建'
}

function getSkillTitle(skill: SkillInfo | null | undefined, item: MarketplaceSkillItem | null | undefined) {
  if (skill) return localizeSkill(skill).name
  return item?.name || item?.pluginId || '未知技能'
}

function getSkillSubtitle(skill: SkillInfo | null | undefined, item: MarketplaceSkillItem | null | undefined) {
  if (skill) return skill.displayNameEn || skill.id
  return item?.pluginId || ''
}

function getSkillDescription(skill: SkillInfo | null | undefined, item: MarketplaceSkillItem | null | undefined) {
  if (skill) return localizeSkill(skill).description || skill.shortDescription || skill.shortDescriptionEn
  return item?.description || ''
}

function getSkillVersion(skill: SkillInfo | null | undefined, item: MarketplaceSkillItem | null | undefined) {
  return skill?.version || item?.version || null
}

function getSkillCategory(skill: SkillInfo | null | undefined, item: MarketplaceSkillItem | null | undefined) {
  return skill?.category || item?.category || null
}

function getSkillTrigger(skill: SkillInfo | null | undefined, item: MarketplaceSkillItem | null | undefined) {
  return skill?.triggerText?.trim() || (skill?.id ? `/${skill.id}` : item?.pluginId ? `/${item.pluginId}` : null)
}

function getSkillUpdatedAt(skill: SkillInfo | null | undefined, item: MarketplaceSkillItem | null | undefined) {
  return formatSkillUpdatedAt(skill?.updatedAt || item?.createdAt || null)
}

function getUsageSteps(trigger: string | null, installed: boolean) {
  if (!installed) {
    return [
      '点击“安装”后，技能会进入已安装列表。',
      '安装完成后，可以在输入框里通过技能 chip 或调用命令使用。',
      '按需要补充上下文，可以继续输入任务要求或添加文件作为附件。',
    ]
  }

  return [
    '点击“使用”后，会回到对话首页并把技能 chip 放入输入框。',
    '按需要补充上下文，可以继续输入任务要求或添加文件作为附件。',
    '发送后，AI 小家会按该技能的规则处理本轮请求。',
    trigger ? `也可以在任意对话输入框手动输入 ${trigger} 加具体要求来触发。` : null,
  ].filter(Boolean) as string[]
}

function getUsageNotes(sourceLabel: string, installed: boolean) {
  const notes = [
    '技能 chip 只对当前这一轮消息生效，发送后会自动清除。',
  ]
  if (installed) {
    notes.push('关闭技能后，它仍保留在管理页和详情页，但不会进入后续对话上下文。')
  }
  if (sourceLabel === '市场') {
    notes.push('市场技能来自企业下发或可安装来源，安装后会进入已安装管理视图。')
  } else if (sourceLabel === '内置') {
    notes.push('内置技能随客户端初始化和官方更新维护。')
  } else if (sourceLabel === '自建') {
    notes.push('自建技能可以从已安装列表导出，也可以通过重新导入新版完成更新。')
  }
  return notes
}

function SkillDialogAvatar({
  skill,
  item,
}: {
  skill?: SkillInfo | null
  item?: MarketplaceSkillItem | null
}) {
  const skillId = skill?.id || item?.pluginId
  const avatarNode = getSkillAvatarNode(skillId)
  const iconName = skill?.icon || item?.icon
  const Icon = getSkillIconComponent(iconName)
  const fallbackText = Array.from(getSkillTitle(skill, item).trim())[0]?.toUpperCase() || '?'

  return (
    <div
      data-testid="skill-detail-dialog-avatar"
      className={`flex h-12 w-12 shrink-0 items-center justify-center rounded-md ${getSkillCardAvatarClass(skillId)}`}
    >
      {avatarNode ?? (
        iconName ? (
          <Icon className="h-6 w-6 text-inherit" aria-hidden />
        ) : (
          <span className="text-lg font-semibold leading-none text-inherit" aria-hidden>
            {fallbackText}
          </span>
        )
      )}
    </div>
  )
}

export function SkillDetailDialog({
  open,
  skill,
  marketplaceItem,
  installing = false,
  onOpenChange,
  onInstall,
  onUse,
}: SkillDetailDialogProps) {
  const showRawSkillContent = useDevSettingsStore((s) => s.showRawSkillContent)
  const [rawContent, setRawContent] = useState<string | null>(null)
  const [loadingDetail, setLoadingDetail] = useState(false)

  useEffect(() => {
    let cancelled = false
    if (!open || !showRawSkillContent || (!skill && !marketplaceItem)) {
      setRawContent(null)
      setLoadingDetail(false)
      return () => {
        cancelled = true
      }
    }

    setLoadingDetail(true)
    const request = skill
      ? getSkillDetail(skill.id).then((next) => next?.rawContent ?? null)
      : marketplaceItem
        ? previewMarketplaceSkill(marketplaceItem.id, marketplaceItem.pluginId).then((next) => next.rawContent)
        : Promise.resolve(null)

    void request
      .then((next) => {
        if (!cancelled) setRawContent(next)
      })
      .catch((error) => {
        console.error('Failed to load skill detail:', error)
        if (!cancelled) setRawContent(null)
      })
      .finally(() => {
        if (!cancelled) setLoadingDetail(false)
      })

    return () => {
      cancelled = true
    }
  }, [marketplaceItem, open, showRawSkillContent, skill])

  const metaItems = useMemo(() => {
    const items = [
      ['来源', getSourceLabel(skill, marketplaceItem)],
      ['版本', getSkillVersion(skill, marketplaceItem)],
      ['分类', getSkillCategory(skill, marketplaceItem)],
      ['技能 ID', skill?.id || marketplaceItem?.pluginId || null],
      ['调用命令', getSkillTrigger(skill, marketplaceItem)],
      ['更新时间', getSkillUpdatedAt(skill, marketplaceItem)],
    ]
    return items.filter(([, value]) => Boolean(value)) as Array<[string, string]>
  }, [marketplaceItem, skill])

  if (!skill && !marketplaceItem) return null

  const title = getSkillTitle(skill, marketplaceItem)
  const subtitle = getSkillSubtitle(skill, marketplaceItem)
  const description = getSkillDescription(skill, marketplaceItem)
  const trigger = getSkillTrigger(skill, marketplaceItem)
  const sourceLabel = getSourceLabel(skill, marketplaceItem)
  const usageSteps = getUsageSteps(trigger, Boolean(skill))
  const usageNotes = getUsageNotes(sourceLabel, Boolean(skill))
  const canUse = Boolean(skill && onUse)
  const canInstall = Boolean(!skill && marketplaceItem && onInstall)

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="w-[800px] max-w-[calc(100vw-32px)]"
        data-testid="skill-detail-dialog"
      >
        <DialogBodyViewport className="max-h-[min(82vh,760px)]" data-testid="skill-detail-dialog-body-viewport">
          <DialogBody className="flex min-w-0 flex-col gap-6 py-6" data-testid="skill-detail-dialog-body">
            <div className="flex min-w-0 items-center gap-3 pr-10">
              <SkillDialogAvatar skill={skill} item={marketplaceItem} />
              <div className="flex min-w-0 flex-1 flex-col">
                <DialogTitle
                  className="truncate text-lg font-semibold leading-6 text-foreground"
                  data-testid="skill-detail-dialog-title"
                >
                  {title}
                </DialogTitle>
                <DialogDescription className="sr-only">
                  {description || title}
                </DialogDescription>
                {subtitle ? (
                  <p className="mt-0.5 truncate text-sm leading-5 text-muted-foreground" data-testid="skill-detail-dialog-subtitle">
                    {subtitle}
                  </p>
                ) : null}
              </div>
            </div>

            <div
              data-testid="skill-detail-dialog-meta"
              className="flex flex-wrap items-center gap-x-5 gap-y-2 rounded-md bg-[rgba(var(--muted-rgb),0.40)] px-4 py-3 text-xs text-muted-foreground"
            >
              {metaItems.map(([label, value]) => (
                <span key={label} className="min-w-0">
                  <span className="text-[rgba(var(--muted-foreground-rgb),0.80)]">{label}</span>
                  <span className="mx-1 text-[rgba(var(--muted-foreground-rgb),0.50)]">/</span>
                  <span className="text-[rgba(var(--foreground-rgb),0.80)]">{value}</span>
                </span>
              ))}
            </div>

            {description ? (
              <p data-testid="skill-detail-dialog-description" className="text-sm leading-6 text-foreground">
                {description}
              </p>
            ) : null}

            <section className="flex flex-col gap-2" data-testid="skill-detail-dialog-usage">
              <div className="text-md font-semibold text-foreground">使用说明</div>
              <ol className="flex list-decimal flex-col gap-1.5 pl-5 text-sm text-muted-foreground">
                {usageSteps.map((step) => (
                  <li key={step}>{step}</li>
                ))}
              </ol>
            </section>

            <section className="flex flex-col gap-2" data-testid="skill-detail-dialog-notes">
              <div className="text-md font-semibold text-foreground">注意事项</div>
              <ul className="flex list-disc flex-col gap-1.5 pl-5 text-sm text-muted-foreground">
                {usageNotes.map((note) => (
                  <li key={note}>{note}</li>
                ))}
              </ul>
            </section>

            {showRawSkillContent ? (
              <section className="flex min-w-0 flex-col gap-3 rounded-md border border-border bg-card p-4" data-testid="skill-detail-dialog-raw-section">
                <div className="text-sm font-semibold text-foreground">原始技能内容</div>
                {loadingDetail ? (
                  <div className="py-4 text-center text-sm text-muted-foreground">正在加载技能详情...</div>
                ) : rawContent ? (
                  <div className="min-w-0 max-w-full overflow-x-auto rounded-md bg-[rgba(var(--muted-rgb),0.35)] p-3 [&_.assistant-markdown]:min-w-max [&_.assistant-markdown]:text-xs [&_.assistant-markdown]:leading-5">
                    <AssistantMarkdown text={normalizeRawSkillMarkdown(rawContent)} disableCodeHighlight />
                  </div>
                ) : (
                  <div className="text-sm text-muted-foreground">暂无原始技能内容。</div>
                )}
              </section>
            ) : null}
          </DialogBody>
        </DialogBodyViewport>
        <DialogFooter className="border-t border-border" data-testid="skill-detail-dialog-footer">
          {canUse && skill ? (
            <Button className="min-w-32" onClick={() => onUse?.(skill)}>
              使用
            </Button>
          ) : canInstall && marketplaceItem ? (
            <Button
              className="min-w-32"
              loading={installing}
              disabled={installing}
              onClick={() => onInstall?.(marketplaceItem)}
              aria-label={`安装 ${title}`}
            >
              {installing ? '安装中' : '安装'}
            </Button>
          ) : null}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
