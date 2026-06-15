import { Loader2, ShieldQuestionMark } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'

export type SidebarRowStatus =
  | 'permission-review'
  | 'waiting-reply'
  | 'loading'
  | null

interface SidebarRowStatusIndicatorProps {
  status?: SidebarRowStatus
}

export function SidebarRowStatusIndicator({
  status = null,
}: SidebarRowStatusIndicatorProps) {
  const { t } = useTranslation()

  if (!status) return null

  if (status === 'loading') {
    return (
      <Loader2
        aria-label={t('sidebar.conversationLoading')}
        data-icon="loader"
        className="h-3.5 w-3.5 animate-spin text-muted-foreground"
      />
    )
  }

  const label =
    status === 'permission-review'
      ? t('sidebar.status.permissionReviewChip')
      : t('sidebar.status.waitingReplyChip')
  const tooltip =
    status === 'permission-review'
      ? t('sidebar.status.permissionReviewTooltip')
      : t('sidebar.status.waitingReplyTooltip')

  return (
    <TooltipProvider delayDuration={400}>
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="inline-flex h-5 items-center gap-1 rounded-full bg-primary/10 px-1.5 text-[10px] font-medium leading-none text-primary">
            <ShieldQuestionMark className="h-3 w-3" />
            {label}
          </span>
        </TooltipTrigger>
        <TooltipContent side="top">{tooltip}</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}
