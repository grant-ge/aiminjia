import { useTranslation } from 'react-i18next'
import { useState } from 'react'
import { HomeMascotHero } from '@/components/home/HomeMascotHero'
import { HomeQuickExamples, type HomeQuickExampleSelection } from '@/components/home/HomeQuickExamples'
import { HomeTaskComposerCard } from '@/components/home/HomeTaskComposerCard'
import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { useBrandingStore } from '@/stores/brandingStore'

export function HomePage() {
  const { t } = useTranslation()
  const logoUrl = useBrandingStore((s) => s.logoUrl)
  const [quickPrompt, setQuickPrompt] = useState<HomeQuickExampleSelection | null>(null)
  return (
    <PageSectionShell className="min-h-full justify-center">
      <div className="mx-auto -mt-4 flex w-[760px] max-w-full flex-col items-start gap-6">
        <HomeMascotHero
          mascotUrl={logoUrl}
          title={t('homePage.heroTitle')}
        />
        <div className="flex w-full flex-col gap-16">
          <HomeQuickExamples onSelect={setQuickPrompt} />
          <HomeTaskComposerCard quickPrompt={quickPrompt} />
        </div>
      </div>
    </PageSectionShell>
  )
}
