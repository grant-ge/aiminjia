import { ArrowLeft } from 'lucide-react'

import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { SkillActionBar } from '@/components/skills/SkillActionBar'
import { SkillDetailHero } from '@/components/skills/SkillDetailHero'
import { SkillMetaRow } from '@/components/skills/SkillMetaRow'
import { SkillUsageBlock } from '@/components/skills/SkillUsageBlock'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'
import { localizeSkill } from '@/lib/skillLocalization'

import { formatSkillUpdatedAt } from './formatSkillUpdatedAt'

interface SkillDetailPageProps {
  skillId: string
}

export function SkillDetailPage({ skillId }: SkillDetailPageProps) {
  const skill = useSkillStore((s) => s.getById(skillId))
  const setRoute = useUiStore((s) => s.setRoute)
  const setPendingSkill = useUiStore((s) => s.setPendingSkill)
  const goToSkillCenter = () => setRoute({ kind: 'skill-center' })

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
    <button
      type="button"
      aria-label="返回技能中心"
      onClick={goToSkillCenter}
      className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground"
    >
      <ArrowLeft className="h-4 w-4" />
    </button>
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
          { label: '调用命令', value: skill.triggerText?.trim() || `/${skill.id}` },
          ...(formatSkillUpdatedAt(skill.updatedAt)
            ? [{ label: '更新时间', value: formatSkillUpdatedAt(skill.updatedAt) as string }]
            : []),
        ]}
      />
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
