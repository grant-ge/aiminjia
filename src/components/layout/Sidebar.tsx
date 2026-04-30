import { AppSidebar } from '@/components/sidebar/AppSidebar'

interface SidebarProps {
  onOpenSettings?: () => void
}

export function Sidebar(props: SidebarProps) {
  void props
  return <AppSidebar />
}
