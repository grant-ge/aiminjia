/**
 * @designSource design.pen#jTgSA
 * @sizing padding [6,8], gap 8
 */
import { Settings } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'

interface SidebarFooterSettingsProps {
  onClick: () => void
}

export function SidebarFooterSettings({ onClick }: SidebarFooterSettingsProps) {
  const { t } = useTranslation()
  return (
    <Button unstyled
      type="button"
      data-aijia-open-settings
      onClick={onClick}
      className="my-2 flex h-9 w-full items-center gap-2 rounded-md px-2.5 text-left text-sm font-medium text-[rgba(var(--sidebar-foreground-rgb),0.70)] transition-colors hover:bg-[rgba(var(--sidebar-accent-rgb),0.60)] hover:text-sidebar-foreground"
    >
      <Settings className="h-4 w-4 shrink-0 text-muted-foreground" />
      <span>{t('nav.settings')}</span>
    </Button>
  )
}
