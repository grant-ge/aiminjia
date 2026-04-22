import { AppSidebar } from '@/components/sidebar/AppSidebar'

interface SidebarProps {
  onOpenSettings?: () => void
}

export function Sidebar(_: SidebarProps) {
  return <AppSidebar />
}
