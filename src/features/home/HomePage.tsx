import { useMemo, useState } from 'react'

import { HomeCategoryChipRow } from '@/components/home/HomeCategoryChipRow'
import { HomeMascotHero } from '@/components/home/HomeMascotHero'
import { HomeSuggestionPanel } from '@/components/home/HomeSuggestionPanel'
import { HomeSkillCenterPill } from '@/components/home/HomeSkillCenterPill'
import { HomeTaskComposerCard } from '@/components/home/HomeTaskComposerCard'
import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { HOME_EXPERT_CATEGORIES, HOME_SUGGESTIONS, type HomeSuggestionItem } from '@/data/home-suggestions'
import { useChat } from '@/hooks/useChat'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore, type Route } from '@/stores/uiStore'

export function HomePage() {
  const [activeChip, setActiveChip] = useState('recommend')
  const setRoute = useUiStore((s) => s.setRoute)
  const getSkillById = useSkillStore((s) => s.getById)
  const { sendUserMessage } = useChat()

  const suggestionItems = useMemo(
    () => HOME_SUGGESTIONS[activeChip] ?? HOME_SUGGESTIONS.recommend,
    [activeChip],
  )

  const handleSelectSuggestion = (item: HomeSuggestionItem) => {
    const triggerText = item.skillId ? getSkillById(item.skillId)?.triggerText?.trim() : ''
    const message = triggerText ? `${triggerText} ${item.prompt}` : item.prompt
    void sendUserMessage(message)
  }

  return (
    <PageSectionShell
      padding="px-10 pt-8 pb-7"
      gap="gap-4"
      className="min-h-full justify-center"
    >
      <div className="mx-auto flex w-[820px] flex-col items-center gap-4">
        <HomeMascotHero
          mascotUrl="/home-mascot-fill-13.svg"
          title="创建你的下一条任务"
          subtitle="用清晰的任务描述和参数，让 AI 更快给出可执行结果。"
        />
        <div className="w-full">
          <HomeTaskComposerCard />
        </div>
        <div className="border border-border w-full rounded-[22px]">
          <HomeCategoryChipRow
            items={HOME_EXPERT_CATEGORIES}
            activeKey={activeChip}
            onSelect={setActiveChip}
          />
          <HomeSuggestionPanel
            items={suggestionItems}
            onSelect={handleSelectSuggestion}
          />
        </div>
        <HomeSkillCenterPill
          onClick={() => setRoute({ kind: 'skill-center' } as Route)}
        />
      </div>
    </PageSectionShell>
  )
}
