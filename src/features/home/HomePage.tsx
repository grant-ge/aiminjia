import { useState } from 'react'
import { CheckCircle2, Inbox, Loader2 } from 'lucide-react'

import { HomeCategoryChipRow } from '@/components/home/HomeCategoryChipRow'
import { HomeMascotHero } from '@/components/home/HomeMascotHero'
import { HomeSkillCenterPill } from '@/components/home/HomeSkillCenterPill'
import { HomeStatusList } from '@/components/home/HomeStatusList'
import { HomeTaskComposerCard } from '@/components/home/HomeTaskComposerCard'
import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { useBrandingStore } from '@/stores/brandingStore'
import { useUiStore, type Route } from '@/stores/uiStore'

const CHIP_ITEMS = [
  { key: 'recommend', label: '为你推荐' },
  { key: 'writing', label: '文案有意' },
  { key: 'industry', label: '行业研究' },
  { key: 'file', label: '文件智能' },
  { key: 'commerce', label: '电商运营' },
  { key: 'ding', label: '玩转钉钉' },
]

const STATUS_ITEMS = [
  {
    key: 'empty',
    variant: 'empty' as const,
    icon: <Inbox className="h-4 w-4 text-primary" />,
    title: '空状态占位',
    desc: '暂无任务结果。你可以从上方输入任务，或选择下方推荐分类快速开始。',
  },
  {
    key: 'loading',
    variant: 'loading' as const,
    icon: <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />,
    title: '加载状态占位',
    desc: 'AI 正在分析任务上下文并生成执行计划，请稍候...',
  },
  {
    key: 'success',
    variant: 'success' as const,
    icon: <CheckCircle2 className="h-4 w-4" style={{ color: '#16A34A' }} />,
    title: '成功状态占位',
    desc: '任务已创建成功，可前往对话页继续补充指令，或进入技能中心选择能力模块。',
  },
]

export function HomePage() {
  const [activeChip, setActiveChip] = useState('recommend')
  const setRoute = useUiStore((s) => s.setRoute)
  const logoUrl = useBrandingStore((s) => s.logoUrl)

  return (
    <PageSectionShell
      padding="px-10 pt-8 pb-7"
      gap="gap-4"
    >
      <div className="mx-auto flex w-[820px] flex-col items-center gap-4">
        <HomeMascotHero
          mascotUrl={logoUrl}
          title="创建你的下一条任务"
          subtitle="用清晰的任务描述和参数，让 AI 更快给出可执行结果。"
        />
        <div className="w-full">
          <HomeTaskComposerCard />
        </div>
        <HomeCategoryChipRow
          items={CHIP_ITEMS}
          activeKey={activeChip}
          onSelect={setActiveChip}
        />
        <HomeStatusList items={STATUS_ITEMS} />
        <HomeSkillCenterPill
          onClick={() => setRoute({ kind: 'skill-center' } as Route)}
        />
      </div>
    </PageSectionShell>
  )
}
