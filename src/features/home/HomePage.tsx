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
      <div className="mx-auto flex w-[820px] flex-col items-center gap-4 -mt-6">
        <HomeMascotHero
          mascotUrl="/home-mascot-fill-13.svg"
          title="创建你的下一条任务"
          subtitle="用清晰的任务描述和参数，让 AI 更快给出可执行结果。"
        />
        <div className="w-full">
          <HomeTaskComposerCard />
        </div>
      </div>
    </PageSectionShell>
  )
}
