import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  ArrowDownRight,
  BarChart2,
  BriefcaseBusiness,
  CalendarClock,
  ChevronLeft,
  ChevronRight,
  Code2,
  FileText,
  Link2,
  Sparkles,
  UsersRound,
  type LucideIcon,
} from 'lucide-react'

import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

type QuickExampleCategoryKey =
  | 'office'
  | 'analysis'
  | 'docs'
  | 'coding'
  | 'automation'
  | 'connection'
  | 'hrExpert'
  | 'generalAssistant'

export type HomeQuickExampleRequirement =
  | 'workspace'
  | 'excelAttachment'
  | 'skillDiscovery'
  | 'businessConnection'

interface QuickExampleCategory {
  key: QuickExampleCategoryKey
  icon: LucideIcon
}

interface QuickExampleItem {
  key: string
  category: QuickExampleCategoryKey
  requirements?: HomeQuickExampleRequirement[]
}

export interface HomeQuickExampleSelection {
  mode: 'clear' | 'fill'
  prompt: string
  requirements?: HomeQuickExampleRequirement[]
}

interface HomeQuickExamplesProps {
  onSelect: (selection: HomeQuickExampleSelection) => void
}

interface QuickExampleScrollerProps {
  children: ReactNode
  scrollLeftLabel: string
  scrollRightLabel: string
  testId: string
}

const CATEGORIES: QuickExampleCategory[] = [
  { key: 'connection', icon: Link2 },
  { key: 'hrExpert', icon: UsersRound },
  { key: 'generalAssistant', icon: Sparkles },
  { key: 'office', icon: BriefcaseBusiness },
  { key: 'analysis', icon: BarChart2 },
  { key: 'docs', icon: FileText },
  { key: 'coding', icon: Code2 },
  { key: 'automation', icon: CalendarClock },
]

const EXAMPLES: QuickExampleItem[] = [
  { key: 'weeklyPlan', category: 'office' },
  { key: 'meetingNotes', category: 'office' },
  { key: 'presentationOutline', category: 'office' },
  { key: 'professionalReply', category: 'office' },
  { key: 'projectRetrospective', category: 'office' },
  { key: 'interviewQuestions', category: 'office' },
  { key: 'spreadsheetInsight', category: 'analysis' },
  { key: 'anomalyReview', category: 'analysis' },
  { key: 'businessReview', category: 'analysis' },
  { key: 'dashboardAdvice', category: 'analysis' },
  { key: 'salesFunnel', category: 'analysis' },
  { key: 'surveyAnalysis', category: 'analysis' },
  { key: 'sourceSummary', category: 'docs' },
  { key: 'prdDraft', category: 'docs' },
  { key: 'contractRisk', category: 'docs' },
  { key: 'sop', category: 'docs' },
  { key: 'resumeTemplate', category: 'docs' },
  { key: 'proposalDraft', category: 'docs' },
  { key: 'portfolioSite', category: 'coding' },
  { key: 'snakeGame', category: 'coding' },
  { key: 'todoApp', category: 'coding' },
  { key: 'markdownEditor', category: 'coding' },
  { key: 'projectOnboarding', category: 'coding', requirements: ['workspace'] },
  { key: 'bugTriage', category: 'coding', requirements: ['workspace'] },
  { key: 'featurePlan', category: 'coding', requirements: ['workspace'] },
  { key: 'codeReview', category: 'coding', requirements: ['workspace'] },
  { key: 'workflowBreakdown', category: 'automation' },
  { key: 'scheduleDraft', category: 'automation' },
  { key: 'skillDesign', category: 'automation' },
  { key: 'handoffBrief', category: 'automation' },
  { key: 'competitorMonitor', category: 'automation' },
  { key: 'weeklyReportReminder', category: 'automation' },
  { key: 'dingtalkMessage', category: 'connection', requirements: ['businessConnection'] },
  { key: 'rehcmRecentChanges', category: 'connection', requirements: ['businessConnection', 'skillDiscovery'] },
  { key: 'payrollLatestPayslip', category: 'connection', requirements: ['businessConnection', 'skillDiscovery'] },
  { key: 'compensationFairness', category: 'hrExpert', requirements: ['skillDiscovery', 'excelAttachment'] },
  { key: 'talentReview', category: 'hrExpert', requirements: ['skillDiscovery', 'excelAttachment'] },
  { key: 'eventResearchReport', category: 'generalAssistant' },
  { key: 'opinionToPpt', category: 'generalAssistant' },
  { key: 'excelDataCleanup', category: 'generalAssistant', requirements: ['excelAttachment'] },
]

