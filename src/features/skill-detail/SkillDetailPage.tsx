import { ArrowLeft } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'

import { AssistantMarkdown } from '@/components/chat-scene/AssistantMarkdown'
import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { SkillActionBar } from '@/components/skills/SkillActionBar'
import { SkillDetailHero } from '@/components/skills/SkillDetailHero'
import { SkillMetaRow } from '@/components/skills/SkillMetaRow'
import { SkillUsageBlock } from '@/components/skills/SkillUsageBlock'
import { useDevSettingsStore } from '@/stores/devSettingsStore'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'
import { localizeSkill } from '@/lib/skillLocalization'
import { getSkillDetail, type SkillDetailInfo } from '@/lib/tauri'

import { formatSkillUpdatedAt } from './formatSkillUpdatedAt'
import { Button } from '@/components/ui/button'

interface SkillDetailPageProps {
  skillId: string
}

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
  const [detail, setDetail] = useState<SkillDetailInfo | null>(null)
  const setRoute = useUiStore((s) => s.setRoute)
  const setPendingSkill = useUiStore((s) => s.setPendingSkill)
  const goToSkillCenter = () => setRoute({ kind: 'skill-center' })

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

  const handleUseSkill = () => {
    if (!skill) return
    const localized = localizeSkill(skill)
    const trigger = (skill.triggerText?.trim() || `/${skill.id}`)
    setPendingSkill({
      id: skill.id,
      label: localized.name,
      trigger,
    })
    setRoute({ kind: 'home' })
  }

  const backButton = (
    <Button
      type="button"
      variant="ghost"
      size="icon"
      aria-label="返回技能中心"
      onClick={goToSkillCenter}
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

  const localized = skill ? localizeSkill(skill) : null

  return (
    <PageSectionShell
      topBar={
        <PageTopBar
          variant="breadcrumb"
          leading={backButton}
          breadcrumbs={[
            { label: '技能中心', onClick: goToSkillCenter },
            { label: localized?.name ?? skill.id, current: true },
          ]}
        />
      }
    >
      <SkillDetailHero
        title={localized?.name ?? skill.id}
        subtitle={localized?.description || `通过命令 ${skill.triggerText?.trim() || `/${skill.id}`} 快速调用`}
        actionBar={
          <SkillActionBar
            primaryLabel="使用"
            onPrimary={handleUseSkill}
          />
        }
      />
      <SkillMetaRow
        items={[
          { label: '来源', value: skill.source === 'builtin' ? 'AI 小家内置' : '已安装' },
          ...(skill.version ? [{ label: '版本', value: skill.version }] : []),
          ...(skill.category ? [{ label: '分类', value: skill.category }] : []),
          { label: '技能 ID', value: skill.id },
          { label: '调用命令', value: skill.triggerText?.trim() || `/${skill.id}` },
          ...(formatSkillUpdatedAt(skill.updatedAt)
            ? [{ label: '更新时间', value: formatSkillUpdatedAt(skill.updatedAt) as string }]
            : []),
        ]}
      />
      <SkillSpecificDetails detail={detail} />
      <SkillUsageBlock
        usageSteps={[
          '点击右上角"使用"按钮,自动跳转到对话首页,输入框上方会出现技能 chip。',
          '按需要补充上下文：可拖拽/选择文件作为附件,或直接在输入框继续描述任务。',
          '回车发送后,AI 小家会按该技能的执行规则完成工作并产出结果。',
          `也可以在任意对话输入框手动输入 ${skill.triggerText?.trim() || `/${skill.id}`} + 你的具体要求来触发。`,
        ]}
        notes={[
          '技能 chip 仅对当前这一轮消息生效,发送后会自动清除;再次使用请重新进入技能详情或在输入框中重新调用命令。',
          '建议描述任务时尽量具体(目标 / 约束 / 期望输出格式),技能内置的执行逻辑会按上下文展开,信息越足结果越准。',
          '若技能依赖文件输入(如表格 / 文档 / 截图),请确保附件已加载完成再点击发送。',
          skill.source === 'builtin'
            ? '本技能为 AI 小家内置,会随客户端版本统一升级,无需手动维护。'
            : '本技能为本地安装,如需更新请在技能中心重新导入最新版本的 SKILL 包。',
        ]}
      />
    </PageSectionShell>
  )
}
