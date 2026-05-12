import { HomeMascotHero } from '@/components/home/HomeMascotHero'
import { HomeTaskComposerCard } from '@/components/home/HomeTaskComposerCard'
import { PageSectionShell } from '@/components/shell/PageSectionShell'

export function HomePage() {
  return (
    <PageSectionShell
      padding="px-10 pt-8 pb-7"
      gap="gap-4"
      className="min-h-full justify-center"
    >
      <div className="mx-auto flex w-[820px] flex-col items-center gap-10 -mt-6">
        <HomeMascotHero
          mascotUrl="/home-mascot-fill-13.svg"
          title="千头万绪在前，先理一端"
        />
        <div className="w-full">
          <HomeTaskComposerCard />
        </div>
      </div>
    </PageSectionShell>
  )
}
