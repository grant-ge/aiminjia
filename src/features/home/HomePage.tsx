import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { PageTopBar } from '@/components/shell/PageTopBar'

import { HomeSuggestionList } from '@/components/home/HomeSuggestionList'
import { HomeTaskComposerCard } from '@/components/home/HomeTaskComposerCard'

export function HomePage() {
  return (
    <PageSectionShell
      header={<PageTopBar variant="default" />}
      className="max-w-none gap-8 px-10 pb-12 pt-8"
    >
      <HomeTaskComposerCard />
      <HomeSuggestionList />
    </PageSectionShell>
  )
}
