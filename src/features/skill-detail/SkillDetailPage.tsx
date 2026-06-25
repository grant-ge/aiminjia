import { ArrowLeft } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'

import { AssistantMarkdown } from '@/components/chat-scene/AssistantMarkdown'
import { SegmentedControl } from '@/components/common/SegmentedControl'
import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { SkillActionBar } from '@/components/skills/SkillActionBar'
import { SkillDetailHero } from '@/components/skills/SkillDetailHero'
import { SkillMetaRow } from '@/components/skills/SkillMetaRow'
import { SkillUsageBlock } from '@/components/skills/SkillUsageBlock'
import { useDevSettingsStore } from '@/stores/devSettingsStore'
import { localizeSkill } from '@/lib/skillLocalization'
import {
  canToggleSkillEnablement,
  isBuiltinSkill,
  isMarketSkill,
  isSkillEnabled,
} from '@/lib/skillAvailability'
import { getSkillDetail, type SkillDetailInfo } from '@/lib/tauri'
import { useNotificationStore } from '@/stores/notificationStore'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

import { formatSkillUpdatedAt } from './formatSkillUpdatedAt'
import { Button } from '@/components/ui/button'

interface SkillDetailPageProps {
  skillId: string
}

const TOGGLE_OPTIONS: Array<{ value: 'off' | 'on'; label: string }> = [
  { value: 'off', label: '关' },
  { value: 'on', label: '开' },
]

interface BodySection {
  title: string
  body: string
}

interface PreviewSection {
  title: string
  body: string
}

function compactText(value: string | null | undefined): string {
  return (value ?? '').replace(/\s+/g, ' ').trim()
}

function normalizeRawSkillMarkdown(value: string): string {
  const trimmed = value.trim()
  const frontmatterMatch = /^---\n([\s\S]*?)\n---\n?([\s\S]*)$/.exec(trimmed)
  if (!frontmatterMatch) return trimmed
  const [, frontmatter, body] = frontmatterMatch
  return ['```yaml', frontmatter.trim(), '```', body.trim()].filter(Boolean).join('\n\n')
}

