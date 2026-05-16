import { useTranslation } from 'react-i18next'
import { HomeMascotHero } from '@/components/home/HomeMascotHero'
import { HomeTaskComposerCard } from '@/components/home/HomeTaskComposerCard'
import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { useBrandingStore } from '@/stores/brandingStore'

export function HomePage() {
  const { t } = useTranslation()
  const logoUrl = useBrandingStore((s) => s.logoUrl)
  return (
    <PageSectionShell
      padding="px-10 pt-8 pb-7"
      gap="gap-4"
      className="min-h-full justify-center"
    >
      <div className="mx-auto flex w-[820px] flex-col items-center gap-10 -mt-6">
        <HomeMascotHero
          mascotUrl={logoUrl}
          title={t('homePage.heroTitle')}
        />
        <div className="w-full">
          <HomeTaskComposerCard />
        </div>
      </div>
    </PageSectionShell>
  )
}