function QuickExampleScroller({
  children,
  scrollLeftLabel,
  scrollRightLabel,
  testId,
}: QuickExampleScrollerProps) {
  const scrollRef = useRef<HTMLDivElement>(null)
  const [canScrollLeft, setCanScrollLeft] = useState(false)
  const [canScrollRight, setCanScrollRight] = useState(false)

  const updateScrollState = useCallback(() => {
    const el = scrollRef.current
    if (!el) {
      setCanScrollLeft(false)
      setCanScrollRight(false)
      return
    }
    const maxScrollLeft = Math.max(0, el.scrollWidth - el.clientWidth)
    setCanScrollLeft(el.scrollLeft > 1)
    setCanScrollRight(el.scrollLeft < maxScrollLeft - 1)
  }, [])

  useEffect(() => {
    const el = scrollRef.current
    if (!el) return

    el.addEventListener('scroll', updateScrollState, { passive: true })
    window.addEventListener('resize', updateScrollState)
    const resizeObserver = typeof ResizeObserver === 'undefined'
      ? null
      : new ResizeObserver(updateScrollState)
    resizeObserver?.observe(el)

    const frame = window.requestAnimationFrame(updateScrollState)
    return () => {
      window.cancelAnimationFrame(frame)
      el.removeEventListener('scroll', updateScrollState)
      window.removeEventListener('resize', updateScrollState)
      resizeObserver?.disconnect()
    }
  }, [children, updateScrollState])

  const scrollByPage = (direction: 'left' | 'right') => {
    const el = scrollRef.current
    if (!el) return
    const amount = Math.max(180, el.clientWidth * 0.65)
    el.scrollBy({
      left: direction === 'left' ? -amount : amount,
      behavior: 'smooth',
    })
    window.setTimeout(updateScrollState, 260)
  }

  return (
    <div className="relative min-w-0">
      <div
        ref={scrollRef}
        data-testid={testId}
        className="flex min-w-0 items-center gap-2 overflow-x-auto overflow-y-hidden py-0.5 [-ms-overflow-style:none] [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      >
        {children}
      </div>

      {canScrollLeft ? (
        <div
          className="pointer-events-none absolute inset-y-0 left-0 flex w-16 items-center opacity-100 transition-opacity"
          style={{ background: 'linear-gradient(to right, var(--background), rgba(var(--background-rgb), 0.90), transparent)' }}
        >
          <Button
            type="button"
            variant="secondary"
            size="icon"
            aria-label={scrollLeftLabel}
            onClick={() => scrollByPage('left')}
            className="pointer-events-auto h-7 w-7 rounded-full border border-border bg-[rgba(var(--background-rgb),0.90)] shadow-sm backdrop-blur"
            icon={<ChevronLeft className="h-4 w-4" />}
          />
        </div>
      ) : null}

      {canScrollRight ? (
        <div
          className="pointer-events-none absolute inset-y-0 right-0 flex w-16 items-center justify-end opacity-100 transition-opacity"
          style={{ background: 'linear-gradient(to left, var(--background), rgba(var(--background-rgb), 0.90), transparent)' }}
        >
          <Button
            type="button"
            variant="secondary"
            size="icon"
            aria-label={scrollRightLabel}
            onClick={() => scrollByPage('right')}
            className="pointer-events-auto h-7 w-7 rounded-full border border-border bg-[rgba(var(--background-rgb),0.90)] shadow-sm backdrop-blur"
            icon={<ChevronRight className="h-4 w-4" />}
          />
        </div>
      ) : null}
    </div>
  )
}