function stripMarkdownNoise(value: string): string {
  return value
    .replace(/```[\s\S]*?```/g, '')
    .replace(/^\s{0,3}[-*+]\s+/gm, '')
    .replace(/^\s{0,3}\d+\.\s+/gm, '')
    .replace(/\*\*(.*?)\*\*/g, '$1')
    .replace(/`([^`]+)`/g, '$1')
    .trim()
}

function previewLines(value: string, maxLines = 3): string {
  const lines = stripMarkdownNoise(value)
    .split('\n')
    .map((line) => compactText(line))
    .filter(Boolean)
  const visible = lines.slice(0, maxLines)
  if (lines.length > maxLines && visible.length > 0) {
    const lastIndex = visible.length - 1
    visible[lastIndex] = `${visible[lastIndex]}...`
  }
  return visible.join(' ')
}

function userFacingTitle(title: string): string | null {
  if (/(输入|前置|准备|资料|上下文)/.test(title)) return '开始前准备'
  if (/(执行|步骤|流程|工作流|Step|操作)/i.test(title)) return '工作方式'
  if (/(输出|交付|结果|产出|导出|格式)/.test(title)) return '交付结果'
  if (/(规则|原则|限制|边界|注意|检查|质量|校验)/.test(title)) return '注意事项'
  return null
}

function parseBodySections(body: string): BodySection[] {
  const sections: BodySection[] = []
  const headingPattern = /^#{2,3}\s+(.+)$/gm
  const matches = [...body.matchAll(headingPattern)]
  const usedTitles = new Set<string>()

  matches.forEach((match, index) => {
    const title = userFacingTitle(compactText(match[1]))
    const start = (match.index ?? 0) + match[0].length
    const end = matches[index + 1]?.index ?? body.length
    const sectionBody = previewLines(body.slice(start, end))
    if (title && sectionBody && !usedTitles.has(title)) {
      usedTitles.add(title)
      sections.push({ title, body: sectionBody })
    }
  })

  return sections.slice(0, 4)
}

function buildPreviewSections(detail: SkillDetailInfo): PreviewSection[] {
  const bodySections = parseBodySections(detail.body)
  const sections: PreviewSection[] = []
  if (detail.argumentHint) {
    sections.push({ title: '开始前准备', body: compactText(detail.argumentHint) })
  }
  bodySections.forEach((section) => {
    if (!sections.some((item) => item.title === section.title)) {
      sections.push(section)
    }
  })
  return sections.slice(0, 4)
}

function SkillSpecificDetails({ detail }: { detail: SkillDetailInfo | null }) {
  const showRawSkillContent = useDevSettingsStore((s) => s.showRawSkillContent)
  const sections = useMemo(() => (detail ? buildPreviewSections(detail) : []), [detail])
  const capabilityItems = [
    detail?.context ? `上下文: ${detail.context}` : null,
    detail?.model ? `模型: ${detail.model}` : null,
    detail?.effort ? `推理强度: ${detail.effort}` : null,
    detail?.userInvocable === false ? '不可直接调用' : '可直接调用',
    detail?.disableModelInvocation ? '禁用模型调用' : null,
  ].filter(Boolean) as string[]

  if (!detail) return null

  return (
    <section className="flex w-full flex-col gap-4">
      <div className="grid w-full grid-cols-1 gap-4 lg:grid-cols-[minmax(0,1fr)_360px]">
        {detail.whenToUse ? (
          <section className="flex min-w-0 flex-col gap-2 rounded-md border border-border bg-card p-4">
            <div className="text-sm font-semibold text-foreground">适用场景</div>
            <p className="text-xs leading-5 text-muted-foreground">{compactText(detail.whenToUse)}</p>
          </section>
        ) : null}

        {sections.length > 0 ? (
          <section className="flex min-w-0 flex-col gap-3 rounded-md border border-border bg-card p-4">
            <div className="text-sm font-semibold text-foreground">技能说明</div>
            <div className="grid gap-3 md:grid-cols-2">
              {sections.map((section) => (
                <div key={section.title} className="flex min-w-0 flex-col gap-1.5 rounded-md bg-muted/40 p-3">
                  <div className="text-xs font-semibold text-foreground">{section.title}</div>
                  <p className="break-words text-xs leading-5 text-muted-foreground">{section.body}</p>
                </div>
              ))}
            </div>
          </section>
        ) : null}
      </div>

      {showRawSkillContent && (detail.allowedTools.length > 0 || capabilityItems.length > 0) ? (
        <section className="flex flex-col gap-2 rounded-md border border-border bg-card p-4">
          <div className="text-sm font-semibold text-foreground">调试信息</div>
          {detail.allowedTools.length > 0 ? (
            <div className="flex flex-wrap gap-1.5">
              {detail.allowedTools.map((tool) => (
                <span key={tool} className="rounded-[2px] bg-muted px-2 py-0.5 text-2xs text-muted-foreground">
                  {tool}
                </span>
              ))}
            </div>
          ) : null}
          {capabilityItems.length > 0 ? (
            <div className="flex flex-col gap-1 text-xs leading-5 text-muted-foreground">
              {capabilityItems.map((item) => (
                <div key={item}>{item}</div>
              ))}
            </div>
          ) : null}
        </section>
      ) : null}

      {showRawSkillContent && detail.rawContent ? (
        <section className="flex flex-col gap-2 rounded-md border border-border bg-card p-4">
          <div className="text-sm font-semibold text-foreground">原始技能内容</div>
          <div className="max-h-[420px] overflow-auto rounded-md bg-muted/40 p-3 [&_.assistant-markdown]:text-xs [&_.assistant-markdown]:leading-5">
            <AssistantMarkdown text={normalizeRawSkillMarkdown(detail.rawContent)} disableCodeHighlight />
          </div>
        </section>
      ) : null}
    </section>
  )
}

export function SkillDetailPage({ skillId }: SkillDetailPageProps) {
  const skill = useSkillStore((s) => s.getById(skillId))
  const setSkillEnabled = useSkillStore((s) => s.setSkillEnabled)
  const pushNotification = useNotificationStore((s) => s.push)
  const [detail, setDetail] = useState<SkillDetailInfo | null>(null)
  const setRoute = useUiStore((s) => s.setRoute)
  const goBack = useUiStore((s) => s.goBack)
  const canGoBack = useUiStore((s) => s.canGoBack)
  const setPendingSkill = useUiStore((s) => s.setPendingSkill)
  const [isChangingEnabled, setIsChangingEnabled] = useState(false)

  const goToSkillCenter = () => setRoute({ kind: 'skill-center' })
  const handleBack = () => {
    if (canGoBack()) {
      goBack()
      return
    }
    goToSkillCenter()
  }

  useEffect(() => {
    let cancelled = false
    setDetail(null)
    void getSkillDetail(skillId)
      .then((next) => {
        if (!cancelled) setDetail(next)
      })
      .catch((error) => {
        console.error('Failed to load skill detail:', error)
        if (!cancelled) setDetail(null)
      })
    return () => {
      cancelled = true
    }
  }, [skillId])

  const backButton = (
    <Button
      type="button"
      variant="ghost"
      size="icon"
      aria-label="返回"
      onClick={handleBack}
      icon={<ArrowLeft />}
    />
  )

  if (!skill) {
    return (
      <PageSectionShell
        topBar={
          <PageTopBar
            variant="breadcrumb"
            leading={backButton}
            breadcrumbs={[
              { label: '技能中心', onClick: goToSkillCenter },
              { label: '未知技能', current: true },
            ]}
          />
        }
      >
        <div className="text-sm text-muted-foreground">技能不存在或尚未加载。</div>
      </PageSectionShell>
    )
  }

  const localized = localizeSkill(skill)
  const enabled = isSkillEnabled(skill)
  const manageable = canToggleSkillEnablement(skill)
  const market = isMarketSkill(skill)
  const builtin = isBuiltinSkill(skill)
  const trigger = skill.triggerText?.trim() || `/${skill.id}`
  const updatedAt = formatSkillUpdatedAt(skill.updatedAt)

  const handleSetEnabled = async (nextEnabled: boolean): Promise<boolean> => {
    if (!manageable) return false
    setIsChangingEnabled(true)
    try {
      await setSkillEnabled(skill.id, nextEnabled)
      return true
    } catch (err) {
      pushNotification({
        level: 'error',
        title: nextEnabled ? '开启技能失败' : '关闭技能失败',
        message: err instanceof Error ? err.message : String(err),
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
      return false
    } finally {
      setIsChangingEnabled(false)
    }
  }

  const handleUseSkill = async () => {
    if (manageable && !enabled) {
      const enabledForUse = await handleSetEnabled(true)
      if (!enabledForUse) return
    }
    setPendingSkill({
      id: skill.id,
      label: localized.name,
      trigger,
    })
    setRoute({ kind: 'home' })
  }

  const primaryLabel = market ? '添加并使用' : enabled ? '使用' : '开启并使用'
  const secondaryLabel = manageable ? (enabled ? '关闭' : '保持关闭') : undefined
  const handleSecondary = manageable
    ? enabled
      ? () => void handleSetEnabled(false)
      : goToSkillCenter
    : undefined
  const secondaryAction = manageable ? (enabled ? 'disable' : 'keep-disabled') : undefined

  return (
    <PageSectionShell
      topBar={
        <PageTopBar
          variant="breadcrumb"
          leading={backButton}
          breadcrumbs={[
            { label: '技能中心', onClick: goToSkillCenter },
            { label: localized.name, current: true },
          ]}
        />
      }
    >
      <div
        data-aijia-skill-detail
        data-aijia-skill-id={skill.id}
        data-aijia-skill-enabled={String(enabled)}
        className="contents"
      >
      <SkillDetailHero
        title={localized.name}
        subtitle={localized.description || `通过命令 ${trigger} 快速调用`}
        actionBar={
          <div className="flex flex-wrap items-center justify-end gap-2.5">
            {manageable ? (
              <div className="flex items-center gap-2 rounded-md border border-border bg-card px-2.5 py-1.5 text-sm text-muted-foreground">
                <span>{enabled ? '已开启' : '已关闭'}</span>
                <SegmentedControl<'off' | 'on'>
                  className="w-20 shrink-0"
                  value={enabled ? 'on' : 'off'}
                  disabled={isChangingEnabled}
                  data-aijia-skill-toggle={skill.id}
                  ariaLabel={`${localized.name} 技能开关`}
                  onValueChange={(value) => void handleSetEnabled(value === 'on')}
                  options={TOGGLE_OPTIONS}
                />
              </div>
            ) : null}
            <SkillActionBar
              primaryLabel={primaryLabel}
              primaryDisabled={isChangingEnabled}
              onPrimary={() => void handleUseSkill()}
              secondaryLabel={secondaryLabel}
              onSecondary={handleSecondary}
              secondaryAction={secondaryAction}
            />
          </div>
        }
      />

      <SkillMetaRow
        items={[
          {
            label: '来源',
            value: market ? '市场' : builtin ? 'AI 小家内置' : '已安装',
          },
          ...(skill.version ? [{ label: '版本', value: skill.version }] : []),
          ...(skill.category ? [{ label: '分类', value: skill.category }] : []),
          { label: '技能 ID', value: skill.id },
          { label: '调用命令', value: trigger },
          ...(manageable ? [{ label: '状态', value: enabled ? '已开启' : '已关闭' }] : []),
          ...(updatedAt ? [{ label: '更新时间', value: updatedAt }] : []),
        ]}
      />

      <SkillSpecificDetails detail={detail} />
      <SkillUsageBlock
        usageSteps={[
          `点击右上角“${primaryLabel}”后，会回到对话首页并把技能 chip 放入输入框。`,
          '按需要补充上下文，可以继续输入任务要求或添加文件作为附件。',
          '发送后，AI 小家会按该技能的规则处理本轮请求。',
          enabled || !manageable
            ? `也可以在任意对话输入框手动输入 ${trigger} 加具体要求来触发。`
            : '当前技能关闭时不会出现在输入框技能选择或 slash 快捷入口中。',
        ]}
        notes={[
          '技能 chip 只对当前这一轮消息生效，发送后会自动清除。',
          '关闭技能后，它仍保留在管理页和详情页，但不会进入后续对话上下文。',
          market
            ? '市场技能来自企业下发或可安装来源；安装后会进入已安装管理视图。'
            : builtin
              ? '内置技能随客户端初始化和官方更新维护，可在这里按账号关闭或开启。'
              : '本地安装技能可以重新导入新版 SKILL 包完成更新。',
        ]}
      />
      </div>
    </PageSectionShell>
  )
}
