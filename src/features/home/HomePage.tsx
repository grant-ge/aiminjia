import { useTranslation } from 'react-i18next'
import { HomeMascotHero } from '@/components/home/HomeMascotHero'
import { HomeTaskComposerCard } from '@/components/home/HomeTaskComposerCard'
import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { useBrandingStore } from '@/stores/brandingStore'

export function HomePage() {
  const { t } = useTranslation()
  const logoUrl = useBrandingStore((s) => s.logoUrl)
  return (
    <PageSectionShell className="min-h-full justify-center">
      <div className="mx-auto -mt-4 flex w-[760px] max-w-full flex-col items-center gap-8">
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