export function HomeQuickExamples({ onSelect }: HomeQuickExamplesProps) {
  const { t } = useTranslation()
  const [activeCategory, setActiveCategory] = useState<QuickExampleCategoryKey | null>(null)
  const [showExamples, setShowExamples] = useState(false)
  const visibleExamples = useMemo(
    () => (activeCategory ? EXAMPLES.filter((item) => item.category === activeCategory) : []),
    [activeCategory],
  )

  const handleCategoryClick = (category: QuickExampleCategoryKey) => {
    setActiveCategory(category)
    setShowExamples(true)
    onSelect({ mode: 'clear', prompt: '' })
  }

  const handleExampleClick = (item: QuickExampleItem, prompt: string) => {
    onSelect({ mode: 'fill', prompt, requirements: item.requirements })
    setShowExamples(false)
  }

  return (
    <section
      data-testid="home-quick-examples"
      aria-label={t('homePage.quickExamples.ariaLabel')}
      className="w-full overflow-hidden"
    >
      <div className="relative min-h-8">
        <div
          data-testid="home-quick-category-face"
          aria-hidden={showExamples}
          inert={showExamples}
          className={cn(
            'transition-all duration-200 ease-out',
            showExamples
              ? 'pointer-events-none absolute inset-0 -translate-y-full opacity-0'
              : 'relative translate-y-0 opacity-100',
          )}
        >
          <QuickExampleScroller
            scrollLeftLabel={t('homePage.quickExamples.scrollLeft')}
            scrollRightLabel={t('homePage.quickExamples.scrollRight')}
            testId="home-quick-category-scroll"
          >
            {CATEGORIES.map((category) => {
              const Icon = category.icon
              return (
                <Button
                  key={category.key}
                  type="button"
                  variant="secondary"
                  size="md"
                  aria-pressed={false}
                  onClick={() => handleCategoryClick(category.key)}
                  className="h-8 shrink-0 rounded-md bg-[rgba(var(--muted-rgb),0.80)] px-3 font-medium text-[rgba(var(--foreground-rgb),0.78)] shadow-none hover:bg-muted hover:text-foreground"
                  icon={<Icon />}
                >
                  {t(`homePage.quickExamples.categories.${category.key}`)}
                </Button>
              )
            })}
          </QuickExampleScroller>
        </div>

        <div
          data-testid="home-quick-example-face"
          aria-hidden={!showExamples}
          inert={!showExamples}
          className={cn(
            'transition-all duration-200 ease-out',
            showExamples
              ? 'relative translate-y-0 opacity-100'
              : 'pointer-events-none absolute inset-0 translate-y-full opacity-0',
          )}
        >
          <QuickExampleScroller
            scrollLeftLabel={t('homePage.quickExamples.scrollLeft')}
            scrollRightLabel={t('homePage.quickExamples.scrollRight')}
            testId="home-quick-example-scroll"
          >
            {visibleExamples.map((item) => {
              const title = t(`homePage.quickExamples.examples.${item.key}.title`)
              const prompt = t(`homePage.quickExamples.examples.${item.key}.prompt`)
              return (
                <Button
                  key={item.key}
                  type="button"
                  variant="secondary"
                  size="md"
                  onClick={() => handleExampleClick(item, prompt)}
                  className="h-8 shrink-0 rounded-md bg-muted px-3 text-sm font-medium text-muted-foreground shadow-none hover:bg-[rgba(var(--muted-rgb),0.80)] hover:text-foreground"
                  suffixIcon={<ArrowDownRight className="h-3.5 w-3.5 text-muted-foreground" />}
                >
                  {title}
                </Button>
              )
            })}
          </QuickExampleScroller>
        </div>
      </div>
    </section>
  )
}
