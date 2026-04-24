import { Sparkles } from 'lucide-react'

import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'
import { SkillActionBar } from '@/components/skills/SkillActionBar'
import { SkillCard } from '@/components/skills/SkillCard'
import { SkillDetailHero } from '@/components/skills/SkillDetailHero'
import { SkillMetaRow } from '@/components/skills/SkillMetaRow'
import { SkillTryGrid } from '@/components/skills/SkillTryGrid'
import { SkillUsageBlock } from '@/components/skills/SkillUsageBlock'
import { useChat } from '@/hooks/useChat'
import { useSkillStore } from '@/stores/skillStore'

interface SkillDetailPageProps {
  skillId: string
}

const TRY_PROMPTS = [
  '依据这份表格，分析本月经营数据，输出 KPI 达成率、趋势图和 P0/P1 行动建议。',
  '帮我分析表格数据，自动挖掘 KPI、趋势和异常，输出可视化报告。',
  '把这份多 sheet Excel 拆开分析，各模块独立出报告并关联对比。',
]

export function SkillDetailPage({ skillId }: SkillDetailPageProps) {
  const skill = useSkillStore((s) => s.getById(skillId))
  const { createConversationFromSkill } = useChat()

  if (!skill) {
    return (
      <PageSectionShell topBar={<PageTopBar variant="default" />} padding="px-10 pt-10 pb-8" gap="gap-4">
        <div className="text-sm text-muted-foreground">技能不存在或尚未加载。</div>
      </PageSectionShell>
    )
  }

  return (
    <PageSectionShell
      topBar={<PageTopBar variant="default" />}
      padding="px-10 pt-7 pb-8"
      gap="gap-6"
    >
      <SkillDetailHero
        iconNode={<Sparkles className="h-9 w-9 text-primary" />}
        title={skill.displayName}
        subtitle={skill.shortDescription || skill.description}
        actionBar={
          <SkillActionBar
            secondaryLabel="禁用"
            primaryLabel="使用"
            onSecondary={() => {}}
            onPrimary={() => void createConversationFromSkill(skill.id)}
          />
        }
      />
      <SkillMetaRow
        items={[
          { label: '来源', value: skill.source === 'builtin' ? 'AI 小家内置' : '已安装' },
          { label: '更新时间', value: '2026-04-20' },
        ]}
      />
      <SkillTryGrid>
        {TRY_PROMPTS.map((p, i) => (
          <SkillCard
            key={i}
            iconNode={<Sparkles className="h-4 w-4 text-primary" />}
            title={skill.displayName}
            meta={skill.source === 'builtin' ? '内置' : '自定义'}
            desc={p}
            onClick={() => void createConversationFromSkill(skill.id)}
          />
        ))}
      </SkillTryGrid>
      <SkillUsageBlock
        text={skill.description || '上传 Excel 或 CSV 表格，一键生成可视化数据分析报告。'}
      />
    </PageSectionShell>
  )
}
