/**
 * @designSource design.pen#IIzfj acctCard + nKzUU notice
 * @sizing acctCard r-14 padding 18; notice r-14 padding 24
 */
import { Info } from 'lucide-react'

import { Button } from '@/components/ui/button'

interface AccountPanelProps {
  user: { name: string; tenantName: string; avatarUrl: string }
  onLogout: () => void
}

export function AccountPanel({ user, onLogout }: AccountPanelProps) {
  return (
    <>
      <div className="flex items-center gap-3.5 rounded-[14px] bg-secondary p-[18px]">
        <div className="h-12 w-12 shrink-0 overflow-hidden rounded-full bg-primary">
          {user.avatarUrl ? (
            <img src={user.avatarUrl} alt="" className="h-full w-full object-cover" />
          ) : (
            <span className="flex h-full w-full items-center justify-center text-lg font-semibold text-primary-foreground">
              {user.name.charAt(0).toUpperCase()}
            </span>
          )}
        </div>
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <div className="text-sm font-bold text-foreground">{user.name}</div>
          <div className="truncate text-[13px] text-muted-foreground">{user.tenantName}</div>
        </div>
        <Button variant="outline" onClick={onLogout}>
          退出登录
        </Button>
      </div>
      <div className="flex flex-col items-center gap-1.5 rounded-[14px] bg-secondary px-6 py-6 text-center">
        <Info className="h-4 w-4 text-muted-foreground" />
        <div className="text-[13px] text-muted-foreground">
          账户信息以企业 SSO / 登录账号为准，如需更换请退出后重新登录。
        </div>
      </div>
    </>
  )
}
