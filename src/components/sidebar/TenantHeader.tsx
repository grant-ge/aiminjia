import { useAuthStore } from '@/stores/authStore'
import { useBrandingStore } from '@/stores/brandingStore'

export function TenantHeader() {
  const user = useAuthStore((state) => state.user)
  const tenant = useAuthStore((state) => state.tenant)
  const productName = useBrandingStore((state) => state.productName)

  return (
    <div className="border-b border-sidebar-border px-4 py-4">
      <div className="text-sm font-semibold text-sidebar-foreground">{productName}</div>
      <div className="mt-1 text-xs text-muted-foreground">
        {tenant?.name ?? user?.name ?? user?.username ?? '未登录'}
      </div>
    </div>
  )
}
